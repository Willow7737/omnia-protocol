//! Consensus state persistence for crash recovery.
//!
//! Provides a trait [`ConsensusStore`] and a redb-backed implementation
//! [`RedbConsensusStore`] that persists consensus engine state to disk.
//! On restart, the engine can restore its state without replaying
//! events from genesis.
//!
//! # Persistence Strategy
//!
//! Consensus state is persisted as a single snapshot containing:
//! - Current round number (computed from per-node round tracking)
//! - BLAKE3-derived round seed for deterministic hash-based leader selection
//! - Total committed events count
//! - Last finalized round number
//! - Active validator set
//! - Equivocation tracking metadata (validator → last seen sequence)
//!
//! State is saved after each round advancement to ensure crash recovery
//! can resume from the most recently committed round.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use omnia_primitives::NodeId;

/// Errors that can occur during consensus store operations.
#[derive(Error, Debug)]
pub enum ConsensusStoreError {
    /// Database I/O error.
    #[error("database error: {0}")]
    Database(String),
    /// Serialization/deserialization error.
    #[error("serialization error: {0}")]
    Serialization(String),
    /// Invalid state version.
    #[error("invalid state version: {0}")]
    InvalidVersion(u32),
}

/// Serializable snapshot of consensus engine state.
///
/// Captures the essential state needed to resume consensus after a
/// restart without replaying all events from genesis. The `version`
/// field enables forward-compatible format migrations.
///
/// # Version History
///
/// - **v1**: Initial format — round, seed, committed count, validators,
///   equivocation tracking.
/// - **v2**: Added `first_event_for_sequence` map for full equivocation
///   tracking restoration after crash recovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusState {
    /// Current consensus round number.
    pub current_round: u64,
    /// BLAKE3-derived round seed.
    pub round_seed: [u8; 32],
    /// Total committed events count.
    pub committed_events: u64,
    /// Last finalized round number.
    pub last_finalized_round: u64,
    /// Active validator set.
    pub active_validators: Vec<NodeId>,
    /// Equivocation tracking: validator → last seen sequence.
    pub equivocation_tracking: HashMap<NodeId, u64>,
    /// Full first_event_for_sequence map for equivocation detection restoration.
    /// Maps (creator, sequence) → first EventId seen for that pair.
    /// Added in v2.
    pub first_event_for_sequence: HashMap<(NodeId, u64), [u8; 32]>,
    /// State format version for forward compatibility.
    pub version: u32,
}

/// Trait for consensus state persistence backends.
///
/// Implementations store and retrieve [`ConsensusState`] snapshots,
/// enabling consensus engines to recover from crashes without full
/// genesis replay.
///
/// # Example
///
/// ```ignore
/// use omnia_consensus::consensus_store::{ConsensusStore, RedbConsensusStore};
/// use std::path::Path;
///
/// let store = RedbConsensusStore::open(Path::new("/data/consensus.redb")).unwrap();
/// if let Some(state) = store.load_state().unwrap() {
///     println!("Resuming from round {}", state.current_round);
/// }
/// ```
pub trait ConsensusStore: Send + Sync {
    /// Save the current consensus state.
    ///
    /// Persists a complete snapshot of the consensus engine state.
    /// Overwrites any previously saved state.
    ///
    /// # Errors
    ///
    /// Returns [`ConsensusStoreError`] if the state cannot be serialized
    /// or the database cannot be written to.
    fn save_state(&self, state: &ConsensusState) -> Result<(), ConsensusStoreError>;

    /// Load the last persisted consensus state (if any).
    ///
    /// Returns `Ok(None)` if no state has been persisted yet (fresh start).
    ///
    /// # Errors
    ///
    /// Returns [`ConsensusStoreError`] if the database cannot be read
    /// or the stored data cannot be deserialized.
    fn load_state(&self) -> Result<Option<ConsensusState>, ConsensusStoreError>;

