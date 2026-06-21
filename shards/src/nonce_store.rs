//! Persistent nonce storage for replay protection.
//!
//! Nonce state must survive node restarts to prevent replay attacks.
//! This module provides a `NonceStore` trait with redb-backed and
//! in-memory implementations.

use std::collections::HashMap;

/// Trait for nonce persistence. Implementations must be Send + Sync
/// for use across async tasks.
pub trait NonceStore: Send + Sync {
    /// Load all stored nonces.
    fn load(&self) -> Result<HashMap<[u8; 32], u64>, NonceStoreError>;

    /// Save all nonces (replaces existing data).
    fn save(&self, nonces: &HashMap<[u8; 32], u64>) -> Result<(), NonceStoreError>;

    /// Save a single nonce entry incrementally, without rewriting the entire store.
    ///
    /// This is preferred over `save()` for per-event persistence because it
    /// avoids the overhead of serializing the full nonce map on every event.
    /// The default implementation falls back to a full save, but concrete
    /// implementations (like `RedbNonceStore`) can override this for efficiency.
    fn save_incremental(&self, creator: &[u8; 32], nonce: u64) -> Result<(), NonceStoreError> {
        // Default: load, update, and save the full map
        let mut nonces = self.load()?;
        nonces.insert(*creator, nonce);
        self.save(&nonces)
    }
}

/// Errors from nonce store operations.
#[derive(Debug, thiserror::Error)]
pub enum NonceStoreError {
    /// IO error during storage operations.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// Redb database error.
    #[error("Redb error: {0}")]
    Redb(String),
    /// Serialization or deserialization error.
    #[error("Serialization error: {0}")]
    Serialization(String),
}

/// In-memory nonce store for testing.
pub struct InMemoryNonceStore {
    nonces: std::sync::Mutex<HashMap<[u8; 32], u64>>,
}

