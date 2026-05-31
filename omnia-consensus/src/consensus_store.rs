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
}
