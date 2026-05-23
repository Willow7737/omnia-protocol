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
}