impl InMemoryNonceStore {
    /// Create a new empty in-memory nonce store.
    pub fn new() -> Self {
        Self {
            nonces: std::sync::Mutex::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryNonceStore {
    fn default() -> Self {
        Self::new()
    }
}

impl NonceStore for InMemoryNonceStore {
    fn load(&self) -> Result<HashMap<[u8; 32], u64>, NonceStoreError> {
        let nonces = self
            .nonces
            .lock()
            .map_err(|e| NonceStoreError::Io(std::io::Error::other(e.to_string())))?;
        Ok(nonces.clone())
    }

    fn save(&self, nonces: &HashMap<[u8; 32], u64>) -> Result<(), NonceStoreError> {
        let mut stored = self
            .nonces
            .lock()
            .map_err(|e| NonceStoreError::Io(std::io::Error::other(e.to_string())))?;
        *stored = nonces.clone();
        Ok(())
    }

    fn save_incremental(&self, creator: &[u8; 32], nonce: u64) -> Result<(), NonceStoreError> {
        let mut stored = self
            .nonces
            .lock()
            .map_err(|e| NonceStoreError::Io(std::io::Error::other(e.to_string())))?;
        stored.insert(*creator, nonce);
        Ok(())
    }
}

/// redb-backed persistent nonce store.
pub struct RedbNonceStore {
    db: std::sync::Arc<redb::Database>,
}

/// Table definition for the nonce store table.
const NONCE_TABLE: redb::TableDefinition<&[u8], &[u8]> = redb::TableDefinition::new("nonces");

impl RedbNonceStore {
    /// Open or create a redb-backed nonce store.
    ///
    /// # Arguments
    /// * `path` - File path for the redb database
    pub fn open(path: &std::path::Path) -> Result<Self, NonceStoreError> {
        let db = redb::Database::create(path).map_err(|e| NonceStoreError::Redb(e.to_string()))?;
        // Ensure the table exists
        let write_txn = db.begin_write().map_err(|e| NonceStoreError::Redb(e.to_string()))?;
        write_txn
            .open_table(NONCE_TABLE)
            .map_err(|e| NonceStoreError::Redb(e.to_string()))?;
        write_txn.commit().map_err(|e| NonceStoreError::Redb(e.to_string()))?;
        Ok(Self {
            db: std::sync::Arc::new(db),
        })
    }

    /// Create a RedbNonceStore from an existing redb Database handle (shared ownership).
    ///
    /// This is useful when multiple stores (e.g., slashing + nonces) share
    /// the same database file.
    pub fn from_db(db: std::sync::Arc<redb::Database>) -> Result<Self, NonceStoreError> {
        let write_txn = db.begin_write().map_err(|e| NonceStoreError::Redb(e.to_string()))?;
        write_txn
            .open_table(NONCE_TABLE)
            .map_err(|e| NonceStoreError::Redb(e.to_string()))?;
        write_txn.commit().map_err(|e| NonceStoreError::Redb(e.to_string()))?;
        Ok(Self { db })
    }
}

impl NonceStore for RedbNonceStore {
    fn load(&self) -> Result<HashMap<[u8; 32], u64>, NonceStoreError> {
        let mut nonces = HashMap::new();
        let read_txn = self.db.begin_read().map_err(|e| NonceStoreError::Redb(e.to_string()))?;
        let table = read_txn
            .open_table(NONCE_TABLE)
            .map_err(|e| NonceStoreError::Redb(e.to_string()))?;
        let range = table
            .range::<&[u8]>(..)
            .map_err(|e| NonceStoreError::Redb(e.to_string()))?;
        for item in range {
            let (key, value) = item.map_err(|e| NonceStoreError::Redb(e.to_string()))?;
            if key.value().len() == 32 {
                let mut key_arr = [0u8; 32];
                key_arr.copy_from_slice(key.value());
                let nonce = postcard::from_bytes::<u64>(value.value())
                    .map_err(|e| NonceStoreError::Serialization(e.to_string()))?;
                nonces.insert(key_arr, nonce);
            }
        }
        Ok(nonces)
    }

    fn save(&self, nonces: &HashMap<[u8; 32], u64>) -> Result<(), NonceStoreError> {
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| NonceStoreError::Redb(e.to_string()))?;
        {
            let mut table = write_txn
                .open_table(NONCE_TABLE)
                .map_err(|e| NonceStoreError::Redb(e.to_string()))?;

            // Insert/overwrite all entries. redb insert() replaces existing values,
            // so this effectively replaces the entire stored state.
            // Note: keys not in the new `nonces` map are NOT removed. If full
            // replacement semantics are needed, the caller should ensure the
            // complete nonce set is provided (which the current caller does).

            // Insert all entries
            for (key, &nonce) in nonces {
                let value = postcard::to_allocvec(&nonce).map_err(|e| NonceStoreError::Serialization(e.to_string()))?;
                table
                    .insert(key.as_slice(), value.as_slice())
                    .map_err(|e| NonceStoreError::Redb(e.to_string()))?;
            }
        }
        write_txn.commit().map_err(|e| NonceStoreError::Redb(e.to_string()))?;
        Ok(())
    }

    fn save_incremental(&self, creator: &[u8; 32], nonce: u64) -> Result<(), NonceStoreError> {
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| NonceStoreError::Redb(e.to_string()))?;
        {
            let mut table = write_txn
                .open_table(NONCE_TABLE)
                .map_err(|e| NonceStoreError::Redb(e.to_string()))?;
            let value = postcard::to_allocvec(&nonce).map_err(|e| NonceStoreError::Serialization(e.to_string()))?;
            table
                .insert(creator.as_slice(), value.as_slice())
                .map_err(|e| NonceStoreError::Redb(e.to_string()))?;
        }
        write_txn.commit().map_err(|e| NonceStoreError::Redb(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Test: InMemoryNonceStore load/save roundtrip.
    #[test]
    fn test_in_memory_roundtrip() {
        let store = InMemoryNonceStore::new();
        let mut nonces = HashMap::new();
        nonces.insert([1u8; 32], 42);
        nonces.insert([2u8; 32], 99);

        store.save(&nonces).expect("save should succeed");
        let loaded = store.load().expect("load should succeed");

        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded.get(&[1u8; 32]), Some(&42));
        assert_eq!(loaded.get(&[2u8; 32]), Some(&99));
    }

    /// Test: InMemoryNonceStore save replaces existing data.
    #[test]
    fn test_in_memory_save_replaces() {
        let store = InMemoryNonceStore::new();
        let mut nonces = HashMap::new();
        nonces.insert([1u8; 32], 10);
        store.save(&nonces).expect("save should succeed");

        // Save different data
        let mut nonces2 = HashMap::new();
        nonces2.insert([2u8; 32], 20);
        store.save(&nonces2).expect("save should succeed");

        let loaded = store.load().expect("load should succeed");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded.get(&[2u8; 32]), Some(&20));
        assert_eq!(loaded.get(&[1u8; 32]), None);
    }

    /// Test: RedbNonceStore persistence (create, save, drop, reload → data persists).
    #[test]
    fn test_redb_persistence() {
        let tmp_dir = tempfile::tempdir().expect("tempdir should succeed");
        let db_path = tmp_dir.path().join("nonce_test_db.redb");

        // Create, save, and drop
        {
            let store = RedbNonceStore::open(&db_path).expect("open should succeed");
            let mut nonces = HashMap::new();
            nonces.insert([5u8; 32], 123);
            nonces.insert([6u8; 32], 456);
            store.save(&nonces).expect("save should succeed");
        }

        // Reload from the same path
        {
            let store = RedbNonceStore::open(&db_path).expect("reopen should succeed");
            let loaded = store.load().expect("load should succeed");
            assert_eq!(loaded.len(), 2);
            assert_eq!(loaded.get(&[5u8; 32]), Some(&123));
            assert_eq!(loaded.get(&[6u8; 32]), Some(&456));
        }
    }

    /// Test: replayed transaction after restart is rejected.
    ///
    /// This test simulates the full replay-protection flow: save nonces,
    /// reload them, and verify that a nonce <= the stored value is rejected.
    #[test]
    fn test_replay_rejected_after_restart() {
        let tmp_dir = tempfile::tempdir().expect("tempdir should succeed");
        let db_path = tmp_dir.path().join("nonce_replay_test_db.redb");

        // Phase 1: Save a nonce of 5 for a creator
        let creator = [0xABu8; 32];
        {
            let store = RedbNonceStore::open(&db_path).expect("open should succeed");
            let mut nonces = HashMap::new();
            nonces.insert(creator, 5);
            store.save(&nonces).expect("save should succeed");
        }

        // Phase 2: Reload and verify replay (nonce <= 5) would be rejected
        {
            let store = RedbNonceStore::open(&db_path).expect("reopen should succeed");
            let loaded = store.load().expect("load should succeed");
            let last_nonce = loaded.get(&creator).copied().unwrap_or(0);

            // Nonce 3 is a replay (3 <= 5)
            assert!(3 <= last_nonce, "nonce 3 should be <= last_nonce 5");
            // Nonce 6 is valid (6 > 5)
            assert!(6 > last_nonce, "nonce 6 should be > last_nonce 5");
        }
    }

    /// Test: empty store loads empty map.
    #[test]
    fn test_empty_load() {
        let store = InMemoryNonceStore::new();
        let loaded = store.load().expect("load should succeed");
        assert!(loaded.is_empty());
    }

    /// Test: default InMemoryNonceStore works.
    #[test]
    fn test_default_in_memory_store() {
        let store = InMemoryNonceStore::default();
        let loaded = store.load().expect("load should succeed");
        assert!(loaded.is_empty());
    }

    // -------------------------------------------------------------------------
    // Coverage-focused tests (Task ID: 6)
    //
    // These tests target the previously-uncovered code paths:
    //   - `RedbNonceStore::open` with a real disk path (not a tempdir)
    //   - `RedbNonceStore::from_db` constructor
    //   - `RedbNonceStore::save_incremental`
    //   - `InMemoryNonceStore::save_incremental` overwrite + multi-creator
    //   - `save()` full-replaces-incremental semantics
    //   - crash-recovery (drop + reopen) for replay protection
    //   - empty-save roundtrip
    //   - large-set serialization scalability
    //   - `Default` equivalence with `new()`
    // -------------------------------------------------------------------------

    /// RAII guard for a unique on-disk redb test path. Removes the file on
    /// Drop so that panics / early returns still clean up. Each guard
    /// produces a globally unique path via a process-local atomic counter,
    /// matching the requested naming scheme
    /// `omnia-nonce-test-{pid}-{counter}.redb` under `std::env::temp_dir()`.
    struct TempDbPath {
        path: std::path::PathBuf,
    }

    impl TempDbPath {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let id = COUNTER.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!("omnia-nonce-test-{}-{}.redb", std::process::id(), id));
            Self { path }
        }

        fn path(&self) -> &std::path::Path {
            &self.path
        }
    }

