//! # Omnia Protocol — Layer 1: The Substrate
//!
//! The Substrate is the foundational layer of the Omnia Protocol. It provides:
//!
//! - **Causal Graph (DAG)**: A directed acyclic graph of events for tracking
//!   causality and enabling parallel transaction processing
//! - **Vector Clocks**: Logical clocks for determining happened-before relationships
//!   without any centralized time source
//! - **CRDTs**: Conflict-free Replicated Data Types for deterministic state
//!   convergence without coordination
//! - **Gossip Protocol**: Epidemic event propagation for efficient, fault-tolerant
//!   distribution across the network
//! - **Consensus**: BFT finality mechanism for deterministic event ordering
//!
//! The substrate also supports **Layer 2 event processors** (such as domain
//! shards) through the `EventProcessor` trait. Processors can be attached to
//! the substrate via `with_shard_processor()` and are invoked automatically
//! in the main run loop for each newly-committed event.
//!
//! # Role in the workspace
//!
//! `omnia-substrate` is the **integration crate** for the Omnia Protocol.
//! It re-exports and coordinates the core crates — `omnia-primitives`,
//! `omnia-consensus`, `omnia-crypto`, `omnia-network`, and `omnia-adapters`
//! — and is the recommended entry point for node implementations and
//! higher-level consumers. Direct use of the split crates is appropriate
//! only for crate-specific tooling (fuzzers, benchmarks, etc.).
//!
//! # Safety
//!
//! See [`SAFETY.md`](../../SAFETY.md) at the workspace root for the
//! justification of `deny(unsafe_code)` rather than `forbid(unsafe_code)`:
//! `omnia-crypto` wraps `blst` (a C library) for BLS12-381 signatures,
//! which requires `unsafe` FFI bindings. All `unsafe` usage is confined
//! to `omnia-crypto::bls` and transitively reaches this crate via the
//! `bls` feature flag.

#![deny(clippy::unwrap_used)]
// C-1 fix (audit v0.1.68): use `deny(unsafe_code)` rather than
// `forbid(unsafe_code)`. The blst C library (used by omnia-crypto::bls)
// requires `unsafe` FFI bindings; transitively, this crate's `bls` feature
// pulls in that `unsafe` code. `forbid` would prevent the feature from
// being enabled at all. `deny` still triggers compile errors for any
// `unsafe` block written *inside* this crate, but permits it in transitive
// dependencies — which is the intended policy. See SAFETY.md.
#![deny(unsafe_code)]
#![warn(missing_docs)]
#![warn(unused_qualifications)]

pub mod blake3_domain;
#[cfg(feature = "bls")]
pub mod bls;
pub mod causal_graph;
pub mod consensus;
pub mod consensus_store;
pub mod crdt;
pub mod crypto;
pub mod crypto_schemes;
pub mod event;
pub mod genesis;
pub mod genesis_replay;
pub mod keystore;
pub mod lane0;
pub mod mempool;
pub mod migration;
pub mod rate_limiter;
pub mod slashing;
pub mod slashing_undo;
pub mod snapshot;
pub mod snapshot_replication;
#[cfg(feature = "bls")]
pub mod threshold;
pub mod vector_clock;
pub mod vrf;
pub mod wire_format;

// Re-export the migrated crates so downstream consumers can access them
// via `omnia_substrate::omnia_crypto::…`, `omnia_substrate::omnia_consensus::…`, etc.
#[cfg(feature = "zk")]
pub use omnia_adapters;
pub use omnia_consensus;
pub use omnia_crypto;
#[cfg(feature = "network")]
pub use omnia_network;

// Re-export commonly used types
#[cfg(feature = "bls")]
pub use bls::{
    aggregate_public_keys, aggregate_signatures, verify_aggregate, verify_aggregate_with_pop, BlsError, BlsKeypair,
    BlsProofOfPossession, BlsPublicKey, BlsSignature,
};
pub use causal_graph::{CausalGraph, CausalGraphError, GraphSnapshot, GraphStats, PrunedEventMetadata};
pub use consensus::{
    ConsensusConfig, ConsensusEngine, ConsensusError, ConsensusState, DefaultConsensusEngine, RoundTimer,
};
pub use consensus_store::{
    ConsensusState as PersistedConsensusState, ConsensusStore, ConsensusStoreError, RedbConsensusStore,
};
pub use crdt::{CrdtError, CvRDT, GCounter, LwwRegister, OrSet};
pub use crypto::{generate_keypair, NodeKeypair, NodePublicKey};
pub use crypto_schemes::{CryptoProfile, HashScheme, SchemeVersion, SignatureScheme, VrfScheme, ZkScheme};
pub use omnia_primitives::{
    blake3_hash_domain, deserialize_with_version, serialize_with_version, CausalOrder, Event, EventBatch, EventHeader,
    EventId, EventRequest, EventStatus, EventValidationError, LogicalClock, NodeId, VectorClock, VectorClockError,
    WireFormatError, MAX_EVENT_AGE_MS, MAX_PAYLOAD_SIZE, MAX_TIMESTAMP_DRIFT_MS, WIRE_FORMAT_VERSION,
};
// Re-export networking types from omnia-network (backward compatibility)
// Only available when the `network` feature is enabled.
pub use genesis_replay::{replay_genesis, ReplayConfig, ReplayResult};
pub use keystore::{EncryptedKeyStore, KeyPurpose, KeyRotationProof, KeyStoreError};
pub use mempool::{Mempool, MempoolError};
#[cfg(feature = "network")]
pub use omnia_network::{
    fast_sync::{
        select_target_checkpoint, FastSyncManager, SyncCheckpoint, SyncError, SyncNetwork, SyncRequest, SyncResponse,
        SyncResult, SyncSnapshot,
    },
    gossip::{
        deserialize_compressed, serialize_compressed, GossipConfig, GossipDigest, GossipError, GossipEvent,
        GossipMessage, GossipProtocol, GossipStats,
    },
    network::{
        check_version_compatibility, configure_gossipsub_scoring, NetworkCommand, NetworkConfig, NetworkEvent,
        OmniaBehaviour, OmniaNetwork, PeerScoreTracker, VersionCompatibility, VersionHandshake,
    },
    PROTOCOL_IDENTIFIER as NET_PROTOCOL_IDENTIFIER, PROTOCOL_VERSION as NET_PROTOCOL_VERSION,
};
pub use rate_limiter::RateLimiter;
pub use slashing::{
    InMemorySlashingStore, JailState, RedbSlashingStore, SlashOffense, SlashOutcome, SlashPenalty, SlashingEngine,
    SlashingEvent, SlashingEventType, SlashingState, SlashingStore, SlashingStoreError, DEFAULT_EJECTION_THRESHOLD,
    DEFAULT_SLASH_THRESHOLD,
};
pub use slashing_undo::{SlashingUndoError, SlashingUndoManager, SlashingUndoRecord, SlashingUndoRequest};
pub use snapshot::{SnapshotError, StateSnapshot};
pub use snapshot_replication::{find_latest_snapshot, replicate_snapshot, ReplicationConfig};
#[cfg(feature = "bls")]
pub use threshold::{
    AeadCiphertext, DkgError, DkgPhase, DkgResult, DkgSharePackage, DkgVerificationResult, FeldmanVssSession, KeyShare,
    PartialSignature, ScalarBytes, ThresholdConfig, ThresholdError, ThresholdKeyManager, ThresholdSignature,
};

#[cfg(all(feature = "bls", feature = "deprecated-dkg"))]
#[allow(deprecated)]
pub use threshold::DkgSession;
pub use vrf::{
    deterministic_compute, deterministic_verify, ecdsa_prove, ecdsa_verify, select_leader, select_leader_v2,
    DeterministicHashError, DeterministicOutput, EcdsaProofOutput, HashVersion,
};

/// Semantic version of this crate
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Protocol version identifier — used for network compatibility negotiation.
///
/// Nodes with incompatible protocol versions will refuse to establish
/// P2P connections. This version is incremented when wire-format changes,
/// consensus rule changes, or other breaking protocol changes are made.
///
/// The string format follows semver: "major.minor.patch".
/// - Major version changes: breaking wire/consensus changes
/// - Minor version changes: new optional features (backward-compatible)
/// - Patch version changes: bug fixes (fully compatible)
pub const PROTOCOL_VERSION: &str = "4.0.0";

/// libp2p protocol identifier for the Omnia request-response protocol.
///
/// Used by the request-response behaviour for sync and state exchange.
/// The version suffix ensures peers speak the same protocol dialect.
pub const PROTOCOL_IDENTIFIER: &str = "/omnia/4.0.0";

/// Target throughput (transactions per second)
pub const TARGET_TPS: u32 = 10_000;

/// Target latency for finality (milliseconds)
pub const TARGET_FINALITY_MS: u64 = 5_000;

/// Depth of the VRF leader schedule (primary + backups) consulted when
/// deciding who may propose a round (AUDIT-2026-07 H3, #353). If the primary
/// and the first few backups are all slashed/silent, the round falls through
/// to the normal timeout path.
const LEADER_SCHEDULE_DEPTH: usize = 4;

/// Parse the consensus seed from the `OMNIA_CONSENSUS_SEED` environment variable.
///
/// Accepts a hex-encoded 64-character (32-byte) seed for cryptographic strength.
/// If the environment variable is not set or is invalid, generates a random seed.
///
/// **Internal helper retained for backward compatibility.** New code paths
/// — especially those that surface errors to the operator (e.g. `main()`) —
/// should use [`try_parse_consensus_seed`] instead, which returns a `Result`
/// rather than silently falling back to a random seed on invalid hex.
///
/// NEW-M1: this function is now unused since SubstrateConfig::new() delegates
/// to try_new(). Kept for backward compat with any external callers.
#[allow(dead_code)]
fn parse_consensus_seed() -> [u8; 32] {
    if let Ok(hex_seed) = std::env::var("OMNIA_CONSENSUS_SEED") {
        if hex_seed.len() == 64 {
            if let Ok(bytes) = hex::decode(&hex_seed) {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                return arr;
            }
        }
        tracing::warn!(
            "OMNIA_CONSENSUS_SEED must be 64 hex characters (provided: {} chars). Using random seed.",
            hex_seed.len()
        );
    }

    let mut seed = [0u8; 32];
    getrandom::getrandom(&mut seed).expect(
        "Failed to generate random consensus seed — cryptographic RNG is unavailable. \
         Set OMNIA_CONSENSUS_SEED environment variable or ensure system RNG is functional.",
    );
    seed
}

