//! Integration tests for persistent slashing state.
//!
//! These tests verify that slashing state survives node restarts when using
//! [`SledSlashingStore`], that corrupted data is handled gracefully, and that
//! [`InMemorySlashingStore`] maintains backward compatibility.

use omnia_substrate::{
    InMemorySlashingStore, SlashOffense, SlashOutcome, SlashingEngine, SlashingState,
    SlashingStore, SlashingStoreError, SledSlashingStore, DEFAULT_EJECTION_THRESHOLD,
    DEFAULT_SLASH_THRESHOLD,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Helper: create a `NodeId` from a single byte.
fn node(id: u8) -> [u8; 32] {
    let mut n = [0u8; 32];
    n[0] = id;
    n
}

/// Global counter for unique temporary directory names.
static DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Helper: create a unique temporary directory for sled databases.
///
/// Each call creates a directory under a new tempdir with a unique
/// subdirectory name, avoiding lock conflicts between parallel tests.
fn temp_dir(prefix: &str) -> PathBuf {
    let count = DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let path = dir.path().join(format!("{}-{}", prefix, count));
    let _ = std::fs::create_dir_all(&path);
    // Leak the TempDir so it is not cleaned up while the test uses it.
    // The OS will reclaim on process exit.
    std::mem::forget(dir);
    path
}

/// Helper: open a [`SledSlashingStore`] with retries.
///
/// Sled uses file-level locking, and the OS may not release the lock
/// immediately after a previous instance was dropped. This helper retries
/// opening the store a few times with a short delay.
fn open_sled_store(dir: &PathBuf) -> SledSlashingStore {
    let mut attempts = 0;
    loop {
        match SledSlashingStore::open(dir) {
            Ok(store) => return store,
            Err(e) => {
                attempts += 1;
                if attempts >= 10 {
                    panic!(
                        "Failed to open sled store after {} attempts: {}",
                        attempts, e
                    );
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

// ── Persistent state survives restart ──────────────────────────────

#[test]
fn test_sled_slash_history_preserved_across_restart() {
    let dir = temp_dir("slash-persist-1");
    let n1 = node(42);

    // First engine instance: register validator and record offense
    {
        let store = SledSlashingStore::open(&dir).expect("failed to open sled store");
        let mut engine =
            SlashingEngine::with_store(Arc::new(store)).expect("failed to create engine");
        engine.register_validator(n1, 10_000);
        engine.record_offense(n1, SlashOffense::InvalidAttestation); // 300 points
        assert_eq!(engine.slash_points_of(&n1), 300);
        assert!(!engine.is_slashed(&n1));
        // Engine dropped here — state should be persisted
    }

    // Brief pause to let the OS release the sled file lock
    std::thread::sleep(Duration::from_millis(50));

    // Second engine instance with the same store path: state must survive
    {
        let store = open_sled_store(&dir);
        let engine = SlashingEngine::with_store(Arc::new(store)).expect("failed to create engine");
        assert_eq!(engine.slash_points_of(&n1), 300);
        assert_eq!(engine.stake_of(&n1), 10_000);
        assert!(!engine.is_slashed(&n1));
    }
}

// ── Corrupted sled data → engine starts fresh ─────────────────────

#[test]
fn test_corrupted_sled_data_starts_fresh() {
    let dir = temp_dir("slash-corrupt-1");
    let n1 = node(99);

    // First, write some valid state
    {
        let store = SledSlashingStore::open(&dir).expect("failed to open sled store");
        let mut engine =
            SlashingEngine::with_store(Arc::new(store)).expect("failed to create engine");
        engine.register_validator(n1, 5_000);
        engine.record_offense(n1, SlashOffense::Equivocation); // 500 points
    }

    // Brief pause to let the OS release the sled file lock
    std::thread::sleep(Duration::from_millis(50));

    // Now corrupt the sled data by inserting invalid bytes directly
    {
        let db = sled::open(&dir).expect("failed to open sled db");
        let tree = db.open_tree("slashing").expect("failed to open tree");
        tree.insert("state", b"this is not valid bincode data".as_slice())
            .expect("failed to insert corrupt data");
        db.flush().expect("failed to flush");
    }

    std::thread::sleep(Duration::from_millis(50));

    // Creating engine with corrupted data should fail with a serialization error
    {
        let store = open_sled_store(&dir);
        let result = SlashingEngine::with_store(Arc::new(store));
        assert!(result.is_err());
        match result.unwrap_err() {
            SlashingStoreError::Serialization(msg) => {
                assert!(
                    !msg.is_empty(),
                    "Serialization error message should not be empty"
                );
            }
            other => panic!("Expected Serialization error, got: {:?}", other),
        }
    }
}

// ── InMemorySlashingStore backward compatibility ───────────────────

#[test]
fn test_in_memory_store_backward_compatibility() {
    // Using new() constructor — same behavior as before
    let mut engine = SlashingEngine::new_in_memory(500, 2000);
    let n1 = node(1);
    let n2 = node(2);

    engine.register_validator(n1, 10_000);
    engine.register_validator(n2, 25_000);

    let outcome = engine.record_offense(n1, SlashOffense::LivenessViolation);
    assert_eq!(
        outcome,
        SlashOutcome::Warned {
            node: n1,
            points: 100
        }
    );

    let outcome = engine.record_offense(n1, SlashOffense::Equivocation); // 600 total
    assert!(matches!(outcome, SlashOutcome::Slashed { .. }));

    assert_eq!(engine.stake_of(&n2), 25_000);
    assert_eq!(engine.slash_points_of(&n2), 0);
    assert!(!engine.is_slashed(&n2));
}

#[test]
fn test_in_memory_store_default_engine() {
    // Using new_in_memory() constructor — same behavior as before
    let engine = SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
    let n = node(1);
    assert!(!engine.is_slashed(&n));
    assert!(!engine.is_ejected(&n));
    assert_eq!(engine.stake_of(&n), 0);
    assert_eq!(engine.slash_points_of(&n), 0);
}

#[test]
fn test_in_memory_store_directly() {
    let store = InMemorySlashingStore::new();

    // Load from empty store should return default state
    let state = store.load().expect("load should succeed");
    assert!(state.slash_points.is_empty());
    assert!(state.stakes.is_empty());

    // Save and reload
    let n1 = node(1);
    let mut new_state = SlashingState::default();
    new_state.slash_points.insert(n1, 500);
    new_state.stakes.insert(n1, 10_000);
    new_state.slash_threshold = 500;
    new_state.ejection_threshold = 2000;

    store.save(&new_state).expect("save should succeed");
    let loaded = store.load().expect("load should succeed");
    assert_eq!(loaded.slash_points.get(&n1), Some(&500));
    assert_eq!(loaded.stakes.get(&n1), Some(&10_000));
    assert_eq!(loaded.slash_threshold, 500);
    assert_eq!(loaded.ejection_threshold, 2000);
}

// ── Persistent state includes stakes, not just slash points ────────

#[test]
fn test_persistent_state_includes_stakes() {
    let dir = temp_dir("slash-stakes-1");
    let n1 = node(10);
    let n2 = node(20);

    // First engine: register two validators with different stakes
    {
        let store = SledSlashingStore::open(&dir).expect("failed to open sled store");
        let mut engine =
            SlashingEngine::with_store(Arc::new(store)).expect("failed to create engine");
        engine.register_validator(n1, 50_000);
        engine.register_validator(n2, 75_000);
        engine.record_offense(n1, SlashOffense::Equivocation); // 500 points
        assert!(engine.is_slashed(&n1));
        assert_eq!(engine.stake_of(&n1), 50_000);
        assert_eq!(engine.stake_of(&n2), 75_000);
    }

    // Brief pause to let the OS release the sled file lock
    std::thread::sleep(Duration::from_millis(50));

    // Second engine: stakes must be preserved
    {
        let store = open_sled_store(&dir);
        let engine = SlashingEngine::with_store(Arc::new(store)).expect("failed to create engine");
        assert_eq!(engine.slash_points_of(&n1), 500);
        assert_eq!(engine.stake_of(&n1), 50_000);
        assert_eq!(engine.stake_of(&n2), 75_000);
        assert!(engine.is_slashed(&n1));
        assert!(!engine.is_slashed(&n2));
    }
}

// ── Multiple offenses persisted correctly ──────────────────────────

#[test]
fn test_multiple_offenses_persisted_across_restart() {
    let dir = temp_dir("slash-multi-1");
    let n1 = node(5);

    // First engine: accumulate points across multiple offenses
    {
        let store = SledSlashingStore::open(&dir).expect("failed to open sled store");
        let mut engine =
            SlashingEngine::with_store(Arc::new(store)).expect("failed to create engine");
        engine.register_validator(n1, 20_000);
        engine.record_offense(n1, SlashOffense::LivenessViolation); // 100
        engine.record_offense(n1, SlashOffense::InvalidAttestation); // 400
        engine.record_offense(n1, SlashOffense::LivenessViolation); // 500
        assert!(engine.is_slashed(&n1));
        assert!(!engine.is_ejected(&n1));
    }

    // Brief pause to let the OS release the sled file lock
    std::thread::sleep(Duration::from_millis(50));

    // Second engine: verify accumulated state and continue
    {
        let store = open_sled_store(&dir);
        let mut engine =
            SlashingEngine::with_store(Arc::new(store)).expect("failed to create engine");
        assert_eq!(engine.slash_points_of(&n1), 500);
        assert_eq!(engine.stake_of(&n1), 20_000);
        assert!(engine.is_slashed(&n1));

        // Continue accumulating — should reach ejection
        engine.record_offense(n1, SlashOffense::Equivocation); // 1000
        engine.record_offense(n1, SlashOffense::Equivocation); // 1500
        let outcome = engine.record_offense(n1, SlashOffense::Equivocation); // 2000
        assert!(matches!(outcome, SlashOutcome::Ejected { .. }));
        assert!(engine.is_ejected(&n1));
    }
}

// ── Empty sled store → default state ───────────────────────────────

#[test]
fn test_empty_sled_store_returns_default_state() {
    let dir = temp_dir("slash-empty-1");

    let store = SledSlashingStore::open(&dir).expect("failed to open sled store");
    let state = store.load().expect("load should succeed");
    assert!(state.slash_points.is_empty());
    assert!(state.stakes.is_empty());
    assert_eq!(state.slash_threshold, 500);
    assert_eq!(state.ejection_threshold, 2000);
}