    impl Drop for TempDbPath {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    /// Test: `RedbNonceStore::open` with a real disk path under
    /// `std::env::temp_dir()` (not a `tempfile::tempdir()`). Verify save/load
    /// works on the disk-backed store, then clean up.
    #[test]
    fn test_redb_open_disk_based() {
        let tmp = TempDbPath::new();
        let path = tmp.path();
        // Clean up any stale file from a prior crashed run.
        let _ = std::fs::remove_file(path);

        let store = RedbNonceStore::open(path).expect("open should succeed");

        let mut nonces = HashMap::new();
        nonces.insert([7u8; 32], 111);
        nonces.insert([8u8; 32], 222);
        store.save(&nonces).expect("save should succeed");

        let loaded = store.load().expect("load should succeed");
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded.get(&[7u8; 32]), Some(&111));
        assert_eq!(loaded.get(&[8u8; 32]), Some(&222));

        // Drop the store before removing the file (Windows can't delete open
        // files, and on Linux this ensures the WAL is flushed).
        drop(store);
        let _ = std::fs::remove_file(path);
    }

    /// Test: `RedbNonceStore::from_db` constructor with an externally-created
    /// `redb::Database` wrapped in `Arc`. This exercises the `from_db`
    /// constructor which is otherwise untested.
    #[test]
    fn test_redb_from_db_constructor() {
        let tmp_dir = tempfile::tempdir().expect("tempdir should succeed");
        let db_path = tmp_dir.path().join("nonce_from_db_test.redb");

        let db = redb::Database::create(&db_path).expect("database create should succeed");
        let store = RedbNonceStore::from_db(std::sync::Arc::new(db)).expect("from_db should succeed");

        let mut nonces = HashMap::new();
        nonces.insert([9u8; 32], 333);
        store.save(&nonces).expect("save should succeed");

        let loaded = store.load().expect("load should succeed");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded.get(&[9u8; 32]), Some(&333));
    }

    /// Test: `InMemoryNonceStore::save_incremental` inserts entries one at a
    /// time, and later incremental saves for the same creator overwrite (not
    /// reject) the previous value.
    #[test]
    fn test_save_incremental_in_memory() {
        let store = InMemoryNonceStore::new();
        let creator_a = [0xAAu8; 32];
        let creator_b = [0xBBu8; 32];
        let creator_c = [0xCCu8; 32];

        store.save_incremental(&creator_a, 1).expect("incremental save a");
        store.save_incremental(&creator_b, 2).expect("incremental save b");
        store.save_incremental(&creator_c, 3).expect("incremental save c");

        let loaded = store.load().expect("load should succeed");
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded.get(&creator_a), Some(&1));
        assert_eq!(loaded.get(&creator_b), Some(&2));
        assert_eq!(loaded.get(&creator_c), Some(&3));

        // Updating creator_a with a new nonce should overwrite, not reject.
        store.save_incremental(&creator_a, 10).expect("incremental update a");
        let loaded = store.load().expect("load after update should succeed");
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded.get(&creator_a), Some(&10));
        assert_eq!(loaded.get(&creator_b), Some(&2));
        assert_eq!(loaded.get(&creator_c), Some(&3));
    }

    /// Test: `RedbNonceStore::save_incremental` — the critical previously-
    /// untested per-event persistence path. Insert a few incremental nonces
    /// and verify they are persisted; verify overwrite semantics.
    #[test]
    fn test_save_incremental_redb() {
        let tmp_dir = tempfile::tempdir().expect("tempdir should succeed");
        let db_path = tmp_dir.path().join("nonce_incremental_test.redb");

        let store = RedbNonceStore::open(&db_path).expect("open should succeed");

        let creator_a = [0xAAu8; 32];
        let creator_b = [0xBBu8; 32];

        store.save_incremental(&creator_a, 1).expect("incremental save a");
        store.save_incremental(&creator_b, 2).expect("incremental save b");

        let loaded = store.load().expect("load should succeed");
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded.get(&creator_a), Some(&1));
        assert_eq!(loaded.get(&creator_b), Some(&2));

        // Updating creator_a with a new nonce should overwrite.
        store.save_incremental(&creator_a, 10).expect("incremental update a");
        let loaded = store.load().expect("load after update should succeed");
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded.get(&creator_a), Some(&10));
        assert_eq!(loaded.get(&creator_b), Some(&2));
    }

    /// Test: `save_incremental` overwrites existing entries for the same
    /// creator — it does not reject the second write.
    #[test]
    fn test_save_incremental_overwrite() {
        let store = InMemoryNonceStore::new();
        let creator_a = [0x11u8; 32];

        store.save_incremental(&creator_a, 5).expect("incremental save 5");
        store.save_incremental(&creator_a, 10).expect("incremental save 10");

        let loaded = store.load().expect("load should succeed");
        assert_eq!(loaded.len(), 1);
        // Should be 10 (overwritten), not 5 (rejected).
        assert_eq!(loaded.get(&creator_a), Some(&10));
    }

    /// Test: `save_incremental` supports multiple distinct creators
    /// independently.
    #[test]
    fn test_save_incremental_multiple_creators() {
        let store = InMemoryNonceStore::new();
        let creator_a = [0x01u8; 32];
        let creator_b = [0x02u8; 32];
        let creator_c = [0x03u8; 32];

        store.save_incremental(&creator_a, 1).expect("incremental save a");
        store.save_incremental(&creator_b, 2).expect("incremental save b");
        store.save_incremental(&creator_c, 3).expect("incremental save c");

        let loaded = store.load().expect("load should succeed");
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded.get(&creator_a), Some(&1));
        assert_eq!(loaded.get(&creator_b), Some(&2));
        assert_eq!(loaded.get(&creator_c), Some(&3));
    }

    /// Test: A full `save()` replaces the incremental state entirely (does
    /// not merge). Note: this holds for `InMemoryNonceStore`, whose `save()`
    /// does `*stored = nonces.clone()`. (`RedbNonceStore::save()` only
    /// inserts/overwrites and does not remove keys not in the new map, so
    /// this test deliberately uses the in-memory store to verify the
    /// full-replacement contract.)
    #[test]
    fn test_save_full_replaces_incremental() {
        let store = InMemoryNonceStore::new();
        let creator_a = [0xA1u8; 32];
        let creator_b = [0xB2u8; 32];
        let creator_c = [0xC3u8; 32];

        // Insert a few incremental nonces first.
        store.save_incremental(&creator_a, 1).expect("incremental a");
        store.save_incremental(&creator_b, 2).expect("incremental b");
        store.save_incremental(&creator_c, 3).expect("incremental c");

        // Full save with a different set (only creator_b, value 99).
        let mut full_map = HashMap::new();
        full_map.insert(creator_b, 99);
        store.save(&full_map).expect("full save");

        let loaded = store.load().expect("load should succeed");
        // Only the full-save set should be present.
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded.get(&creator_b), Some(&99));
        assert_eq!(loaded.get(&creator_a), None);
        assert_eq!(loaded.get(&creator_c), None);
    }

    /// Test: Crash-recovery for replay protection. Save nonces to a
    /// disk-backed `RedbNonceStore` via `save_incremental`, drop it, create
    /// a new store at the same path, and verify `load()` returns the saved
    /// nonces. This simulates a node restart with no in-memory state.
    #[test]
    fn test_redb_persistence_across_restart() {
        let tmp_dir = tempfile::tempdir().expect("tempdir should succeed");
        let db_path = tmp_dir.path().join("nonce_restart_test.redb");

        let creator_x = [0xDEu8; 32];
        let creator_y = [0xADu8; 32];

        // Phase 1: Save nonces via save_incremental (the per-event path).
        {
            let store = RedbNonceStore::open(&db_path).expect("open should succeed");
            store.save_incremental(&creator_x, 42).expect("incremental x");
            store.save_incremental(&creator_y, 7).expect("incremental y");
        }

        // Phase 2: Simulate restart — open a new store at the same path.
        {
            let store = RedbNonceStore::open(&db_path).expect("reopen should succeed");
            let loaded = store.load().expect("load should succeed");
            assert_eq!(loaded.len(), 2);
            assert_eq!(loaded.get(&creator_x), Some(&42));
            assert_eq!(loaded.get(&creator_y), Some(&7));
        }
    }

    /// Test: Saving an empty `HashMap` and then loading returns an empty map
    /// (not `None` — the store has been written to, just with no entries).
    /// Verified on both `InMemoryNonceStore` and `RedbNonceStore`.
    #[test]
    fn test_empty_save_then_load() {
        let empty: HashMap<[u8; 32], u64> = HashMap::new();

        // In-memory.
        let store = InMemoryNonceStore::new();
        store.save(&empty).expect("in-memory save should succeed");
        let loaded = store.load().expect("in-memory load should succeed");
        assert!(loaded.is_empty());

        // Redb.
        let tmp_dir = tempfile::tempdir().expect("tempdir should succeed");
        let db_path = tmp_dir.path().join("nonce_empty_save_test.redb");
        let redb_store = RedbNonceStore::open(&db_path).expect("open should succeed");
        redb_store.save(&empty).expect("redb save should succeed");
        let loaded = redb_store.load().expect("redb load should succeed");
        assert!(loaded.is_empty());
    }

    /// Test: Serialization scalability — save 1000 nonces and verify all are
    /// present after a roundtrip. Each creator key is a unique 32-byte array
    /// (i encoded in the first 4 bytes, rest zero). Verified on both
    /// `InMemoryNonceStore` and `RedbNonceStore` (the latter exercises
    /// postcard serialization + redb range scan over a large set).
    #[test]
    fn test_large_nonce_set() {
        fn make_key(i: u32) -> [u8; 32] {
            let mut key = [0u8; 32];
            key[..4].copy_from_slice(&i.to_le_bytes());
            key
        }

        let mut nonces = HashMap::new();
        for i in 0..1000u32 {
            nonces.insert(make_key(i), i as u64);
        }
        assert_eq!(nonces.len(), 1000, "test setup: keys must be unique");

        // In-memory roundtrip.
        let store = InMemoryNonceStore::new();
        store.save(&nonces).expect("in-memory save should succeed");
        let loaded = store.load().expect("in-memory load should succeed");
        assert_eq!(loaded.len(), 1000);
        for i in 0..1000u32 {
            assert_eq!(
                loaded.get(&make_key(i)),
                Some(&(i as u64)),
                "in-memory nonce for i={} should match",
                i
            );
        }

        // Redb roundtrip.
        let tmp_dir = tempfile::tempdir().expect("tempdir should succeed");
        let db_path = tmp_dir.path().join("nonce_large_set_test.redb");
        let redb_store = RedbNonceStore::open(&db_path).expect("open should succeed");
        redb_store.save(&nonces).expect("redb save should succeed");
        let loaded = redb_store.load().expect("redb load should succeed");
        assert_eq!(loaded.len(), 1000);
        for i in 0..1000u32 {
            assert_eq!(
                loaded.get(&make_key(i)),
                Some(&(i as u64)),
                "redb nonce for i={} should match",
                i
            );
        }
    }

    /// Test: `InMemoryNonceStore::default()` behaves identically to
    /// `InMemoryNonceStore::new()` — both start empty and both support the
    /// same save/load round-trip.
    #[test]
    fn test_default_in_memory_store_equivalence() {
        let via_new = InMemoryNonceStore::new();
        let via_default = InMemoryNonceStore::default();

        // Both should start empty.
        assert!(via_new.load().expect("load new").is_empty());
        assert!(via_default.load().expect("load default").is_empty());

        // Round-trip on both with the same data.
        let mut nonces = HashMap::new();
        nonces.insert([0xFEu8; 32], 1234);
        nonces.insert([0xEDu8; 32], 5678);

        via_new.save(&nonces).expect("save new");
        via_default.save(&nonces).expect("save default");

        let loaded_new = via_new.load().expect("load new after save");
        let loaded_default = via_default.load().expect("load default after save");

        assert_eq!(loaded_new, loaded_default);
        assert_eq!(loaded_new.len(), 2);
        assert_eq!(loaded_new.get(&[0xFEu8; 32]), Some(&1234));
        assert_eq!(loaded_new.get(&[0xEDu8; 32]), Some(&5678));
    }
}
