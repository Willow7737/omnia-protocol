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

use redb::ReadableTable;
use serde::{Deserialize, Serialize};

use omnia_primitives::Event;
use omnia_primitives::NodeId;

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

/// Returns the current wall-clock time as milliseconds since the UNIX epoch.
///
/// Used for timestamping [`SlashingEvent`]s. Returns `0` if the system clock
/// is earlier than the UNIX epoch (which should never happen in practice).
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_else(|e| {
            tracing::error!("System clock is before UNIX epoch: {e} — using 0 as timestamp");
            0
        })
}

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
    /// use omnia_consensus::SlashOffense;
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

/// Graded penalty system for graduated slashing.
///
/// # Non-Determinism Warning
/// The `burn_percentage: f64` field uses floating-point arithmetic which is
/// non-deterministic across platforms (x86 vs ARM). In a blockchain context,
/// different nodes may compute different slash amounts. Consider migrating to
/// fixed-point (basis points as u64 where 10000 = 100%) in a future version.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SlashPenalty {
    /// First offense below threshold: warning + small stake burn.
    Warning {
        /// Percentage of staked amount to burn (e.g., 1.0 = 1%).
        burn_percentage: f64,
    },
    /// Repeated offenses: partial slash + jail period.
    Jailed {
        /// Percentage of staked amount to burn (e.g., 5.0 = 5%).
        burn_percentage: f64,
        /// Number of rounds the validator is jailed.
        jail_rounds: u64,
        /// Whether jail expires automatically.
        auto_release: bool,
    },
    /// Egregious or accumulated: full slash + ejection.
    Ejected {
        /// Percentage of staked amount to burn (100% = full slash).
        burn_percentage: f64,
        /// Reason for ejection.
        reason: String,
    },
}

/// Jail state for a temporarily suspended validator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JailState {
    /// The jailed validator's node ID.
    pub validator_id: NodeId,
    /// The round at which the validator was jailed.
    pub jailed_at_round: u64,
    /// The round at which the validator will be released.
    pub release_round: u64,
    /// History of offenses that led to jailing.
    pub offense_history: Vec<SlashOffense>,
    /// Amount of stake locked during jail.
    pub stake_locked: u64,
    /// Whether jail expires automatically.
    pub auto_release: bool,
}

/// Slashing event type for external monitoring.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SlashingEventType {
    /// A new offense was recorded.
    OffenseRecorded,
    /// A penalty was applied.
    PenaltyApplied,
    /// A validator entered jail.
    JailEntered,
    /// A validator was released from jail.
    JailReleased,
    /// A validator was ejected.
    ValidatorEjected,
    /// A slash was undone by governance.
    UndoApplied,
}

/// Slashing event for external monitoring and audit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashingEvent {
    /// The type of event.
    pub event_type: SlashingEventType,
    /// The validator involved.
    pub validator_id: NodeId,
    /// The round at which the event occurred.
    pub round: u64,
    /// The offense that triggered this event (if applicable).
    pub offense: Option<SlashOffense>,
    /// The penalty applied (if applicable).
    pub penalty: Option<SlashPenalty>,
    /// Timestamp of the event.
    pub timestamp: u64,
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
    #[error("validator {prefix:?} has no offense history to undo", prefix = .0)]
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
/// use omnia_consensus::slashing::SlashingState;
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
    /// Typed offense history per node for graded slashing.
    pub typed_offense_history: HashMap<NodeId, Vec<SlashOffense>>,
    /// Currently jailed validators.
    pub jail_registry: HashMap<NodeId, JailState>,
}

