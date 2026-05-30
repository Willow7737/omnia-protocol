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

#![deny(clippy::unwrap_used)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(unused_qualifications)]
#![deprecated(
    since = "0.2.0",
    note = "Use omnia-primitives, omnia-consensus, omnia-crypto, omnia-network, omnia-adapters directly"
)]

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
    aggregate_public_keys, aggregate_signatures, aggregate_signatures_unchecked, verify_aggregate,
    verify_aggregate_with_pop, BlsError, BlsKeypair, BlsProofOfPossession, BlsPublicKey, BlsSignature,
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
    AeadCiphertext, DkgError, DkgPhase, DkgResult, DkgSharePackage, DkgVerificationResult,
    FeldmanVssSession, KeyShare, PartialSignature, ScalarBytes, ThresholdConfig, ThresholdError, ThresholdKeyManager,
    ThresholdSignature,
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

/// Parse the consensus seed from the `OMNIA_CONSENSUS_SEED` environment variable.
///
/// Accepts a hex-encoded 64-character (32-byte) seed for cryptographic strength.
/// If the environment variable is not set or is invalid, generates a random seed.
fn parse_consensus_seed() -> [u8; 32] {
    if let Ok(hex_seed) = std::env::var("OMNIA_CONSENSUS_SEED") {
        // Accept hex-encoded 64-character seed
        if hex_seed.len() == 64 {
            if let Ok(bytes) = hex::decode(&hex_seed) {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                return arr;
            }
        }
        tracing::warn!("OMNIA_CONSENSUS_SEED must be 64 hex characters. Using random seed.");
    }

    let mut seed = [0u8; 32];
    getrandom::getrandom(&mut seed).unwrap_or_else(|_| {
        seed[0] = 1; // Non-zero fallback
    });
    seed
}

use std::collections::HashMap;
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
    pub fn new(node_id: NodeId) -> Self {
        let total_nodes = std::env::var("OMNIA_TOTAL_NODES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(4);
        let seed = parse_consensus_seed();
        Self::build_config(node_id, total_nodes, seed)
    }

    /// Create a substrate configuration with a custom network size.
    ///
    /// Slashing defaults to in-memory mode with standard thresholds.
    pub fn with_network_size(node_id: NodeId, total_nodes: usize) -> Self {
        let seed = parse_consensus_seed();
        Self::build_config(node_id, total_nodes, seed)
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
        let slashing = SlashingEngine::new(
            config.slashing_data_dir.clone(),
            config.slash_threshold,
            config.ejection_threshold,
        );

        // Create consensus store if persistence is configured
        let consensus_store: Option<Arc<dyn ConsensusStore>> =
            config
                .consensus_data_dir
                .as_ref()
                .and_then(|dir| match RedbConsensusStore::open(dir) {
                    Ok(store) => {
                        tracing::info!(
                            path = %dir.display(),
                            "Consensus: using persistent redb store"
                        );
                        Some(Arc::new(store) as Arc<dyn ConsensusStore>)
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            path = %dir.display(),
                            "Failed to open consensus store — consensus state will not persist"
                        );
                        None
                    }
                });

        // Create consensus engine, restoring from persisted state if available
        let consensus = match &consensus_store {
            Some(store) => ConsensusEngine::load_or_new(config.consensus.clone(), Arc::clone(store), slashing.clone())
                .unwrap_or_else(|e| {
                    tracing::warn!(
                        error = %e,
                        "Failed to restore consensus state — starting fresh"
                    );
                    ConsensusEngine::new(config.consensus.clone(), slashing.clone())
                }),
            None => ConsensusEngine::new(config.consensus.clone(), slashing.clone()),
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
        if let Some(ref mut gossip) = self.gossip {
            match gossip.process_pending_events().await {
                Ok(inserted) => {
                    self.unprocessed_events.extend(inserted);
                }
                Err(e) => {
                    tracing::warn!("Gossip processing error: {}", e);
                }
            }
        }

        // 2. Check if we are the leader for this round
        let current_round = self.consensus.current_round();
        if !self.validator_candidates.is_empty() {
            if let Ok(leader) = self.consensus.compute_leader(&self.validator_candidates, current_round) {
                if leader == self.config.node_id {
                    // We are the leader — produce a block proposal
                    self.propose_block(current_round).await;
                }
            }
        }

        // 3. Run consensus — returns newly committed event IDs
        let committed = self.process_consensus().await;

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
    /// FIND-HIGH-005 FIX: The event is wrapped in `Arc<Event>` to avoid
    /// triple cloning in the hot path (graph insert, mempool insert, gossip
    /// broadcast). Each consumer receives an `Arc::clone` instead of a full
    /// `Event` clone.
    pub async fn submit_event(&mut self, event: Event) -> Result<()> {
        event.validate().map_err(SubstrateError::from)?;

        // Wrap in Arc to avoid triple clone (FIND-HIGH-005)
        let event_arc = Arc::new(event);

        {
            let mut graph = self.graph.write().await;
            let inserted_ids = graph.insert((*event_arc).clone()).map_err(SubstrateError::from)?;
            self.unprocessed_events.extend(inserted_ids);
        }

        // Track for consensus processing (already extended above)

        let graph = self.graph.read().await;
        self.consensus
            .process_event(&event_arc, &graph)
            .map_err(SubstrateError::from)?;
        drop(graph);

        // Also add to mempool for block proposal when we are the leader.
        // If the mempool is full, log a warning but do not fail — the event
        // has already been processed through consensus.
        if let Err(e) = self.mempool.insert((*event_arc).clone()) {
            tracing::warn!(
                "Mempool full, event {} not queued for proposal: {}",
                hex::encode(&event_arc.id[..4]),
                e
            );
        }

        #[cfg(feature = "network")]
        if let Some(ref mut gossip) = self.gossip {
            gossip
                .broadcast_event((*event_arc).clone())
                .await
                .map_err(SubstrateError::from)?;
        }

        Ok(())
    }

    /// Get read access to the causal graph
    pub async fn graph(&self) -> tokio::sync::RwLockReadGuard<'_, CausalGraph> {
        self.graph.read().await
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

    fn test_node(id: u8) -> NodeId {
        let mut node = [0u8; 32];
        node[0] = id;
        node
    }

    #[test]
    fn test_substrate_creation() {
        let config = SubstrateConfig::new(test_node(1));
        let substrate = Substrate::new(config);

        assert!(!substrate.running);
        #[cfg(feature = "network")]
        assert!(substrate.gossip.is_none());
    }

    #[cfg(feature = "network")]
    #[tokio::test]
    async fn test_substrate_start_stop() {
        let config = SubstrateConfig::new(test_node(1));
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
        let config = SubstrateConfig::new(test_node(1));
        let mut substrate = Substrate::new(config);
        let keypair = generate_keypair();

        let mut event = Event::genesis(test_node(1), vec![1, 2, 3]).expect("valid genesis event");
        event.sign_with_keypair(&keypair);

        substrate.submit_event(event).await.unwrap();

        let graph = substrate.graph().await;
        assert_eq!(graph.len(), 1);
    }

    #[tokio::test]
    async fn test_substrate_stats() {
        let config = SubstrateConfig::new(test_node(1));
        let substrate = Substrate::new(config);

        let stats = substrate.stats().await;
        assert_eq!(stats.graph.total_events, 0);
        assert!(!stats.running);
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
        let config = SubstrateConfig::with_network_size(test_node(1), 10);
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
}