    /// Save just the current round number (lightweight).
    ///
    /// Useful for quick round-tracking without a full state snapshot.
    fn save_round(&self, round: u64) -> Result<(), ConsensusStoreError>;

    /// Load the last persisted round number.
    ///
    /// Returns `0` if no round has been persisted.
    fn load_round(&self) -> Result<u64, ConsensusStoreError>;
}

/// redb-backed consensus state store.
///
/// Persists consensus state to an embedded redb database for crash
/// recovery. The database is ACID-compliant, ensuring that state
/// is never corrupted even during power failures.
///
/// # Example
///
/// ```ignore
/// use omnia_consensus::consensus_store::RedbConsensusStore;
/// use std::path::Path;
///
/// let store = RedbConsensusStore::open(Path::new("/data/consensus.redb")).unwrap();
/// store.save_round(42).unwrap();
/// assert_eq!(store.load_round().unwrap(), 42);
/// ```
pub struct RedbConsensusStore {
    db: redb::Database,
}

// redb table definitions
const CONSENSUS_STATE_TABLE: redb::TableDefinition<&str, &[u8]> = redb::TableDefinition::new("consensus_state");
const ROUND_TABLE: redb::TableDefinition<&str, u64> = redb::TableDefinition::new("consensus_round");

impl RedbConsensusStore {
    /// Open (or create) a consensus store at the given path.
    ///
    /// If the database does not exist, redb will create it. If it already
    /// exists, previously persisted state will be available via
    /// [`ConsensusStore::load_state`].
    ///
    /// # Arguments
    ///
    /// * `path` — File path for the redb database.
    ///
    /// # Errors
    ///
    /// Returns [`ConsensusStoreError::Database`] if the database cannot
    /// be opened or the tables cannot be created.
    pub fn open(path: &Path) -> Result<Self, ConsensusStoreError> {
        let db = redb::Database::create(path).map_err(|e| ConsensusStoreError::Database(e.to_string()))?;
        // Create tables if they don't exist
        let write_tx = db
            .begin_write()
            .map_err(|e| ConsensusStoreError::Database(e.to_string()))?;
        {
            let _table = write_tx
                .open_table(CONSENSUS_STATE_TABLE)
                .map_err(|e| ConsensusStoreError::Database(e.to_string()))?;
            let _table = write_tx
                .open_table(ROUND_TABLE)
                .map_err(|e| ConsensusStoreError::Database(e.to_string()))?;
        }
        write_tx
            .commit()
            .map_err(|e| ConsensusStoreError::Database(e.to_string()))?;
        Ok(Self { db })
    }

    /// Create an in-memory consensus store (for testing).
    ///
    /// State is not persisted to disk — when the process exits, all
    /// state is lost. Use [`RedbConsensusStore::open`] for production.
    pub fn in_memory() -> Result<Self, ConsensusStoreError> {
        let db = redb::Database::builder()
            .create_with_backend(redb::backends::InMemoryBackend::new())
            .map_err(|e| ConsensusStoreError::Database(e.to_string()))?;
        let write_tx = db
            .begin_write()
            .map_err(|e| ConsensusStoreError::Database(e.to_string()))?;
        {
            let _table = write_tx
                .open_table(CONSENSUS_STATE_TABLE)
                .map_err(|e| ConsensusStoreError::Database(e.to_string()))?;
            let _table = write_tx
                .open_table(ROUND_TABLE)
                .map_err(|e| ConsensusStoreError::Database(e.to_string()))?;
        }
        write_tx
            .commit()
            .map_err(|e| ConsensusStoreError::Database(e.to_string()))?;
        Ok(Self { db })
    }
}