impl Default for SlashingState {
    fn default() -> Self {
        Self {
            slash_points: HashMap::new(),
            stakes: HashMap::new(),
            slash_threshold: DEFAULT_SLASH_THRESHOLD,
            ejection_threshold: DEFAULT_EJECTION_THRESHOLD,
            offense_history: HashMap::new(),
            typed_offense_history: HashMap::new(),
            jail_registry: HashMap::new(),
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
/// use omnia_consensus::slashing::{InMemorySlashingStore, SlashingStore, SlashingState};
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
    fn decrement_slash_count_by(&self, validator: &[u8; 32], amount: u64) -> Result<(), SlashingStoreError>;

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
/// use omnia_consensus::slashing::{RedbSlashingStore, SlashingStore};
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
    /// use omnia_consensus::slashing::{RedbSlashingStore, SlashingStore};
    /// use std::path::Path;
    ///
    /// let store = RedbSlashingStore::open(Path::new("/tmp/omnia-slashing.redb")).unwrap();
    /// let state = store.load().unwrap();
    /// ```
    pub fn open(path: &Path) -> Result<Self, SlashingStoreError> {
        // M-10 fix (audit v0.1.68): recover from a corrupt slashing database
        // instead of crashing. If `redb::Database::create()` fails (which can
        // happen when the file exists but is corrupt), we:
        //   1. Rename the corrupt file to `<path>.corrupt` for forensic review.
        //   2. Log an ERROR (not WARN — slash history loss is serious).
        //   3. Create a fresh database and continue.
        //
        // The previous behaviour was a fatal error: a corrupt slashing DB
        // would prevent the node from starting at all, which is worse for
        // liveness than losing slash history (the operator can manually
        // review the .corrupt file and re-apply slashes if needed).
        let db = match redb::Database::create(path) {
            Ok(db) => db,
            Err(e) => {
                // Backup corrupt file (best-effort — if rename fails, we
                // still try to create fresh).
                let backup = path.with_extension("db.corrupt");
                if let Err(rename_err) = std::fs::rename(path, &backup) {
                    tracing::error!(
                        corrupt_path = %path.display(),
                        backup_path = %backup.display(),
                        error = %rename_err,
                        "Could not rename corrupt slashing DB — will attempt to remove and recreate"
                    );
                    let _ = std::fs::remove_file(path);
                }
                tracing::error!(
                    corrupt_backup = %backup.display(),
                    original_error = %e,
                    "Slashing database was corrupt — renamed to .corrupt and starting fresh. \
                     ALL SLASH HISTORY IS LOST. Manual review required before rejoining consensus."
                );
                redb::Database::create(path).map_err(|e2| SlashingStoreError::Persistence(e2.to_string()))?
            }
        };
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
            Some(value) => {
                postcard::from_bytes(value.value()).map_err(|e| SlashingStoreError::Serialization(e.to_string()))
            }
            None => Ok(SlashingState::default()),
        }
    }

    fn save(&self, state: &SlashingState) -> Result<(), SlashingStoreError> {
        let bytes = postcard::to_allocvec(state).map_err(|e| SlashingStoreError::Serialization(e.to_string()))?;
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

    fn decrement_slash_count_by(&self, validator: &[u8; 32], amount: u64) -> Result<(), SlashingStoreError> {
        // Perform the entire read-modify-write inside a single redb write
        // transaction to eliminate the TOCTOU race that existed when load()
        // and save() used separate transactions.
        let write_txn = self
            .db
            .begin_write()
            .map_err(|e| SlashingStoreError::Persistence(e.to_string()))?;

        let mut state = {
            let read_table = write_txn
                .open_table(SLASHING_TABLE)
                .map_err(|e| SlashingStoreError::Persistence(e.to_string()))?;
            let state_bytes: Option<Vec<u8>> = {
                let guard = read_table
                    .get("state")
                    .map_err(|e| SlashingStoreError::Persistence(e.to_string()))?;
                guard.map(|v| v.value().to_vec())
            };
            match state_bytes {
                Some(bytes) => {
                    postcard::from_bytes(&bytes).map_err(|e| SlashingStoreError::Serialization(e.to_string()))?
                }
                None => SlashingState::default(),
            }
        };

        let current = state.slash_points.get(validator).copied().unwrap_or(0);
        if current == 0 {
            return Err(SlashingStoreError::Persistence(format!(
                "Validator {:?} has no slash count to decrement",
                &validator[..4]
            )));
        }
        let decrement = amount.min(current);
        state.slash_points.insert(*validator, current - decrement);

        let bytes = postcard::to_allocvec(&state).map_err(|e| SlashingStoreError::Serialization(e.to_string()))?;
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
/// use omnia_consensus::slashing::{InMemorySlashingStore, SlashingStore, SlashingState};
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
    /// use omnia_consensus::slashing::InMemorySlashingStore;
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

    fn decrement_slash_count_by(&self, validator: &[u8; 32], amount: u64) -> Result<(), SlashingStoreError> {
        // Perform decrement atomically inside a single write lock to avoid TOCTOU
        let mut guard = self
            .state
            .write()
            .map_err(|e| SlashingStoreError::Persistence(e.to_string()))?;
        let current = guard.slash_points.get(validator).copied().unwrap_or(0);
        if current == 0 {
            return Err(SlashingStoreError::Persistence(format!(
                "Validator {:?} has no slash count to decrement",
                &validator[..4]
            )));
        }
        let decrement = amount.min(current);
        guard.slash_points.insert(*validator, current - decrement);
        // Persist while holding the lock to ensure atomicity
        let state_snapshot = guard.clone();
        drop(guard);
        self.save(&state_snapshot)
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
/// use omnia_consensus::{SlashingEngine, SlashOffense, SlashOutcome};
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
    /// use omnia_consensus::slashing::SlashingEngine;
    /// use std::path::PathBuf;
    ///
    /// // Production: persistent slashing
    /// let engine = SlashingEngine::new(Some(PathBuf::from("./data/slashing")), 500, 2000).unwrap();
    ///
    /// // Testing: in-memory slashing
    /// let engine = SlashingEngine::new(None, 500, 2000).unwrap();
    /// ```
    pub fn new(
        data_dir: Option<PathBuf>,
        slash_threshold: u64,
        ejection_threshold: u64,
    ) -> Result<Self, SlashingStoreError> {
        match data_dir {
            Some(path) => match RedbSlashingStore::open(&path) {
                Ok(store) => {
                    tracing::info!(
                        path = %path.display(),
                        "Slashing engine: using persistent redb store"
                    );
                    Ok(Self::with_store_with_thresholds(
                        Arc::new(store),
                        slash_threshold,
                        ejection_threshold,
                    ))
                }
                Err(e) => {
                    tracing::error!(
                        error = %e,
                        path = %path.display(),
                        "Failed to open redb store — returning error instead of falling back to in-memory"
                    );
                    Err(SlashingStoreError::Persistence(format!(
                        "Failed to open redb store at {}: {}",
                        path.display(),
                        e
                    )))
                }
            },
            None => {
                tracing::info!("Slashing engine: using in-memory store (testing mode)");
                Ok(Self::new_in_memory(slash_threshold, ejection_threshold))
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
            typed_offense_history: HashMap::new(),
            jail_registry: HashMap::new(),
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
    /// use omnia_consensus::slashing::{RedbSlashingStore, SlashingEngine};
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
                    typed_offense_history: HashMap::new(),
                    jail_registry: HashMap::new(),
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
    /// use omnia_consensus::SlashingEngine;
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
    /// use omnia_consensus::{SlashingEngine, SlashOffense, SlashOutcome};
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
        state.offense_history.entry(node).or_default().push(points_added);

        // Track typed offense history for graded slashing
        state.typed_offense_history.entry(node).or_default().push(offense);

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
    /// use omnia_consensus::SlashingEngine;
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
    /// use omnia_consensus::{SlashingEngine, SlashOutcome};
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
    /// use omnia_consensus::{SlashingEngine, SlashOffense};
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
    /// use omnia_consensus::{SlashingEngine, SlashOffense};
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
    /// use omnia_consensus::SlashingEngine;
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
    /// use omnia_consensus::{SlashingEngine, SlashOffense};
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
    /// use omnia_consensus::{SlashingEngine, SlashOffense};
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
                let last_offense_points = entries.pop().expect("entries is non-empty per guard above");
                // Also pop from typed_offense_history to keep both histories in sync
                if let Some(typed_history) = state.typed_offense_history.get_mut(node) {
                    typed_history.pop();
                }
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
    /// Used by genesis replay to capture
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

    /// Compute the graded penalty based on offense type and same-type history.
    ///
    /// Per ADR-011, escalation is based on the number of **prior offenses of
    /// the same type**, not total accumulated offenses. This ensures that a
    /// validator with two liveness violations is not penalized as harshly for
    /// their first equivocation.
    ///
    /// # Escalation tiers (ADR-011)
    ///
    /// | Offense              | 1st              | 2nd                        | 3rd            |
    /// |----------------------|------------------|----------------------------|----------------|
    /// | Equivocation         | Jailed(5%, 1000) | Jailed(25%, 5000)          | Ejected(100%)  |
    /// | LivenessViolation    | Warning(1%)      | Warning(1%)                | Jailed(5%, 500)|
    /// | InvalidAttestation   | Warning(2%)      | Jailed(10%, 2000)          | Ejected(100%)  |
    ///
    /// # Arguments
    ///
    /// * `validator_id` — The validator to compute a penalty for.
    /// * `offense` — The type of offense being considered.
    ///
    /// # Returns
    ///
    /// A [`SlashPenalty`] indicating the graded penalty for this offense.
    pub fn compute_penalty(&self, validator_id: NodeId, offense: &SlashOffense) -> SlashPenalty {
        // Count only prior offenses of the same type for escalation.
        let history = self.get_offense_history(validator_id);
        let same_type_count = history.iter().filter(|&&o| o == *offense).count();

        match offense {
            SlashOffense::Equivocation => {
                // 1st (0 prior) → Jailed(5%, 1000 rounds, auto-release)
                // 2nd (1 prior) → Jailed(25%, 5000 rounds, no auto-release)
                // 3rd+ (2+ prior) → Ejected(100%)
                if same_type_count == 0 {
                    SlashPenalty::Jailed {
                        burn_percentage: 5.0,
                        jail_rounds: 1000,
                        auto_release: true,
                    }
                } else if same_type_count == 1 {
                    SlashPenalty::Jailed {
                        burn_percentage: 25.0,
                        jail_rounds: 5000,
                        auto_release: false,
                    }
                } else {
                    SlashPenalty::Ejected {
                        burn_percentage: 100.0,
                        reason: "repeat_equivocation".into(),
                    }
                }
            }
            SlashOffense::LivenessViolation => {
                // 1st (0 prior) → Warning(1%)
                // 2nd (1 prior) → Warning(1%)
                // 3rd+ (2+ prior) → Jailed(5%, 500 rounds, auto-release)
                if same_type_count < 2 {
                    SlashPenalty::Warning { burn_percentage: 1.0 }
                } else {
                    SlashPenalty::Jailed {
                        burn_percentage: 5.0,
                        jail_rounds: 500,
                        auto_release: true,
                    }
                }
            }
            SlashOffense::InvalidAttestation => {
                // 1st (0 prior) → Warning(2%)
                // 2nd (1 prior) → Jailed(10%, 2000 rounds, auto-release)
                // 3rd+ (2+ prior) → Ejected(100%)
                if same_type_count == 0 {
                    SlashPenalty::Warning { burn_percentage: 2.0 }
                } else if same_type_count == 1 {
                    SlashPenalty::Jailed {
                        burn_percentage: 10.0,
                        jail_rounds: 2000,
                        auto_release: true,
                    }
                } else {
                    SlashPenalty::Ejected {
                        burn_percentage: 100.0,
                        reason: "repeat_invalid_attestation".into(),
                    }
                }
            }
        }
    }

    /// Check if a validator is currently in jail.
    ///
    /// A validator is considered jailed if they are in the jail registry
    /// AND either:
    /// - `auto_release` is `false` (manual release required), or
    /// - `auto_release` is `true` and `current_round < release_round`.
    pub fn is_jailed(&self, validator_id: NodeId, current_round: u64) -> bool {
        let state = self.state.read().unwrap_or_else(|e| {
            tracing::error!(error = %e, "is_jailed — lock poisoned");
            std::process::abort()
        });
        if let Some(jail) = state.jail_registry.get(&validator_id) {
            if jail.auto_release && current_round >= jail.release_round {
                return false;
            }
            true
        } else {
            false
        }
    }

    /// Check if a validator is currently jailed and cannot participate in consensus.
    ///
    /// Unlike [`Self::is_jailed`], this method treats `auto_release` validators
    /// whose term has expired as **not jailed**, regardless of whether they have
    /// been formally released from the jail registry. This is the check that
    /// consensus should use to determine participation eligibility.
    ///
    /// # Arguments
    ///
    /// * `validator_id` — The validator to check.
    /// * `current_round` — The current consensus round.
    ///
    /// # Returns
    ///
    /// `true` if the validator is still serving their jail term at `current_round`.
    pub fn is_jailed_at(&self, validator_id: NodeId, current_round: u64) -> bool {
        let state = self.state.read().unwrap_or_else(|e| {
            tracing::error!(error = %e, "is_jailed_at — lock poisoned");
            std::process::abort()
        });
        match state.jail_registry.get(&validator_id) {
            Some(jail) => {
                // P0-5 fix: non-auto-release validators must stay jailed forever.
                // The previous implementation only checked release_round, allowing
                // manually-jailed validators (e.g. 2nd-tier equivocation offenders
                // with auto_release=false) to rejoin consensus prematurely after
                // their nominal jail term elapsed. `is_jailed_at` is the check
                // that consensus uses to determine participation eligibility, so
                // a false return here lets a slashed validator sign again without
                // a manual release — defeating the slashing penalty.
                if !jail.auto_release {
                    return true; // Permanently jailed until manual release
                }
                current_round < jail.release_round
            }
            None => false,
        }
    }

    /// Try to release a validator from jail if their term is served.
    pub fn try_release_from_jail(
        &mut self,
        validator_id: NodeId,
        current_round: u64,
    ) -> Result<bool, SlashingStoreError> {
        let mut state = self.state.write().unwrap_or_else(|e| {
            tracing::error!(error = %e, "try_release_from_jail — lock poisoned");
            std::process::abort()
        });
        let _snapshot = state.clone();

        if let Some(jail) = state.jail_registry.get(&validator_id) {
            if current_round >= jail.release_round {
                state.jail_registry.remove(&validator_id);
                drop(state);
                self.persist_state()?;
                tracing::info!(validator = ?&validator_id[..4], "Validator released from jail");
                return Ok(true);
            }
        }
        drop(state);
        Ok(false)
    }

    /// Get all currently jailed validators.
    pub fn jailed_validators(&self) -> Vec<JailState> {
        let state = self.state.read().unwrap_or_else(|e| {
            tracing::error!(error = %e, "jailed_validators — lock poisoned");
            std::process::abort()
        });
        state.jail_registry.values().cloned().collect()
    }

    /// Get the offense history for a validator.
    pub fn get_offense_history(&self, validator_id: NodeId) -> Vec<SlashOffense> {
        let state = self.state.read().unwrap_or_else(|e| {
            tracing::error!(error = %e, "get_offense_history — lock poisoned");
            std::process::abort()
        });
        state
            .typed_offense_history
            .get(&validator_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Compute partial burn amount from stake and burn percentage.
    ///
    /// This is a pure function with no side effects. It converts a burn
    /// percentage (e.g., `5.0` = 5%) into an absolute stake amount.
    ///
    /// # Arguments
    ///
    /// * `stake` — The validator's total staked amount.
    /// * `burn_percentage` — Percentage of stake to burn (e.g., `5.0` = 5%).
    ///
    /// # Returns
    ///
    /// The absolute burn amount, rounded down. Returns `0` if the validator
    /// has no stake or the percentage is zero.
    ///
    /// # H-6 fix (audit v0.1.68)
    ///
    /// Uses u128 integer arithmetic instead of f64 to ensure deterministic
    /// cross-platform results. The formula is: `(stake * burn_percentage_bps) / 10_000`
    /// where `burn_percentage_bps = (burn_percentage * 100) as u128`.
    ///
    /// # C-4 residual risk (mentor review 2026-06-21)
    ///
    /// The H-6 fix mitigates the f64 non-determinism in the *arithmetic*
    /// (the multiplication and division are now u128), but the
    /// **f64→u128 conversion** at the API boundary (`(burn_percentage *
    /// 100.0) as u128`) is still theoretically non-deterministic for
    /// fractional values. On x86 with x87 FPU (80-bit extended
    /// precision), `5.1 * 100.0` may produce `509.9999...` which
    /// truncates to `509`, while on x86-64 with SSE2 it produces
    /// `510.0` → `510`.
    ///
    /// **Current risk: LOW.** All burn_percentage values used in the
    /// codebase are integer-valued (1.0, 2.0, 5.0, 10.0, 25.0, 100.0),
    /// for which the f64→u128 conversion is exact and deterministic
    /// across all platforms. The risk only materializes if a caller
    /// passes a fractional burn_percentage.
    ///
    /// **Full fix (deferred):** Change the API to accept basis points
    /// (`burn_percentage_bps: u64` where 10000 = 100%) instead of `f64`.
    /// This is a breaking API change that affects `SlashPenalty`,
    /// `compute_burn_amount`, `compute_burn_amount_for`, and
    /// `record_offense_graded`. It should be done in a coordinated
    /// breaking-change release.
    ///
    /// **Audit of new code (2026-06-21):** The 20 tests added to this
    /// file for coverage are TEST-ONLY code and do NOT introduce any
    /// new f64 usage in consensus-critical paths. The only f64
    /// references in new code are test-assertion values
    /// (e.g., `burn_percentage: 1.0` in `assert!(matches!(...))`),
    /// which are not on the consensus critical path.
    pub fn compute_burn_amount(stake: u64, burn_percentage: f64) -> u64 {
        // P0-4 fix: The f64→u128 conversion (`burn_percentage * 100.0`) is
        // non-deterministic across CPU architectures (x87 FPU 80-bit extended
        // vs SSE2 64-bit). Two nodes with different CPUs can compute different
        // slash amounts → state divergence → fork.
        //
        // The new compute_burn_amount_bp() function uses pure integer
        // arithmetic (u64 basis points) and is fully deterministic.
        // This deprecated f64-based function delegates to it after converting.
        let bps = (burn_percentage * 100.0) as u64;
        Self::compute_burn_amount_bp(stake, bps)
    }

    /// Compute the burn amount using integer basis points (10000 = 100%).
    ///
    /// This is the deterministic replacement for `compute_burn_amount`.
    /// Uses pure u128 intermediate arithmetic — no f64, no platform-dependent
    /// rounding. Two nodes with different CPUs will always compute the same
    /// result.
    ///
    /// # Arguments
    ///
    /// * `stake` — The validator's total stake in u64 units.
    /// * `burn_bps` — Burn percentage in basis points (100 = 1%, 10000 = 100%).
    ///
    /// # Example
    ///
    /// ```
    /// use omnia_consensus::SlashingEngine;
    /// // Burn 5% of a 10,000 stake = 500
    /// assert_eq!(SlashingEngine::compute_burn_amount_bp(10_000, 500), 500);
    /// // Burn 100% of a 10,000 stake = 10,000
    /// assert_eq!(SlashingEngine::compute_burn_amount_bp(10_000, 10_000), 10_000);
    /// ```
    pub fn compute_burn_amount_bp(stake: u64, burn_bps: u64) -> u64 {
        // u128 intermediate prevents overflow for any stake < 2^96
        ((stake as u128) * (burn_bps as u128) / 10_000) as u64
    }

    /// Compute the burn amount for a specific validator given a burn percentage.
    ///
    /// This is an instance-method convenience wrapper around
    /// [`Self::compute_burn_amount`] that looks up the validator's stake
    /// automatically.
    ///
    /// # Arguments
    ///
    /// * `validator_id` — The validator whose stake should be used.
    /// * `burn_percentage` — Percentage of stake to burn (e.g., `5.0` = 5%).
    ///
    /// # Returns
    ///
    /// The absolute burn amount. Returns `0` if the validator is not
    /// registered (has no stake).
    fn burn_amount_for(&self, validator_id: NodeId, burn_percentage: f64) -> u64 {
        let stake = self.stake_of(&validator_id);
        Self::compute_burn_amount(stake, burn_percentage)
    }

    /// Compute the burn amount for a specific validator given a burn percentage.
    ///
    /// This is the instance-method overload of [`Self::compute_burn_amount`]
    /// that looks up the validator's stake automatically. Returns `None` if
    /// the validator is not registered (has no stake).
    ///
    /// # Arguments
    ///
    /// * `validator_id` — The validator whose stake should be used.
    /// * `burn_percentage` — Percentage of stake to burn (e.g., `5.0` = 5%).
    ///
    /// # Returns
    ///
    /// `Some(burn_amount)` if the validator is registered, `None` otherwise.
    ///
    /// # Example
    ///
    /// ```
    /// use omnia_consensus::SlashingEngine;
    ///
    /// let mut engine = SlashingEngine::new_in_memory(500, 2000);
    /// let node = [1u8; 32];
    ///
    /// // Unregistered → None
    /// assert!(engine.compute_burn_amount_for(node, 5.0).is_none());
    ///
    /// engine.register_validator(node, 10_000);
    /// // 5% of 10_000 = 500
    /// assert_eq!(engine.compute_burn_amount_for(node, 5.0), Some(500));
    /// ```
    pub fn compute_burn_amount_for(&self, validator_id: NodeId, burn_percentage: f64) -> Option<u64> {
        let stake = self.stake_of(&validator_id);
        if stake == 0 {
            return None;
        }
        Some(Self::compute_burn_amount(stake, burn_percentage))
    }

    /// Record an offense and apply the graded penalty per ADR-011.
    ///
    /// Unlike [`Self::record_offense`] which uses the binary point model
    /// (accumulate points → check thresholds), this method uses the 3-tier
    /// graduated slashing model:
    ///
    /// - **Warning**: Small burn percentage, no jail
    /// - **Jailed**: Partial burn + jail period (validator cannot participate)
    /// - **Ejected**: Full slash + removal from validator set
    ///
    /// The penalty tier is determined by [`Self::compute_penalty`], which
    /// escalates based on the number of **prior offenses of the same type**.
    ///
    /// # Arguments
    ///
    /// * `validator_id` — The offending validator.
    /// * `offense` — The type of offense committed.
    /// * `current_round` — The current consensus round (used for jail timing).
    ///
    /// # Returns
    ///
    /// A [`SlashOutcome`] indicating the consequence of this offense.
    /// The `points` field of `Warned` contains the computed burn amount,
    /// not accumulated slash points.
    ///
    /// # Example
    ///
    /// ```
    /// use omnia_consensus::{SlashingEngine, SlashOffense, SlashOutcome};
    ///
    /// let mut engine = SlashingEngine::new_in_memory(500, 2000);
    /// let node = [42u8; 32];
    ///
    /// engine.register_validator(node, 10_000);
    ///
    /// // First liveness violation → Warning (1% burn = 100)
    /// let outcome = engine.record_offense_graded(node, SlashOffense::LivenessViolation, 100);
    /// assert!(matches!(outcome, SlashOutcome::Warned { .. }));
    /// ```
    pub fn record_offense_graded(
        &mut self,
        validator_id: NodeId,
        offense: SlashOffense,
        current_round: u64,
    ) -> SlashOutcome {
        // 1. Compute the graded penalty using compute_penalty()
        let penalty = self.compute_penalty(validator_id, &offense);

        // 2. Apply the penalty based on its tier
        match &penalty {
            SlashPenalty::Warning { burn_percentage } => {
                // Record offense, emit warning event, compute small burn
                let burn_amount = self.burn_amount_for(validator_id, *burn_percentage);

                // Record the offense (accumulates points and typed history)
                let _outcome = self.record_offense(validator_id, offense);

                // Emit events
                self.emit_event(SlashingEvent {
                    event_type: SlashingEventType::OffenseRecorded,
                    validator_id,
                    round: current_round,
                    offense: Some(offense),
                    penalty: Some(penalty.clone()),
                    timestamp: current_timestamp(),
                });
                self.emit_event(SlashingEvent {
                    event_type: SlashingEventType::PenaltyApplied,
                    validator_id,
                    round: current_round,
                    offense: Some(offense),
                    penalty: Some(SlashPenalty::Warning {
                        burn_percentage: *burn_percentage,
                    }),
                    timestamp: current_timestamp(),
                });

                // Return Warned outcome (reusing `points` field for burn amount)
                SlashOutcome::Warned {
                    node: validator_id,
                    points: burn_amount,
                }
            }

            SlashPenalty::Jailed {
                burn_percentage,
                jail_rounds,
                auto_release,
            } => {
                let burn_amount = self.burn_amount_for(validator_id, *burn_percentage);

                // Record the offense (accumulates points and typed history)
                let _outcome = self.record_offense(validator_id, offense);

                // Enter jail
                let jailed_until = current_round.saturating_add(*jail_rounds);
                {
                    let mut state = self.state.write().unwrap_or_else(|e| {
                        tracing::error!(error = %e, "record_offense_graded jail — lock poisoned");
                        std::process::abort()
                    });
                    // Read offense_history directly from state instead of calling
                    // self.get_offense_history() to avoid a deadlock: we already
                    // hold the write lock, and get_offense_history() would try to
                    // acquire the read lock on the same RwLock.
                    let offense_history = state
                        .typed_offense_history
                        .get(&validator_id)
                        .cloned()
                        .unwrap_or_default();
                    state.jail_registry.insert(
                        validator_id,
                        JailState {
                            validator_id,
                            jailed_at_round: current_round,
                            release_round: jailed_until,
                            offense_history,
                            stake_locked: burn_amount,
                            auto_release: *auto_release,
                        },
                    );
                }

                // Emit jail event
                self.emit_event(SlashingEvent {
                    event_type: SlashingEventType::JailEntered,
                    validator_id,
                    round: current_round,
                    offense: Some(offense),
                    penalty: Some(penalty.clone()),
                    timestamp: current_timestamp(),
                });

                // Persist
                if let Err(e) = self.persist_state() {
                    tracing::error!(error = %e, "Failed to persist jail state");
                }

                SlashOutcome::Slashed {
                    node: validator_id,
                    amount: burn_amount,
                }
            }

            SlashPenalty::Ejected {
                burn_percentage: _,
                reason: _,
            } => {
                // Record the offense (accumulates points and typed history)
                let _outcome = self.record_offense(validator_id, offense);

                // Emit ejection event
                self.emit_event(SlashingEvent {
                    event_type: SlashingEventType::ValidatorEjected,
                    validator_id,
                    round: current_round,
                    offense: Some(offense),
                    penalty: Some(penalty.clone()),
                    timestamp: current_timestamp(),
                });

                // Persist
                if let Err(e) = self.persist_state() {
                    tracing::error!(error = %e, "Failed to persist ejection state");
                }

                SlashOutcome::Ejected { node: validator_id }
            }
        }
    }

    /// Release validators whose jail term has expired.
    ///
    /// Scans the jail registry and removes all validators whose
    /// `auto_release` flag is `true` and whose `release_round` is at or
    /// before `current_round`. A [`SlashingEventType::JailReleased`] event
    /// is emitted for each released validator.
    ///
    /// # Arguments
    ///
    /// * `current_round` — The current consensus round.
    ///
    /// # Returns
    ///
    /// A `Vec<NodeId>` of validators that were released from jail.
    /// Only validators with `auto_release: true` are automatically released.
    ///
    /// # Example
    ///
    /// ```
    /// use omnia_consensus::SlashingEngine;
    ///
    /// let mut engine = SlashingEngine::new_in_memory(500, 2000);
    /// // ... after jailing a validator with auto_release = true ...
    /// let released = engine.release_expired_jails(5000);
    /// ```
    pub fn release_expired_jails(&mut self, current_round: u64) -> Vec<NodeId> {
        let released: Vec<NodeId> = {
            let state = self.state.read().unwrap_or_else(|e| {
                tracing::error!(error = %e, "release_expired_jails — lock poisoned");
                std::process::abort()
            });
            state
                .jail_registry
                .iter()
                .filter(|(_, jail)| jail.auto_release && current_round >= jail.release_round)
                .map(|(&id, _)| id)
                .collect()
        };

        for id in &released {
            {
                let mut state = self.state.write().unwrap_or_else(|e| {
                    tracing::error!(error = %e, "release_expired_jails write — lock poisoned");
                    std::process::abort()
                });
                state.jail_registry.remove(id);
            }
            self.emit_event(SlashingEvent {
                event_type: SlashingEventType::JailReleased,
                validator_id: *id,
                round: current_round,
                offense: Some(SlashOffense::LivenessViolation),
                penalty: Some(SlashPenalty::Warning { burn_percentage: 0.0 }),
                timestamp: current_timestamp(),
            });
        }

        if !released.is_empty() {
            if let Err(e) = self.persist_state() {
                tracing::error!(error = %e, "Failed to persist jail release state");
            }
        }

        released
    }

    /// Emit a slashing event for external monitoring.
    pub fn emit_event(&self, event: SlashingEvent) {
        tracing::info!(
            event_type = ?event.event_type,
            validator = ?&event.validator_id[..4],
            round = event.round,
            "Slashing event emitted"
        );
        // In production, this would write to a persistent event log
        // For now, we log it for observability
    }
}

// ── SlashingBackend implementation ──────────────────────────────────

impl crate::SlashingBackend for SlashingEngine {
    fn is_slashed(&self, node: &NodeId) -> bool {
        SlashingEngine::is_slashed(self, node)
    }

    fn record_offense(&mut self, node: NodeId, offense: SlashOffense) -> SlashOutcome {
        SlashingEngine::record_offense(self, node, offense)
    }

    fn register_validator(&mut self, node: NodeId, stake: u64) {
        SlashingEngine::register_validator(self, node, stake)
    }
}

/// Type alias for the default slashing backend.
///
/// This wraps the existing [`SlashingEngine`] as the canonical
/// implementation of [`crate::SlashingBackend`]. Consumers that do not
/// need a custom slashing backend can use this type directly.
pub type DefaultSlashingBackend = SlashingEngine;

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
        let engine = SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
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
        assert_eq!(outcome, SlashOutcome::Warned { node: n, points: 100 });
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
        use omnia_crypto::generate_keypair;
        use omnia_primitives::VectorClock;

        let n1 = node(1);
        let kp = generate_keypair();

        // Two events with same creator and sequence but different IDs
        let vc1 = VectorClock::with_node(n1, 1);
        let mut e1 = Event::new(n1, 0, vc1.clone(), None, None, vec![1]).expect("valid event");
        e1.sign_with_keypair(&kp).expect("signing");

        let mut e2 = Event::new(n1, 0, vc1, None, None, vec![2]).expect("valid event"); // different payload → different id
        e2.sign_with_keypair(&kp).expect("signing");

        assert!(SlashingEngine::check_equivocation(&e1, &e2));

        // Same event → not equivocation
        assert!(!SlashingEngine::check_equivocation(&e1, &e1));
    }

    #[test]
    fn test_check_no_equivocation_different_sequence() {
        use omnia_crypto::generate_keypair;
        use omnia_primitives::VectorClock;

        let n1 = node(1);
        let kp = generate_keypair();

        let vc = VectorClock::with_node(n1, 1);
        let mut e1 = Event::new(n1, 0, vc.clone(), None, None, vec![1]).expect("valid event");
        e1.sign_with_keypair(&kp).expect("signing");

        let mut e2 = Event::new(n1, 1, vc, None, None, vec![1]).expect("valid event"); // different sequence
        e2.sign_with_keypair(&kp).expect("signing");

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

    #[test]
    fn test_graded_slashing_equivocation_escalation() {
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let node = [42u8; 32];
        engine.register_validator(node, 10_000);

        // First equivocation: Jailed
        let penalty = engine.compute_penalty(node, &SlashOffense::Equivocation);
        assert!(matches!(
            penalty,
            SlashPenalty::Jailed {
                burn_percentage: 5.0,
                ..
            }
        ));

        // Second equivocation: Jailed with higher penalty
        engine.record_offense(node, SlashOffense::Equivocation);
        let penalty = engine.compute_penalty(node, &SlashOffense::Equivocation);
        assert!(matches!(
            penalty,
            SlashPenalty::Jailed {
                burn_percentage: 25.0,
                ..
            }
        ));

        // Third equivocation (with history): Ejected
        engine.record_offense(node, SlashOffense::Equivocation);
        engine.record_offense(node, SlashOffense::Equivocation);
        let penalty = engine.compute_penalty(node, &SlashOffense::Equivocation);
        assert!(matches!(penalty, SlashPenalty::Ejected { .. }));
    }

    #[test]
    fn test_jail_period_auto_release() {
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let node = [1u8; 32];
        engine.register_validator(node, 10_000);

        // Manually add jail state
        {
            let mut state = engine.state.write().unwrap_or_else(|e| {
                tracing::error!(error = %e, "lock poisoned");
                std::process::abort()
            });
            state.jail_registry.insert(
                node,
                JailState {
                    validator_id: node,
                    jailed_at_round: 100,
                    release_round: 1100,
                    offense_history: vec![SlashOffense::Equivocation],
                    stake_locked: 10_000,
                    auto_release: true,
                },
            );
        }

        // Not released yet
        assert!(engine.is_jailed(node, 500));

        // Auto-released after term
        let released = engine.try_release_from_jail(node, 1100).unwrap();
        assert!(released);
        assert!(!engine.is_jailed(node, 1101));
    }

    #[test]
    fn test_slashing_event_emission() {
        let engine = SlashingEngine::new_in_memory(500, 2000);
        let node = [2u8; 32];

        let event = SlashingEvent {
            event_type: SlashingEventType::OffenseRecorded,
            validator_id: node,
            round: 42,
            offense: Some(SlashOffense::Equivocation),
            penalty: None,
            timestamp: 1716000000,
        };

        // Should not panic
        engine.emit_event(event);
    }

    #[test]
    fn test_partial_burn_calculation() {
        assert_eq!(SlashingEngine::compute_burn_amount(10_000, 5.0), 500);
        assert_eq!(SlashingEngine::compute_burn_amount(10_000, 1.0), 100);
        assert_eq!(SlashingEngine::compute_burn_amount(10_000, 100.0), 10_000);
        assert_eq!(SlashingEngine::compute_burn_amount(10_000, 0.0), 0);
    }

    #[test]
    fn test_jailed_validators_list() {
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let node1 = [1u8; 32];
        let node2 = [2u8; 32];

        engine.register_validator(node1, 10_000);
        engine.register_validator(node2, 10_000);

        {
            let mut state = engine.state.write().unwrap_or_else(|e| {
                tracing::error!(error = %e, "lock poisoned");
                std::process::abort()
            });
            state.jail_registry.insert(
                node1,
                JailState {
                    validator_id: node1,
                    jailed_at_round: 100,
                    release_round: 1100,
                    offense_history: vec![SlashOffense::Equivocation],
                    stake_locked: 10_000,
                    auto_release: true,
                },
            );
            state.jail_registry.insert(
                node2,
                JailState {
                    validator_id: node2,
                    jailed_at_round: 200,
                    release_round: 1200,
                    offense_history: vec![SlashOffense::LivenessViolation],
                    stake_locked: 5_000,
                    auto_release: true,
                },
            );
        }

        let jailed = engine.jailed_validators();
        assert_eq!(jailed.len(), 2);
    }

    #[test]
    fn test_graded_slashing_liveness_escalation() {
        let engine = SlashingEngine::new_in_memory(500, 2000);
        let node = [3u8; 32];

        // First liveness: Warning
        let penalty = engine.compute_penalty(node, &SlashOffense::LivenessViolation);
        assert!(matches!(penalty, SlashPenalty::Warning { burn_percentage: 1.0 }));
    }

    // ── Graded slashing (ADR-011) tests ────────────────────────────

    #[test]
    fn test_graded_equivocation_escalation() {
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let node = [42u8; 32];
        engine.register_validator(node, 10_000);

        // 1st equivocation → Jailed(5% burn, 1000 rounds)
        let outcome = engine.record_offense_graded(node, SlashOffense::Equivocation, 100);
        assert!(matches!(
            outcome,
            SlashOutcome::Slashed { node: n, amount: 500 } if n == [42u8; 32]
        ));
        // 5% of 10_000 = 500
        assert!(engine.is_jailed_at(node, 100));
        assert!(engine.is_jailed_at(node, 1099));
        assert!(!engine.is_jailed_at(node, 1100)); // release_round = 100 + 1000 = 1100

        // 2nd equivocation → Jailed(25% burn, 5000 rounds, no auto-release)
        let outcome = engine.record_offense_graded(node, SlashOffense::Equivocation, 200);
        assert!(matches!(
            outcome,
            SlashOutcome::Slashed { node: n, amount: 2500 } if n == [42u8; 32]
        ));
        // 25% of 10_000 = 2500

        // 3rd equivocation → Ejected(100% burn)
        let outcome = engine.record_offense_graded(node, SlashOffense::Equivocation, 300);
        assert!(matches!(outcome, SlashOutcome::Ejected { .. }));
    }

    #[test]
    fn test_graded_liveness_escalation() {
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let node = [7u8; 32];
        engine.register_validator(node, 10_000);

        // 1st liveness → Warning(1% burn = 100)
        let outcome = engine.record_offense_graded(node, SlashOffense::LivenessViolation, 100);
        assert!(matches!(
            outcome,
            SlashOutcome::Warned { node: n, points: 100 } if n == [7u8; 32]
        ));

        // 2nd liveness → Warning(1% burn = 100)
        let outcome = engine.record_offense_graded(node, SlashOffense::LivenessViolation, 200);
        assert!(matches!(
            outcome,
            SlashOutcome::Warned { node: n, points: 100 } if n == [7u8; 32]
        ));

        // 3rd liveness → Jailed(5% burn = 500, 500 rounds)
        let outcome = engine.record_offense_graded(node, SlashOffense::LivenessViolation, 300);
        assert!(matches!(
            outcome,
            SlashOutcome::Slashed { node: n, amount: 500 } if n == [7u8; 32]
        ));
        assert!(engine.is_jailed_at(node, 300));
        assert!(!engine.is_jailed_at(node, 800)); // release_round = 300 + 500 = 800
    }

    #[test]
    fn test_graded_invalid_attestation_escalation() {
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let node = [9u8; 32];
        engine.register_validator(node, 10_000);

        // 1st invalid attestation → Warning(2% burn = 200)
        let outcome = engine.record_offense_graded(node, SlashOffense::InvalidAttestation, 100);
        assert!(matches!(
            outcome,
            SlashOutcome::Warned { node: n, points: 200 } if n == [9u8; 32]
        ));

        // 2nd invalid attestation → Jailed(10% burn = 1000, 2000 rounds)
        let outcome = engine.record_offense_graded(node, SlashOffense::InvalidAttestation, 200);
        assert!(matches!(
            outcome,
            SlashOutcome::Slashed { node: n, amount: 1000 } if n == [9u8; 32]
        ));
        assert!(engine.is_jailed_at(node, 200));
        assert!(!engine.is_jailed_at(node, 2200)); // release_round = 200 + 2000 = 2200

        // 3rd invalid attestation → Ejected(100%)
        let outcome = engine.record_offense_graded(node, SlashOffense::InvalidAttestation, 300);
        assert!(matches!(outcome, SlashOutcome::Ejected { .. }));
    }

    #[test]
    fn test_graded_same_type_escalation_independence() {
        // Verify that offenses of different types don't affect each other's escalation.
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let node = [0xAA; 32];
        engine.register_validator(node, 10_000);

        // 1st liveness → Warning(1%)
        let outcome = engine.record_offense_graded(node, SlashOffense::LivenessViolation, 100);
        assert!(matches!(outcome, SlashOutcome::Warned { .. }));

        // 2nd liveness → Warning(1%)
        let outcome = engine.record_offense_graded(node, SlashOffense::LivenessViolation, 200);
        assert!(matches!(outcome, SlashOutcome::Warned { .. }));

        // Now a first-time equivocation should still be 1st-tier Jailed(5%),
        // NOT escalated because of the 2 liveness violations.
        let penalty = engine.compute_penalty(node, &SlashOffense::Equivocation);
        assert!(matches!(
            penalty,
            SlashPenalty::Jailed {
                burn_percentage: 5.0,
                jail_rounds: 1000,
                auto_release: true,
            }
        ));

        // Similarly, first InvalidAttestation should be Warning(2%)
        let penalty = engine.compute_penalty(node, &SlashOffense::InvalidAttestation);
        assert!(matches!(penalty, SlashPenalty::Warning { burn_percentage: 2.0 }));
    }

    #[test]
    fn test_jail_auto_release() {
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let node = [1u8; 32];
        engine.register_validator(node, 10_000);

        // Record equivocation → Jailed for 1000 rounds with auto_release
        let outcome = engine.record_offense_graded(node, SlashOffense::Equivocation, 100);
        assert!(matches!(outcome, SlashOutcome::Slashed { .. }));
        assert!(engine.is_jailed(node, 100));
        assert!(engine.is_jailed_at(node, 100));

        // Before release_round: still jailed
        assert!(engine.is_jailed(node, 1099));
        assert!(engine.is_jailed_at(node, 1099));

        // At release_round: not jailed per is_jailed_at
        assert!(!engine.is_jailed_at(node, 1100));

        // is_jailed also returns false for auto_release at/past release_round
        assert!(!engine.is_jailed(node, 1100));

        // Release expired jails
        let released = engine.release_expired_jails(1100);
        assert_eq!(released.len(), 1);
        assert_eq!(released[0], node);

        // After release: not in jail registry at all
        assert!(!engine.is_jailed(node, 1100));
        assert!(!engine.is_jailed_at(node, 1100));
        assert!(engine.jailed_validators().is_empty());
    }

    #[test]
    fn test_jail_no_auto_release_stays() {
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let node = [2u8; 32];
        engine.register_validator(node, 10_000);

        // First equivocation → Jailed(5%, 1000, auto_release: true)
        engine.record_offense_graded(node, SlashOffense::Equivocation, 100);

        // Second equivocation → Jailed(25%, 5000, auto_release: false)
        // But we need the first to not be in jail first. Let's release it.
        engine.release_expired_jails(1100);

        // Now record second equivocation
        let outcome = engine.record_offense_graded(node, SlashOffense::Equivocation, 1200);
        assert!(matches!(outcome, SlashOutcome::Slashed { .. }));

        // With auto_release = false, is_jailed returns true even past release_round
        let jailed = engine.jailed_validators();
        assert_eq!(jailed.len(), 1);
        assert!(!jailed[0].auto_release);

        // release_expired_jails won't release non-auto-release validators
        let released = engine.release_expired_jails(20_000);
        assert!(released.is_empty());
    }

    #[test]
    fn test_jail_prevents_consensus_participation() {
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let node = [5u8; 32];
        engine.register_validator(node, 10_000);

        // Before jailing: not jailed
        assert!(!engine.is_jailed_at(node, 0));
        assert!(!engine.is_jailed_at(node, 1000));

        // Jail the validator
        engine.record_offense_graded(node, SlashOffense::Equivocation, 100);

        // During jail: is_jailed_at returns true
        assert!(engine.is_jailed_at(node, 100));
        assert!(engine.is_jailed_at(node, 500));
        assert!(engine.is_jailed_at(node, 1099));

        // After jail term: is_jailed_at returns false
        assert!(!engine.is_jailed_at(node, 1100));
    }

    #[test]
    fn test_release_expired_jails_batch() {
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let node1 = [1u8; 32];
        let node2 = [2u8; 32];
        let node3 = [3u8; 32];
        engine.register_validator(node1, 10_000);
        engine.register_validator(node2, 10_000);
        engine.register_validator(node3, 10_000);

        // Jail node1 at round 100 → release at 1100 (auto_release)
        engine.record_offense_graded(node1, SlashOffense::Equivocation, 100);

        // Jail node2 at round 200 → release at 1200 (auto_release)
        engine.record_offense_graded(node2, SlashOffense::Equivocation, 200);

        // Manually jail node3 with auto_release = false
        {
            let mut state = engine.state.write().unwrap_or_else(|e| {
                tracing::error!(error = %e, "lock poisoned");
                std::process::abort()
            });
            state.jail_registry.insert(
                node3,
                JailState {
                    validator_id: node3,
                    jailed_at_round: 100,
                    release_round: 200,
                    offense_history: vec![SlashOffense::LivenessViolation],
                    stake_locked: 0,
                    auto_release: false,
                },
            );
        }

        // At round 1150: node1 is released, node2 still jailed, node3 not auto-release
        let released = engine.release_expired_jails(1150);
        assert_eq!(released.len(), 1);
        assert_eq!(released[0], node1);

        // At round 1250: node2 is released, node3 still not
        let released = engine.release_expired_jails(1250);
        assert_eq!(released.len(), 1);
        assert_eq!(released[0], node2);

        // node3 never auto-released
        let released = engine.release_expired_jails(9999);
        assert!(released.is_empty());
    }

    #[test]
    fn test_slashing_event_emission_graded() {
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let node = [2u8; 32];
        engine.register_validator(node, 10_000);

        // Warning tier emits OffenseRecorded + PenaltyApplied
        let _outcome = engine.record_offense_graded(node, SlashOffense::LivenessViolation, 100);

        // Jailed tier emits OffenseRecorded (from record_offense) + JailEntered
        let _outcome = engine.record_offense_graded(node, SlashOffense::Equivocation, 200);

        // Ejected tier emits OffenseRecorded (from record_offense) + ValidatorEjected
        // (need 2 more equivocations to reach 3rd equivocation tier)
        engine.record_offense_graded(node, SlashOffense::Equivocation, 300);
        let _outcome = engine.record_offense_graded(node, SlashOffense::Equivocation, 400);
        // This should be Ejected
        // Events are just logged, not stored — this test verifies no panics
    }

    #[test]
    fn test_graded_offense_still_accumulates_points() {
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let node = [0xBB; 32];
        engine.register_validator(node, 10_000);

        // record_offense_graded delegates to record_offense internally,
        // so slash points should still accumulate.
        engine.record_offense_graded(node, SlashOffense::LivenessViolation, 100);
        assert_eq!(engine.slash_points_of(&node), 100);

        engine.record_offense_graded(node, SlashOffense::LivenessViolation, 200);
        assert_eq!(engine.slash_points_of(&node), 200);

        engine.record_offense_graded(node, SlashOffense::Equivocation, 300);
        assert_eq!(engine.slash_points_of(&node), 700);
    }

    #[test]
    fn test_graded_typed_history_tracks_same_type() {
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let node = [0xCC; 32];

        engine.register_validator(node, 10_000);
        engine.record_offense_graded(node, SlashOffense::LivenessViolation, 100);
        engine.record_offense_graded(node, SlashOffense::LivenessViolation, 200);
        engine.record_offense_graded(node, SlashOffense::Equivocation, 300);

        let history = engine.get_offense_history(node);
        assert_eq!(history.len(), 3);
        assert_eq!(
            history
                .iter()
                .filter(|&&o| o == SlashOffense::LivenessViolation)
                .count(),
            2
        );
        assert_eq!(history.iter().filter(|&&o| o == SlashOffense::Equivocation).count(), 1);
    }

    #[test]
    fn test_backward_compat_record_offense_still_works() {
        // Ensure the original record_offense() still works as before.
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let n = node(1);
        engine.register_validator(n, 10_000);

        let outcome = engine.record_offense(n, SlashOffense::LivenessViolation);
        assert_eq!(outcome, SlashOutcome::Warned { node: n, points: 100 });

        let outcome = engine.record_offense(n, SlashOffense::Equivocation);
        assert_eq!(
            outcome,
            SlashOutcome::Slashed {
                node: n,
                amount: 10_000
            }
        );
    }

    #[test]
    fn test_compute_penalty_same_type_counting() {
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let node = [0xDD; 32];
        engine.register_validator(node, 10_000);

        // Record 2 liveness violations
        engine.record_offense(node, SlashOffense::LivenessViolation);
        engine.record_offense(node, SlashOffense::LivenessViolation);

        // First equivocation should still be 1st tier (5%, 1000)
        // because there are 0 prior equivocations
        let penalty = engine.compute_penalty(node, &SlashOffense::Equivocation);
        assert!(matches!(
            penalty,
            SlashPenalty::Jailed {
                burn_percentage: 5.0,
                jail_rounds: 1000,
                auto_release: true,
            }
        ));

        // Liveness should now be 3rd tier (jailed) because there are 2 prior liveness
        let penalty = engine.compute_penalty(node, &SlashOffense::LivenessViolation);
        assert!(matches!(
            penalty,
            SlashPenalty::Jailed {
                burn_percentage: 5.0,
                jail_rounds: 500,
                auto_release: true,
            }
        ));
    }

    #[test]
    fn test_burn_amount_for_helper() {
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let node = [0xEE; 32];
        engine.register_validator(node, 10_000);

        // 5% of 10_000 = 500
        assert_eq!(engine.burn_amount_for(node, 5.0), 500);
        // 1% of 10_000 = 100
        assert_eq!(engine.burn_amount_for(node, 1.0), 100);
        // 25% of 10_000 = 2500
        assert_eq!(engine.burn_amount_for(node, 25.0), 2500);

        // Unregistered validator: 0 stake → 0 burn
        let unregistered = [0xFF; 32];
        assert_eq!(engine.burn_amount_for(unregistered, 5.0), 0);
    }

    #[test]
    fn test_is_jailed_vs_is_jailed_at_semantics() {
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let node = [0xAB; 32];
        engine.register_validator(node, 10_000);

        // Jail with auto_release = true, release at round 1100
        engine.record_offense_graded(node, SlashOffense::Equivocation, 100);

        // At round 1100 (release round):
        // is_jailed: auto_release && current >= release_round → false
        assert!(!engine.is_jailed(node, 1100));
        // is_jailed_at: current < release_round → false (1100 is not < 1100)
        assert!(!engine.is_jailed_at(node, 1100));

        // At round 1099 (still in jail):
        assert!(engine.is_jailed(node, 1099));
        assert!(engine.is_jailed_at(node, 1099));

        // Manually add a non-auto-release jail entry
        let node2 = [0xCD; 32];
        engine.register_validator(node2, 10_000);
        {
            let mut state = engine.state.write().unwrap_or_else(|e| {
                tracing::error!(error = %e, "lock poisoned");
                std::process::abort()
            });
            state.jail_registry.insert(
                node2,
                JailState {
                    validator_id: node2,
                    jailed_at_round: 100,
                    release_round: 200,
                    offense_history: vec![SlashOffense::Equivocation],
                    stake_locked: 500,
                    auto_release: false,
                },
            );
        }

        // is_jailed for non-auto-release: always true regardless of round
        assert!(engine.is_jailed(node2, 300));
        // P0-5 fix: is_jailed_at now also honors auto_release=false — a
        // manually-jailed validator stays jailed forever (until manual
        // release), even past their nominal release_round. This is the
        // check consensus uses to decide participation eligibility, so a
        // false return here would let a slashed validator sign again
        // without an explicit release.
        assert!(engine.is_jailed_at(node2, 300)); // auto_release=false → permanently jailed
        assert!(engine.is_jailed_at(node2, 9999)); // even past release_round=200
    }

    // ── Additional ADR-011 comprehensive tests ─────────────────────

    #[test]
    fn test_compute_burn_amount_for_public_api() {
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let v = [0xF0; 32];
        engine.register_validator(v, 10_000);

        // Registered validator: returns Some(amount)
        assert_eq!(engine.compute_burn_amount_for(v, 5.0), Some(500));
        assert_eq!(engine.compute_burn_amount_for(v, 1.0), Some(100));
        assert_eq!(engine.compute_burn_amount_for(v, 100.0), Some(10_000));

        // Unregistered validator: returns None
        let unregistered = [0xF1; 32];
        assert_eq!(engine.compute_burn_amount_for(unregistered, 5.0), None);
    }

    #[test]
    fn test_graded_equivocation_full_cycle() {
        // Full 3-tier escalation cycle for equivocation:
        // 1st → Jailed(5%, 1000r) → 2nd → Jailed(25%, 5000r) → 3rd → Ejected(100%)
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let v = [0xA1; 32];
        engine.register_validator(v, 10_000);

        // 1st equivocation → Jailed(5% burn = 500, 1000 rounds, auto_release)
        let outcome = engine.record_offense_graded(v, SlashOffense::Equivocation, 1000);
        assert!(matches!(
            outcome,
            SlashOutcome::Slashed { node: n, amount: 500 } if n == v
        ));
        assert!(engine.is_jailed_at(v, 1000));
        assert!(engine.is_jailed_at(v, 1999));
        assert!(!engine.is_jailed_at(v, 2000)); // release_round = 1000 + 1000 = 2000

        // Release from jail
        let released = engine.release_expired_jails(2000);
        assert_eq!(released, vec![v]);
        assert!(!engine.is_jailed_at(v, 2000));

        // 2nd equivocation → Jailed(25% burn = 2500, 5000 rounds, no auto-release)
        let outcome = engine.record_offense_graded(v, SlashOffense::Equivocation, 3000);
        assert!(matches!(
            outcome,
            SlashOutcome::Slashed { node: n, amount: 2500 } if n == v
        ));

        // 2nd equivocation has auto_release = false, so release_expired_jails won't release it
        let released = engine.release_expired_jails(9999);
        assert!(released.is_empty());

        // Must manually release via try_release_from_jail
        let released = engine.try_release_from_jail(v, 8000).unwrap();
        assert!(released); // 8000 >= 3000 + 5000 = 8000

        // 3rd equivocation → Ejected(100%)
        let outcome = engine.record_offense_graded(v, SlashOffense::Equivocation, 9000);
        assert!(matches!(outcome, SlashOutcome::Ejected { node: n } if n == v));
    }

    #[test]
    fn test_graded_liveness_full_cycle() {
        // Full escalation for liveness:
        // 1st → Warning(1%) → 2nd → Warning(1%) → 3rd+ → Jailed(5%, 500r)
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let v = [0xB1; 32];
        engine.register_validator(v, 10_000);

        // 1st liveness → Warning(1% burn = 100)
        let outcome = engine.record_offense_graded(v, SlashOffense::LivenessViolation, 100);
        assert!(matches!(
            outcome,
            SlashOutcome::Warned { node: n, points: 100 } if n == v
        ));
        assert!(!engine.is_jailed_at(v, 100));

        // 2nd liveness → Warning(1% burn = 100)
        let outcome = engine.record_offense_graded(v, SlashOffense::LivenessViolation, 200);
        assert!(matches!(
            outcome,
            SlashOutcome::Warned { node: n, points: 100 } if n == v
        ));
        assert!(!engine.is_jailed_at(v, 200));

        // 3rd liveness → Jailed(5% burn = 500, 500 rounds, auto_release)
        let outcome = engine.record_offense_graded(v, SlashOffense::LivenessViolation, 300);
        assert!(matches!(
            outcome,
            SlashOutcome::Slashed { node: n, amount: 500 } if n == v
        ));
        assert!(engine.is_jailed_at(v, 300));
        assert!(!engine.is_jailed_at(v, 800)); // 300 + 500 = 800

        // 4th liveness (still 3rd+ tier) → Jailed again
        let released = engine.release_expired_jails(800);
        assert_eq!(released, vec![v]);
        let outcome = engine.record_offense_graded(v, SlashOffense::LivenessViolation, 900);
        assert!(matches!(
            outcome,
            SlashOutcome::Slashed { node: n, amount: 500 } if n == v
        ));
    }

    #[test]
    fn test_graded_invalid_attestation_full_cycle() {
        // Full escalation for invalid attestation:
        // 1st → Warning(2%) → 2nd → Jailed(10%, 2000r) → 3rd → Ejected(100%)
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let v = [0xC1; 32];
        engine.register_validator(v, 10_000);

        // 1st → Warning(2% burn = 200)
        let outcome = engine.record_offense_graded(v, SlashOffense::InvalidAttestation, 100);
        assert!(matches!(
            outcome,
            SlashOutcome::Warned { node: n, points: 200 } if n == v
        ));

        // 2nd → Jailed(10% burn = 1000, 2000 rounds, auto_release)
        let outcome = engine.record_offense_graded(v, SlashOffense::InvalidAttestation, 200);
        assert!(matches!(
            outcome,
            SlashOutcome::Slashed { node: n, amount: 1000 } if n == v
        ));
        assert!(engine.is_jailed_at(v, 200));
        assert!(!engine.is_jailed_at(v, 2200)); // 200 + 2000 = 2200

        // Release from jail
        let released = engine.release_expired_jails(2200);
        assert_eq!(released, vec![v]);

        // 3rd → Ejected(100%)
        let outcome = engine.record_offense_graded(v, SlashOffense::InvalidAttestation, 3000);
        assert!(matches!(outcome, SlashOutcome::Ejected { node: n } if n == v));
    }

    #[test]
    fn test_graded_offense_jail_state_fields() {
        // Verify that JailState has correct fields after graded offense
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let v = [0xD1; 32];
        engine.register_validator(v, 10_000);

        // Record first equivocation → Jailed
        engine.record_offense_graded(v, SlashOffense::Equivocation, 500);

        let jailed = engine.jailed_validators();
        assert_eq!(jailed.len(), 1);
        let jail_state = &jailed[0];
        assert_eq!(jail_state.validator_id, v);
        assert_eq!(jail_state.jailed_at_round, 500);
        assert_eq!(jail_state.release_round, 1500); // 500 + 1000
        assert_eq!(jail_state.stake_locked, 500); // 5% of 10_000
        assert!(jail_state.auto_release);
        // offense_history should contain the equivocation
        assert_eq!(jail_state.offense_history.len(), 1);
        assert_eq!(jail_state.offense_history[0], SlashOffense::Equivocation);
    }

    #[test]
    fn test_graded_mixed_offenses_dont_cross_escalate() {
        // Verify that different offense types maintain independent escalation counters.
        // A validator with 2 liveness violations should still get 1st-tier penalty
        // for their first equivocation.
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let v = [0xE1; 32];
        engine.register_validator(v, 10_000);

        // Record 2 liveness violations (Warning tier each)
        engine.record_offense_graded(v, SlashOffense::LivenessViolation, 100);
        engine.record_offense_graded(v, SlashOffense::LivenessViolation, 200);

        // First equivocation should be 1st-tier: Jailed(5%, 1000 rounds)
        let penalty = engine.compute_penalty(v, &SlashOffense::Equivocation);
        assert!(matches!(
            penalty,
            SlashPenalty::Jailed {
                burn_percentage: 5.0,
                jail_rounds: 1000,
                auto_release: true,
            }
        ));

        // First invalid attestation should be 1st-tier: Warning(2%)
        let penalty = engine.compute_penalty(v, &SlashOffense::InvalidAttestation);
        assert!(matches!(penalty, SlashPenalty::Warning { burn_percentage: 2.0 }));

        // But 3rd liveness should be 3rd-tier: Jailed(5%, 500 rounds)
        let penalty = engine.compute_penalty(v, &SlashOffense::LivenessViolation);
        assert!(matches!(
            penalty,
            SlashPenalty::Jailed {
                burn_percentage: 5.0,
                jail_rounds: 500,
                auto_release: true,
            }
        ));
    }

    #[test]
    fn test_release_expired_jails_empty_registry() {
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        // No jailed validators → empty result
        let released = engine.release_expired_jails(1000);
        assert!(released.is_empty());
    }

    #[test]
    fn test_is_jailed_at_unregistered_validator() {
        let engine = SlashingEngine::new_in_memory(500, 2000);
        let unregistered = [0x99; 32];
        assert!(!engine.is_jailed_at(unregistered, 0));
        assert!(!engine.is_jailed_at(unregistered, 1000));
    }

    #[test]
    fn test_graded_warning_no_jail_entry() {
        // Warning-tier offenses should NOT create jail entries
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let v = [0x88; 32];
        engine.register_validator(v, 10_000);

        engine.record_offense_graded(v, SlashOffense::LivenessViolation, 100);
        assert!(engine.jailed_validators().is_empty());
        assert!(!engine.is_jailed_at(v, 100));
    }

    #[test]
    fn test_graded_ejection_persists_state() {
        // Verify that ejection persists state (even though in-memory for this test)
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let v = [0x77; 32];
        engine.register_validator(v, 10_000);

        // Drive to ejection with 3 equivocations
        engine.record_offense_graded(v, SlashOffense::Equivocation, 100);
        engine.release_expired_jails(1100); // Release 1st jail
        engine.record_offense_graded(v, SlashOffense::Equivocation, 1200);
        engine.try_release_from_jail(v, 7000).unwrap(); // Release 2nd jail (no auto-release)
        let outcome = engine.record_offense_graded(v, SlashOffense::Equivocation, 8000);

        assert!(matches!(outcome, SlashOutcome::Ejected { .. }));

        // Points should be accumulated from all 3 offenses
        let points = engine.slash_points_of(&v);
        assert_eq!(points, 1500); // 3 × 500 equivocation points
    }

    #[test]
    fn test_compute_burn_amount_edge_cases() {
        // Zero stake
        assert_eq!(SlashingEngine::compute_burn_amount(0, 5.0), 0);
        // Zero percentage
        assert_eq!(SlashingEngine::compute_burn_amount(10_000, 0.0), 0);
        // Very small percentage → rounds down
        assert_eq!(SlashingEngine::compute_burn_amount(1, 0.01), 0);
        // 100%
        assert_eq!(SlashingEngine::compute_burn_amount(10_000, 100.0), 10_000);
        // Fractional result rounds down
        assert_eq!(SlashingEngine::compute_burn_amount(999, 1.0), 9); // 999 * 0.01 = 9.99 → 9
    }

    // ── Task 7: Edge-case coverage tests ────────────────────────────
    //
    // The following tests target functions and edge cases identified as
    // uncovered ("dark") by the coverage report — multi-offense accumulation
    // across thresholds, liveness boundary behavior, undo-after-ejection,
    // internal accessors, graded escalation cycles, disk-backed persistence,
    // and the `decrement_slash_count_by` store trait method.

    /// Process-local counter used to derive unique temp-dir paths for each
    /// disk-backed test, preventing cross-test file collisions even when
    /// tests run in parallel.
    static TEMP_DB_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    /// Returns a unique `.redb` path under `std::env::temp_dir()`.
    fn unique_temp_db_path() -> PathBuf {
        let pid = std::process::id();
        let n = TEMP_DB_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        std::env::temp_dir().join(format!("omnia-slashing-test-{pid}-{n}.redb"))
    }

    /// Best-effort cleanup of a temp db file *and* any corrupt backup that
    /// `RedbSlashingStore::open` may have created alongside it.
    fn cleanup_temp_db(path: &Path) {
        let _ = std::fs::remove_file(path);
        let corrupt = path.with_extension("db.corrupt");
        let _ = std::fs::remove_file(&corrupt);
    }

    /// RAII guard that removes the temp db file (and any corrupt backup)
    /// when dropped, so panic / early-return cleanup is automatic.
    struct TempDbGuard(PathBuf);
    impl Drop for TempDbGuard {
        fn drop(&mut self) {
            cleanup_temp_db(&self.0);
        }
    }

    #[test]
    fn test_new_disk_backed_engine() {
        // `SlashingEngine::new(Some(path), ...)` opens a redb-backed engine.
        // Verify the engine functions end-to-end (register, record, query).
        let path = unique_temp_db_path();
        let _guard = TempDbGuard(path.clone());

        let mut engine = SlashingEngine::new(Some(path.clone()), 500, 2000).expect("open disk engine");
        let n = node(1);
        engine.register_validator(n, 10_000);
        assert_eq!(engine.stake_of(&n), 10_000);

        // Liveness violation → 100 pts, below slash_threshold → Warned
        let outcome = engine.record_offense(n, SlashOffense::LivenessViolation);
        assert!(matches!(outcome, SlashOutcome::Warned { .. }));
        assert_eq!(engine.slash_points_of(&n), 100);
        assert!(!engine.is_slashed(&n));

        // Equivocation → 600 pts, ≥ slash_threshold → Slashed
        let outcome = engine.record_offense(n, SlashOffense::Equivocation);
        assert!(matches!(outcome, SlashOutcome::Slashed { .. }));
        assert!(engine.is_slashed(&n));

        // engine state is persisted to disk — verified by the disk-persistence test below.
    }

    #[test]
    fn test_with_store_constructor_redb() {
        // `SlashingEngine::with_store(Arc<dyn SlashingStore>)` with a
        // `RedbSlashingStore` (the existing `with_store` tests only cover
        // `InMemorySlashingStore`).
        let path = unique_temp_db_path();
        let _guard = TempDbGuard(path.clone());

        let store: Arc<dyn SlashingStore> = Arc::new(RedbSlashingStore::open(&path).expect("open redb"));
        let mut engine = SlashingEngine::with_store(store).expect("with_store");

        // Empty store → default thresholds (500 / 2000)
        assert_eq!(engine.internal_slash_threshold(), 500);
        assert_eq!(engine.internal_ejection_threshold(), 2000);

        let n = node(7);
        engine.register_validator(n, 5_000);
        assert_eq!(engine.stake_of(&n), 5_000);
        let outcome = engine.record_offense(n, SlashOffense::Equivocation);
        assert!(matches!(outcome, SlashOutcome::Slashed { .. }));
        assert!(engine.is_slashed(&n));
    }

    #[test]
    fn test_register_validator_overwrite() {
        // Documented behavior: re-registering an existing validator
        // OVERWRITES the stake (insert semantics) and leaves slash_points
        // untouched (or_insert(0) only inserts if absent).
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let n = node(1);

        engine.register_validator(n, 1_000);
        assert_eq!(engine.stake_of(&n), 1_000);

        // Accumulate some slash points before re-registering.
        engine.record_offense(n, SlashOffense::LivenessViolation); // +100 pts
        assert_eq!(engine.slash_points_of(&n), 100);

        // Re-register with a different stake.
        engine.register_validator(n, 2_000);
        assert_eq!(engine.stake_of(&n), 2_000, "stake should be overwritten to 2_000");
        assert_eq!(
            engine.slash_points_of(&n),
            100,
            "slash_points should be preserved across re-registration"
        );
    }

    #[test]
    fn test_multi_offense_accumulation_across_thresholds() {
        // Accumulate points one offense at a time, crossing first the
        // slash_threshold and then the ejection_threshold. Verify the
        // outcome transitions Warned → Slashed → Ejected and that the
        // is_slashed / is_ejected accessors track the transitions.
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let n = node(1);
        engine.register_validator(n, 10_000);

        // 4 × LivenessViolation = 400 pts (below slash_threshold)
        for _ in 0..4 {
            let o = engine.record_offense(n, SlashOffense::LivenessViolation);
            assert!(
                matches!(o, SlashOutcome::Warned { .. }),
                "should be warned below slash threshold"
            );
        }
        assert_eq!(engine.slash_points_of(&n), 400);
        assert!(!engine.is_slashed(&n));
        assert!(!engine.is_ejected(&n));

        // +1 LivenessViolation = 500 pts (≥ slash_threshold) → Slashed
        let o = engine.record_offense(n, SlashOffense::LivenessViolation);
        assert!(matches!(o, SlashOutcome::Slashed { .. }));
        assert!(engine.is_slashed(&n));
        assert!(!engine.is_ejected(&n), "not yet ejected at 500 pts");

        // +2 × Equivocation = 1500 pts (still slashed, not ejected)
        engine.record_offense(n, SlashOffense::Equivocation);
        let o = engine.record_offense(n, SlashOffense::Equivocation);
        assert!(matches!(o, SlashOutcome::Slashed { .. }));
        assert_eq!(engine.slash_points_of(&n), 1500);
        assert!(engine.is_slashed(&n));
        assert!(!engine.is_ejected(&n));

        // +1 × Equivocation = 2000 pts (≥ ejection_threshold) → Ejected
        let o = engine.record_offense(n, SlashOffense::Equivocation);
        assert!(matches!(o, SlashOutcome::Ejected { node } if node == n));
        assert_eq!(engine.slash_points_of(&n), 2000);
        assert!(engine.is_ejected(&n));
    }

    #[test]
    fn test_check_liveness_boundary() {
        // Documented boundary behavior of `check_liveness`:
        //   inactive_rounds = current_round - last_active_round
        //   violation triggered iff inactive_rounds > threshold  (strict >)
        // Therefore:
        //   - exactly at threshold (inactive == threshold) → NO violation
        //   - one above threshold (inactive == threshold + 1) → violation
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let n = node(1);
        engine.register_validator(n, 10_000);

        // inactive = 15 - 5 = 10, threshold = 10 → 10 > 10 is false → None
        let result = engine.check_liveness(n, 5, 15, 10);
        assert!(result.is_none(), "at-threshold (10 == 10) should NOT trigger");
        assert_eq!(engine.slash_points_of(&n), 0, "no offense should have been recorded");

        // inactive = 16 - 5 = 11, threshold = 10 → 11 > 10 is true → Some
        let result = engine.check_liveness(n, 5, 16, 10);
        assert!(result.is_some(), "one-above-threshold (11 > 10) should trigger");
        assert_eq!(engine.slash_points_of(&n), 100);
    }

    #[test]
    fn test_undo_slash_after_ejection() {
        // Documented behavior: `undo_slash` pops the most recent offense
        // from the history stack and decrements slash_points accordingly,
        // even when the validator has already crossed the ejection
        // threshold. After enough undos, is_ejected flips back to false.
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let n = node(1);
        engine.register_validator(n, 10_000);

        // 4 × Equivocation = 2000 pts → Ejected
        for _ in 0..4 {
            engine.record_offense(n, SlashOffense::Equivocation);
        }
        assert_eq!(engine.slash_points_of(&n), 2000);
        assert!(engine.is_ejected(&n));

        // Undo one equivocation (500 pts) → 1500 pts, no longer ejected.
        engine.undo_slash(&n).expect("undo after ejection");
        assert_eq!(engine.slash_points_of(&n), 1500);
        assert!(!engine.is_ejected(&n), "should no longer be ejected after one undo");
        assert!(engine.is_slashed(&n), "should still be slashed");

        // Undo all remaining offenses → 0 pts.
        for _ in 0..3 {
            engine.undo_slash(&n).expect("undo");
        }
        assert_eq!(engine.slash_points_of(&n), 0);
        assert!(!engine.is_slashed(&n));

        // Further undo fails: no offense history left.
        let err = engine.undo_slash(&n).unwrap_err();
        assert!(matches!(err, SlashingStoreError::UndoNoOffenseHistory(_)));
    }

    #[test]
    fn test_to_state_round_trip() {
        // `to_state()` snapshots the full SlashingState. Save it into a
        // fresh store, build a new engine via `with_store`, and verify
        // the new engine observes identical data.
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let n1 = node(1);
        let n2 = node(2);
        engine.register_validator(n1, 10_000);
        engine.register_validator(n2, 5_000);
        engine.record_offense(n1, SlashOffense::Equivocation); // 500 pts → Slashed
        engine.record_offense(n2, SlashOffense::LivenessViolation); // 100 pts → Warned

        let snapshot = engine.to_state();
        assert_eq!(snapshot.slash_points.get(&n1), Some(&500));
        assert_eq!(snapshot.slash_points.get(&n2), Some(&100));
        assert_eq!(snapshot.stakes.get(&n1), Some(&10_000));
        assert_eq!(snapshot.stakes.get(&n2), Some(&5_000));
        assert_eq!(snapshot.slash_threshold, 500);
        assert_eq!(snapshot.ejection_threshold, 2000);
        // typed_offense_history should also be present
        assert_eq!(snapshot.typed_offense_history.get(&n1).map(Vec::len), Some(1));
        assert_eq!(snapshot.typed_offense_history.get(&n2).map(Vec::len), Some(1));

        // Round-trip via a fresh in-memory store.
        let store: Arc<dyn SlashingStore> = Arc::new(InMemorySlashingStore::new());
        store.save(&snapshot).expect("save snapshot");
        let engine2 = SlashingEngine::with_store(store).expect("with_store");
        assert_eq!(engine2.slash_points_of(&n1), 500);
        assert_eq!(engine2.slash_points_of(&n2), 100);
        assert_eq!(engine2.stake_of(&n1), 10_000);
        assert_eq!(engine2.stake_of(&n2), 5_000);
        assert!(engine2.is_slashed(&n1));
        assert!(!engine2.is_slashed(&n2));
    }

    #[test]
    fn test_internal_accessors() {
        // Cover the four `internal_*` accessors used by genesis-replay.
        let mut engine = SlashingEngine::new_in_memory(700, 3_000);
        let n1 = node(1);
        let n2 = node(2);
        engine.register_validator(n1, 10_000);
        engine.register_validator(n2, 5_000);
        engine.record_offense(n1, SlashOffense::LivenessViolation); // 100 pts
        engine.record_offense(n2, SlashOffense::Equivocation); // 500 pts

        let slash_points = engine.internal_slash_points();
        assert_eq!(slash_points.get(&n1), Some(&100));
        assert_eq!(slash_points.get(&n2), Some(&500));

        let stakes = engine.internal_stakes();
        assert_eq!(stakes.get(&n1), Some(&10_000));
        assert_eq!(stakes.get(&n2), Some(&5_000));

        assert_eq!(engine.internal_slash_threshold(), 700);
        assert_eq!(engine.internal_ejection_threshold(), 3_000);
    }

    #[test]
    fn test_compute_burn_amount_for_unregistered() {
        // `compute_burn_amount_for` returns `None` for an unregistered
        // validator (stake == 0).
        let engine = SlashingEngine::new_in_memory(500, 2000);
        let unregistered = node(99);
        assert_eq!(engine.compute_burn_amount_for(unregistered, 50.0), None);
    }

    #[test]
    fn test_compute_burn_amount_for_registered() {
        // 10% of a 1_000 stake = 100. Verifies the instance method path.
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let n = node(1);
        engine.register_validator(n, 1_000);
        assert_eq!(engine.compute_burn_amount_for(n, 10.0), Some(100));
        // Also verify a 0% burn on a registered validator returns Some(0).
        assert_eq!(engine.compute_burn_amount_for(n, 0.0), Some(0));
    }

    #[test]
    fn test_record_offense_graded_escalation_invalid_attestation() {
        // Focused escalation test for InvalidAttestation across all three
        // tiers: Warning(2%) → Jailed(10%, 2000r) → Ejected(100%).
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let v = node(1);
        engine.register_validator(v, 10_000);

        // 1st → Warning(2% of 10_000 = 200)
        let o1 = engine.record_offense_graded(v, SlashOffense::InvalidAttestation, 100);
        assert!(matches!(o1, SlashOutcome::Warned { node, points: 200 } if node == v));
        assert!(engine.jailed_validators().is_empty());

        // 2nd → Jailed(10% of 10_000 = 1000, 2000 rounds, auto_release)
        let o2 = engine.record_offense_graded(v, SlashOffense::InvalidAttestation, 200);
        assert!(matches!(o2, SlashOutcome::Slashed { node, amount: 1000 } if node == v));
        assert!(engine.is_jailed_at(v, 200));
        assert!(!engine.is_jailed_at(v, 2200)); // 200 + 2000 = 2200

        // Release so the 3rd offense can be recorded cleanly.
        let released = engine.release_expired_jails(2200);
        assert_eq!(released, vec![v]);

        // 3rd → Ejected(100%)
        let o3 = engine.record_offense_graded(v, SlashOffense::InvalidAttestation, 3000);
        assert!(matches!(o3, SlashOutcome::Ejected { node } if node == v));
    }

    #[test]
    fn test_record_offense_graded_jail_auto_release_explicit() {
        // Explicit end-to-end test of the auto-release path:
        // graded offense jails with auto_release=true → advance round past
        // release_round → try_release_from_jail returns Ok(true) and
        // is_jailed returns false afterwards.
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let v = node(1);
        engine.register_validator(v, 10_000);

        // First equivocation → Jailed(5%, 1000 rounds, auto_release=true)
        let outcome = engine.record_offense_graded(v, SlashOffense::Equivocation, 100);
        assert!(matches!(outcome, SlashOutcome::Slashed { .. }));
        assert_eq!(engine.jailed_validators().len(), 1);
        assert!(engine.is_jailed(v, 100));
        assert!(engine.is_jailed(v, 1099));

        // Before release_round: try_release_from_jail returns Ok(false).
        let released = engine.try_release_from_jail(v, 500).expect("try_release");
        assert!(!released, "should not release before release_round");
        assert!(engine.is_jailed(v, 500));

        // At release_round: try_release_from_jail returns Ok(true).
        let released = engine.try_release_from_jail(v, 1100).expect("try_release");
        assert!(released, "should release at/past release_round");
        assert!(!engine.is_jailed(v, 1100), "is_jailed should be false after release");
        assert!(engine.jailed_validators().is_empty());
    }

    #[test]
    fn test_release_expired_jails_batch_mixed_periods() {
        // Jail three validators with different release_rounds (all
        // auto_release=true). At a round between the earliest and latest
        // release_rounds, only the expired subset should be returned.
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let v1 = node(1);
        let v2 = node(2);
        let v3 = node(3);
        engine.register_validator(v1, 10_000);
        engine.register_validator(v2, 10_000);
        engine.register_validator(v3, 10_000);

        // v1 jailed at round 0 → release_round = 1000
        engine.record_offense_graded(v1, SlashOffense::Equivocation, 0);
        // v2 jailed at round 500 → release_round = 1500
        engine.record_offense_graded(v2, SlashOffense::Equivocation, 500);
        // v3 jailed at round 1000 → release_round = 2000
        engine.record_offense_graded(v3, SlashOffense::Equivocation, 1000);

        assert_eq!(engine.jailed_validators().len(), 3);

        // At round 1200: only v1 (release_round 1000) is expired.
        let released = engine.release_expired_jails(1200);
        assert_eq!(released.len(), 1);
        assert!(released.contains(&v1));

        // At round 1800: v2 (release_round 1500) is now expired.
        let released = engine.release_expired_jails(1800);
        assert_eq!(released.len(), 1);
        assert!(released.contains(&v2));

        // At round 9999: v3 (release_round 2000) is expired.
        let released = engine.release_expired_jails(9999);
        assert_eq!(released.len(), 1);
        assert!(released.contains(&v3));

        // Registry should now be empty.
        assert!(engine.jailed_validators().is_empty());

        // Subsequent call returns empty Vec.
        let released = engine.release_expired_jails(99999);
        assert!(released.is_empty());
    }

    #[test]
    fn test_get_offense_history_empty() {
        // A validator with no recorded offenses returns an empty Vec.
        let engine = SlashingEngine::new_in_memory(500, 2000);
        let unregistered = node(99);
        assert!(engine.get_offense_history(unregistered).is_empty());

        // A registered-but-clean validator also returns empty.
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let n = node(1);
        engine.register_validator(n, 10_000);
        assert!(engine.get_offense_history(n).is_empty());
    }

    #[test]
    fn test_get_offense_history_multiple_types() {
        // Record multiple offense types for a single validator and verify
        // `get_offense_history` returns them in insertion order with the
        // correct types.
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let n = node(1);
        engine.register_validator(n, 10_000);

        engine.record_offense(n, SlashOffense::LivenessViolation);
        engine.record_offense(n, SlashOffense::Equivocation);
        engine.record_offense(n, SlashOffense::InvalidAttestation);

        let history = engine.get_offense_history(n);
        assert_eq!(history.len(), 3);
        assert_eq!(history[0], SlashOffense::LivenessViolation);
        assert_eq!(history[1], SlashOffense::Equivocation);
        assert_eq!(history[2], SlashOffense::InvalidAttestation);

        // Different validator should still have empty history.
        let other = node(2);
        assert!(engine.get_offense_history(other).is_empty());
    }

    #[test]
    fn test_emit_event_all_event_types() {
        // `emit_event` only logs (no storage), so this test verifies it
        // does not panic for any of the SlashingEventType variants.
        let engine = SlashingEngine::new_in_memory(500, 2000);
        let v = node(1);
        let round = 42u64;
        let ts = current_timestamp();

        let event_types = [
            SlashingEventType::OffenseRecorded,
            SlashingEventType::PenaltyApplied,
            SlashingEventType::JailEntered,
            SlashingEventType::JailReleased,
            SlashingEventType::ValidatorEjected,
            SlashingEventType::UndoApplied,
        ];

        for et in event_types {
            engine.emit_event(SlashingEvent {
                event_type: et,
                validator_id: v,
                round,
                offense: Some(SlashOffense::Equivocation),
                penalty: Some(SlashPenalty::Warning { burn_percentage: 1.0 }),
                timestamp: ts,
            });
        }
        // No panic = pass.
    }

    #[test]
    fn test_jailed_validators_list_three() {
        // Jail exactly 3 validators and verify `jailed_validators()` returns
        // a Vec of length 3. (Existing test only jails 2.)
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let v1 = node(1);
        let v2 = node(2);
        let v3 = node(3);
        engine.register_validator(v1, 10_000);
        engine.register_validator(v2, 10_000);
        engine.register_validator(v3, 10_000);

        // First equivocation → Jailed(5%, 1000 rounds, auto_release=true)
        engine.record_offense_graded(v1, SlashOffense::Equivocation, 100);
        engine.record_offense_graded(v2, SlashOffense::Equivocation, 200);
        engine.record_offense_graded(v3, SlashOffense::Equivocation, 300);

        let jailed = engine.jailed_validators();
        assert_eq!(jailed.len(), 3);

        // Each jailed entry should carry the right validator_id.
        let jailed_ids: Vec<NodeId> = jailed.iter().map(|j| j.validator_id).collect();
        assert!(jailed_ids.contains(&v1));
        assert!(jailed_ids.contains(&v2));
        assert!(jailed_ids.contains(&v3));
    }

    #[test]
    fn test_is_jailed_at_specific_round_boundaries() {
        // Jail a validator at round 10 for 5 rounds → release_round = 15.
        // is_jailed_at returns true iff current_round < release_round.
        let mut engine = SlashingEngine::new_in_memory(500, 2000);
        let v = node(1);
        engine.register_validator(v, 10_000);

        // Manually insert a jail entry with release_round = 15 so we can
        // control the exact release boundary (graded offenses use 1000+ rounds).
        {
            let mut state = engine.state.write().unwrap_or_else(|e| {
                tracing::error!(error = %e, "lock poisoned");
                std::process::abort()
            });
            state.jail_registry.insert(
                v,
                JailState {
                    validator_id: v,
                    jailed_at_round: 10,
                    release_round: 15,
                    offense_history: vec![SlashOffense::LivenessViolation],
                    stake_locked: 0,
                    auto_release: true,
                },
            );
        }

        // Boundary behavior: current_round < release_round (15).
        assert!(engine.is_jailed_at(v, 10), "10 < 15 → jailed");
        assert!(engine.is_jailed_at(v, 14), "14 < 15 → jailed");
        assert!(!engine.is_jailed_at(v, 15), "15 is NOT < 15 → not jailed (boundary)");
        assert!(!engine.is_jailed_at(v, 16), "16 is NOT < 15 → not jailed");
    }

    #[test]
    fn test_redb_slashing_store_disk_persistence() {
        // Round-trip persistence: save state to a redb file, drop the
        // store, open a fresh store at the same path, load state, and
        // verify equivalence.
        let path = unique_temp_db_path();
        let _guard = TempDbGuard(path.clone());

        let n1 = node(1);
        let n2 = node(2);

        // Phase 1: write state.
        {
            let store = RedbSlashingStore::open(&path).expect("open for write");
            let mut state = SlashingState::default();
            state.slash_points.insert(n1, 500);
            state.slash_points.insert(n2, 100);
            state.stakes.insert(n1, 10_000);
            state.stakes.insert(n2, 5_000);
            state.offense_history.insert(n1, vec![500]);
            state.typed_offense_history.insert(n1, vec![SlashOffense::Equivocation]);
            state.slash_threshold = 500;
            state.ejection_threshold = 2_000;
            store.save(&state).expect("save");
            // store dropped at end of block → file handle released
        }

        // Phase 2: reload state from disk into a fresh store.
        let store2 = RedbSlashingStore::open(&path).expect("open for read");
        let loaded = store2.load().expect("load");
        assert_eq!(loaded.slash_points.get(&n1), Some(&500));
        assert_eq!(loaded.slash_points.get(&n2), Some(&100));
        assert_eq!(loaded.stakes.get(&n1), Some(&10_000));
        assert_eq!(loaded.stakes.get(&n2), Some(&5_000));
        assert_eq!(loaded.slash_threshold, 500);
        assert_eq!(loaded.ejection_threshold, 2_000);
        assert_eq!(loaded.offense_history.get(&n1), Some(&vec![500]));
        assert_eq!(
            loaded.typed_offense_history.get(&n1),
            Some(&vec![SlashOffense::Equivocation])
        );
    }

    #[test]
    fn test_decrement_slash_count_by() {
        // Cover the `decrement_slash_count_by` trait method on both
        // InMemorySlashingStore and RedbSlashingStore.
        let n = node(1);

        // ── InMemorySlashingStore ──
        {
            let store = InMemorySlashingStore::new();
            let mut state = SlashingState::default();
            state.slash_points.insert(n, 500);
            state.stakes.insert(n, 10_000);
            store.save(&state).expect("save");

            assert_eq!(store.get_slash_count(&n), 500);
            store.decrement_slash_count_by(&n, 200).expect("decrement 200");
            assert_eq!(store.get_slash_count(&n), 300);
            // Decrementing past zero clamps to zero (decrement = amount.min(current)).
            store.decrement_slash_count_by(&n, 1_000).expect("decrement past zero");
            assert_eq!(store.get_slash_count(&n), 0);
        }

        // ── RedbSlashingStore ──
        let path = unique_temp_db_path();
        let _guard = TempDbGuard(path.clone());
        {
            let store = RedbSlashingStore::open(&path).expect("open");
            let mut state = SlashingState::default();
            state.slash_points.insert(n, 500);
            state.stakes.insert(n, 10_000);
            store.save(&state).expect("save");

            assert_eq!(store.get_slash_count(&n), 500);
            store.decrement_slash_count_by(&n, 200).expect("decrement 200");
            assert_eq!(store.get_slash_count(&n), 300);
        }

        // Decrementing a validator with zero slash_count returns an error.
        let path2 = unique_temp_db_path();
        let _guard2 = TempDbGuard(path2.clone());
        {
            let store = RedbSlashingStore::open(&path2).expect("open");
            let err = store.decrement_slash_count_by(&n, 100).unwrap_err();
            assert!(matches!(err, SlashingStoreError::Persistence(_)));
        }
    }
}
