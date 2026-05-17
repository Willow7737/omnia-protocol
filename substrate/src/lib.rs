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

pub mod bls;
pub mod causal_graph;
pub mod consensus;
pub mod crdt;
pub mod crypto;
pub mod crypto_schemes;
pub mod event;
pub mod genesis_replay;
pub mod gossip;
pub mod keystore;
pub mod migration;
pub mod network;
pub mod rate_limiter;
pub mod slashing;
pub mod slashing_undo;
pub mod snapshot;
pub mod snapshot_replication;
pub mod threshold;
pub mod vector_clock;
pub mod vrf;
pub mod wire_format;

// Re-export commonly used types
pub use bls::{
    aggregate_public_keys, aggregate_signatures, verify_aggregate, verify_aggregate_with_pop,
    BlsError, BlsKeypair, BlsProofOfPossession, BlsPublicKey, BlsSignature,
};
pub use causal_graph::{
    CausalGraph, CausalGraphError, GraphSnapshot, GraphStats, PrunedEventMetadata,
};
pub use consensus::{ConsensusConfig, ConsensusEngine, ConsensusError, ConsensusState, RoundTimer};
pub use crdt::{CrdtError, CvRDT, GCounter, LwwRegister, OrSet};
pub use crypto::{generate_keypair, NodeKeypair, NodePublicKey};
pub use crypto_schemes::{
    CryptoProfile, HashScheme, SchemeVersion, SignatureScheme, VrfScheme, ZkScheme,
};
pub use event::{
    Event, EventBatch, EventHeader, EventId, EventRequest, EventStatus, EventValidationError,
    MAX_EVENT_AGE_MS, MAX_PAYLOAD_SIZE, MAX_TIMESTAMP_DRIFT_MS,
};
pub use genesis_replay::{replay_genesis, ReplayConfig, ReplayResult};
pub use gossip::{
    GossipConfig, GossipDigest, GossipError, GossipEvent, GossipMessage, GossipProtocol,
    GossipStats,
};
pub use keystore::{EncryptedKeyStore, KeyRotationProof, KeyStoreError};
pub use network::{
    check_version_compatibility, NetworkCommand, NetworkEvent, OmniaBehaviour, OmniaNetwork,
    VersionCompatibility, VersionHandshake,
};
pub use rate_limiter::RateLimiter;
pub use slashing::{
    InMemorySlashingStore, RedbSlashingStore, SlashOffense, SlashOutcome, SlashingEngine,
    SlashingState, SlashingStore, SlashingStoreError, DEFAULT_EJECTION_THRESHOLD,
    DEFAULT_SLASH_THRESHOLD,
};
pub use slashing_undo::{SlashingUndoManager, SlashingUndoRecord, SlashingUndoRequest};
pub use snapshot::{SnapshotError, StateSnapshot};
pub use snapshot_replication::{find_latest_snapshot, replicate_snapshot, ReplicationConfig};
pub use threshold::{
    KeyShare, PartialSignature, ThresholdConfig, ThresholdKeyManager, ThresholdSignature,
};
pub use vector_clock::{CausalOrder, NodeId, VectorClock, VectorClockError};
pub use vrf::{select_leader, vrf_compute, vrf_verify, VrfError, VrfOutput};
pub use wire_format::{
    deserialize_with_version, serialize_with_version, WireFormatError, WIRE_FORMAT_VERSION,
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

use std::path::PathBuf;

use thiserror::Error;

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
///     fn process_event(&mut self, event: &Event) -> Result<(), String> {
///         // Handle the event
///         Ok(())
///     }
/// }
/// ```
pub trait EventProcessor: Send + Sync {
    /// Process a single event. Return an error string if processing fails.
    fn process_event(&mut self, event: &Event) -> std::result::Result<(), String>;
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
}

impl SubstrateConfig {
    /// Create a new substrate configuration with default settings.
    ///
    /// Slashing defaults to in-memory mode (`slashing_data_dir = None`)
    /// with standard thresholds (slash at 500, eject at 2000).
    /// Production callers should set `slashing_data_dir` before
    /// constructing the substrate.
    pub fn new(node_id: NodeId) -> Self {
        let total_nodes = 4;
        let mut seed = [0u8; 32];
        seed[0] = 1; // Non-zero to avoid debug-build panic
        Self {
            node_id,
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
        }
    }

    /// Create a substrate configuration with a custom network size.
    ///
    /// Slashing defaults to in-memory mode with standard thresholds.
    pub fn with_network_size(node_id: NodeId, total_nodes: usize) -> Self {
        let mut seed = [0u8; 32];
        seed[0] = 1; // Non-zero to avoid debug-build panic
        Self {
            node_id,
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
        }
    }
}

/// The main Substrate runtime that coordinates all components
pub struct Substrate {
    config: SubstrateConfig,
    graph: std::sync::Arc<tokio::sync::RwLock<CausalGraph>>,
    gossip: Option<GossipProtocol>,
    consensus: ConsensusEngine,
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
        let consensus = ConsensusEngine::new(config.consensus.clone(), slashing.clone());
        let graph = std::sync::Arc::new(tokio::sync::RwLock::new(CausalGraph::new()));

        Self {
            config,
            graph,
            gossip: None,
            consensus,
            slashing,
            running: false,
            shard_processor: None,
            unprocessed_events: Vec::new(),
        }
    }

    /// Initialize the gossip protocol
    pub fn init_gossip(&mut self) {
        self.gossip = Some(GossipProtocol::new(
            self.config.node_id,
            self.config.gossip.clone(),
            std::sync::Arc::clone(&self.graph),
        ));
    }

    /// Start the substrate runtime
    pub async fn start(&mut self) {
        self.running = true;
        if let Some(ref mut gossip) = self.gossip {
            gossip.start().await;
        }
    }

    /// Stop the substrate runtime
    pub fn stop(&mut self) {
        self.running = false;
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

    /// Run the substrate main loop. Processes network events, consensus,
    /// and Layer 2 shard processors until `stop()` is called.
    ///
    /// Only committed events are forwarded to the shard processor,
    /// ensuring that shards never observe un-finalized state that
    /// could later be rolled back.
    pub async fn run(&mut self) {
        self.running = true;

        while self.running {
            // 1. Drain network events into graph + queue
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

            // 2. Run consensus — returns newly committed event IDs
            let committed = self.process_consensus().await;

            // 3. Process committed events through shard processor
            if let Some(ref mut processor) = self.shard_processor {
                let graph = self.graph.read().await;
                for event_id in &committed {
                    match graph.get_checked(event_id) {
                        Ok(event) => {
                            if let Err(e) = processor.process_event(event) {
                                tracing::warn!(
                                    "Shard processor error for event {}: {}",
                                    hex::encode(&event_id[..4]),
                                    e
                                );
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

            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    /// Start with network and run main loop
    pub async fn start_with_network(&mut self, network: OmniaNetwork) {
        if let Some(ref mut gossip) = self.gossip {
            if let Err(e) = gossip.start_with_network(network).await {
                tracing::error!("Failed to start gossip with network: {}", e);
            }
        }
        self.run().await;
    }

    /// Submit an event to the substrate for processing
    pub async fn submit_event(&mut self, event: Event) -> Result<()> {
        event.validate().map_err(SubstrateError::from)?;

        {
            let mut graph = self.graph.write().await;
            graph.insert(event.clone()).map_err(SubstrateError::from)?;
        }

        // Track for consensus processing
        self.unprocessed_events.push(event.id);

        let graph = self.graph.read().await;
        self.consensus
            .process_event(&event, &graph)
            .map_err(SubstrateError::from)?;
        drop(graph);

        if let Some(ref mut gossip) = self.gossip {
            gossip
                .broadcast_event(event)
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

    /// Process consensus for unprocessed events only.
    ///
    /// Drains the `unprocessed_events` queue, making consensus O(new_events)
    /// instead of O(total_events). Events are added to the queue when inserted
    /// locally (via `submit_event()`) or from the network (via gossip).
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
                    tracing::warn!(
                        "Event {} not found in graph for consensus",
                        hex::encode(&id[..4])
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
        assert!(substrate.gossip.is_none());
    }

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

        let mut event = Event::genesis(test_node(1), vec![1, 2, 3]);
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
}
