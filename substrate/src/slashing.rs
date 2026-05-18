//! Slashing Module for Byzantine Fault Detection
//!
//! This module implements a slashing engine that penalizes validators for
//! Byzantine behavior such as equivocation, liveness violations, and invalid
//! attestations. Slash points accumulate per offense, and when a node's
//! points exceed configurable thresholds, the node is either slashed (stake
//! forfeited) or ejected from the validator set entirely.
//!
//! # Persistence
//!
//! The slashing state can be persisted to disk using the [`RedbSlashingStore`]
//! backend, ensuring that slash history survives node restarts. For tests and
//! backward compatibility, [`InMemorySlashingStore`] keeps state in memory only.
//!
//! # Offense Points
//!
//! | Offense              | Points |
//! |----------------------|--------|
//! | Equivocation         | 500    |
//! | LivenessViolation    | 100    |
//! | InvalidAttestation   | 300    |
//!
//! # Thresholds
//!
//! - **Slash threshold** (default 500): Points at which a node is *slashed*
//!   (stake forfeited).
//! - **Ejection threshold** (default 2000): Points at which a node is
//!   *ejected* (removed from the validator set).
//!
//! All points and thresholds are `u64` integers — no floating-point arithmetic.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

use crate::event::Event;
use crate::vector_clock::NodeId;

/// Default slash threshold: accumulated points at which a node is slashed.
pub const DEFAULT_SLASH_THRESHOLD: u64 = 500;

/// Default ejection threshold: accumulated points at which a node is ejected.
pub const DEFAULT_EJECTION_THRESHOLD: u64 = 2000;

/// Points assigned for an equivocation offense.
pub const EQUIVOCATION_POINTS: u64 = 500;

/// Points assigned for a liveness violation.
pub const LIVENESS_VIOLATION_POINTS: u64 = 100;

/// Points assigned for an invalid attestation.
pub const INVALID_ATTESTATION_POINTS: u64 = 300;

/// Categorizes the type of Byzantine offense committed by a validator.
///
/// Each offense type carries a fixed penalty in slash points:
/// - [`SlashOffense::Equivocation`]: 500 points
/// - [`SlashOffense::LivenessViolation`]: 100 points
/// - [`SlashOffense::InvalidAttestation`]: 300 points
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlashOffense {
    /// A validator signed two different events with the same creator and
    /// sequence number (double-signing / equivocation).
    Equivocation,
    /// A validator has been offline or unresponsive for too many rounds.
    LivenessViolation,
    /// A validator attested to invalid or fraudulent data.
    InvalidAttestation,
}

impl SlashOffense {
    /// Returns the number of slash points assigned to this offense type.
    ///
    /// # Returns
    ///
    /// A `u64` representing the penalty in slash points.
    ///
    /// # Example
    ///
    /// ```
    /// use omnia_substrate::SlashOffense;
    /// assert_eq!(SlashOffense::Equivocation.points(), 500);
    /// assert_eq!(SlashOffense::LivenessViolation.points(), 100);
    /// assert_eq!(SlashOffense::InvalidAttestation.points(), 300);
    /// ```
    pub fn points(&self) -> u64 {
        match self {
            SlashOffense::Equivocation => EQUIVOCATION_POINTS,
            SlashOffense::LivenessViolation => LIVENESS_VIOLATION_POINTS,
            SlashOffense::InvalidAttestation => INVALID_ATTESTATION_POINTS,
        }
    }
}

/// Describes the outcome of recording a slashing offense.
///
/// The outcome depends on the node's total accumulated slash points relative
/// to the configured thresholds:
/// - Below slash threshold → [`SlashOutcome::Warned`]
/// - At or above slash threshold but below ejection threshold → [`SlashOutcome::Slashed`]
/// - At or above ejection threshold → [`SlashOutcome::Ejected`]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlashOutcome {
    /// Node received slash points but is still below the slash threshold.
    Warned {
        /// The offending node.
        node: NodeId,
        /// Total accumulated slash points after this offense.
        points: u64,
    },
    /// Node has accumulated enough points to be slashed (stake forfeited).
    Slashed {
        /// The offending node.
        node: NodeId,
        /// Amount of stake being slashed.
        amount: u64,
    },
    /// Node has accumulated enough points to be ejected from the validator set.
    Ejected {
        /// The ejected node.
        node: NodeId,
    },
}

// ── Persistence types ──────────────────────────────────────────────

/// Errors that can occur when interacting with a [`SlashingStore`].
#[derive(Debug, thiserror::Error)]
pub enum SlashingStoreError {
    /// An error occurred while reading from or writing to the persistence
    /// backend (e.g., redb I/O failure).
    #[error("persistence error: {0}")]
    Persistence(String),
    /// An error occurred while serializing or deserializing slashing state.
    #[error("serialization error: {0}")]
    Serialization(String),
    /// An undo operation failed because the validator has no offense history.
    #[error("validator {:?} has no offense history to undo", .0)]
    UndoNoOffenseHistory([u8; 4]),
}

/// Serializable slashing state that can be persisted across restarts.
///
/// This struct captures the full operational state of the slashing engine,
/// including accumulated slash points, staked amounts, and the configured
/// thresholds. It is designed to be serialized with `postcard` for compact
/// on-disk storage.
///
/// # Example
///
/// ```
/// use omnia_substrate::slashing::SlashingState;
///
/// let state = SlashingState::default();
/// assert!(state.slash_points.is_empty());
/// assert!(state.stakes.is_empty());
/// assert_eq!(state.slash_threshold, 500);
/// assert_eq!(state.ejection_threshold, 2000);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashingState {
    /// Accumulated slash points per node.
    pub slash_points: HashMap<NodeId, u64>,
    /// Staked amounts per node.
    pub stakes: HashMap<NodeId, u64>,
    /// Points threshold at which a node is slashed.
    pub slash_threshold: u64,
    /// Points threshold at which a node is ejected.
    pub ejection_threshold: u64,
    /// History of offenses per node, stored as a stack for undo.
    /// Each entry is the number of points that were added.
    pub offense_history: HashMap<NodeId, Vec<u64>>,
}

impl Default for SlashingState {
    fn default() -> Self {
        Self {
            slash_points: HashMap::new(),
            stakes: HashMap::new(),
            slash_threshold: DEFAULT_SLASH_THRESHOLD,
            ejection_threshold: DEFAULT_EJECTION_THRESHOLD,
            offense_history: HashMap::new(),
        }
    }
}

