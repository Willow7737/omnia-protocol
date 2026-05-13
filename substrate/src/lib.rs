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

#![warn(missing_docs)]
#![warn(unused_qualifications)]

pub mod causal_graph;
pub mod consensus;
pub mod crdt;
pub mod crypto;
pub mod event;
pub mod gossip;
pub mod network;
pub mod vector_clock;

// Re-export commonly used types
pub use causal_graph::{CausalGraph, CausalGraphError, GraphSnapshot, GraphStats};
pub use consensus::{ConsensusConfig, ConsensusEngine, ConsensusError, ConsensusState};
pub use crdt::{CvRDT, GCounter, LwwRegister, OrSet};
pub use crypto::{generate_keypair, NodeKeypair, NodePublicKey};
pub use event::{
    Event, EventBatch, EventHeader, EventId, EventRequest, EventStatus, EventValidationError,
    MAX_EVENT_AGE_MS, MAX_TIMESTAMP_DRIFT_MS,
};
pub use gossip::{GossipConfig, GossipDigest, GossipError, GossipEvent, GossipMessage, GossipProtocol, GossipStats};
pub use network::{NetworkCommand, NetworkEvent, OmniaBehaviour, OmniaNetwork};
pub use vector_clock::{CausalOrder, NodeId, VectorClock, VectorClockError};

/// Semantic version of this crate
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Protocol version identifier
pub const PROTOCOL_VERSION: u32 = 1;

/// Target throughput (transactions per second)
pub const TARGET_TPS: u32 = 10_000;

/// Target latency for finality (milliseconds)
pub const TARGET_FINALITY_MS: u64 = 5_000;

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
    VectorClock(#[from] VectorClockError),
    #[error("Causal graph error: {0}")]
    CausalGraph(#[from] CausalGraphError),
    #[error("Event validation error: {0}")]
    EventValidation(#[from] EventValidationError),
    #[error("Gossip error: {0}")]
    Gossip(#[from] GossipError),
    #[error("Consensus error: {0}")]
    Consensus(#[from] ConsensusError),
    #[error("Configuration error: {0}")]
    Config(String),
}

/// Result type for substrate operations
pub type Result<T> = std::result::Result<T, SubstrateError>;

/// Configuration for the entire substrate layer
#[derive(Debug, Clone)]
pub struct SubstrateConfig {
    pub node_id: NodeId,
    pub gossip: GossipConfig,
    pub consensus: ConsensusConfig,
    pub total_nodes: usize,
}

impl SubstrateConfig {
    pub fn new(node_id: NodeId) -> Self {
        let total_nodes = 4;
        Self {
            node_id,
            gossip: GossipConfig::default(),
            consensus: ConsensusConfig {
                total_nodes,
                ..Default::default()
            },
            total_nodes,
        }
    }

    pub fn with_network_size(node_id: NodeId, total_nodes: usize) -> Self {
        Self {
            node_id,
            gossip: GossipConfig::default(),
            consensus: ConsensusConfig {
                total_nodes,
                ..Default::default()
            },
            total_nodes,
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
    pub fn new(config: SubstrateConfig) -> Self {
        let graph = std::sync::Arc::new(tokio::sync::RwLock::new(CausalGraph::new()));
        let consensus = ConsensusEngine::new(config.consensus.clone());

        Self {
            config,
            graph,
            gossip: None,
            consensus,
            running: false,
            shard_processor: None,
            unprocessed_events: Vec::new(),
        }
    }

    pub fn init_gossip(&mut self) {
        self.gossip = Some(GossipProtocol::new(
            self.config.node_id,
            self.config.gossip.clone(),
            std::sync::Arc::clone(&self.graph),
        ));
    }

    pub async fn start(&mut self) {
        self.running = true;
        if let Some(ref mut gossip) = self.gossip {
            gossip.start().await;
        }
    }

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
                    if let Some(event) = graph.get(event_id) {
                        if let Err(e) = processor.process_event(event) {
                            tracing::warn!(
                                "Shard processor error for event {}: {}",
                                hex::encode(&event_id[..4]),
                                e
                            );
                        }
                    }
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    /// Start with network and run main loop.
    pub async fn start_with_network(&mut self, network: network::OmniaNetwork) {
        if let Some(ref mut gossip) = self.gossip {
            gossip.start_with_network(network).await;
        }
        self.run().await;
    }

    pub async fn submit_event(&mut self, mut event: Event) -> Result<()> {
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

    pub async fn graph(&self) -> tokio::sync::RwLockReadGuard<CausalGraph> {
        self.graph.read().await
    }

    pub fn consensus_stats(&self) -> consensus::ConsensusStats {
        self.consensus.stats()
    }

    pub fn gossip_stats(&self) -> Option<&GossipStats> {
        self.gossip.as_ref().map(|g| g.stats())
    }

    pub fn is_finalized(&self, event_id: &EventId) -> bool {
        self.consensus.is_committed(event_id)
    }

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
                if let Some(event) = graph.get(&event_id) {
                    if !event.payload.is_empty() {
                        if let Err(e) = processor.process_event(event) {
                            tracing::warn!("Shard processor error: {}", e);
                        }
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
            if let Some(event) = graph.get(id) {
                if let Ok(committed) = self.consensus.process_event(event, &graph) {
                    all_committed.extend(committed);
                }
            }
        }

        all_committed
    }
}

#[derive(Debug, Clone)]
pub struct SubstrateStats {
    pub graph: GraphStats,
    pub consensus: consensus::ConsensusStats,
    pub running: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::generate_keypair;
    use crate::event::Event;
    use crate::vector_clock::VectorClock;

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
        assert_eq!(PROTOCOL_VERSION, 1);
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