/// Parse `OMNIA_CONSENSUS_SEED` with proper error propagation (H-12 fix).
///
/// Behaviour:
/// - If `OMNIA_CONSENSUS_SEED` is **unset**: returns `Ok(random_seed())` and
///   logs an info-level message. This is the expected production default.
/// - If `OMNIA_CONSENSUS_SEED` is **set and valid** (64-char hex, 32 bytes):
///   returns `Ok(decoded_seed)`.
/// - If `OMNIA_CONSENSUS_SEED` is **set but invalid** (wrong length or non-hex):
///   returns `Err(ConsensusSeedError::InvalidHex { ... })`.
///
/// # Why
///
/// Audit finding H-12 (v0.1.68): the previous implementation silently
/// discarded invalid hex and fell back to a random seed. In production, an
/// operator who fat-fingers the env var would unknowingly run a node with
/// a *different* consensus seed than the rest of the network — causing
/// silent forking. Failing loudly lets the operator fix the typo before
/// the node joins consensus.
pub fn try_parse_consensus_seed() -> ConsensusSeedResult<[u8; 32]> {
    match std::env::var("OMNIA_CONSENSUS_SEED") {
        Ok(hex_str) => {
            if hex_str.len() != 64 {
                return Err(ConsensusSeedError::InvalidLength {
                    actual: hex_str.len(),
                    expected: 64,
                });
            }
            let bytes = hex::decode(&hex_str).map_err(|e| ConsensusSeedError::InvalidHex {
                source: e,
                raw: hex_str,
            })?;
            if bytes.len() != 32 {
                return Err(ConsensusSeedError::InvalidLength {
                    actual: bytes.len() * 2, // hex char count
                    expected: 64,
                });
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            tracing::info!("OMNIA_CONSENSUS_SEED loaded from environment");
            Ok(arr)
        }
        Err(_) => {
            tracing::info!("OMNIA_CONSENSUS_SEED not set; using random consensus seed");
            let mut seed = [0u8; 32];
            getrandom::getrandom(&mut seed).map_err(ConsensusSeedError::RngUnavailable)?;
            Ok(seed)
        }
    }
}

/// Error returned by [`try_parse_consensus_seed`].
#[derive(Debug, thiserror::Error)]
pub enum ConsensusSeedError {
    /// `OMNIA_CONSENSUS_SEED` was set but did not contain valid hexadecimal.
    #[error(
        "OMNIA_CONSENSUS_SEED contains invalid hex: {source}. \
         Either fix the value (must be 64 hex chars / 32 bytes) or unset the \
         variable to use a random seed."
    )]
    InvalidHex {
        /// The underlying hex decode error.
        source: hex::FromHexError,
        /// The raw (invalid) string that was provided.
        raw: String,
    },

    /// `OMNIA_CONSENSUS_SEED` was set but had the wrong length.
    #[error(
        "OMNIA_CONSENSUS_SEED must be 64 hex characters (32 bytes). \
         Provided: {actual} chars."
    )]
    InvalidLength {
        /// Actual length provided (in hex characters).
        actual: usize,
        /// Required length (always 64).
        expected: usize,
    },

    /// `OMNIA_TOTAL_NODES` was set but could not be parsed as a usize.
    /// NEW-M1 fix: prevents silent fallback to 4 on a typo'd value.
    #[error(
        "OMNIA_TOTAL_NODES='{raw}' is not a valid usize. \
         Fix the value or unset the variable to use the default (4)."
    )]
    InvalidTotalNodes {
        /// The raw (invalid) string that was provided.
        raw: String,
    },

    /// The system RNG was unavailable and no seed was provided via the env var.
    #[error("System RNG unavailable — cannot generate random consensus seed. Set OMNIA_CONSENSUS_SEED manually.")]
    RngUnavailable(#[from] getrandom::Error),
}

/// Result alias for operations that can fail with [`ConsensusSeedError`].
///
/// Defined separately from the crate-wide `Result<T>` (which uses
/// `SubstrateError`) because consensus-seed parsing produces a more
/// specific error type that callers may want to match on directly.
pub type ConsensusSeedResult<T> = std::result::Result<T, ConsensusSeedError>;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use thiserror::Error;

/// Errors that can occur during event processing by a Layer 2 processor.
#[derive(Error, Debug)]
pub enum EventProcessorError {
    /// The event payload could not be deserialized.
    #[error("payload deserialization failed: {0}")]
    Deserialization(String),
    /// The event failed validation (e.g., replay, oversized payload).
    #[error("validation failed: {0}")]
    ValidationFailed(String),
    /// The target shard was not found in the router.
    #[error("unknown shard: {0}")]
    UnknownShard(String),
    /// A shard-specific processing error occurred.
    #[error("shard error: {0}")]
    ShardError(String),
    /// An internal error occurred during event processing.
    #[error("internal error: {0}")]
    Internal(String),
}

/// Trait for Layer 2 event processors (e.g., domain shards).
///
/// Implementations receive every event that passes through consensus
/// and can perform domain-specific state transitions. The substrate
/// treats event processors as opaque — it doesn't know or care about
/// the internal structure of the processor.
///
/// # Example
///
/// ```ignore
/// use omnia_substrate::EventProcessor;
///
/// struct MyShard;
///
/// impl EventProcessor for MyShard {
///     fn process_event(&mut self, event: &Event) -> Result<(), EventProcessorError> {
///         // Handle the event
///         Ok(())
///     }
/// }
/// ```
pub trait EventProcessor: Send + Sync {
    /// Process a single event. Return an error if processing fails.
    fn process_event(&mut self, event: &Event) -> std::result::Result<(), EventProcessorError>;
}