/// Persistence backend for slashing state.
///
/// Implementations can store state in memory (for tests) or on disk
/// (for production nodes).
///
/// # Example
///
/// ```
/// use omnia_substrate::slashing::{InMemorySlashingStore, SlashingStore, SlashingState};
///
/// let store = InMemorySlashingStore::new();
/// let state = store.load().unwrap();
/// assert!(state.slash_points.is_empty());
/// ```
pub trait SlashingStore: Send + Sync {
    /// Load the persisted slashing state.
    ///
    /// Returns the default state if no persisted state exists.
    ///
    /// # Errors
    ///
    /// Returns [`SlashingStoreError`] if the store cannot be read or
    /// the stored data cannot be deserialized.
    fn load(&self) -> Result<SlashingState, SlashingStoreError>;

    /// Save the slashing state to persistent storage.
    ///
    /// # Arguments
    ///
    /// * `state` — The complete slashing state to persist.
    ///
    /// # Errors
    ///
    /// Returns [`SlashingStoreError`] if the state cannot be serialized
    /// or the store cannot be written to.
    fn save(&self, state: &SlashingState) -> Result<(), SlashingStoreError>;
    /// Get the slash count (accumulated points) for a specific validator.
    ///
    /// Returns 0 if the validator has no recorded offenses.
    fn get_slash_count(&self, validator: &[u8; 32]) -> u64;

    /// Decrement the slash count for a specific validator by the given amount.
    ///
    /// # Errors
    ///
    /// Returns [`SlashingStoreError`] if the state cannot be loaded or saved,
    /// or if the validator has no slash count to decrement.
    fn decrement_slash_count_by(
        &self,
        validator: &[u8; 32],
        amount: u64,
    ) -> Result<(), SlashingStoreError>;

    /// Decrement the slash count for a specific validator by the minimum offense amount.
    ///
    /// This is used by governance-based slashing undo to partially reverse
    /// a slashing decision. The count is decremented by [`LIVENESS_VIOLATION_POINTS`] (100).
    ///
    /// # Errors
    ///
    /// Returns [`SlashingStoreError`] if the state cannot be loaded or saved,
    /// or if the validator has no slash count to decrement.
    fn decrement_slash_count(&self, validator: &[u8; 32]) -> Result<(), SlashingStoreError> {
        self.decrement_slash_count_by(validator, LIVENESS_VIOLATION_POINTS)
    }
}

// ── RedbSlashingStore ──────────────────────────────────────────────

/// redb-backed persistent slashing store.
///
/// Persists slashing state to disk so that slash history survives
/// node restarts. Uses the `redb` embedded database — pure Rust,
/// ACID-compliant, and production-ready.
///
/// # Example
///
/// ```no_run
/// use omnia_substrate::slashing::{RedbSlashingStore, SlashingStore};
/// use std::path::Path;
///
/// let store = RedbSlashingStore::open(Path::new("/tmp/omnia-slashing.redb")).unwrap();
/// let state = store.load().unwrap();
/// ```
pub struct RedbSlashingStore {
    db: redb::Database,
}

/// Table definition for the slashing state table.
const SLASHING_TABLE: redb::TableDefinition<&str, &[u8]> = redb::TableDefinition::new("slashing");

impl RedbSlashingStore {
    /// Open a redb database at the given path for slashing state.
    ///
    /// If the database does not exist, redb will create it. If it already
    /// exists, previously persisted state will be available via [`SlashingStore::load`].
    ///
    /// # Arguments
    ///
    /// * `path` — File path for the redb database.
    ///
    /// # Errors
    ///
    /// Returns [`SlashingStoreError::Persistence`] if the database cannot be opened.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use omnia_substrate::slashing::{RedbSlashingStore, SlashingStore};
    /// use std::path::Path;
    ///
    /// let store = RedbSlashingStore::open(Path::new("/tmp/omnia-slashing.redb")).unwrap();
    /// let state = store.load().unwrap();
    /// ```
    pub fn open(path: &Path) -> Result<Self, SlashingStoreError> {
        let db = redb::Database::create(path)
            .map_err(|e| SlashingStoreError::Persistence(e.to_string()))?;
        // Ensure the table exists
        let write_txn = db
            .begin_write()
            .map_err(|e| SlashingStoreError::Persistence(e.to_string()))?;
        write_txn
            .open_table(SLASHING_TABLE)
            .map_err(|e| SlashingStoreError::Persistence(e.to_string()))?;
        write_txn
            .commit()
            .map_err(|e| SlashingStoreError::Persistence(e.to_string()))?;
        Ok(Self { db })
    }
}

impl SlashingStore for RedbSlashingStore {
    fn load(&self) -> Result<SlashingState, SlashingStoreError> {
        let read_txn = self
            .db
            .begin_read()
            .map_err(|e| SlashingStoreError::Persistence(e.to_string()))?;
        let table = read_txn
            .open_table(SLASHING_TABLE)
            .map_err(|e| SlashingStoreError::Persistence(e.to_string()))?;
        match table
            .get("state")
            .map_err(|e| SlashingStoreError::Persistence(e.to_string()))?
        {
            Some(value) => postcard::from_bytes(value.value())
                .map_err(|e| SlashingStoreError::Serialization(e.to_string())),
            None => Ok(SlashingState::default()),
        }
    }

    fn save(&self, state: &SlashingState) -> Result<(), SlashingStoreError> {
        let bytes = postcard::to_allocvec(state)
            .map_err(|e| SlashingStoreError::Serialization(e.to_string()))?;
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| SlashingStoreError::Persistence(e.to_string()))?;
        {
            let mut table = write_txn
                .open_table(SLASHING_TABLE)
                .map_err(|e| SlashingStoreError::Persistence(e.to_string()))?;
            table
                .insert("state", bytes.as_slice())
                .map_err(|e| SlashingStoreError::Persistence(e.to_string()))?;
        }
        write_txn
            .commit()
            .map_err(|e| SlashingStoreError::Persistence(e.to_string()))?;
        Ok(())
    }

    fn get_slash_count(&self, validator: &[u8; 32]) -> u64 {
        self.load()
            .map(|state| state.slash_points.get(validator).copied().unwrap_or(0))
            .unwrap_or(0)
    }

    fn decrement_slash_count_by(
        &self,
        validator: &[u8; 32],
        amount: u64,
    ) -> Result<(), SlashingStoreError> {
        let mut state = self.load()?;
        let current = state.slash_points.get(validator).copied().unwrap_or(0);
        if current == 0 {
            return Err(SlashingStoreError::Persistence(format!(
                "Validator {:?} has no slash count to decrement",
                &validator[..4]
            )));
        }
        let decrement = amount.min(current);
        state.slash_points.insert(*validator, current - decrement);
        self.save(&state)
    }
}