impl ConsensusStore for RedbConsensusStore {
    fn save_state(&self, state: &ConsensusState) -> Result<(), ConsensusStoreError> {
        let serialized = postcard::to_allocvec(state).map_err(|e| ConsensusStoreError::Serialization(e.to_string()))?;

        let write_tx = self
            .db
            .begin_write()
            .map_err(|e| ConsensusStoreError::Database(e.to_string()))?;
        {
            let mut table = write_tx
                .open_table(CONSENSUS_STATE_TABLE)
                .map_err(|e| ConsensusStoreError::Database(e.to_string()))?;
            table
                .insert("current", serialized.as_slice())
                .map_err(|e| ConsensusStoreError::Database(e.to_string()))?;
        }
        write_tx
            .commit()
            .map_err(|e| ConsensusStoreError::Database(e.to_string()))?;
        Ok(())
    }

    fn load_state(&self) -> Result<Option<ConsensusState>, ConsensusStoreError> {
        let read_tx = self
            .db
            .begin_read()
            .map_err(|e| ConsensusStoreError::Database(e.to_string()))?;
        let table = read_tx
            .open_table(CONSENSUS_STATE_TABLE)
            .map_err(|e| ConsensusStoreError::Database(e.to_string()))?;

        match table.get("current") {
            Ok(Some(value)) => {
                let bytes = value.value();
                let state: ConsensusState =
                    postcard::from_bytes(bytes).map_err(|e| ConsensusStoreError::Serialization(e.to_string()))?;
                Ok(Some(state))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(ConsensusStoreError::Database(e.to_string())),
        }
    }

    fn save_round(&self, round: u64) -> Result<(), ConsensusStoreError> {
        let write_tx = self
            .db
            .begin_write()
            .map_err(|e| ConsensusStoreError::Database(e.to_string()))?;
        {
            let mut table = write_tx
                .open_table(ROUND_TABLE)
                .map_err(|e| ConsensusStoreError::Database(e.to_string()))?;
            table
                .insert("current", round)
                .map_err(|e| ConsensusStoreError::Database(e.to_string()))?;
        }
        write_tx
            .commit()
            .map_err(|e| ConsensusStoreError::Database(e.to_string()))?;
        Ok(())
    }

    fn load_round(&self) -> Result<u64, ConsensusStoreError> {
        let read_tx = self
            .db
            .begin_read()
            .map_err(|e| ConsensusStoreError::Database(e.to_string()))?;
        let table = read_tx
            .open_table(ROUND_TABLE)
            .map_err(|e| ConsensusStoreError::Database(e.to_string()))?;

        match table.get("current") {
            Ok(Some(value)) => Ok(value.value()),
            Ok(None) => Ok(0),
            Err(e) => Err(ConsensusStoreError::Database(e.to_string())),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_consensus_state_persistence_round_trip() {
        let store = RedbConsensusStore::in_memory().unwrap();
        let state = ConsensusState {
            current_round: 42,
            round_seed: [1u8; 32],
            committed_events: 1000,
            last_finalized_round: 40,
            active_validators: vec![[2u8; 32]],
            equivocation_tracking: HashMap::from([([3u8; 32], 5u64)]),
            first_event_for_sequence: HashMap::new(),
            version: 2,
        };

        store.save_state(&state).unwrap();
        let loaded = store.load_state().unwrap().unwrap();

        assert_eq!(loaded.current_round, 42);
        assert_eq!(loaded.round_seed, [1u8; 32]);
        assert_eq!(loaded.committed_events, 1000);
        assert_eq!(loaded.last_finalized_round, 40);
        assert_eq!(loaded.active_validators, vec![[2u8; 32]]);
        assert_eq!(loaded.equivocation_tracking.get(&[3u8; 32]), Some(&5u64));
        assert_eq!(loaded.version, 2);
    }

    #[test]
    fn test_consensus_state_format_version() {
        let store = RedbConsensusStore::in_memory().unwrap();
        let state = ConsensusState {
            current_round: 1,
            round_seed: [0u8; 32],
            committed_events: 0,
            last_finalized_round: 0,
            active_validators: vec![],
            equivocation_tracking: HashMap::new(),
            first_event_for_sequence: HashMap::new(),
            version: 1,
        };
        store.save_state(&state).unwrap();
        let loaded = store.load_state().unwrap().unwrap();
        assert_eq!(loaded.version, 1);
    }

    #[test]
    fn test_consensus_state_empty_store() {
        let store = RedbConsensusStore::in_memory().unwrap();
        let loaded = store.load_state().unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn test_round_persistence_round_trip() {
        let store = RedbConsensusStore::in_memory().unwrap();

        // Default is 0
        assert_eq!(store.load_round().unwrap(), 0);

        store.save_round(42).unwrap();
        assert_eq!(store.load_round().unwrap(), 42);

        store.save_round(100).unwrap();
        assert_eq!(store.load_round().unwrap(), 100);
    }

    #[test]
    fn test_state_overwrite() {
        let store = RedbConsensusStore::in_memory().unwrap();

        let state1 = ConsensusState {
            current_round: 10,
            round_seed: [1u8; 32],
            committed_events: 100,
            last_finalized_round: 8,
            active_validators: vec![],
            equivocation_tracking: HashMap::new(),
            first_event_for_sequence: HashMap::new(),
            version: 1,
        };

        let state2 = ConsensusState {
            current_round: 20,
            round_seed: [2u8; 32],
            committed_events: 200,
            last_finalized_round: 18,
            active_validators: vec![[5u8; 32]],
            equivocation_tracking: HashMap::new(),
            first_event_for_sequence: HashMap::new(),
            version: 1,
        };

        store.save_state(&state1).unwrap();
        store.save_state(&state2).unwrap();

        let loaded = store.load_state().unwrap().unwrap();
        assert_eq!(loaded.current_round, 20);
        assert_eq!(loaded.round_seed, [2u8; 32]);
        assert_eq!(loaded.committed_events, 200);
    }

    #[test]
    fn test_large_validator_set() {
        let store = RedbConsensusStore::in_memory().unwrap();
        let validators: Vec<NodeId> = (0..100)
            .map(|i| {
                let mut id = [0u8; 32];
                id[0] = i;
                id
            })
            .collect();

        let mut equivocation = HashMap::new();
        for (i, v) in validators.iter().enumerate() {
            equivocation.insert(*v, i as u64);
        }

        let state = ConsensusState {
            current_round: 500,
            round_seed: [0u8; 32],
            committed_events: 10_000,
            last_finalized_round: 490,
            active_validators: validators.clone(),
            equivocation_tracking: equivocation,
            first_event_for_sequence: HashMap::new(),
            version: 1,
        };

        store.save_state(&state).unwrap();
        let loaded = store.load_state().unwrap().unwrap();

        assert_eq!(loaded.active_validators.len(), 100);
        assert_eq!(loaded.equivocation_tracking.len(), 100);
    }

    /// RAII guard that ensures a temp file is removed when dropped, even if a
    /// test assertion panics. Used by disk-based store tests.
    struct TempFileGuard(std::path::PathBuf);

    impl TempFileGuard {
        fn new(path: std::path::PathBuf) -> Self {
            Self(path)
        }
    }

    impl Drop for TempFileGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    /// Build a unique temp file path for disk-based store tests.
    fn unique_temp_path(counter: u32) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("omnia-consensus-test-{}-{}.redb", std::process::id(), counter))
    }

    #[test]
    fn test_open_disk_based_store() {
        let path = unique_temp_path(1);
        // Ensure a clean slate in case a prior run left a file behind.
        let _ = std::fs::remove_file(&path);
        let guard = TempFileGuard::new(path.clone());

        let store = RedbConsensusStore::open(&path).unwrap();
        store.save_round(7).unwrap();
        assert_eq!(store.load_round().unwrap(), 7);

        // Full state round-trip on disk.
        let state = ConsensusState {
            current_round: 42,
            round_seed: [9u8; 32],
            committed_events: 1234,
            last_finalized_round: 40,
            active_validators: vec![[1u8; 32], [2u8; 32]],
            equivocation_tracking: HashMap::from([([1u8; 32], 3u64)]),
            first_event_for_sequence: HashMap::new(),
            version: 2,
        };
        store.save_state(&state).unwrap();
        let loaded = store.load_state().unwrap().unwrap();
        assert_eq!(loaded.current_round, 42);
        assert_eq!(loaded.round_seed, [9u8; 32]);
        assert_eq!(loaded.active_validators, vec![[1u8; 32], [2u8; 32]]);

        // Drop guard (and store) — file will be removed.
        drop(store);
        drop(guard);
        assert!(!path.exists());
    }

    #[test]
    fn test_state_persistence_across_restart() {
        // 1. In-memory store: state is lost when the store is dropped.
        let mem_state = ConsensusState {
            current_round: 5,
            round_seed: [1u8; 32],
            committed_events: 50,
            last_finalized_round: 4,
            active_validators: vec![[7u8; 32]],
            equivocation_tracking: HashMap::new(),
            first_event_for_sequence: HashMap::new(),
            version: 2,
        };
        {
            let mem_store = RedbConsensusStore::in_memory().unwrap();
            mem_store.save_state(&mem_state).unwrap();
            // Visible within the same instance.
            assert_eq!(mem_store.load_state().unwrap().unwrap().current_round, 5);
        }
        // New in-memory store: nothing persisted.
        let fresh_mem = RedbConsensusStore::in_memory().unwrap();
        assert!(fresh_mem.load_state().unwrap().is_none());

        // 2. Disk-based store: state survives a "process restart" (drop + reopen).
        let path = unique_temp_path(2);
        let _ = std::fs::remove_file(&path);
        let guard = TempFileGuard::new(path.clone());

        let disk_state = ConsensusState {
            current_round: 99,
            round_seed: [42u8; 32],
            committed_events: 999,
            last_finalized_round: 95,
            active_validators: vec![[11u8; 32], [22u8; 32], [33u8; 32]],
            equivocation_tracking: HashMap::from([([11u8; 32], 1u64), ([22u8; 32], 2u64)]),
            first_event_for_sequence: HashMap::from([(([11u8; 32], 1u64), [0xAB; 32])]),
            version: 2,
        };

        {
            let disk_store = RedbConsensusStore::open(&path).unwrap();
            disk_store.save_state(&disk_state).unwrap();
            disk_store.save_round(99).unwrap();
        }

        // Simulate crash recovery: open a brand-new store at the same path.
        let recovered = RedbConsensusStore::open(&path).unwrap();
        let loaded = recovered
            .load_state()
            .unwrap()
            .expect("state should persist across restart");
        assert_eq!(loaded.current_round, 99);
        assert_eq!(loaded.round_seed, [42u8; 32]);
        assert_eq!(loaded.committed_events, 999);
        assert_eq!(loaded.last_finalized_round, 95);
        assert_eq!(loaded.active_validators, vec![[11u8; 32], [22u8; 32], [33u8; 32]]);
        assert_eq!(loaded.equivocation_tracking.len(), 2);
        assert_eq!(loaded.equivocation_tracking.get(&[11u8; 32]), Some(&1u64));
        assert_eq!(
            loaded.first_event_for_sequence.get(&([11u8; 32], 1u64)),
            Some(&[0xAB; 32])
        );
        assert_eq!(recovered.load_round().unwrap(), 99);

        drop(recovered);
        drop(guard);
        assert!(!path.exists());
    }

    #[test]
    fn test_round_persistence_default_zero() {
        let store = RedbConsensusStore::in_memory().unwrap();
        // Fresh store: no round saved yet → default is 0.
        assert_eq!(store.load_round().unwrap(), 0);

        // Also verify on a fresh disk-based store.
        let path = unique_temp_path(3);
        let _ = std::fs::remove_file(&path);
        let guard = TempFileGuard::new(path.clone());
        {
            let disk_store = RedbConsensusStore::open(&path).unwrap();
            assert_eq!(disk_store.load_round().unwrap(), 0);
        }
        drop(guard);
    }

    #[test]
    fn test_state_with_first_event_for_sequence() {
        let store = RedbConsensusStore::in_memory().unwrap();

        let first_event_map: HashMap<(NodeId, u64), [u8; 32]> =
            HashMap::from([(([1u8; 32], 1u64), [0xAA; 32]), (([2u8; 32], 5u64), [0xBB; 32])]);

        let state = ConsensusState {
            current_round: 3,
            round_seed: [0u8; 32],
            committed_events: 0,
            last_finalized_round: 0,
            active_validators: vec![[1u8; 32], [2u8; 32]],
            equivocation_tracking: HashMap::new(),
            first_event_for_sequence: first_event_map.clone(),
            version: 2,
        };

        store.save_state(&state).unwrap();
        let loaded = store.load_state().unwrap().unwrap();

        assert_eq!(loaded.first_event_for_sequence.len(), 2);
        assert_eq!(loaded.first_event_for_sequence, first_event_map);
        // Spot-check individual entries.
        assert_eq!(
            loaded.first_event_for_sequence.get(&([1u8; 32], 1u64)),
            Some(&[0xAA; 32])
        );
        assert_eq!(
            loaded.first_event_for_sequence.get(&([2u8; 32], 5u64)),
            Some(&[0xBB; 32])
        );
    }

    #[test]
    fn test_state_with_empty_validators() {
        let store = RedbConsensusStore::in_memory().unwrap();

        let state = ConsensusState {
            current_round: 0,
            round_seed: [0u8; 32],
            committed_events: 0,
            last_finalized_round: 0,
            active_validators: Vec::new(),
            equivocation_tracking: HashMap::new(),
            first_event_for_sequence: HashMap::new(),
            version: 2,
        };

        store.save_state(&state).unwrap();
        let loaded = store.load_state().unwrap().unwrap();

        assert!(loaded.active_validators.is_empty());
        assert!(loaded.equivocation_tracking.is_empty());
        assert!(loaded.first_event_for_sequence.is_empty());
        assert_eq!(loaded.current_round, 0);
    }

    #[test]
    fn test_state_with_max_round() {
        let store = RedbConsensusStore::in_memory().unwrap();

        let state = ConsensusState {
            current_round: u64::MAX,
            round_seed: [0xFF; 32],
            committed_events: u64::MAX,
            last_finalized_round: u64::MAX - 1,
            active_validators: vec![[0xFF; 32]],
            equivocation_tracking: HashMap::from([([0xFF; 32], u64::MAX)]),
            first_event_for_sequence: HashMap::from([(([0xFF; 32], u64::MAX), [0xFF; 32])]),
            version: 2,
        };

        store.save_state(&state).unwrap();
        let loaded = store.load_state().unwrap().unwrap();

        assert_eq!(loaded.current_round, u64::MAX);
        assert_eq!(loaded.last_finalized_round, u64::MAX - 1);
        assert_eq!(loaded.committed_events, u64::MAX);
        assert_eq!(loaded.round_seed, [0xFF; 32]);
        assert_eq!(loaded.equivocation_tracking.get(&[0xFF; 32]), Some(&u64::MAX));
        assert_eq!(
            loaded.first_event_for_sequence.get(&([0xFF; 32], u64::MAX)),
            Some(&[0xFF; 32])
        );
    }

    #[test]
    fn test_round_save_load_cycle() {
        let store = RedbConsensusStore::in_memory().unwrap();

        // Start at default 0.
        assert_eq!(store.load_round().unwrap(), 0);

        // Save 0 explicitly — should still read back as 0.
        store.save_round(0).unwrap();
        assert_eq!(store.load_round().unwrap(), 0);

        // Save 1 — reads back as 1.
        store.save_round(1).unwrap();
        assert_eq!(store.load_round().unwrap(), 1);

        // Overwrite back to a smaller value — must not be sticky.
        store.save_round(0).unwrap();
        assert_eq!(store.load_round().unwrap(), 0);
    }

    #[test]
    fn test_state_overwrite_smaller() {
        let store = RedbConsensusStore::in_memory().unwrap();

        // Large initial state: 100 validators, 50 equivocation entries, 1000 committed events.
        let large_validators: Vec<NodeId> = (0..100)
            .map(|i| {
                let mut id = [0u8; 32];
                id[0] = i as u8;
                id[1] = (i >> 8) as u8;
                id
            })
            .collect();

        let mut large_equivocation = HashMap::new();
        for (i, v) in large_validators.iter().take(50).enumerate() {
            large_equivocation.insert(*v, i as u64);
        }

        let mut large_first_event = HashMap::new();
        for (i, v) in large_validators.iter().take(50).enumerate() {
            let mut event_id = [0u8; 32];
            event_id[0] = i as u8;
            large_first_event.insert((*v, i as u64), event_id);
        }

        let large_state = ConsensusState {
            current_round: 1_000,
            round_seed: [0x11; 32],
            committed_events: 1_000,
            last_finalized_round: 990,
            active_validators: large_validators.clone(),
            equivocation_tracking: large_equivocation,
            first_event_for_sequence: large_first_event,
            version: 2,
        };
        store.save_state(&large_state).unwrap();

        // Sanity-check the large state was written.
        let large_loaded = store.load_state().unwrap().unwrap();
        assert_eq!(large_loaded.active_validators.len(), 100);
        assert_eq!(large_loaded.equivocation_tracking.len(), 50);
        assert_eq!(large_loaded.first_event_for_sequence.len(), 50);

        // Now overwrite with a small state — must replace, not merge.
        let small_state = ConsensusState {
            current_round: 1,
            round_seed: [0x22; 32],
            committed_events: 0,
            last_finalized_round: 0,
            active_validators: Vec::new(),
            equivocation_tracking: HashMap::new(),
            first_event_for_sequence: HashMap::new(),
            version: 2,
        };
        store.save_state(&small_state).unwrap();

        let small_loaded = store.load_state().unwrap().unwrap();
        assert_eq!(small_loaded.current_round, 1);
        assert_eq!(small_loaded.round_seed, [0x22; 32]);
        assert_eq!(small_loaded.committed_events, 0);
        assert_eq!(small_loaded.last_finalized_round, 0);
        assert!(small_loaded.active_validators.is_empty());
        assert!(small_loaded.equivocation_tracking.is_empty());
        assert!(small_loaded.first_event_for_sequence.is_empty());
    }

    #[test]
    fn test_concurrent_save_load() {
        use std::sync::Arc;
        use std::thread;

        let store = Arc::new(RedbConsensusStore::in_memory().unwrap());

        let state = ConsensusState {
            current_round: 42,
            round_seed: [1u8; 32],
            committed_events: 100,
            last_finalized_round: 40,
            active_validators: vec![[2u8; 32]],
            equivocation_tracking: HashMap::new(),
            first_event_for_sequence: HashMap::new(),
            version: 2,
        };

        // Spawn N writer threads and N reader threads sharing the same store.
        const N: usize = 4;
        thread::scope(|s| {
            for _ in 0..N {
                let store = Arc::clone(&store);
                let state = state.clone();
                s.spawn(move || {
                    for r in 0..50 {
                        let mut st = state.clone();
                        st.current_round = r;
                        store.save_state(&st).unwrap();
                    }
                });
            }
            for _ in 0..N {
                let store = Arc::clone(&store);
                s.spawn(move || {
                    for _ in 0..50 {
                        // Should never panic — load_state must always succeed.
                        let _ = store.load_state().unwrap();
                    }
                });
            }
        });

        // Final sanity check.
        let loaded = store.load_state().unwrap().unwrap();
        assert!(loaded.current_round < 50);
    }
}
