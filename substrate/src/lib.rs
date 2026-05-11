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
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                    Application Layer                     │
//! ├─────────────────────────────────────────────────────────┤
//! │  Consensus  │  Gossip Protocol  │  CRDT State Manager   │
//! ├─────────────────────────────────────────────────────────┤
//! │              Causal Graph (DAG)                         │
//! ├─────────────────────────────────────────────────────────┤
//! │  Event    │  Vector Clock    │  Cryptographic Identity  │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage
//!
//! ```rust,no_run
//! use omnia_substrate::*;
//!
//! // Create a node identity
//! let node_id = [0u8; 32]; // In production, use proper key derivation
//!
//! // Initialize the causal graph
//! let mut graph = CausalGraph::new();
//!
//! // Create a genesis event
//! let genesis = Event::genesis(node_id, vec![/* payload */]);
//! graph.insert(genesis).unwrap();
//!
//! // Events are automatically ordered by vector clocks
//! // Concurrent events can be processed in parallel
//! ```
//!
//! ## Design Principles
//!
//! 1. **No global clock**: All ordering is based on causality
//! 2. **Parallel by default**: Causally independent events execute concurrently
//! 3. **CRDT convergence**: State merges deterministically without coordination
//! 4. **Modular finality**: Pluggable consensus for different security/performance needs

#![warn(missing_docs)]
#![warn(unused_qualifications)]

pub mod causal_graph;
pub mod consensus;
pub mod crdt;
pub mod event;
pub mod gossip;
pub mod vector_clock;

// Re-export commonly used types
pub use causal_graph::{CausalGraph, CausalGraphError, GraphSnapshot, GraphStats};
pub use consensus::{ConsensusConfig, ConsensusEngine, ConsensusError, ConsensusState};
pub use crdt::{CvRDT, GCounter, LwwRegister, OrSet};
pub use event::{Event, EventBatch, EventHeader, EventId, EventRequest, EventStatus, EventValidationError};
pub use gossip::{GossipConfig, GossipDigest, GossipError, GossipMessage, GossipProtocol, GossipStats};
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
    /// Node identity
    pub node_id: NodeId,
    /// Gossip protocol configuration
    pub gossip: GossipConfig,
    /// Consensus engine configuration
    pub consensus: ConsensusConfig,
    /// Total nodes in the network
    pub total_nodes: usize,
}

impl SubstrateConfig {
    /// Create a default configuration for a node
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

    /// Create a configuration for a specific network size
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
///
/// This is the primary entry point for using Layer 1. It manages:
/// - The causal graph (event storage)
/// - Gossip protocol (event propagation)
/// - Consensus engine (finality determination)
pub struct Substrate {
    /// Configuration
    config: SubstrateConfig,
    /// The causal graph storing all events
    graph: std::sync::Arc<std::sync::Mutex<CausalGraph>>,
    /// Gossip protocol for event propagation
    gossip: Option<GossipProtocol>,
    /// Consensus engine for finality
    consensus: ConsensusEngine,
    /// Whether the substrate is running
    running: bool,
}

impl Substrate {
    /// Create a new Substrate instance
    pub fn new(config: SubstrateConfig) -> Self {
        let graph = std::sync::Arc::new(std::sync::Mutex::new(CausalGraph::new()));
        let consensus = ConsensusEngine::new(config.consensus.clone());

        Self {
            config,
            graph,
            gossip: None,
            consensus,
            running: false,
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

    /// Start the substrate
    pub fn start(&mut self) {
        self.running = true;
        if let Some(ref mut gossip) = self.gossip {
            gossip.start();
        }
    }

    /// Stop the substrate
    pub fn stop(&mut self) {
        self.running = false;
        if let Some(ref mut gossip) = self.gossip {
            gossip.stop();
        }
    }

    /// Submit a new event to the substrate
    ///
    /// The event will be:
    /// 1. Validated
    /// 2. Added to the local causal graph
    /// 3. Processed through consensus
    /// 4. Gossiped to peers
    pub fn submit_event(&mut self, mut event: Event) -> Result<()> {
        // Validate
        event.validate().map_err(SubstrateError::from)?;

        // Add to graph
        {
            let mut graph = self.graph.lock().map_err(|e| {
                SubstrateError::Config(format!("Lock poisoned: {}", e))
            })?;
            graph.insert(event.clone()).map_err(SubstrateError::from)?;
        }

        // Process through consensus
        let graph = self.graph.lock().map_err(|e| {
            SubstrateError::Config(format!("Lock poisoned: {}", e))
        })?;
        self.consensus.process_event(&event, &graph).map_err(SubstrateError::from)?;
        drop(graph);

        // Gossip to peers
        if let Some(ref mut gossip) = self.gossip {
            gossip.broadcast_event(event).map_err(SubstrateError::from)?;
        }

        Ok(())
    }

    /// Get the causal graph (read access)
    pub fn graph(&self) -> std::sync::LockResult<std::sync::MutexGuard<CausalGraph>> {
        self.graph.lock()
    }

    /// Get consensus statistics
    pub fn consensus_stats(&self) -> consensus::ConsensusStats {
        self.consensus.stats()
    }

    /// Get gossip statistics (if gossip is enabled)
    pub fn gossip_stats(&self) -> Option<&GossipStats> {
        self.gossip.as_ref().map(|g| g.stats())
    }

    /// Check if an event is finalized
    pub fn is_finalized(&self, event_id: &EventId) -> bool {
        self.consensus.is_committed(event_id)
    }

    /// Get all finalized events
    pub fn finalized_events(&self) -> Vec<EventId> {
        self.consensus.get_committed()
    }

    /// Get substrate statistics
    pub fn stats(&self) -> SubstrateStats {
        let graph_stats = self.graph.lock().map(|g| g.stats()).unwrap_or_default();
        let consensus_stats = self.consensus.stats();

        SubstrateStats {
            graph: graph_stats,
            consensus: consensus_stats,
            running: self.running,
        }
    }
}

/// Statistics for the entire substrate
#[derive(Debug, Clone)]
pub struct SubstrateStats {
    /// Causal graph statistics
    pub graph: GraphStats,
    /// Consensus statistics
    pub consensus: consensus::ConsensusStats,
    /// Whether the substrate is running
    pub running: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn test_substrate_start_stop() {
        let config = SubstrateConfig::new(test_node(1));
        let mut substrate = Substrate::new(config);

        substrate.init_gossip();
        assert!(substrate.gossip.is_some());

        substrate.start();
        assert!(substrate.running);
        assert!(substrate.gossip.as_ref().unwrap().is_running());

        substrate.stop();
        assert!(!substrate.running);
    }

    #[test]
    fn test_submit_event() {
        let config = SubstrateConfig::new(test_node(1));
        let mut substrate = Substrate::new(config);

        let mut event = Event::genesis(test_node(1), vec![1, 2, 3]);
        event.sign(vec![1, 2, 3]);

        substrate.submit_event(event).unwrap();

        let graph = substrate.graph().unwrap();
        assert_eq!(graph.len(), 1);
    }

    #[test]
    fn test_substrate_stats() {
        let config = SubstrateConfig::new(test_node(1));
        let substrate = Substrate::new(config);

        let stats = substrate.stats();
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