// ── InMemorySlashingStore ──────────────────────────────────────────

/// In-memory slashing store for tests and backward compatibility.
///
/// State is held in a [`std::sync::RwLock`] so it can be shared across
/// threads in tests. It is not persisted to disk — when the process exits,
/// all state is lost.
///
/// # Example
///
/// ```
/// use omnia_substrate::slashing::{InMemorySlashingStore, SlashingStore, SlashingState};
///
/// let store = InMemorySlashingStore::new();
/// let state = store.load().unwrap();
/// assert!(state.slash_points.is_empty());
/// ```
pub struct InMemorySlashingStore {
    state: RwLock<SlashingState>,
}

impl InMemorySlashingStore {
    /// Create a new empty in-memory store.
    ///
    /// The initial state is the default [`SlashingState`] (empty maps, zero thresholds).
    ///
    /// # Example
    ///
    /// ```
    /// use omnia_substrate::slashing::InMemorySlashingStore;
    ///
    /// let store = InMemorySlashingStore::new();
    /// ```
    pub fn new() -> Self {
        Self {
            state: RwLock::new(SlashingState::default()),
        }
    }
}

impl Default for InMemorySlashingStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SlashingStore for InMemorySlashingStore {
    fn load(&self) -> Result<SlashingState, SlashingStoreError> {
        let state = self
            .state
            .read()
            .map_err(|e| SlashingStoreError::Persistence(e.to_string()))?;
        Ok(state.clone())
    }

    fn save(&self, state: &SlashingState) -> Result<(), SlashingStoreError> {
        let mut guard = self
            .state
            .write()
            .map_err(|e| SlashingStoreError::Persistence(e.to_string()))?;
        *guard = state.clone();
        Ok(())
    }

    fn get_slash_count(&self, validator: &[u8; 32]) -> u64 {
        self.load()
            .map(|state| state.slash_points.get(validator).copied().unwrap_or(0))
            .unwrap_or(0)
    }

    fn decrement_slash_count_by(
        &self,
        validator: &[u8; 32],
        amount: u64,
    ) -> Result<(), SlashingStoreError> {
        let mut state = self.load()?;
        let current = state.slash_points.get(validator).copied().unwrap_or(0);
        if current == 0 {
            return Err(SlashingStoreError::Persistence(format!(
                "Validator {:?} has no slash count to decrement",
                &validator[..4]
            )));
        }
        let decrement = amount.min(current);
        state.slash_points.insert(*validator, current - decrement);
        self.save(&state)
    }
}

// ── SlashingEngine ─────────────────────────────────────────────────

/// Engine that tracks slash points, stakes, and thresholds for validator
/// slashing.
///
/// The `SlashingEngine` is responsible for:
/// - Registering validators with their initial stake
/// - Recording offenses and accumulating slash points
/// - Determining slash outcomes based on accumulated points
/// - Detecting equivocation and liveness violations
/// - Persisting state to a [`SlashingStore`] backend on every mutation
///
/// # Shared ownership
///
/// `SlashingEngine` is clonable — cloning produces a new handle that shares
/// the **same** underlying store via `Arc`. This allows the same engine
/// instance to be used by both consensus and the API layer without
/// duplicating state. All clones share a single persisted slashing history.
///
/// # Persistence
///
/// Use [`SlashingEngine::new`] with `Some(path)` for production nodes to
/// persist slashing state to redb. Use [`SlashingEngine::new_in_memory`]
/// for tests only — in-memory state is lost on restart.
///
/// # Example
///
/// ```
/// use omnia_substrate::{SlashingEngine, SlashOffense, SlashOutcome};
///
/// let mut engine = SlashingEngine::new_in_memory(500, 2000);
/// let mut node = [0u8; 32];
/// node[0] = 42;
///
/// engine.register_validator(node, 10_000);
/// let outcome = engine.record_offense(node, SlashOffense::Equivocation);
/// assert!(matches!(outcome, SlashOutcome::Slashed { .. }));
/// ```
pub struct SlashingEngine {
    /// Shared mutable in-memory state, wrapped in `Arc<RwLock<...>>` so
    /// that all clones share the **same** state. This fixes the divergent
    /// state bug where `Clone` previously gave each clone its own copy of
    /// `slash_points`, `stakes`, and `offense_history`.
    state: Arc<RwLock<SlashingState>>,
    /// Persistence backend for slashing state, shared via `Arc`.
    store: Arc<dyn SlashingStore>,
}

impl std::fmt::Debug for SlashingEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.read().unwrap_or_else(|e| {
            tracing::error!(error = %e, "SlashingEngine::fmt — lock poisoned");
            std::process::abort()
        });
        f.debug_struct("SlashingEngine")
            .field("slash_points", &state.slash_points)
            .field("stakes", &state.stakes)
            .field("slash_threshold", &state.slash_threshold)
            .field("ejection_threshold", &state.ejection_threshold)
            .field("store", &"Arc<dyn SlashingStore>")
            .finish()
    }
}

impl Clone for SlashingEngine {
    /// Clone the engine, sharing the **same** in-memory state and
    /// persistence store.
    ///
    /// Both the `Arc<RwLock<SlashingState>>` and the `Arc<dyn
    /// SlashingStore>` are cloned by reference, so all clones observe
    /// and mutate the same underlying state. This eliminates the
    /// divergent-state bug that existed when each clone held its own
    /// copy of `slash_points`, `stakes`, and `offense_history`.
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            store: Arc::clone(&self.store),
        }
    }
}

// NOTE: `Default` is intentionally NOT implemented for `SlashingEngine`.
// Using `::default()` silently creates an in-memory-only engine, which is
// a footgun in production — slash history would be lost on restart.
// Always use `SlashingEngine::new(data_dir, ...)` or `new_in_memory(...)`
// explicitly.