/// Errors at the substrate layer
#[derive(Error, Debug)]
pub enum SubstrateError {
    #[error("Vector clock error: {0}")]
    /// Vector clock error
    VectorClock(#[from] VectorClockError),
    #[error("Causal graph error: {0}")]
    /// Causal graph error
    CausalGraph(#[from] CausalGraphError),
    #[error("Event validation error: {0}")]
    /// Event validation error
    EventValidation(#[from] EventValidationError),
    #[cfg(feature = "network")]
    #[error("Gossip error: {0}")]
    /// Gossip protocol error
    Gossip(#[from] GossipError),
    #[error("Consensus error: {0}")]
    /// Consensus engine error
    Consensus(#[from] ConsensusError),
    #[error("Configuration error: {0}")]
    /// Configuration error
    Config(String),
    #[error("Snapshot error: {0}")]
    /// Event-snapshot persistence / deserialization error
    Snapshot(String),
}

/// Result type for substrate operations
pub type Result<T> = std::result::Result<T, SubstrateError>;

/// Configuration for the entire substrate layer
#[derive(Debug, Clone)]
pub struct SubstrateConfig {
    /// Unique identifier for this node
    pub node_id: NodeId,
    /// Gossip protocol configuration
    #[cfg(feature = "network")]
    pub gossip: GossipConfig,
    /// Consensus engine configuration
    pub consensus: ConsensusConfig,
    /// Total number of nodes in the network
    pub total_nodes: usize,
    /// Directory for persistent slashing state (redb).
    ///
    /// If `None`, slashing state is kept in memory only (for tests).
    /// Production nodes should always set this to ensure slash history
    /// survives restarts.
    pub slashing_data_dir: Option<PathBuf>,
    /// Slash points threshold at which a validator is *slashed* (stake forfeited).
    pub slash_threshold: u64,
    /// Slash points threshold at which a validator is *ejected* from the validator set.
    pub ejection_threshold: u64,
    /// Maximum allowed event payload size in bytes.
    ///
    /// Events exceeding this size are rejected before processing,
    /// preventing DoS via oversized payloads. Defaults to 1 MiB
    /// (`MAX_PAYLOAD_SIZE`).
    pub max_payload_size: usize,
    /// Directory for persistent nonce state (redb).
    ///
    /// If `None`, nonce state is kept in memory only (for tests).
    /// Production nodes should set this to ensure nonce tracking
    /// survives restarts, preventing replay attacks after a crash.
    pub nonce_data_dir: Option<PathBuf>,
    /// Interval (in event count) between automatic snapshots.
    ///
    /// When set to a non-zero value, the node will automatically take
    /// a state snapshot every `snapshot_interval` events. A value of
    /// `0` disables automatic snapshots (archive mode).
    ///
    /// Default: `10000`.
    pub snapshot_interval: u64,
    /// Number of finalized rounds to retain before pruning.
    ///
    /// When set to a non-zero value, events finalized more than
    /// `pruning_depth` rounds ago are pruned (metadata only).
    /// A value of `0` means no pruning (archive mode).
    ///
    /// Default: `0` (archive).
    pub pruning_depth: u64,
    /// Maximum number of events in the mempool.
    ///
    /// The mempool holds events awaiting inclusion in a leader's
    /// block proposal. When the mempool is full, new submissions
    /// are rejected until space is freed.
    ///
    /// Default: `10_000`.
    pub mempool_size: usize,
    /// Maximum number of events per block proposal.
    ///
    /// When a leader produces a block, it drains at most this many
    /// events from the mempool. Larger values increase throughput
    /// but also increase the computational cost of block validation.
    ///
    /// Default: `500`.
    pub max_block_events: usize,
    /// Directory for persistent consensus state (redb).
    ///
    /// If `None`, consensus state is kept in memory only (for tests).
    /// Production nodes should set this to ensure consensus state
    /// survives restarts, avoiding the need to replay all events
    /// from genesis after a crash.
    pub consensus_data_dir: Option<PathBuf>,
    /// Directory for persistent Lane 0 finality state (redb).
    ///
    /// AUDIT-2026-07 C7 (#345): if `None`, the Lane 0 certificate store is
    /// in-memory only and a restart loses every finalized event ID, the
    /// epoch counter, and the current validator set — violating "once
    /// final, always final". Production nodes running Lane 0 MUST set this.
    pub lane0_data_dir: Option<PathBuf>,
    /// Enable fast sync on startup (downloads snapshot from peers).
    ///
    /// When `true`, a late-joining node will attempt to download a
    /// recent state snapshot from peers and replay only the delta
    /// events since that snapshot, instead of replaying all events
    /// from genesis.
    ///
    /// Default: `false`.
    pub fast_sync: bool,
}

impl SubstrateConfig {
    /// Create a new substrate configuration with default settings.
    ///
    /// Slashing defaults to in-memory mode (`slashing_data_dir = None`)
    /// with standard thresholds (slash at 500, eject at 2000).
    /// Production callers should set `slashing_data_dir` before
    /// constructing the substrate.
    ///
    /// NEW-M1 fix: this method now delegates to try_new() and panics on
    /// invalid OMNIA_CONSENSUS_SEED / OMNIA_TOTAL_NODES instead of
    /// silently falling back. The silent fallback was the H-12 bug that
    /// was supposed to be fixed — this closes the gap.
    #[deprecated(note = "Use try_new() for proper error propagation. \
                         This method panics on invalid env vars.")]
    pub fn new(node_id: NodeId) -> Self {
        Self::try_new(node_id)
            .unwrap_or_else(|e| panic!("SubstrateConfig::new failed: {e:?}. Use try_new() for error propagation."))
    }

    /// Create a substrate configuration with a custom network size.
    ///
    /// Slashing defaults to in-memory mode with standard thresholds.
    #[deprecated(note = "Use try_with_network_size() for proper error propagation. \
                         This method panics on invalid env vars.")]
    pub fn with_network_size(node_id: NodeId, total_nodes: usize) -> Self {
        Self::try_with_network_size(node_id, total_nodes).unwrap_or_else(|e| {
            panic!(
                "SubstrateConfig::with_network_size failed: {e:?}. \
                    Use try_with_network_size() for error propagation."
            )
        })
    }

    /// Like [`SubstrateConfig::new`] but propagates consensus-seed errors
    /// instead of silently falling back to a random seed (H-12 fix).
    ///
    /// Use this in production code paths (e.g. `main()`) so that an
    /// operator who fat-fingers `OMNIA_CONSENSUS_SEED` gets a clean
    /// error message and exit instead of unknowingly forking off the
    /// network with a different seed.
    pub fn try_new(node_id: NodeId) -> ConsensusSeedResult<Self> {
        // NEW-M1 fix: also validate OMNIA_TOTAL_NODES — the previous code
        // used .unwrap_or(4) which silently fell back on a typo, causing
        // the node to compute wrong supermajority thresholds and silently
        // fork from the network.
        let total_nodes: usize = match std::env::var("OMNIA_TOTAL_NODES") {
            Ok(v) => v
                .parse()
                .map_err(|_| ConsensusSeedError::InvalidTotalNodes { raw: v })?,
            Err(_) => {
                tracing::warn!("OMNIA_TOTAL_NODES not set, defaulting to 4 — configure this for production");
                4
            }
        };
        let seed = try_parse_consensus_seed()?;
        Ok(Self::build_config(node_id, total_nodes, seed))
    }

    /// Like [`SubstrateConfig::with_network_size`] but propagates
    /// consensus-seed errors (H-12 fix).
    pub fn try_with_network_size(node_id: NodeId, total_nodes: usize) -> ConsensusSeedResult<Self> {
        let seed = try_parse_consensus_seed()?;
        Ok(Self::build_config(node_id, total_nodes, seed))
    }

    /// Build a substrate configuration with the given parameters.
    fn build_config(node_id: NodeId, total_nodes: usize, seed: [u8; 32]) -> Self {
        Self {
            node_id,
            #[cfg(feature = "network")]
            gossip: GossipConfig::default(),
            consensus: ConsensusConfig {
                total_nodes,
                round_seed: seed,
                ..Default::default()
            },
            total_nodes,
            slashing_data_dir: None,
            slash_threshold: DEFAULT_SLASH_THRESHOLD,
            ejection_threshold: DEFAULT_EJECTION_THRESHOLD,
            max_payload_size: MAX_PAYLOAD_SIZE,
            nonce_data_dir: None,
            snapshot_interval: 10_000,
            pruning_depth: 0,
            mempool_size: 10_000,
            max_block_events: 500,
            consensus_data_dir: None,
            lane0_data_dir: None,
            fast_sync: false,
        }
    }
}

/// The main Substrate runtime that coordinates all components
pub struct Substrate {
    config: SubstrateConfig,
    graph: Arc<tokio::sync::RwLock<CausalGraph>>,
    #[cfg(feature = "network")]
    gossip: Option<GossipProtocol>,
    consensus: ConsensusEngine<SlashingEngine>,
    running: bool,
    /// The single slashing engine shared between consensus and the API layer.
    ///
    /// Cloning this field yields a new `SlashingEngine` that shares the
    /// same `Arc<dyn SlashingStore>`, so slash events recorded by consensus
    /// are visible to the API and persisted to the same redb database.
    pub slashing: SlashingEngine,
    /// Optional Layer 2 shard processor (e.g., domain shard router).
    ///
    /// When set, the `run()` loop will feed every newly-committed event
    /// to this processor, ensuring shards only observe finalized state.
    pub shard_processor: Option<Box<dyn EventProcessor>>,
    /// Events waiting for consensus processing.
    ///
    /// Events are added to this queue when inserted (locally via
    /// `submit_event()` or from the network via gossip). `process_consensus()`
    /// drains this queue, making consensus O(new_events) instead of O(total).
    unprocessed_events: Vec<EventId>,
    /// Mempool for pending events awaiting block inclusion.
    ///
    /// Events submitted locally are added to the mempool. When this node
    /// is the deterministic-hash-selected leader for a round, `propose_block()` drains
    /// events from the mempool and inserts them into the causal graph
    /// for consensus processing.
    mempool: Mempool,
    /// Maximum number of events per block proposal.
    max_block_events: usize,
    /// Validator candidates for deterministic hash-based leader selection.
    ///
    /// Maps `NodeId` to `(keypair, stake)` for each validator.
    /// Used by `compute_leader()` to determine the round leader.
    /// If empty, leader selection is skipped in the run loop.
    validator_candidates: HashMap<NodeId, (NodeKeypair, u64)>,
    /// Optional consensus state persistence store.
    ///
    /// When set, consensus state is persisted after each round
    /// advancement, enabling crash recovery without genesis replay.
    /// If `None`, consensus state is in-memory only.
    consensus_store: Option<Arc<dyn ConsensusStore>>,
    /// Lane 0 validator set (ADR-025). `None` = Lane 0 disabled.
    lane0_validators: Option<lane0::ValidatorSet>,
    /// Lane 0 finality certificates (grow-only ack sets per event).
    lane0_store: lane0::CertificateStore,
    /// Own Lane 0 acks awaiting broadcast (flushed each consensus round
    /// and on local submission).
    lane0_outbox: Vec<lane0::SignedAck>,
    /// AUDIT-2026-07 H5 (#355): events that Lane 0 preconfirmed but Lane 1
    /// (the canonical DAG consensus) subsequently rejected — a fast-path
    /// divergence. Tracked so the divergence is observable and the event's
    /// [`FinalityState`] reports `Diverged` instead of a stale
    /// preconfirmation.
    lane0_diverged: HashSet<EventId>,
    /// AUDIT-2026-07 H5 (#355): events whose canonical state has been
    /// anchored to the settlement layer (L1). Reaching this set advances an
    /// event's [`FinalityState`] from `Canonical` to `Final`.
    settled_events: HashSet<EventId>,
}

/// Transaction / event finality lifecycle (AUDIT-2026-07 H5, #355).
///
/// Omnia has two lanes with different guarantees, and consumers **must**
/// distinguish them rather than treat Lane 0 as canonical finality:
///
/// - [`Preconfirmed`](FinalityState::Preconfirmed) — Lane 0 gave a fast,
///   stake-signed preconfirmation. It is **reversible**: if Lane 1 later
///   rejects the event the state becomes [`Diverged`](FinalityState::Diverged).
///   Safe-by-construction for well-formed single-writer UBC transfers, but a
///   consumer that acts on it accepts reversal risk.
/// - [`Canonical`](FinalityState::Canonical) — Lane 1 (DAG BFT consensus)
///   committed the event into the agreed causal order. Irreversible under the
///   BFT threat model.
/// - [`Final`](FinalityState::Final) — canonical **and** anchored to the
///   settlement layer (L1). The strongest guarantee.
/// - [`Diverged`](FinalityState::Diverged) — Lane 0 preconfirmed but Lane 1
///   rejected the event. The fast-path guarantee was violated; anything done
///   on the preconfirmation must be rolled back.
///
/// APIs and SDKs expose this state directly; they never advertise a Lane 0
/// preconfirmation as "final".
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalityState {
    /// Neither lane has decided the event yet.
    Pending,
    /// Lane 0 stake-quorum preconfirmation — fast but reversible.
    Preconfirmed,
    /// Lane 1 (BFT consensus) committed the event into the canonical order.
    Canonical,
    /// Canonical and anchored to the settlement layer (L1).
    Final,
    /// Lane 0 preconfirmed the event but Lane 1 rejected it.
    Diverged,
}

impl FinalityState {
    /// The wire/string name of this state (matches the serde representation).
    pub fn as_str(&self) -> &'static str {
        match self {
            FinalityState::Pending => "pending",
            FinalityState::Preconfirmed => "preconfirmed",
            FinalityState::Canonical => "canonical",
            FinalityState::Final => "final",
            FinalityState::Diverged => "diverged",
        }
    }

    /// Whether this state is safe to treat as irreversible (`Canonical` or
    /// `Final`). `Preconfirmed` is **not** — it can still diverge.
    pub fn is_irreversible(&self) -> bool {
        matches!(self, FinalityState::Canonical | FinalityState::Final)
    }
}

impl Substrate {
    /// Create a new Substrate runtime with the given configuration.
    ///
    /// ONE `SlashingEngine` is created from the config's `slashing_data_dir`,
    /// `slash_threshold`, and `ejection_threshold`. A clone is passed to
    /// `ConsensusEngine::new`, and the original is stored in `self.slashing`
    /// for the API layer to use. Both share the same `Arc<dyn SlashingStore>`,
    /// eliminating the dual-engine gap.
    pub fn new(config: SubstrateConfig) -> Self {
        // SECURITY FIX (audit): the previous implementation silently fell
        // back to in-memory slashing, no consensus persistence, and a fresh
        // consensus state when redb files were corrupt or unreadable. A
        // production node that hit any of these conditions would restart
        // with NO slash history (allowing slashed validators to escape),
        // NO consensus state (restarting from round 0), and NO event
        // sequence continuity.
        //
        // We now log loudly and panic on persistence failures when the
        // operator has explicitly configured a persistence directory.
        // Operators who want the old behavior can simply not set the
        // `*_data_dir` config fields — then no persistence is attempted
        // and the fallback is unnecessary.
        let slashing = SlashingEngine::new(
            config.slashing_data_dir.clone(),
            config.slash_threshold,
            config.ejection_threshold,
        )
        .unwrap_or_else(|e| {
            match &config.slashing_data_dir {
                Some(dir) => {
                    tracing::error!(
                        error = %e,
                        path = %dir.display(),
                        "FAILED to open persistent slashing store. Refusing to silently \
                         fall back to in-memory slashing — a corrupt slashing DB must be \
                         investigated, not hidden. Delete the file manually if you want \
                         to start fresh, or fix the underlying disk/permission issue."
                    );
                    panic!(
                        "Slashing store load failed at {}: {e}. Refusing to start with \
                         replay protection disabled. Remove the file or fix permissions.",
                        dir.display()
                    );
                }
                None => {
                    // No persistence configured — in-memory is the intended mode.
                    tracing::debug!("Slashing engine running in-memory (no persistence configured)");
                    SlashingEngine::new_in_memory(config.slash_threshold, config.ejection_threshold)
                }
            }
        });

        // Create consensus store if persistence is configured
        let consensus_store: Option<Arc<dyn ConsensusStore>> =
            config
                .consensus_data_dir
                .as_ref()
                .map(|dir| match RedbConsensusStore::open(dir) {
                    Ok(store) => {
                        tracing::info!(
                            path = %dir.display(),
                            "Consensus: using persistent redb store"
                        );
                        Arc::new(store) as Arc<dyn ConsensusStore>
                    }
                    Err(e) => {
                        tracing::error!(
                            error = %e,
                            path = %dir.display(),
                            "FAILED to open consensus store. Refusing to silently fall \
                             back to no-persistence mode — consensus state would not \
                             survive restarts, breaking sequence continuity."
                        );
                        panic!(
                            "Consensus store load failed at {}: {e}. Refusing to start \
                             without consensus persistence. Remove the file or fix permissions.",
                            dir.display()
                        );
                    }
                });

        // Create consensus engine, restoring from persisted state if available
        let consensus = match &consensus_store {
            Some(store) => ConsensusEngine::load_or_new(config.consensus.clone(), Arc::clone(store), slashing.clone())
                .unwrap_or_else(|e| {
                    tracing::error!(
                        error = %e,
                        "FAILED to restore consensus state from persistent store. \
                         Refusing to silently start fresh — this would reset the round \
                         counter and could cause the node to fork off the network."
                    );
                    panic!(
                        "Consensus state restoration failed: {e}. Refusing to start fresh. \
                         Remove the consensus DB if you want to start from genesis."
                    );
                }),
            None => ConsensusEngine::new(config.consensus.clone(), slashing.clone()),
        };

        // AUDIT-2026-07 C7 (#345): build the Lane 0 certificate store,
        // restoring persisted finality (finalized set, epoch, validator
        // set) when a data dir is configured. Without persistence a restart
        // silently loses finality — production Lane 0 nodes must set
        // `lane0_data_dir`.
        let (lane0_store, lane0_restored_validators) = match &config.lane0_data_dir {
            Some(dir) => match lane0::RedbLane0Store::open(dir) {
                Ok(store) => match lane0::CertificateStore::with_store(Arc::new(store)) {
                    Ok(cert_store) => {
                        tracing::info!(path = %dir.display(), "Lane 0: using persistent redb store");
                        let restored = cert_store.restored_validators().cloned();
                        (cert_store, restored)
                    }
                    Err(e) => {
                        tracing::error!(error = %e, path = %dir.display(),
                            "FAILED to restore Lane 0 state. Refusing to silently start fresh — \
                             that would violate 'once final, always final' across the restart.");
                        panic!(
                            "Lane 0 state restoration failed at {}: {e}. Remove the DB to start fresh.",
                            dir.display()
                        );
                    }
                },
                Err(e) => {
                    tracing::error!(error = %e, path = %dir.display(), "FAILED to open Lane 0 store");
                    panic!(
                        "Lane 0 store open failed at {}: {e}. Fix permissions or remove the file.",
                        dir.display()
                    );
                }
            },
            None => (lane0::CertificateStore::new(), None),
        };

        let graph = Arc::new(tokio::sync::RwLock::new(CausalGraph::new()));
        let mempool_size = config.mempool_size;
        let max_block_events = config.max_block_events;

        Self {
            config,
            graph,
            #[cfg(feature = "network")]
            gossip: None,
            consensus,
            slashing,
            running: false,
            shard_processor: None,
            unprocessed_events: Vec::new(),
            mempool: Mempool::new(mempool_size),
            max_block_events,
            validator_candidates: HashMap::new(),
            consensus_store,
            // AUDIT-2026-07 C7 (#345): resume with the validator set the
            // store persisted, so a node restarting mid-rotation cannot
            // accept acks from a superseded set.
            lane0_validators: lane0_restored_validators,
            lane0_store,
            lane0_outbox: Vec::new(),
            lane0_diverged: HashSet::new(),
            settled_events: HashSet::new(),
        }
    }

    /// Initialize the gossip protocol
    #[cfg(feature = "network")]
    pub fn init_gossip(&mut self) {
        self.gossip = Some(GossipProtocol::new(
            self.config.node_id,
            self.config.gossip.clone(),
            Arc::clone(&self.graph),
        ));
    }

    /// Start the substrate runtime
    pub async fn start(&mut self) {
        self.running = true;
        #[cfg(feature = "network")]
        if let Some(ref mut gossip) = self.gossip {
            gossip.start().await;
        }
    }

    /// Stop the substrate runtime
    pub fn stop(&mut self) {
        self.running = false;
        #[cfg(feature = "network")]
        if let Some(ref mut gossip) = self.gossip {
            gossip.stop();
        }
    }

    /// Attach a Layer 2 shard processor (e.g., a shard router).
    ///
    /// The processor will be called for each newly-committed event
    /// during the main run loop, after consensus processing. Only
    /// committed events are forwarded, ensuring shards never see
    /// un-finalized state.
    pub fn with_shard_processor(mut self, processor: Box<dyn EventProcessor>) -> Self {
        self.shard_processor = Some(processor);
        self
    }

    /// Register validator candidates for deterministic hash-based leader selection.
    ///
    /// Each entry maps a `NodeId` to its `(keypair, stake)`. The leader
    /// for a given round is selected deterministically from this set
    /// using the `select_leader()` function.
    ///
    /// If this method is not called, the run loop will skip the leader
    /// check and no blocks will be proposed.
    pub fn with_validator_candidates(mut self, candidates: HashMap<NodeId, (NodeKeypair, u64)>) -> Self {
        self.validator_candidates = candidates;
        self
    }

    /// Register a single validator candidate.
    ///
    /// Convenience method for adding validators one at a time.
    pub fn add_validator(&mut self, node_id: NodeId, keypair: NodeKeypair, stake: u64) {
        self.validator_candidates.insert(node_id, (keypair, stake));
    }

    /// Get an iterator over the registered validator candidates.
    ///
    /// Each entry is `(NodeId, stake)` — the keypair is intentionally
    /// not exposed through this accessor to avoid leaking private signing
    /// material to API callers. Use `add_validator` or
    /// `with_validator_candidates` to populate the registry.
    ///
    /// Used by the `GET /api/v1/validators` endpoint.
    pub fn validator_candidates_iter(&self) -> impl Iterator<Item = (&NodeId, u64)> {
        self.validator_candidates.iter().map(|(id, (_kp, stake))| (id, *stake))
    }

    /// Number of registered validator candidates.
    pub fn validator_count(&self) -> usize {
        self.validator_candidates.len()
    }

    /// Current consensus round number.
    ///
    /// Used by the `GET /api/v1/validators` endpoint to determine
    /// whether each validator is currently jailed.
    pub fn current_round(&self) -> u64 {
        self.consensus.current_round()
    }

    /// Total number of events the consensus engine has committed.
    ///
    /// Unlike the HTTP layer's in-memory event store, this counter is
    /// restored from the persistent consensus store on restart, so it
    /// survives process restarts (issue #260). `/readyz` and
    /// `/api/v1/node/info` use it to report `finalized_height`.
    pub fn committed_count(&self) -> u64 {
        self.consensus.committed_count()
    }

    /// Get a reference to the mempool.
    pub fn mempool(&self) -> &Mempool {
        &self.mempool
    }

    /// Get a mutable reference to the mempool.
    pub fn mempool_mut(&mut self) -> &mut Mempool {
        &mut self.mempool
    }

    /// Run the substrate main loop. Processes network events, leader
    /// selection, block production, consensus, and Layer 2 shard processors
    /// until `stop()` is called.
    ///
    /// Uses a round timer with `tokio::select!` for event-driven wakeup
    /// instead of a fixed 100ms poll loop. The round timer fires at the
    /// configured consensus round interval, waking the loop to check for
    /// leader duties and process consensus rounds.
    ///
    /// Only committed events are forwarded to the shard processor,
    /// ensuring that shards never observe un-finalized state that
    /// could later be rolled back.
    pub async fn run(&mut self) {
        self.running = true;

        // Round timer: fires at the consensus round interval (default 1 second).
        // This replaces the previous 100ms sleep poll loop with an event-driven
        // approach that only wakes when necessary.
        let round_duration =
            tokio::time::Duration::from_millis(self.config.consensus.round_timeout_ms.clamp(100, 10_000));
        let mut round_timer = tokio::time::interval(round_duration);

        while self.running {
            tokio::select! {
                // Wake on round timeout — check leader duties and process consensus
                _ = round_timer.tick() => {
                    self.process_consensus_round().await;
                }
            }
        }
    }

    /// Process a single consensus round: drain gossip, check leader,
    /// run consensus, and forward committed events to shard processor.
    ///
    /// This is the main integration point between the P2P network layer
    /// and the consensus engine. When the `network` feature is enabled
    /// and gossip has been initialized via `init_gossip()`, this method
    /// drains incoming gossip events from the network, validates them,
    /// inserts them into the causal graph, and feeds them into consensus.
    pub async fn process_consensus_round(&mut self) {
        // 1. Drain network events into graph + queue
        #[cfg(feature = "network")]
        {
            let mut newly_inserted: Vec<EventId> = Vec::new();
            let mut aux_messages: Vec<(String, Vec<u8>)> = Vec::new();
            if let Some(ref mut gossip) = self.gossip {
                match gossip.process_pending_events().await {
                    Ok(inserted) => {
                        newly_inserted = inserted;
                    }
                    Err(e) => {
                        tracing::warn!("Gossip processing error: {}", e);
                    }
                }
                aux_messages = gossip.take_aux_messages();
                // Keepalive (issue #259): on an idle network nothing else
                // generates traffic, so without heartbeats every peer
                // eventually exceeds the partition threshold and the mesh
                // dissolves. No-op unless a heartbeat is due.
                gossip.maybe_send_heartbeat().await;
                // Anti-entropy (issue #315): periodically advertise our DAG
                // frontier so peers that lost events to bounded-queue drops
                // can request and recover them.
                gossip.maybe_send_sync_digest().await;
                // Mesh repair (issue #411): peers were dialled once at
                // startup and never again, so a dropped link stayed dropped
                // until a node restarted. No-op while the mesh is complete.
                gossip.maybe_redial_peers().await;
            }
            self.unprocessed_events.extend(newly_inserted.iter().copied());

            // Lane 0 (ADR-025): fold acks received from peers, ack the
            // events this node just validated + inserted, and broadcast
            // our own acks.
            self.lane0_fold_received(aux_messages);
            self.lane0_ack_inserted(&newly_inserted);
            self.lane0_flush_outbox().await;
        }

        // 2. Check if we are the leader for this round.
        //
        // AUDIT-2026-07 C1 (#339): leader selection now uses the
        // VRF-keyed, stake-weighted schedule driven by the unpredictable
        // beacon. We take an ordered schedule (primary + backups) so that
        // if the primary is slashed, a backup can step in immediately
        // rather than waiting out a round timeout (zero-timeout failover).
        // We propose if we are the highest-ranked non-slashed validator in
        // the schedule — every node computes the identical schedule, so
        // this stays single-leader.
        let current_round = self.consensus.current_round();
        if !self.validator_candidates.is_empty() {
            // AUDIT-2026-07 H3 (#353): only the round's active leader may
            // propose. `is_active_leader_for_round` computes the VRF-keyed
            // schedule and skips slashed members (zero-timeout backup
            // failover), so leader-based consensus is enforced rather than
            // decorative. `propose_block` re-checks this as a defensive guard.
            if self.consensus.is_active_leader_for_round(
                &self.config.node_id,
                &self.validator_candidates,
                current_round,
                LEADER_SCHEDULE_DEPTH,
            ) {
                self.propose_block(current_round).await;
            }
        }

        // 3. Run consensus — returns newly committed event IDs
        let committed = self.process_consensus().await;

        // 3b. AUDIT-2026-07 C1 (#339): fold the committed DAG frontier into
        // the leader-election beacon. This is what makes future leaders
        // unpredictable — committed IDs depend on user signatures that do
        // not exist until the events are created, and every node folds the
        // identical committed set, so the beacon evolves deterministically
        // without any extra network traffic.
        if !committed.is_empty() {
            self.consensus.advance_beacon_from_committed(&committed);
        }

        // 4. Process committed events through shard processor
        if let Some(ref mut processor) = self.shard_processor {
            let graph = self.graph.read().await;
            for event_id in &committed {
                match graph.get_checked(event_id) {
                    Ok(event) => {
                        if let Err(e) = processor.process_event(event) {
                            tracing::warn!("Shard processor error for event {}: {}", hex::encode(&event_id[..4]), e);
                        }
                    }
                    Err(CausalGraphError::EventPruned(_)) => {
                        tracing::warn!(
                            "Skipping pruned event {} in shard processor",
                            hex::encode(&event_id[..4])
                        );
                    }
                    Err(_) => {
                        tracing::warn!(
                            "Event {} not found in graph for shard processing",
                            hex::encode(&event_id[..4])
                        );
                    }
                }
            }
        }

        // 5. Apply Lane 1-committed validator-set changes (ADR-025
        // Stage 4 trigger). Runs strictly on COMMITTED events — commit
        // order is agreed by the DAG commit rule, so every honest node
        // applies the same rotations at the same logical point (the
        // epoch fence).
        self.apply_committed_vset_changes(&committed).await;
    }

    /// Scan committed events for Lane 1 validator-set changes and apply
    /// them to Lane 0 (see [`lane0::ValidatorSetChange`]).
    ///
    /// Authorization (v1): the committed event's creator public key must
    /// be a member of the currently active Lane 0 validator set —
    /// existing validators govern their own succession. Unauthorized or
    /// malformed changes are logged and skipped; they never abort the
    /// consensus round. No-op while Lane 0 is disabled.
    async fn apply_committed_vset_changes(&mut self, committed: &[EventId]) {
        if self.lane0_validators.is_none() || committed.is_empty() {
            return;
        }

        // Collect decoded changes first: applying a rotation needs
        // `&mut self` while the graph read guard borrows `self`.
        let mut changes: Vec<(EventId, [u8; 32], lane0::ValidatorSetChange)> = Vec::new();
        {
            let graph = self.graph.read().await;
            for event_id in committed {
                let Ok(event) = graph.get_checked(event_id) else {
                    continue;
                };
                match lane0::decode_vset_change(&event.payload) {
                    Ok(Some(change)) => changes.push((*event_id, event.creator_pubkey(), change)),
                    Ok(None) => {}
                    Err(e) => {
                        tracing::warn!(
                            event = %hex::encode(&event_id[..4]),
                            "Malformed validator-set change skipped: {e}"
                        );
                    }
                }
            }
        }

        for (event_id, creator_pubkey, change) in changes {
            let authorized = self
                .lane0_validators
                .as_ref()
                .is_some_and(|set| set.contains(&creator_pubkey));
            if !authorized {
                tracing::warn!(
                    event = %hex::encode(&event_id[..4]),
                    creator = %hex::encode(&creator_pubkey[..4]),
                    "Validator-set change REJECTED: creator is not a current Lane 0 validator"
                );
                continue;
            }
            match lane0::ValidatorSet::new(change.entries) {
                Ok(new_set) => {
                    let newly_final = self.rotate_lane0_validators(new_set);
                    tracing::info!(
                        event = %hex::encode(&event_id[..4]),
                        newly_final = newly_final.len(),
                        "Validator-set change applied from Lane 1 commit"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        event = %hex::encode(&event_id[..4]),
                        "Validator-set change with invalid set skipped: {e}"
                    );
                }
            }
        }
    }

    /// Start with network and run main loop
    #[cfg(feature = "network")]
    pub async fn start_with_network(&mut self, network: OmniaNetwork) {
        if let Some(ref mut gossip) = self.gossip {
            if let Err(e) = gossip.start_with_network(network).await {
                tracing::error!("Failed to start gossip with network: {}", e);
            }
        }
        self.run().await;
    }

    /// Wire an OmniaNetwork into the gossip protocol without starting
    /// the substrate main loop.
    ///
    /// This is designed for use in background task architectures where
    /// the caller manages their own consensus loop (e.g., calling
    /// `process_consensus_round()` periodically) rather than using
    /// the built-in `run()` loop.
    ///
    /// After calling this method, incoming gossip events will be queued
    /// in the gossip protocol's internal buffer. Call
    /// `process_consensus_round()` to drain those events into the
    /// causal graph and run consensus.
    #[cfg(feature = "network")]
    pub async fn wire_network(&mut self, network: OmniaNetwork) -> Result<()> {
        if let Some(ref mut gossip) = self.gossip {
            gossip.start_with_network(network).await?;
            Ok(())
        } else {
            Err(SubstrateError::Config(
                "Gossip protocol not initialized — call init_gossip() first".into(),
            ))
        }
    }

    /// Submit an event to the substrate for processing.
    ///
    /// The event is validated, inserted into the causal graph, processed
    /// through consensus, broadcast via gossip, and added to the mempool
    /// for potential block proposal. If the mempool is full, the event
    /// is still processed through consensus but a warning is logged and
    /// it will not be available for block proposals.
    ///
    pub async fn submit_event(&mut self, event: Event) -> Result<()> {
        event.validate().map_err(SubstrateError::from)?;

        // Remove Arc, use event directly with clones
        let inserted_ids = {
            let mut graph = self.graph.write().await;
            let inserted_ids = graph.insert(event.clone()).map_err(SubstrateError::from)?;
            self.unprocessed_events.extend(inserted_ids.iter().copied());
            inserted_ids
        };

        // Track for consensus processing (already extended above)

        let graph = self.graph.read().await;
        self.consensus
            .process_event(&event, &graph)
            .map_err(SubstrateError::from)?;
        drop(graph);

        // Also add to mempool for block proposal when we are the leader.
        // If the mempool is full, log a warning but do not fail — the event
        // has already been processed through consensus.
        if let Err(e) = self.mempool.insert(event.clone()) {
            tracing::warn!(
                "Mempool full, event {} not queued for proposal: {}",
                hex::encode(&event.id[..4]),
                e
            );
        }

        #[cfg(feature = "network")]
        if let Some(ref mut gossip) = self.gossip {
            gossip.broadcast_event(event).await.map_err(SubstrateError::from)?;
        }

        // Lane 0 (ADR-025): ack the freshly inserted event(s) immediately
        // so locally submitted operations reach fast-path finality without
        // waiting for the next consensus round tick.
        self.lane0_ack_inserted(&inserted_ids);
        #[cfg(feature = "network")]
        self.lane0_flush_outbox().await;

        Ok(())
    }

    /// Get read access to the causal graph
    pub async fn graph(&self) -> tokio::sync::RwLockReadGuard<'_, CausalGraph> {
        self.graph.read().await
    }

    /// Save the current causal graph as an event snapshot to the
    /// consensus store (if configured).
    ///
    /// Serializes the graph via [`GraphSnapshot`] + postcard and writes
    /// the blob under the `"events_snapshot"` key. This is intended to
    /// be called periodically from the warm path (after round
    /// processing) and once on SIGTERM, so that a node retains its full
    /// event history across container restarts.
    pub async fn save_event_snapshot(&self) -> Result<()> {
        let Some(ref store) = self.consensus_store else {
            return Ok(());
        };
        let graph = self.graph.read().await;
        if graph.is_empty() {
            return Ok(());
        }
        let snapshot = GraphSnapshot::from(&*graph);
        let bytes =
            postcard::to_allocvec(&snapshot).map_err(|e| SubstrateError::Snapshot(format!("serialization: {e}")))?;
        drop(graph); // release the read lock before the (blocking) redb write
        store
            .save_events_blob(&bytes)
            .map_err(|e| SubstrateError::Snapshot(format!("persist: {e}")))?;
        tracing::debug!(size = bytes.len(), "Event snapshot persisted");
        Ok(())
    }

    /// Restore the causal graph from a previously persisted event
    /// snapshot in the consensus store (if one exists).
    ///
    /// Returns `true` if a snapshot was found and the graph was
    /// restored, `false` if no snapshot existed (fresh start).
    pub async fn restore_event_snapshot(&self) -> Result<bool> {
        let Some(ref store) = self.consensus_store else {
            return Ok(false);
        };
        let Some(bytes) = store
            .load_events_blob()
            .map_err(|e| SubstrateError::Snapshot(format!("load: {e}")))?
        else {
            return Ok(false);
        };
        let snapshot: GraphSnapshot =
            postcard::from_bytes(&bytes).map_err(|e| SubstrateError::Snapshot(format!("deserialization: {e}")))?;
        let restored = CausalGraph::from_snapshot(&snapshot);
        let event_count = restored.len();
        *self.graph.write().await = restored;
        tracing::info!(
            events = event_count,
            bytes = bytes.len(),
            "Causal graph restored from persisted snapshot"
        );
        Ok(true)
    }

    /// Get consensus statistics
    pub fn consensus_stats(&self) -> consensus::ConsensusStats {
        self.consensus.stats()
    }

    /// Get gossip protocol statistics
    #[cfg(feature = "network")]
    pub fn gossip_stats(&self) -> Option<&GossipStats> {
        self.gossip.as_ref().map(|g| g.stats())
    }

    // ── Lane 0 (ADR-025): consensusless fast-path finality ──────────────

    /// Enable Lane 0 with a static validator set (see [`lane0`]).
    ///
    /// Until enabled, Lane 0 is inert: no acks are signed, published, or
    /// accepted.
    pub fn init_lane0(&mut self, validators: lane0::ValidatorSet) {
        tracing::info!(
            validators = validators.len(),
            total_stake = validators.total_stake(),
            "Lane 0 enabled (static validator set)"
        );
        // AUDIT-2026-07 C7 (#345): persist the boot validator set so a
        // restart resumes with it (and the correct epoch) even before any
        // rotation.
        self.lane0_store.set_validators(&validators);
        self.lane0_validators = Some(validators);
    }

    /// Whether Lane 0 is enabled.
    pub fn lane0_enabled(&self) -> bool {
        self.lane0_validators.is_some()
    }

    /// Whether an event has reached Lane 0 finality (stake-weighted
    /// quorum of validator acks). Always `false` when Lane 0 is disabled.
    ///
    /// Note (AUDIT-2026-07 H5, #355): Lane 0 finality is a **reversible
    /// preconfirmation**, not canonical finality. Prefer
    /// [`finality_state`](Self::finality_state), which reports the full
    /// lifecycle and never conflates a preconfirmation with canonical/final.
    pub fn lane0_is_final(&self, event_id: &EventId) -> bool {
        self.lane0_store.is_final(event_id)
    }

    /// Resolve an event's finality lifecycle state (AUDIT-2026-07 H5, #355).
    ///
    /// Combines the Lane 0 preconfirmation, the Lane 1 canonical commit, the
    /// settlement-layer anchor, and any recorded divergence into a single
    /// [`FinalityState`] so callers (and the public API) can distinguish a
    /// reversible fast-path preconfirmation from canonical/final state.
    pub fn finality_state(&self, event_id: &EventId) -> FinalityState {
        if self.lane0_diverged.contains(event_id) {
            return FinalityState::Diverged;
        }
        if self.is_finalized(event_id) {
            // Lane 1 committed the event into the canonical order.
            if self.settled_events.contains(event_id) {
                return FinalityState::Final;
            }
            return FinalityState::Canonical;
        }
        if self.lane0_store.is_final(event_id) {
            return FinalityState::Preconfirmed;
        }
        FinalityState::Pending
    }

    /// Record that Lane 1 (canonical DAG consensus) **rejected** an event
    /// (AUDIT-2026-07 H5, #355). If Lane 0 had already preconfirmed it, this
    /// is a fast-path divergence: it is recorded (so
    /// [`finality_state`](Self::finality_state) reports `Diverged`) and
    /// logged loudly for operators/consumers to reconcile. Returns `true`
    /// when a *new* divergence was recorded.
    ///
    /// A rejection of an event Lane 0 never preconfirmed is normal (an
    /// invalid event) and is ignored here.
    pub fn reconcile_lane1_rejection(&mut self, event_id: EventId) -> bool {
        if !self.lane0_store.is_final(&event_id) {
            return false;
        }
        let newly = self.lane0_diverged.insert(event_id);
        if newly {
            tracing::error!(
                event = %hex::encode(&event_id[..4]),
                "LANE 0/LANE 1 DIVERGENCE — Lane 1 rejected an event Lane 0 preconfirmed; \
                 consumers relying on the preconfirmation must roll back"
            );
        }
        newly
    }

    /// Mark an event's canonical state as anchored to the settlement layer
    /// (L1), advancing it from `Canonical` to `Final` (AUDIT-2026-07 H5,
    /// #355). Called by the settlement adapter when a batch containing the
    /// event is confirmed on L1.
    pub fn mark_settled(&mut self, event_id: EventId) {
        self.settled_events.insert(event_id);
    }

    /// Number of Lane 0/Lane 1 divergences observed so far (AUDIT-2026-07
    /// H5, #355). A non-zero value means the fast path was contradicted by
    /// canonical consensus and should be alerted on.
    pub fn lane0_divergence_count(&self) -> usize {
        self.lane0_diverged.len()
    }

    /// Lane 0 counters `(acks_accepted, acks_rejected, events_finalized)`,
    /// or `None` when Lane 0 is disabled.
    pub fn lane0_stats(&self) -> Option<(u64, u64, u64)> {
        self.lane0_validators.as_ref().map(|_| self.lane0_store.stats())
    }

    /// Number of Lane 0 validator-set rotations applied so far (0 =
    /// original set, never rotated), or `None` when Lane 0 is disabled.
    pub fn lane0_epoch(&self) -> Option<u64> {
        self.lane0_validators.as_ref().map(|_| self.lane0_store.epoch())
    }

    /// The state root agreed on by the quorum for the most recently
    /// Lane-0-finalized event (the rolling BLAKE3 commitment).
    /// Returns `None` when Lane 0 is disabled or no event has reached
    /// finality yet.
    ///
    /// This is the value an operator should anchor on L1 — it is the
    /// root the fleet actually agreed on, not an arbitrary caller-supplied
    /// value.
    pub fn lane0_leading_root(&self) -> Option<[u8; 32]> {
        self.lane0_validators
            .as_ref()
            .and_then(|_| self.lane0_store.last_finalized_root())
    }

    /// Inject a Lane 0 finalized root for integration testing.
    ///
    /// Creates a minimal validator set (one node, stake 1) so that
    /// [`Self::lane0_leading_root`] returns `Some(root)`, and sets the
    /// root on the certificate store. This allows testing the settlement
    /// submission handler without running a full Lane 0 finalization round.
    #[doc(hidden)]
    pub fn test_inject_lane0_root(&mut self, root: [u8; 32]) {
        let kp = crate::crypto::generate_keypair();
        let vs = lane0::ValidatorSet::new(std::iter::once((kp.verifying_key().to_bytes(), 1)))
            .expect("single validator with stake 1");
        self.lane0_validators = Some(vs);
        self.lane0_store.test_set_last_finalized_root(root);
    }

    /// Rotate the Lane 0 validator set (ADR-025 Stage 4: Lane 1 commits as
    /// epoch fences for Lane 0 certificate validity).
    ///
    /// # Caller obligation
    ///
    /// This method must be invoked identically — same `new_validators`,
    /// same logical point in the causal graph — by every honest node.
    /// The only source of that agreement is a **Lane 1 (DAG-consensus
    /// committed) event**: because the commit rule guarantees all honest
    /// nodes compute the same committed order (the `Agreement` property
    /// in `formal-verification/OmniaConsensus.tla`), a validator-set
    /// change carried by a committed event and applied here at commit
    /// time is applied identically everywhere. Calling this from
    /// anything other than committed-event processing (e.g. from a
    /// not-yet-finalized event, or from operator input) breaks that
    /// guarantee and can fork Lane 0 finality across nodes.
    ///
    /// See [`lane0::CertificateStore::rotate_validators`] for the
    /// monotonicity guarantee: certificates already final under the old
    /// set stay final; pending certificates are re-evaluated against the
    /// new set and may finalize immediately.
    ///
    /// No-op (returns an empty `Vec`) if Lane 0 was never enabled via
    /// [`Self::init_lane0`] — a rotation with nothing to rotate.
    pub fn rotate_lane0_validators(&mut self, new_validators: lane0::ValidatorSet) -> Vec<EventId> {
        if self.lane0_validators.is_none() {
            return Vec::new();
        }
        let newly_final = self.lane0_store.rotate_validators(&new_validators);
        tracing::info!(
            epoch = self.lane0_store.epoch(),
            validators = new_validators.len(),
            total_stake = new_validators.total_stake(),
            newly_final = newly_final.len(),
            "Lane 0 validator set rotated"
        );
        self.lane0_validators = Some(new_validators);
        newly_final
    }

    /// Sign Lane 0 acks for freshly inserted events, fold them into the
    /// local certificate store, and queue them for broadcast.
    ///
    /// No-op unless Lane 0 is enabled AND this node's keypair is a member
    /// of the validator set. The events have already passed full
    /// validation (`Event::validate`) and causal-graph insertion — that
    /// is exactly what the ack attests to.
    fn lane0_ack_inserted(&mut self, event_ids: &[EventId]) {
        let Some(ref validators) = self.lane0_validators else {
            return;
        };
        let Some((keypair, _stake)) = self.validator_candidates.get(&self.config.node_id) else {
            return;
        };
        if !validators.contains(&keypair.verifying_key().to_bytes()) {
            return;
        }
        let keypair = keypair.clone();
        for event_id in event_ids {
            // AUDIT-2026-07 H4 (#354): acks commit to the shard state root
            // after applying the event. Per-shard state roots do not exist
            // yet (#365), so we sign with UNBOUND_STATE_ROOT for now — the
            // binding + per-root quorum machinery is in place and activates
            // automatically once #365 supplies a real post-apply root here.
            let ack = lane0::SignedAck::sign(*event_id, lane0::UNBOUND_STATE_ROOT, &keypair);
            // Fold our own ack locally first (a single-validator set
            // self-finalizes here), then queue for broadcast.
            if let Ok(lane0::AckOutcome::NewlyFinal) = self.lane0_store.add_ack(ack.clone(), validators) {
                tracing::debug!(event = %hex::encode(&event_id[..4]), "Lane 0 final (local quorum)");
            }
            self.lane0_outbox.push(ack);
        }
    }

    /// Fold acks received from the network into the certificate store.
    fn lane0_fold_received(&mut self, payloads: Vec<(String, Vec<u8>)>) {
        let Some(ref validators) = self.lane0_validators else {
            return;
        };
        for (topic, data) in payloads {
            if topic != lane0::LANE0_ACKS_TOPIC {
                continue;
            }
            match lane0::decode_ack_batch(&data) {
                Ok(acks) => {
                    for ack in acks {
                        match self.lane0_store.add_ack(ack, validators) {
                            Ok(lane0::AckOutcome::NewlyFinal) => {
                                tracing::debug!("Lane 0 final (network quorum)");
                            }
                            Ok(_) => {}
                            Err(e) => {
                                tracing::warn!("Lane 0 ack rejected: {e}");
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Lane 0 ack batch rejected: {e}");
                }
            }
        }
    }

    /// Broadcast queued Lane 0 acks on the dedicated gossip topic.
    #[cfg(feature = "network")]
    async fn lane0_flush_outbox(&mut self) {
        if self.lane0_outbox.is_empty() {
            return;
        }
        let Some(ref mut gossip) = self.gossip else {
            return;
        };
        // Respect the wire bound: send in chunks if the outbox is huge.
        for chunk in self.lane0_outbox.chunks(lane0::MAX_ACKS_PER_MESSAGE) {
            match lane0::encode_ack_batch(chunk) {
                Ok(bytes) => {
                    if let Err(e) = gossip.publish_raw(lane0::LANE0_ACKS_TOPIC, bytes).await {
                        tracing::warn!("Lane 0 ack broadcast failed: {e}");
                    }
                }
                Err(e) => {
                    tracing::warn!("Lane 0 ack encoding failed: {e}");
                }
            }
        }
        self.lane0_outbox.clear();
    }

    /// Get the current number of connected peers as observed by the
    /// gossip protocol.
    ///
    /// Returns `None` if the gossip protocol is not initialized (e.g.,
    /// when the `network` feature is disabled). Returns `Some(0)` when
    /// gossip is initialized but no peers have been observed yet.
    ///
    /// Used by the node's `/readyz` and `/api/v1/node/peers` endpoints
    /// to report actual peer connectivity. Previously these endpoints
    /// always reported 0 peers because no code path updated the
    /// `AppState.peers` field.
    #[cfg(feature = "network")]
    pub fn connected_peer_count(&self) -> Option<usize> {
        self.gossip.as_ref().map(|g| g.connected_peer_count())
    }

    /// Check if an event has been finalized
    pub fn is_finalized(&self, event_id: &EventId) -> bool {
        self.consensus.is_committed(event_id)
    }

    /// Get all finalized event IDs
    pub fn finalized_events(&self) -> Vec<EventId> {
        self.consensus.get_committed()
    }

    /// Get substrate statistics (async — never blocks inside a Tokio runtime).
    pub async fn stats(&self) -> SubstrateStats {
        let graph_stats = self.graph.read().await.stats();
        let consensus_stats = self.consensus.stats();

        SubstrateStats {
            graph: graph_stats,
            consensus: consensus_stats,
            running: self.running,
        }
    }

    /// Process events through the attached Layer 2 shard processor.
    ///
    /// Iterates over all events in the causal graph and forwards them
    /// to the registered shard processor. Errors are logged but do not
    /// halt the substrate.
    ///
    /// Note: The `run()` loop only forwards *committed* events to the
    /// shard processor. This method processes *all* events and is
    /// primarily kept for direct-use scenarios.
    pub async fn process_event_processors(&mut self) {
        if let Some(ref mut processor) = self.shard_processor {
            let graph = self.graph.read().await;
            for event_id in graph.event_ids() {
                match graph.get_checked(&event_id) {
                    Ok(event) => {
                        if !event.payload.is_empty() {
                            if let Err(e) = processor.process_event(event) {
                                tracing::warn!("Shard processor error: {}", e);
                            }
                        }
                    }
                    Err(CausalGraphError::EventPruned(_)) => {
                        // Pruned events have no payload; skip gracefully
                    }
                    Err(_) => {
                        // Event not found — skip
                    }
                }
            }
        }
    }

    /// Produce a block proposal as the round leader.
    ///
    /// Called when this node is the deterministic-hash-selected leader for the current round.
    /// Drains pending events from the mempool and creates proposal events
    /// for consensus. Events that are already in the graph (e.g., submitted
    /// via `submit_event()`) are skipped gracefully since `CausalGraph::insert()`
    /// returns `DuplicateEvent` for already-inserted events.
    ///
    /// FIND-CRIT-001 FIX: This method is now async to avoid `blocking_write()`
    /// inside an async context, which can deadlock the Tokio runtime.
    async fn propose_block(&mut self, round: u64) -> Vec<EventId> {
        // AUDIT-2026-07 H3 (#353): defensive re-check — refuse to propose
        // unless this node is the active leader for the round, so a proposal
        // can never be produced out of turn even if a caller forgets the
        // gate. No-op when no validator set is configured (leaderless / test
        // mode), preserving existing behaviour.
        if !self.validator_candidates.is_empty()
            && !self.consensus.is_active_leader_for_round(
                &self.config.node_id,
                &self.validator_candidates,
                round,
                LEADER_SCHEDULE_DEPTH,
            )
        {
            tracing::warn!(round, "propose_block called by a non-leader — refusing to propose");
            return Vec::new();
        }

        let pending = self.mempool.drain_up_to(self.max_block_events);
        if pending.is_empty() {
            return Vec::new();
        }

        let mut proposed = Vec::new();
        for event in pending {
            let event_id = event.id;
            // Insert into graph and process through consensus.
            // If the event was already inserted (e.g., via submit_event),
            // CausalGraph::insert returns DuplicateEvent and we skip it —
            // it has already been processed.
            //
            // Uses `.write().await` instead of `blocking_write()` to avoid
            // blocking the async runtime (FIND-CRIT-001).
            {
                let mut graph = self.graph.write().await;
                if let Ok(ids) = graph.insert(event.clone()) {
                    self.unprocessed_events.extend(ids);
                }
            }
            proposed.push(event_id);
        }

        tracing::info!(
            "Leader {} proposed {} events for round {}",
            hex::encode(&self.config.node_id[..4]),
            proposed.len(),
            round
        );

        proposed
    }

    /// Process consensus for unprocessed events only.
    ///
    /// Drains the `unprocessed_events` queue, making consensus O(new_events)
    /// instead of O(total_events). Events are added to the queue when inserted
    /// locally (via `submit_event()`) or from the network (via gossip), or
    /// when proposed as a leader via `propose_block()`.
    pub async fn process_consensus(&mut self) -> Vec<EventId> {
        let graph = self.graph.read().await;
        let mut all_committed = Vec::new();

        // Drain only unprocessed events (topologically ordered)
        let to_process: Vec<EventId> = self.unprocessed_events.drain(..).collect();

        for id in &to_process {
            match graph.get_checked(id) {
                Ok(event) => {
                    if let Ok(committed) = self.consensus.process_event(event, &graph) {
                        all_committed.extend(committed);
                    }
                }
                Err(CausalGraphError::EventPruned(_)) => {
                    tracing::warn!(
                        "Skipping pruned event {} in consensus processing",
                        hex::encode(&id[..4])
                    );
                }
                Err(_) => {
                    tracing::warn!("Event {} not found in graph for consensus", hex::encode(&id[..4]));
                }
            }
        }

        // Persist consensus state after processing if events were committed
        // (indicating round advancement) and a store is configured.
        if !all_committed.is_empty() {
            if let Some(ref store) = self.consensus_store {
                if let Err(e) = self.consensus.persist_state(store.as_ref()) {
                    tracing::warn!(
                        error = %e,
                        "Failed to persist consensus state after round advancement"
                    );
                }
            }
        }

        all_committed
    }
}

/// Statistics about the substrate runtime
#[derive(Debug, Clone)]
pub struct SubstrateStats {
    /// Graph statistics
    pub graph: GraphStats,
    /// Consensus statistics
    pub consensus: consensus::ConsensusStats,
    /// Whether the substrate is running
    pub running: bool,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::crypto::generate_keypair;
    use crate::event::Event;

    /// Serializes tests that modify OMNIA_CONSENSUS_SEED.
    /// Without this, parallel test threads race on the env var.
    static SEED_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn test_node(id: u8) -> NodeId {
        let mut node = [0u8; 32];
        node[0] = id;
        node
    }

    /// Build a `SubstrateConfig` for tests under `SEED_TEST_LOCK` with
    /// `OMNIA_CONSENSUS_SEED` cleared. Uses `try_new()` to avoid panicking
    /// on malformed env vars. Every test that constructs a config must
    /// serialize against the seed-parse tests (which set deliberately
    /// invalid seeds). Routing every construction through this helper
    /// closes that race.
    fn test_config(id: u8) -> SubstrateConfig {
        let _lock = SEED_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("OMNIA_CONSENSUS_SEED");
        SubstrateConfig::try_new(test_node(id)).expect("valid test config")
    }

    #[test]
    fn test_substrate_creation() {
        let config = test_config(1);
        let substrate = Substrate::new(config);

        assert!(!substrate.running);
        #[cfg(feature = "network")]
        assert!(substrate.gossip.is_none());
    }

    #[cfg(feature = "network")]
    #[tokio::test]
    async fn test_substrate_start_stop() {
        let config = test_config(1);
        let mut substrate = Substrate::new(config);

        substrate.init_gossip();
        assert!(substrate.gossip.is_some());

        substrate.start().await;
        assert!(substrate.running);
        assert!(substrate.gossip.as_ref().unwrap().is_running());

        substrate.stop();
        assert!(!substrate.running);
    }

    #[tokio::test]
    async fn test_submit_event() {
        let config = test_config(1);
        let mut substrate = Substrate::new(config);
        let keypair = generate_keypair();

        let mut event = Event::genesis(test_node(1), vec![1, 2, 3]).expect("valid genesis event");
        event.sign_with_keypair(&keypair).expect("signing");

        substrate.submit_event(event).await.unwrap();

        let graph = substrate.graph().await;
        assert_eq!(graph.len(), 1);
    }

    #[tokio::test]
    async fn test_substrate_stats() {
        let config = test_config(1);
        let substrate = Substrate::new(config);

        let stats = substrate.stats().await;
        assert_eq!(stats.graph.total_events, 0);
        assert!(!stats.running);
    }

    /// End-to-end Lane 1 trigger: a committed event carrying a
    /// validator-set change, signed by a current validator, rotates the
    /// set; the same change signed by an outsider is rejected.
    #[tokio::test]
    async fn test_committed_vset_change_applies_with_authorization() {
        let config = test_config(1);
        let mut substrate = Substrate::new(config);

        // Lane 0 active with validator A only.
        let key_a = generate_keypair();
        let initial = lane0::ValidatorSet::new([(key_a.verifying_key().to_bytes(), 1)]).expect("valid set");
        substrate.init_lane0(initial);

        // The change: replace the set with validator B.
        let key_b = generate_keypair();
        let change = lane0::ValidatorSetChange {
            entries: vec![(key_b.verifying_key().to_bytes(), 1)],
        };
        let payload = lane0::encode_vset_change(&change).expect("encodable");

        // These tests exercise apply_committed_vset_changes in isolation
        // — events are inserted into the graph directly (as if committed)
        // rather than run through submit_event, because the consensus
        // engine would flag two same-signer genesis events as
        // equivocation, which is orthogonal to what's under test.
        let insert = |substrate: &mut Substrate, event: Event| {
            let graph = Arc::clone(&substrate.graph);
            async move {
                graph.write().await.insert(event).expect("insert");
            }
        };

        // Outsider C commits the change first — must be rejected.
        let key_c = generate_keypair();
        let mut outsider_event = Event::genesis(test_node(2), payload.clone()).expect("valid event");
        outsider_event.sign_with_keypair(&key_c).expect("signing");
        let outsider_id = outsider_event.id;
        insert(&mut substrate, outsider_event).await;
        substrate.apply_committed_vset_changes(&[outsider_id]).await;
        assert_eq!(
            substrate.lane0_epoch(),
            Some(0),
            "a non-validator's set change must be rejected"
        );

        // Current validator A commits the same change — must apply.
        let mut event = Event::genesis(test_node(3), payload).expect("valid event");
        event.sign_with_keypair(&key_a).expect("signing");
        let event_id = event.id;
        insert(&mut substrate, event).await;
        substrate.apply_committed_vset_changes(&[event_id]).await;
        assert_eq!(substrate.lane0_epoch(), Some(1), "validator-signed change must rotate");

        // And the NEW set governs the next change: A is no longer a
        // member, so a further change signed by A is now rejected.
        let change_back = lane0::ValidatorSetChange {
            entries: vec![(key_a.verifying_key().to_bytes(), 1)],
        };
        let payload_back = lane0::encode_vset_change(&change_back).expect("encodable");
        let mut stale_event = Event::genesis(test_node(4), payload_back).expect("valid event");
        stale_event.sign_with_keypair(&key_a).expect("signing");
        let stale_id = stale_event.id;
        insert(&mut substrate, stale_event).await;
        substrate.apply_committed_vset_changes(&[stale_id]).await;
        assert_eq!(
            substrate.lane0_epoch(),
            Some(1),
            "an ex-validator must not be able to rotate the set back"
        );
    }

    /// Non-vset payloads and malformed changes never rotate; disabled
    /// Lane 0 ignores everything.
    #[tokio::test]
    async fn test_committed_vset_change_ignores_noise() {
        let config = test_config(1);
        let mut substrate = Substrate::new(config);

        let key_a = generate_keypair();
        let initial = lane0::ValidatorSet::new([(key_a.verifying_key().to_bytes(), 1)]).expect("valid set");
        substrate.init_lane0(initial);

        // Direct graph insertion — see the note in
        // test_committed_vset_change_applies_with_authorization.
        // Distinct signers avoid consensus-side equivocation flags.

        // Ordinary payload: ignored.
        let mut plain = Event::genesis(test_node(2), vec![1, 2, 3]).expect("valid event");
        plain.sign_with_keypair(&key_a).expect("signing");
        let plain_id = plain.id;
        substrate.graph.write().await.insert(plain).expect("insert");

        // Tagged but malformed: skipped with a warning.
        let mut malformed_payload = lane0::VSET_PAYLOAD_TAG.to_vec();
        malformed_payload.extend([0xFF, 0xFF, 0xFF]);
        let mut malformed = Event::genesis(test_node(3), malformed_payload).expect("valid event");
        malformed.sign_with_keypair(&generate_keypair()).expect("signing");
        let malformed_id = malformed.id;
        substrate.graph.write().await.insert(malformed).expect("insert");

        substrate.apply_committed_vset_changes(&[plain_id, malformed_id]).await;
        assert_eq!(substrate.lane0_epoch(), Some(0));
    }

    #[test]
    fn test_rotate_lane0_validators_noop_when_disabled() {
        let config = test_config(1);
        let mut substrate = Substrate::new(config);
        assert_eq!(substrate.lane0_epoch(), None);

        let key = generate_keypair();
        let set = lane0::ValidatorSet::new([(key.verifying_key().to_bytes(), 1)]).expect("valid set");
        let newly_final = substrate.rotate_lane0_validators(set);

        assert!(newly_final.is_empty());
        assert_eq!(
            substrate.lane0_epoch(),
            None,
            "rotation must stay a no-op until Lane 0 is enabled via init_lane0"
        );
    }

    #[test]
    fn test_rotate_lane0_validators_delegates_and_tracks_epoch() {
        let config = test_config(1);
        let mut substrate = Substrate::new(config);

        let key1 = generate_keypair();
        let initial = lane0::ValidatorSet::new([(key1.verifying_key().to_bytes(), 1)]).expect("valid set");
        substrate.init_lane0(initial);
        assert_eq!(substrate.lane0_epoch(), Some(0));

        let key2 = generate_keypair();
        let rotated = lane0::ValidatorSet::new([(key2.verifying_key().to_bytes(), 1)]).expect("valid set");
        substrate.rotate_lane0_validators(rotated);

        assert_eq!(
            substrate.lane0_epoch(),
            Some(1),
            "epoch must advance on every rotation, even with no pending certificates"
        );
        assert!(
            substrate.lane0_stats().is_some(),
            "Lane 0 must remain enabled after rotation"
        );
    }

    #[test]
    fn test_constants() {
        assert_eq!(PROTOCOL_VERSION, "4.0.0");
        assert_eq!(PROTOCOL_IDENTIFIER, "/omnia/4.0.0");
        assert_eq!(TARGET_TPS, 10_000);
        assert_eq!(TARGET_FINALITY_MS, 5_000);
    }

    #[test]
    fn test_error_conversion() {
        let vc_err = VectorClockError::InvalidNodeId("test".to_string());
        let substrate_err: SubstrateError = vc_err.into();
        assert!(matches!(substrate_err, SubstrateError::VectorClock(_)));
    }

    #[test]
    fn test_substrate_config_with_network_size() {
        let _lock = SEED_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("OMNIA_CONSENSUS_SEED");
        let config = SubstrateConfig::try_with_network_size(test_node(1), 10).expect("valid test config");
        assert_eq!(config.total_nodes, 10);
        assert_eq!(config.consensus.total_nodes, 10);
    }

    #[test]
    fn test_event_processor_error_variants_display() {
        let e = EventProcessorError::Deserialization("bad payload".to_string());
        assert!(e.to_string().contains("bad payload"));

        let e = EventProcessorError::ValidationFailed("replay".to_string());
        assert!(e.to_string().contains("replay"));

        let e = EventProcessorError::UnknownShard("shard-0".to_string());
        assert!(e.to_string().contains("shard-0"));

        let e = EventProcessorError::ShardError("conflict".to_string());
        assert!(e.to_string().contains("conflict"));

        let e = EventProcessorError::Internal("oops".to_string());
        assert!(e.to_string().contains("oops"));
    }

    // ── H-12: try_parse_consensus_seed tests ───────────────────────────

    #[test]
    fn test_try_parse_consensus_seed_unset_returns_ok() {
        let _lock = SEED_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("OMNIA_CONSENSUS_SEED");
        let result = try_parse_consensus_seed();
        assert!(result.is_ok(), "Unset seed should return Ok(random seed)");
        let seed = result.unwrap();
        // Random seed is 32 bytes — extremely unlikely to be all zeros
        assert_ne!(seed, [0u8; 32], "Random seed should not be all zeros");
    }

    #[test]
    fn test_try_parse_consensus_seed_valid_hex() {
        let _lock = SEED_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let hex_seed = "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20";
        std::env::set_var("OMNIA_CONSENSUS_SEED", hex_seed);
        let result = try_parse_consensus_seed();
        assert!(result.is_ok(), "Valid hex seed should parse");
        let seed = result.unwrap();
        assert_eq!(seed[0], 0x01);
        assert_eq!(seed[31], 0x20);
        std::env::remove_var("OMNIA_CONSENSUS_SEED");
    }

    #[test]
    fn test_try_parse_consensus_seed_invalid_hex() {
        let _lock = SEED_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // 64 chars long (passes length check) but contains non-hex chars
        std::env::set_var(
            "OMNIA_CONSENSUS_SEED",
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
        );
        let result = try_parse_consensus_seed();
        assert!(result.is_err(), "Invalid hex should return Err");
        match result.unwrap_err() {
            ConsensusSeedError::InvalidHex { .. } => {}
            other => panic!("Expected InvalidHex, got {other:?}"),
        }
        std::env::remove_var("OMNIA_CONSENSUS_SEED");
    }

    #[test]
    fn test_try_parse_consensus_seed_wrong_length() {
        let _lock = SEED_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("OMNIA_CONSENSUS_SEED", "0102"); // too short
        let result = try_parse_consensus_seed();
        assert!(result.is_err(), "Wrong-length seed should return Err");
        match result.unwrap_err() {
            ConsensusSeedError::InvalidLength { .. } => {}
            other => panic!("Expected InvalidLength, got {other:?}"),
        }
        std::env::remove_var("OMNIA_CONSENSUS_SEED");
    }

    #[test]
    fn test_consensus_seed_error_display() {
        let e = ConsensusSeedError::InvalidLength {
            actual: 10,
            expected: 64,
        };
        assert!(e.to_string().contains("64 hex characters"));
        assert!(e.to_string().contains("10"));

        let hex_err = hex::decode("zz").unwrap_err();
        let e = ConsensusSeedError::InvalidHex {
            source: hex_err,
            raw: "zz".into(),
        };
        assert!(e.to_string().contains("invalid hex"));
    }

    #[test]
    fn test_try_new_returns_config() {
        let _lock = SEED_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("OMNIA_CONSENSUS_SEED");
        let config = SubstrateConfig::try_new(test_node(1));
        assert!(config.is_ok(), "try_new should succeed with no seed set");
        assert_eq!(config.unwrap().node_id, test_node(1));
    }

    #[test]
    fn test_try_with_network_size_returns_config() {
        let _lock = SEED_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("OMNIA_CONSENSUS_SEED");
        let config = SubstrateConfig::try_with_network_size(test_node(1), 7);
        assert!(config.is_ok(), "try_with_network_size should succeed");
        let config = config.unwrap();
        assert_eq!(config.total_nodes, 7);
        assert_eq!(config.consensus.total_nodes, 7);
    }

    #[test]
    fn test_try_new_invalid_seed_returns_err() {
        let _lock = SEED_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("OMNIA_CONSENSUS_SEED", "bad");
        let result = SubstrateConfig::try_new(test_node(1));
        assert!(result.is_err(), "Invalid seed should propagate error");
        std::env::remove_var("OMNIA_CONSENSUS_SEED");
    }

    // ── Substrate API tests ────────────────────────────────────────────

    #[test]
    fn test_substrate_config_defaults() {
        let config = test_config(1);
        assert_eq!(config.node_id, test_node(1));
        assert_eq!(config.total_nodes, 4); // default
        assert_eq!(config.slash_threshold, DEFAULT_SLASH_THRESHOLD);
        assert_eq!(config.ejection_threshold, DEFAULT_EJECTION_THRESHOLD);
        assert_eq!(config.max_payload_size, MAX_PAYLOAD_SIZE);
        assert_eq!(config.pruning_depth, 0);
        assert!(config.slashing_data_dir.is_none());
        assert!(config.nonce_data_dir.is_none());
        assert!(config.consensus_data_dir.is_none());
    }

    #[test]
    fn test_mempool_accessors() {
        let config = test_config(1);
        let mut substrate = Substrate::new(config);
        assert_eq!(substrate.mempool().len(), 0);

        // Insert an event into the mempool
        let keypair = generate_keypair();
        let mut event = Event::genesis(test_node(1), vec![]).expect("genesis");
        event.sign_with_keypair(&keypair).expect("signing");
        substrate.mempool_mut().insert(event).expect("mempool insert");
        assert_eq!(substrate.mempool().len(), 1);
    }

    #[test]
    fn test_add_validator() {
        let config = test_config(1);
        let mut substrate = Substrate::new(config);
        assert!(substrate.validator_candidates.is_empty());

        let keypair = generate_keypair();
        substrate.add_validator(test_node(2), keypair, 10_000);
        assert_eq!(substrate.validator_candidates.len(), 1);
        assert!(substrate.validator_candidates.contains_key(&test_node(2)));
    }

    // ── AUDIT-2026-07 H5 (#355): finality lifecycle + divergence ──────

    #[test]
    fn test_finality_state_helpers() {
        assert!(FinalityState::Canonical.is_irreversible());
        assert!(FinalityState::Final.is_irreversible());
        assert!(!FinalityState::Preconfirmed.is_irreversible());
        assert!(!FinalityState::Diverged.is_irreversible());
        assert!(!FinalityState::Pending.is_irreversible());
        assert_eq!(FinalityState::Preconfirmed.as_str(), "preconfirmed");
        assert_eq!(FinalityState::Diverged.as_str(), "diverged");
    }

    #[test]
    fn test_finality_state_pending_for_unknown_event() {
        let substrate = Substrate::new(test_config(1));
        assert_eq!(substrate.finality_state(&[9u8; 32]), FinalityState::Pending);
    }

    #[test]
    fn test_finality_preconfirmed_then_diverged() {
        let mut substrate = Substrate::new(test_config(1));

        // Preconfirm an event on Lane 0 via a single-validator quorum.
        let kp = generate_keypair();
        let vset = lane0::ValidatorSet::new([(kp.verifying_key().to_bytes(), 1u64)]).unwrap();
        let ev: EventId = [7u8; 32];
        let ack = lane0::SignedAck::sign(ev, lane0::UNBOUND_STATE_ROOT, &kp);
        substrate.lane0_store.add_ack(ack, &vset).unwrap();

        assert!(substrate.lane0_is_final(&ev));
        assert_eq!(
            substrate.finality_state(&ev),
            FinalityState::Preconfirmed,
            "a Lane 0 quorum is a reversible preconfirmation, not canonical finality"
        );

        // Lane 1 rejects it → the fast path diverged.
        assert!(
            substrate.reconcile_lane1_rejection(ev),
            "first rejection records a divergence"
        );
        assert_eq!(substrate.finality_state(&ev), FinalityState::Diverged);
        assert_eq!(substrate.lane0_divergence_count(), 1);

        // Idempotent: re-reporting the same rejection is not a new divergence.
        assert!(!substrate.reconcile_lane1_rejection(ev));
        assert_eq!(substrate.lane0_divergence_count(), 1);
    }

    #[test]
    fn test_reconcile_ignores_rejection_of_non_preconfirmed_event() {
        let mut substrate = Substrate::new(test_config(1));
        let ev: EventId = [3u8; 32];
        // Lane 1 rejecting an event Lane 0 never preconfirmed is normal, not
        // a divergence.
        assert!(!substrate.reconcile_lane1_rejection(ev));
        assert_eq!(substrate.lane0_divergence_count(), 0);
        assert_eq!(substrate.finality_state(&ev), FinalityState::Pending);
    }

    #[test]
    fn test_with_shard_processor() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        struct CountingProcessor {
            count: Arc<AtomicUsize>,
        }
        impl EventProcessor for CountingProcessor {
            fn process_event(&mut self, _event: &Event) -> std::result::Result<(), EventProcessorError> {
                self.count.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }

        let count = Arc::new(AtomicUsize::new(0));
        let processor = CountingProcessor {
            count: Arc::clone(&count),
        };

        let config = test_config(1);
        let substrate = Substrate::new(config).with_shard_processor(Box::new(processor));
        assert!(substrate.shard_processor.is_some());
    }

    #[test]
    fn test_with_validator_candidates() {
        let config = test_config(1);
        let keypair = generate_keypair();
        let mut candidates = HashMap::new();
        candidates.insert(test_node(2), (keypair, 10_000u64));

        let substrate = Substrate::new(config).with_validator_candidates(candidates);
        assert_eq!(substrate.validator_candidates.len(), 1);
    }

    #[tokio::test]
    async fn test_is_finalized_and_finalized_events() {
        let config = test_config(1);
        let mut substrate = Substrate::new(config);
        let keypair = generate_keypair();

        let mut event = Event::genesis(test_node(1), vec![1]).expect("genesis");
        event.sign_with_keypair(&keypair).expect("signing");
        let event_id = event.id;

        // Before submission: not finalized
        assert!(!substrate.is_finalized(&event_id));
        assert!(substrate.finalized_events().is_empty());

        substrate.submit_event(event).await.unwrap();

        // After submission: may or may not be finalized depending on consensus,
        // but is_finalized should not panic
        let _ = substrate.is_finalized(&event_id);
        let _ = substrate.finalized_events();
    }

    #[test]
    fn test_consensus_stats() {
        let config = test_config(1);
        let substrate = Substrate::new(config);
        let stats = substrate.consensus_stats();
        // Fresh substrate: round 0, no committed events
        assert_eq!(stats.current_max_round, 0);
    }

    #[tokio::test]
    async fn test_process_consensus_empty() {
        let config = test_config(1);
        let mut substrate = Substrate::new(config);
        // No events submitted — process_consensus should return empty
        let committed = substrate.process_consensus().await;
        assert!(committed.is_empty());
    }

    #[tokio::test]
    async fn test_process_consensus_round_empty() {
        let config = test_config(1);
        let mut substrate = Substrate::new(config);
        // Should not panic with empty state
        substrate.process_consensus_round().await;
    }

    #[tokio::test]
    async fn test_process_event_processors_no_processor() {
        let config = test_config(1);
        let mut substrate = Substrate::new(config);
        // No shard processor attached — should be a no-op
        substrate.process_event_processors().await;
    }

    #[tokio::test]
    async fn test_process_event_processors_with_processor() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        struct CountingProcessor {
            count: Arc<AtomicUsize>,
        }
        impl EventProcessor for CountingProcessor {
            fn process_event(&mut self, _event: &Event) -> std::result::Result<(), EventProcessorError> {
                self.count.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }

        let count = Arc::new(AtomicUsize::new(0));
        let processor = CountingProcessor {
            count: Arc::clone(&count),
        };

        let config = test_config(1);
        let mut substrate = Substrate::new(config).with_shard_processor(Box::new(processor));

        // Submit an event with non-empty payload so the processor sees it
        let keypair = generate_keypair();
        let mut event = Event::genesis(test_node(1), vec![1, 2, 3]).expect("genesis");
        event.sign_with_keypair(&keypair).expect("signing");
        substrate.submit_event(event).await.unwrap();

        // Process event processors — should forward the event
        substrate.process_event_processors().await;
        assert_eq!(
            count.load(Ordering::SeqCst),
            1,
            "Processor should have been called once"
        );
    }

    #[test]
    fn test_substrate_error_variants() {
        // Test all From conversions
        let vc_err = VectorClockError::InvalidNodeId("test".to_string());
        let substrate_err: SubstrateError = vc_err.into();
        assert!(matches!(substrate_err, SubstrateError::VectorClock(_)));

        let cg_err = CausalGraphError::DuplicateEvent("test".to_string());
        let substrate_err: SubstrateError = cg_err.into();
        assert!(matches!(substrate_err, SubstrateError::CausalGraph(_)));

        let ev_err = EventValidationError::UnsignedEvent;
        let substrate_err: SubstrateError = ev_err.into();
        assert!(matches!(substrate_err, SubstrateError::EventValidation(_)));

        let config_err = SubstrateError::Config("bad config".into());
        assert!(config_err.to_string().contains("Configuration error"));
    }

    #[tokio::test]
    async fn test_submit_invalid_event_returns_error() {
        let config = test_config(1);
        let mut substrate = Substrate::new(config);

        // Unsigned event should fail validation
        let event = Event::genesis(test_node(1), vec![]).expect("genesis");
        // Note: NOT signing the event
        let result = substrate.submit_event(event).await;
        assert!(result.is_err(), "Unsigned event should be rejected");
    }

    #[tokio::test]
    async fn test_stats_after_submit() {
        let config = test_config(1);
        let mut substrate = Substrate::new(config);
        let keypair = generate_keypair();

        let mut event = Event::genesis(test_node(1), vec![]).expect("genesis");
        event.sign_with_keypair(&keypair).expect("signing");
        substrate.submit_event(event).await.unwrap();

        let stats = substrate.stats().await;
        assert_eq!(stats.graph.total_events, 1);
        assert!(!stats.running);
    }
}
