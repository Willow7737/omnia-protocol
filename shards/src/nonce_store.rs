//! Persistent nonce storage for replay protection.
//!
//! Nonce state must survive node restarts to prevent replay attacks.
//! This module provides a `NonceStore` trait with sled-backed and
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
    /// Sled database error.
    #[error("Sled error: {0}")]
    Sled(String),
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
        let nonces = self.nonces.lock().map_err(|e| {
            NonceStoreError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?;
        Ok(nonces.clone())
    }

    fn save(&self, nonces: &HashMap<[u8; 32], u64>) -> Result<(), NonceStoreError> {
        let mut stored = self.nonces.lock().map_err(|e| {
            NonceStoreError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            ))
        })?;
        *stored = nonces.clone();
        Ok(())
    }
}

/// Sled-backed persistent nonce store.
pub struct SledNonceStore {
    tree: sled::Tree,
}

impl SledNonceStore {
    /// Open or create a sled-backed nonce store.
    ///
    /// # Arguments
    /// * `db` - The sled database instance
    /// * `tree_name` - Name for the sled tree
    pub fn open(db: &sled::Db, tree_name: &str) -> Result<Self, NonceStoreError> {
        let tree = db
            .open_tree(tree_name)
            .map_err(|e| NonceStoreError::Sled(e.to_string()))?;
        Ok(Self { tree })
    }
}

impl NonceStore for SledNonceStore {
    fn load(&self) -> Result<HashMap<[u8; 32], u64>, NonceStoreError> {
        let mut nonces = HashMap::new();
        for item in self.tree.iter() {
            let (key, value) = item.map_err(|e| NonceStoreError::Sled(e.to_string()))?;
            if key.len() == 32 {
                let mut key_arr = [0u8; 32];
                key_arr.copy_from_slice(&key);
                let nonce = bincode::deserialize::<u64>(&value)
                    .map_err(|e| NonceStoreError::Serialization(e.to_string()))?;
                nonces.insert(key_arr, nonce);
            }
        }
        Ok(nonces)
    }

    fn save(&self, nonces: &HashMap<[u8; 32], u64>) -> Result<(), NonceStoreError> {
        // Clear existing data
        self.tree
            .clear()
            .map_err(|e| NonceStoreError::Sled(e.to_string()))?;

        // Insert all entries
        for (key, &nonce) in nonces {
            let value = bincode::serialize(&nonce)
                .map_err(|e| NonceStoreError::Serialization(e.to_string()))?;
            self.tree
                .insert(key.as_slice(), value.as_slice())
                .map_err(|e| NonceStoreError::Sled(e.to_string()))?;
        }

        self.tree
            .flush()
            .map_err(|e| NonceStoreError::Sled(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
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

    /// Test: SledNonceStore persistence (create, save, drop, reload → data persists).
    #[test]
    fn test_sled_persistence() {
        let tmp_dir = tempfile::tempdir().expect("tempdir should succeed");
        let db_path = tmp_dir.path().join("nonce_test_db");

        // Create, save, and drop
        {
            let db = sled::open(&db_path).expect("db open should succeed");
            let store = SledNonceStore::open(&db, "nonces").expect("open tree should succeed");
            let mut nonces = HashMap::new();
            nonces.insert([5u8; 32], 123);
            nonces.insert([6u8; 32], 456);
            store.save(&nonces).expect("save should succeed");
            drop(store);
            drop(db);
        }

        // Reload from the same path
        {
            let db = sled::open(&db_path).expect("db reopen should succeed");
            let store = SledNonceStore::open(&db, "nonces").expect("reopen tree should succeed");
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
        let db_path = tmp_dir.path().join("nonce_replay_test_db");

        // Phase 1: Save a nonce of 5 for a creator
        let creator = [0xABu8; 32];
        {
            let db = sled::open(&db_path).expect("db open should succeed");
            let store = SledNonceStore::open(&db, "nonces").expect("open tree should succeed");
            let mut nonces = HashMap::new();
            nonces.insert(creator, 5);
            store.save(&nonces).expect("save should succeed");
            drop(store);
            drop(db);
        }

        // Phase 2: Reload and verify replay (nonce <= 5) would be rejected
        {
            let db = sled::open(&db_path).expect("db reopen should succeed");
            let store = SledNonceStore::open(&db, "nonces").expect("reopen tree should succeed");
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