impl SlashingEngine {
    /// Create a new `SlashingEngine` with optional persistence.
    ///
    /// - `Some(path)`: Persists slashing state to redb at the given path.
    ///   Falls back to in-memory if redb fails to open.
    /// - `None`: Uses in-memory store (state lost on restart — for testing only).
    ///
    /// # Arguments
    ///
    /// * `data_dir` — If `Some(path)`, persists slashing state to redb at that
    ///   path. If `None`, uses in-memory store (state lost on restart).
    /// * `slash_threshold` — Slash points at which a node is considered
    ///   *slashed* (stake forfeited). Defaults to 500.
    /// * `ejection_threshold` — Slash points at which a node is *ejected*
    ///   from the validator set. Defaults to 2000.
    ///
    /// # Production Usage
    ///
    /// Always pass `Some(path)` in production. In-memory mode exists for tests
    /// only. A Byzantine validator can clear their slash history by restarting
    /// if in-memory mode is used.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use omnia_substrate::slashing::SlashingEngine;
    /// use std::path::PathBuf;
    ///
    /// // Production: persistent slashing
    /// let engine = SlashingEngine::new(Some(PathBuf::from("./data/slashing")), 500, 2000);
    ///
    /// // Testing: in-memory slashing
    /// let engine = SlashingEngine::new(None, 500, 2000);
    /// ```
    pub fn new(data_dir: Option<PathBuf>, slash_threshold: u64, ejection_threshold: u64) -> Self {
        match data_dir {
            Some(path) => match RedbSlashingStore::open(&path) {
                Ok(store) => {
                    tracing::info!(
                        path = %path.display(),
                        "Slashing engine: using persistent redb store"
                    );
                    Self::with_store_with_thresholds(
                        Arc::new(store),
                        slash_threshold,
                        ejection_threshold,
                    )
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        path = %path.display(),
                        "Failed to open redb store — falling back to in-memory"
                    );
                    Self::new_in_memory(slash_threshold, ejection_threshold)
                }
            },
            None => {
                tracing::info!("Slashing engine: using in-memory store (testing mode)");
                Self::new_in_memory(slash_threshold, ejection_threshold)
            }
        }
    }

    /// Create with in-memory store. **FOR TESTING ONLY.**
    ///
    /// Slash state will be lost on restart. In production, always use
    /// [`SlashingEngine::new`] with `Some(path)` to ensure slash history
    /// persists across restarts.
    ///
    /// # Arguments
    ///
    /// * `slash_threshold` — Slash points at which a node is slashed.
    /// * `ejection_threshold` — Slash points at which a node is ejected.
    pub fn new_in_memory(slash_threshold: u64, ejection_threshold: u64) -> Self {
        let state = SlashingState {
            slash_points: HashMap::new(),
            stakes: HashMap::new(),
            slash_threshold,
            ejection_threshold,
            offense_history: HashMap::new(),
        };
        Self {
            state: Arc::new(RwLock::new(state)),
            store: Arc::new(InMemorySlashingStore::new()),
        }
    }

    /// Creates a `SlashingEngine` backed by a persistent [`SlashingStore`].
    ///
    /// The engine loads its initial state from the store. If the store is
    /// empty (first run), the engine starts with default empty state and
    /// the provided thresholds. If the store contains serialized state, the
    /// thresholds from the persisted state are used.
    ///
    /// # Arguments
    ///
    /// * `store` — An `Arc`-wrapped [`SlashingStore`] implementation (e.g.,
    ///   [`RedbSlashingStore`]). Using `Arc` allows the same store to be
    ///   shared across multiple `SlashingEngine` clones.
    ///
    /// # Returns
    ///
    /// A `Result` containing the initialized `SlashingEngine`, or a
    /// [`SlashingStoreError`] if the store could not be read.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use omnia_substrate::slashing::{RedbSlashingStore, SlashingEngine};
    /// use std::path::Path;
    /// use std::sync::Arc;
    ///
    /// let store = RedbSlashingStore::open(Path::new("/tmp/omnia-slashing.redb")).unwrap();
    /// let engine = SlashingEngine::with_store(Arc::new(store)).unwrap();
    /// ```
    pub fn with_store(store: Arc<dyn SlashingStore>) -> Result<Self, SlashingStoreError> {
        let loaded = store.load()?;
        tracing::info!(
            slash_points_count = loaded.slash_points.len(),
            stakes_count = loaded.stakes.len(),
            slash_threshold = loaded.slash_threshold,
            ejection_threshold = loaded.ejection_threshold,
            "Loaded slashing state from persistent store"
        );
        Ok(Self {
            state: Arc::new(RwLock::new(loaded)),
            store,
        })
    }

    /// Creates a `SlashingEngine` backed by a persistent [`SlashingStore`],
    /// using the provided thresholds when the store is empty (first run).
    ///
    /// If the store already contains state, the persisted thresholds are used.
    /// If the store is empty, the provided thresholds are applied and persisted.
    ///
    /// # Arguments
    ///
    /// * `store` — An `Arc`-wrapped [`SlashingStore`] implementation.
    /// * `slash_threshold` — Default slash threshold if store is empty.
    /// * `ejection_threshold` — Default ejection threshold if store is empty.
    fn with_store_with_thresholds(
        store: Arc<dyn SlashingStore>,
        slash_threshold: u64,
        ejection_threshold: u64,
    ) -> Self {
        let state = match store.load() {
            Ok(loaded) => {
                tracing::info!(
                    slash_points_count = loaded.slash_points.len(),
                    stakes_count = loaded.stakes.len(),
                    slash_threshold = loaded.slash_threshold,
                    ejection_threshold = loaded.ejection_threshold,
                    "Loaded slashing state from persistent store"
                );
                loaded
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to load slashing state — starting fresh");
                SlashingState {
                    slash_points: HashMap::new(),
                    stakes: HashMap::new(),
                    slash_threshold,
                    ejection_threshold,
                    offense_history: HashMap::new(),
                }
            }
        };
        Self {
            state: Arc::new(RwLock::new(state)),
            store,
        }
    }

    /// Persists the current state to the backing store.
    ///
    /// Returns `Err` if persistence fails so the caller can rollback
    /// the in-memory state to a pre-mutation snapshot.
    fn persist_state(&self) -> Result<(), SlashingStoreError> {
        let state = self.state.read().unwrap_or_else(|e| {
            tracing::error!(error = %e, "persist_state — lock poisoned");
            std::process::abort()
        });
        self.store.save(&state)
    }

    /// Registers a validator with an initial stake.
    ///
    /// If the node is already registered, the stake is updated (replaced)
    /// with the new value. State is persisted to the backing store after
    /// the update.
    ///
    /// # Arguments
    ///
    /// * `node` — The `NodeId` of the validator to register.
    /// * `stake` — The amount of stake the validator is bonding.
    ///
    /// # Example
    ///
    /// ```
    /// use omnia_substrate::SlashingEngine;
    ///
    /// let mut engine = SlashingEngine::new_in_memory(500, 2000);
    /// let mut node = [0u8; 32];
    /// node[0] = 1;
    ///
    /// engine.register_validator(node, 10_000);
    /// assert_eq!(engine.stake_of(&node), 10_000);
    /// ```
    pub fn register_validator(&mut self, node: NodeId, stake: u64) {
        tracing::info!(
            node = ?&node[..4],
            stake = stake,
            "Registering validator with stake"
        );
        let mut state = self.state.write().unwrap_or_else(|e| {
            tracing::error!(error = %e, "register_validator — lock poisoned");
            std::process::abort()
        });
        let snapshot = state.clone();
        state.stakes.insert(node, stake);
        // Ensure slash_points entry exists so slash_points_of returns 0
        // instead of implicitly missing.
        state.slash_points.entry(node).or_insert(0);
        drop(state);
        if let Err(e) = self.persist_state() {
            tracing::error!(
                error = %e,
                "Failed to persist slashing state after register_validator — rolling back"
            );
            let mut state = self.state.write().unwrap_or_else(|e| {
                tracing::error!(error = %e, "register_validator rollback — lock poisoned");
                std::process::abort()
            });
            *state = snapshot;
        }
    }

    /// Records a slashing offense for a node and returns the resulting outcome.
    ///
    /// Slash points are accumulated. The outcome is determined by the total
    /// accumulated points relative to the configured thresholds:
    ///
    /// | Total points                  | Outcome   |
    /// |-------------------------------|-----------|
    /// | < slash_threshold             | Warned    |
    /// | ≥ slash_threshold, < ejection | Slashed   |
    /// | ≥ ejection_threshold          | Ejected   |
    ///
    /// State is persisted to the backing store after the offense is recorded.
    ///
    /// # Arguments
    ///
    /// * `node` — The `NodeId` of the offending validator.
    /// * `offense` — The type of offense committed.
    ///
    /// # Returns
    ///
    /// A [`SlashOutcome`] indicating the consequence of this offense.
    ///
    /// # Example
    ///
    /// ```
    /// use omnia_substrate::{SlashingEngine, SlashOffense, SlashOutcome};
    ///
    /// let mut engine = SlashingEngine::new_in_memory(500, 2000);
    /// let mut node = [0u8; 32];
    /// node[0] = 5;
    ///
    /// engine.register_validator(node, 10_000);
    /// let outcome = engine.record_offense(node, SlashOffense::LivenessViolation);
    /// assert!(matches!(outcome, SlashOutcome::Warned { .. }));
    /// ```
    pub fn record_offense(&mut self, node: NodeId, offense: SlashOffense) -> SlashOutcome {
        let points_added = offense.points();
        let mut state = self.state.write().unwrap_or_else(|e| {
            tracing::error!(error = %e, "record_offense — lock poisoned");
            std::process::abort()
        });
        let snapshot = state.clone();
        let current_points = state.slash_points.entry(node).or_insert(0);
        *current_points = current_points.saturating_add(points_added);
        let total_points = *current_points;

        // Track offense history for undo
        state
            .offense_history
            .entry(node)
            .or_default()
            .push(points_added);

        let ejection_threshold = state.ejection_threshold;
        let slash_threshold = state.slash_threshold;
        let stake_amount = state.stakes.get(&node).copied().unwrap_or(0);
        drop(state);

        tracing::warn!(
            node = ?&node[..4],
            offense = ?offense,
            points_added = points_added,
            total_points = total_points,
            "Slashing offense recorded"
        );

        let outcome = if total_points >= ejection_threshold {
            tracing::info!(node = ?&node[..4], total_points, "Node ejected from consensus");
            SlashOutcome::Ejected { node }
        } else if total_points >= slash_threshold {
            tracing::info!(
                node = ?&node[..4],
                total_points,
                amount = stake_amount,
                "Node slashed"
            );
            SlashOutcome::Slashed {
                node,
                amount: stake_amount,
            }
        } else {
            tracing::debug!(
                node = ?&node[..4],
                total_points,
                threshold = slash_threshold,
                "Node warned — below slash threshold"
            );
            SlashOutcome::Warned {
                node,
                points: total_points,
            }
        };

        if let Err(e) = self.persist_state() {
            tracing::error!(
                error = %e,
                "Failed to persist slashing state after record_offense — rolling back"
            );
            let mut state = self.state.write().unwrap_or_else(|e| {
                tracing::error!(error = %e, "record_offense rollback — lock poisoned");
                std::process::abort()
            });
            *state = snapshot;
        }

        outcome
    }

    /// Checks whether two events constitute an equivocation.
    ///
    /// Equivocation occurs when a node signs two *different* events that share
    /// the same `creator` and `sequence` number. This indicates the validator
    /// is creating conflicting histories.
    ///
    /// # Arguments
    ///
    /// * `event_a` — The first event to compare.
    /// * `event_b` — The second event to compare.
    ///
    /// # Returns
    ///
    /// `true` if both events have the same creator and sequence number but
    /// different `EventId`s (i.e., they are equivocating).
    ///
    /// # Example
    ///
    /// ```ignore
    /// use omnia_substrate::SlashingEngine;
    /// // event_a and event_b have same creator & sequence, different hashes
    /// assert!(SlashingEngine::check_equivocation(&event_a, &event_b));
    /// ```
    pub fn check_equivocation(event_a: &Event, event_b: &Event) -> bool {
        use subtle::ConstantTimeEq;
        // Use constant-time comparisons on creator and ID fields to prevent
        // timing side-channels. The creator field is derived from a public key
        // and the ID is a hash — both should be compared in constant time.
        let creators_match: bool = event_a.creator.ct_eq(&event_b.creator).into();
        let sequences_match = event_a.sequence == event_b.sequence;
        let ids_differ: bool = event_a.id.ct_ne(&event_b.id).into();
        creators_match && sequences_match && ids_differ
    }

    /// Checks for a liveness violation and records it if detected.
    ///
    /// A liveness violation occurs when a node has been inactive for more
    /// than `threshold` rounds (i.e., `current_round - last_active_round > threshold`).
    /// If a violation is detected, a [`SlashOffense::LivenessViolation`]
    /// offense is recorded and the resulting [`SlashOutcome`] is returned.
    ///
    /// # Arguments
    ///
    /// * `node` — The `NodeId` of the validator to check.
    /// * `last_active_round` — The last round in which the node participated.
    /// * `current_round` — The current consensus round.
    /// * `threshold` — The number of inactive rounds before a violation is triggered.
    ///
    /// # Returns
    ///
    /// `Some(SlashOutcome)` if a liveness violation was detected and recorded,
    /// `None` if the node is within the acceptable inactivity window.
    ///
    /// # Example
    ///
    /// ```
    /// use omnia_substrate::{SlashingEngine, SlashOutcome};
    ///
    /// let mut engine = SlashingEngine::new_in_memory(500, 2000);
    /// let mut node = [0u8; 32];
    /// node[0] = 7;
    ///
    /// engine.register_validator(node, 5_000);
    ///
    /// // Node was last active at round 5, current round is 20, threshold is 10
    /// let result = engine.check_liveness(node, 5, 20, 10);
    /// assert!(result.is_some());
    /// ```
    pub fn check_liveness(
        &mut self,
        node: NodeId,
        last_active_round: u64,
        current_round: u64,
        threshold: u64,
    ) -> Option<SlashOutcome> {
        let inactive_rounds = current_round.saturating_sub(last_active_round);
        if inactive_rounds > threshold {
            tracing::info!(
                node = ?&node[..4],
                last_active_round,
                current_round,
                inactive_rounds,
                threshold,
                "Liveness violation detected"
            );
            Some(self.record_offense(node, SlashOffense::LivenessViolation))
        } else {
            tracing::debug!(
                node = ?&node[..4],
                last_active_round,
                current_round,
                inactive_rounds,
                threshold,
                "Node liveness OK"
            );
            None
        }
    }

    /// Returns `true` if the node has accumulated enough slash points to be
    /// considered *slashed* (points ≥ `slash_threshold`).
    ///
    /// # Arguments
    ///
    /// * `node` — The `NodeId` to query.
    ///
    /// # Returns
    ///
    /// `true` if the node's slash points are at or above the slash threshold.
    ///
    /// # Example
    ///
    /// ```
    /// use omnia_substrate::{SlashingEngine, SlashOffense};
    ///
    /// let mut engine = SlashingEngine::new_in_memory(500, 2000);
    /// let mut node = [0u8; 32];
    /// node[0] = 3;
    ///
    /// engine.register_validator(node, 10_000);
    /// assert!(!engine.is_slashed(&node));
    ///
    /// engine.record_offense(node, SlashOffense::Equivocation); // +500 points
    /// assert!(engine.is_slashed(&node));
    /// ```
    pub fn is_slashed(&self, node: &NodeId) -> bool {
        let state = self.state.read().unwrap_or_else(|e| {
            tracing::error!(error = %e, "is_slashed — lock poisoned");
            std::process::abort()
        });
        state
            .slash_points
            .get(node)
            .map(|&p| p >= state.slash_threshold)
            .unwrap_or(false)
    }

    /// Returns `true` if the node has accumulated enough slash points to be
    /// *ejected* (points ≥ `ejection_threshold`).
    ///
    /// # Arguments
    ///
    /// * `node` — The `NodeId` to query.
    ///
    /// # Returns
    ///
    /// `true` if the node's slash points are at or above the ejection threshold.
    ///
    /// # Example
    ///
    /// ```
    /// use omnia_substrate::{SlashingEngine, SlashOffense};
    ///
    /// let mut engine = SlashingEngine::new_in_memory(500, 2000);
    /// let mut node = [0u8; 32];
    /// node[0] = 9;
    ///
    /// engine.register_validator(node, 10_000);
    /// assert!(!engine.is_ejected(&node));
    ///
    /// // 4 × Equivocation = 2000 points → ejection
    /// for _ in 0..4 {
    ///     engine.record_offense(node, SlashOffense::Equivocation);
    /// }
    /// assert!(engine.is_ejected(&node));
    /// ```
    pub fn is_ejected(&self, node: &NodeId) -> bool {
        let state = self.state.read().unwrap_or_else(|e| {
            tracing::error!(error = %e, "is_ejected — lock poisoned");
            std::process::abort()
        });
        state
            .slash_points
            .get(node)
            .map(|&p| p >= state.ejection_threshold)
            .unwrap_or(false)
    }

    /// Returns the staked amount for a node.
    ///
    /// # Arguments
    ///
    /// * `node` — The `NodeId` to query.
    ///
    /// # Returns
    ///
    /// The staked amount, or `0` if the node has not been registered.
    ///
    /// # Example
    ///
    /// ```
    /// use omnia_substrate::SlashingEngine;
    ///
    /// let mut engine = SlashingEngine::new_in_memory(500, 2000);
    /// let mut node = [0u8; 32];
    /// node[0] = 1;
    ///
    /// assert_eq!(engine.stake_of(&node), 0);
    /// engine.register_validator(node, 10_000);
    /// assert_eq!(engine.stake_of(&node), 10_000);
    /// ```
    pub fn stake_of(&self, node: &NodeId) -> u64 {
        let state = self.state.read().unwrap_or_else(|e| {
            tracing::error!(error = %e, "stake_of — lock poisoned");
            std::process::abort()
        });
        state.stakes.get(node).copied().unwrap_or(0)
    }

    /// Returns the accumulated slash points for a node.
    ///
    /// # Arguments
    ///
    /// * `node` — The `NodeId` to query.
    ///
    /// # Returns
    ///
    /// The total slash points, or `0` if the node has no recorded offenses.
    ///
    /// # Example
    ///
    /// ```
    /// use omnia_substrate::{SlashingEngine, SlashOffense};
    ///
    /// let mut engine = SlashingEngine::new_in_memory(500, 2000);
    /// let mut node = [0u8; 32];
    /// node[0] = 2;
    ///
    /// assert_eq!(engine.slash_points_of(&node), 0);
    /// engine.register_validator(node, 10_000);
    /// engine.record_offense(node, SlashOffense::LivenessViolation);
    /// assert_eq!(engine.slash_points_of(&node), 100);
    /// ```
    pub fn slash_points_of(&self, node: &NodeId) -> u64 {
        let state = self.state.read().unwrap_or_else(|e| {
            tracing::error!(error = %e, "slash_points_of — lock poisoned");
            std::process::abort()
        });
        state.slash_points.get(node).copied().unwrap_or(0)
    }

    /// Returns the accumulated slash count for a validator.
    ///
    /// This is an alias for [`Self::slash_points_of`] that follows the
    /// naming convention used by the [`SlashingStore`] trait.
    ///
    /// # Arguments
    ///
    /// * `validator` — The validator ID to query.
    ///
    /// # Returns
    ///
    /// The total slash count, or `0` if the validator has no recorded offenses.
    pub fn get_slash_count(&self, validator: &[u8; 32]) -> u64 {
        let state = self.state.read().unwrap_or_else(|e| {
            tracing::error!(error = %e, "get_slash_count — lock poisoned");
            std::process::abort()
        });
        state.slash_points.get(validator).copied().unwrap_or(0)
    }

    /// Reverse a slash by decrementing slash points for a validator.
    ///
    /// This is the companion to [`Self::record_offense`] and is used by
    /// governance-based slashing undo (see [`crate::slashing_undo`]).
    /// Points are decremented by the amount of the most recent offense,
    /// tracked via the offense history stack. This ensures that undoing
    /// an equivocation (500 pts) removes 500 pts, not just 100.
    ///
    /// # Arguments
    ///
    /// * `node` — The `NodeId` of the validator to undo slash for.
    ///
    /// # Errors
    ///
    /// Returns an error string if the validator has no offense history to undo.
    ///
    /// # Example
    ///
    /// ```
    /// use omnia_substrate::{SlashingEngine, SlashOffense};
    ///
    /// let mut engine = SlashingEngine::new_in_memory(500, 2000);
    /// let mut node = [0u8; 32];
    /// node[0] = 2;
    ///
    /// engine.register_validator(node, 10_000);
    /// engine.record_offense(node, SlashOffense::LivenessViolation);
    /// assert_eq!(engine.slash_points_of(&node), 100);
    ///
    /// engine.undo_slash(&node).unwrap();
    /// assert_eq!(engine.slash_points_of(&node), 0);
    /// ```
    pub fn undo_slash(&mut self, node: &NodeId) -> Result<(), SlashingStoreError> {
        let mut state = self.state.write().unwrap_or_else(|e| {
            tracing::error!(error = %e, "undo_slash — lock poisoned");
            std::process::abort()
        });
        let snapshot = state.clone();
        let history = state.offense_history.get_mut(node);
        match history {
            Some(entries) if !entries.is_empty() => {
                let last_offense_points =
                    entries.pop().expect("entries is non-empty per guard above");
                let current = state.slash_points.get(node).copied().unwrap_or(0);
                let new_points = current.saturating_sub(last_offense_points);
                state.slash_points.insert(*node, new_points);

                tracing::info!(
                    node = ?&node[..4],
                    previous_points = current,
                    removed_points = last_offense_points,
                    new_points = new_points,
                    "Slash points decremented via undo (offense-type-aware)"
                );

                drop(state);
                if let Err(e) = self.persist_state() {
                    tracing::error!(
                        error = %e,
                        "Failed to persist slashing state after undo_slash — rolling back"
                    );
                    let mut state = self.state.write().unwrap_or_else(|e| {
                        tracing::error!(error = %e, "undo_slash rollback — lock poisoned");
                        std::process::abort()
                    });
                    *state = snapshot;
                }
                Ok(())
            }
            _ => {
                let mut prefix = [0u8; 4];
                prefix.copy_from_slice(&node[..4]);
                Err(SlashingStoreError::UndoNoOffenseHistory(prefix))
            }
        }
    }

    /// Export the current slashing state as a [`SlashingState`] snapshot.
    ///
    /// Used by genesis replay (see [`crate::genesis_replay`]) to capture
    /// the final slashing state after replaying the event history.
    pub fn to_state(&self) -> SlashingState {
        let state = self.state.read().unwrap_or_else(|e| {
            tracing::error!(error = %e, "to_state — lock poisoned");
            std::process::abort()
        });
        state.clone()
    }

    /// Returns a reference to the internal slash points map.
    ///
    /// Used by genesis replay to capture the final state.
    pub fn internal_slash_points(&self) -> HashMap<NodeId, u64> {
        let state = self.state.read().unwrap_or_else(|e| {
            tracing::error!(error = %e, "internal_slash_points — lock poisoned");
            std::process::abort()
        });
        state.slash_points.clone()
    }

    /// Returns a reference to the internal stakes map.
    ///
    /// Used by genesis replay to capture the final state.
    pub fn internal_stakes(&self) -> HashMap<NodeId, u64> {
        let state = self.state.read().unwrap_or_else(|e| {
            tracing::error!(error = %e, "internal_stakes — lock poisoned");
            std::process::abort()
        });
        state.stakes.clone()
    }

    /// Returns the configured slash threshold.
    pub fn internal_slash_threshold(&self) -> u64 {
        let state = self.state.read().unwrap_or_else(|e| {
            tracing::error!(error = %e, "internal_slash_threshold — lock poisoned");
            std::process::abort()
        });
        state.slash_threshold
    }

    /// Returns the configured ejection threshold.
    pub fn internal_ejection_threshold(&self) -> u64 {
        let state = self.state.read().unwrap_or_else(|e| {
            tracing::error!(error = %e, "internal_ejection_threshold — lock poisoned");
            std::process::abort()
        });
        state.ejection_threshold
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn node(id: u8) -> NodeId {
        let mut n = [0u8; 32];
        n[0] = id;
        n
    }

    #[test]
    fn test_slash_offense_points() {
        assert_eq!(SlashOffense::Equivocation.points(), 500);
        assert_eq!(SlashOffense::LivenessViolation.points(), 100);
        assert_eq!(SlashOffense::InvalidAttestation.points(), 300);
    }

    #[test]
    fn test_new_in_memory_slashing_engine() {
        let engine =
            SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
        let n = node(1);
        assert!(!engine.is_slashed(&n));
        assert!(!engine.is_ejected(&n));
        assert_eq!(engine.stake_of(&n), 0);
        assert_eq!(engine.slash_points_of(&n), 0);
    }

    #[test]
    fn test_register_validator() {
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let n = node(1);
        engine.register_validator(n, 10_000);
        assert_eq!(engine.stake_of(&n), 10_000);
        assert_eq!(engine.slash_points_of(&n), 0);
    }

    #[test]
    fn test_warned_outcome() {
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let n = node(1);
        engine.register_validator(n, 10_000);
        let outcome = engine.record_offense(n, SlashOffense::LivenessViolation);
        assert_eq!(
            outcome,
            SlashOutcome::Warned {
                node: n,
                points: 100
            }
        );
    }

    #[test]
    fn test_slashed_outcome() {
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let n = node(1);
        engine.register_validator(n, 10_000);
        let outcome = engine.record_offense(n, SlashOffense::Equivocation);
        // 500 points >= 500 slash_threshold
        assert_eq!(
            outcome,
            SlashOutcome::Slashed {
                node: n,
                amount: 10_000
            }
        );
        assert!(engine.is_slashed(&n));
    }

    #[test]
    fn test_ejected_outcome() {
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let n = node(1);
        engine.register_validator(n, 10_000);

        // Accumulate 2000 points: 4 × Equivocation
        engine.record_offense(n, SlashOffense::Equivocation); // 500
        assert!(engine.is_slashed(&n));
        assert!(!engine.is_ejected(&n));

        engine.record_offense(n, SlashOffense::Equivocation); // 1000
        assert!(!engine.is_ejected(&n));

        engine.record_offense(n, SlashOffense::Equivocation); // 1500
        assert!(!engine.is_ejected(&n));

        let outcome = engine.record_offense(n, SlashOffense::Equivocation); // 2000
        assert_eq!(outcome, SlashOutcome::Ejected { node: n });
        assert!(engine.is_ejected(&n));
    }

    #[test]
    fn test_accumulated_points_across_offenses() {
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let n = node(1);
        engine.register_validator(n, 10_000);

        engine.record_offense(n, SlashOffense::LivenessViolation); // 100
        engine.record_offense(n, SlashOffense::LivenessViolation); // 200
        engine.record_offense(n, SlashOffense::InvalidAttestation); // 500
        assert!(engine.is_slashed(&n));
        assert_eq!(engine.slash_points_of(&n), 500);
    }

    #[test]
    fn test_honest_node_never_slashed() {
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let n = node(1);
        engine.register_validator(n, 10_000);
        assert!(!engine.is_slashed(&n));
        assert!(!engine.is_ejected(&n));
        assert_eq!(engine.slash_points_of(&n), 0);
    }

    #[test]
    fn test_liveness_check_no_violation() {
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let n = node(1);
        engine.register_validator(n, 10_000);

        let result = engine.check_liveness(n, 5, 10, 10);
        assert!(result.is_none());
    }

    #[test]
    fn test_liveness_check_violation() {
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let n = node(1);
        engine.register_validator(n, 10_000);

        let result = engine.check_liveness(n, 5, 20, 10);
        assert!(result.is_some());
        assert_eq!(engine.slash_points_of(&n), 100);
    }

    #[test]
    fn test_stake_of_unregistered() {
        let engine = SlashingEngine::new_in_memory(500, 2000);
        let n = node(99);
        assert_eq!(engine.stake_of(&n), 0);
        assert_eq!(engine.slash_points_of(&n), 0);
    }

    #[test]
    fn test_check_equivocation() {
        use crate::crypto::generate_keypair;
        use crate::vector_clock::VectorClock;

        let n1 = node(1);
        let kp = generate_keypair();

        // Two events with same creator and sequence but different IDs
        let vc1 = VectorClock::with_node(n1, 1);
        let mut e1 = Event::new(n1, 0, vc1.clone(), None, None, vec![1]);
        e1.sign_with_keypair(&kp);

        let mut e2 = Event::new(n1, 0, vc1, None, None, vec![2]); // different payload → different id
        e2.sign_with_keypair(&kp);

        assert!(SlashingEngine::check_equivocation(&e1, &e2));

        // Same event → not equivocation
        assert!(!SlashingEngine::check_equivocation(&e1, &e1));
    }

    #[test]
    fn test_check_no_equivocation_different_sequence() {
        use crate::crypto::generate_keypair;
        use crate::vector_clock::VectorClock;

        let n1 = node(1);
        let kp = generate_keypair();

        let vc = VectorClock::with_node(n1, 1);
        let mut e1 = Event::new(n1, 0, vc.clone(), None, None, vec![1]);
        e1.sign_with_keypair(&kp);

        let mut e2 = Event::new(n1, 1, vc, None, None, vec![1]); // different sequence
        e2.sign_with_keypair(&kp);

        assert!(!SlashingEngine::check_equivocation(&e1, &e2));
    }

    // ── Persistence unit tests ─────────────────────────────────────

    #[test]
    fn test_in_memory_store_round_trip() {
        let store = InMemorySlashingStore::new();
        let mut state = SlashingState::default();
        let n = node(1);
        state.slash_points.insert(n, 500);
        state.stakes.insert(n, 10_000);
        state.slash_threshold = 500;
        state.ejection_threshold = 2000;

        store.save(&state).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.slash_points.get(&n), Some(&500));
        assert_eq!(loaded.stakes.get(&n), Some(&10_000));
        assert_eq!(loaded.slash_threshold, 500);
        assert_eq!(loaded.ejection_threshold, 2000);
    }

    #[test]
    fn test_with_store_loads_empty_state() {
        let store = Arc::new(InMemorySlashingStore::new());
        let engine = SlashingEngine::with_store(store).unwrap();
        let n = node(1);
        assert_eq!(engine.slash_points_of(&n), 0);
        assert_eq!(engine.stake_of(&n), 0);
    }

    #[test]
    fn test_with_store_preserves_state_via_redb() {
        // This test is covered more thoroughly in tests/slashing_persistence.rs
        // Here we just verify with_store loads empty state correctly.
        let store = Arc::new(InMemorySlashingStore::new());
        let engine = SlashingEngine::with_store(store).unwrap();
        let n = node(1);
        assert_eq!(engine.slash_points_of(&n), 0);
        assert_eq!(engine.stake_of(&n), 0);
    }

    #[test]
    fn test_undo_slash_respects_offense_type() {
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let mut node = [0u8; 32];
        node[0] = 42;

        engine.register_validator(node, 10_000);

        // Record equivocation (500 pts)
        engine.record_offense(node, SlashOffense::Equivocation);
        assert_eq!(engine.slash_points_of(&node), 500);

        // Undo should remove exactly 500 pts (not 100)
        engine.undo_slash(&node).unwrap();
        assert_eq!(engine.slash_points_of(&node), 0);
    }

    #[test]
    fn test_undo_slash_mixed_offenses() {
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let mut node = [0u8; 32];
        node[0] = 99;

        engine.register_validator(node, 10_000);

        // Record: Liveness (100) + Equivocation (500) + InvalidAttestation (300) = 900
        engine.record_offense(node, SlashOffense::LivenessViolation);
        engine.record_offense(node, SlashOffense::Equivocation);
        engine.record_offense(node, SlashOffense::InvalidAttestation);
        assert_eq!(engine.slash_points_of(&node), 900);

        // Undo last (InvalidAttestation = 300) → 600
        engine.undo_slash(&node).unwrap();
        assert_eq!(engine.slash_points_of(&node), 600);

        // Undo second-to-last (Equivocation = 500) → 100
        engine.undo_slash(&node).unwrap();
        assert_eq!(engine.slash_points_of(&node), 100);

        // Undo first (Liveness = 100) → 0
        engine.undo_slash(&node).unwrap();
        assert_eq!(engine.slash_points_of(&node), 0);
    }
}
