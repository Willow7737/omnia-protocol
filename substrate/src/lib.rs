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

#![warn(missing_docs)]
#![warn(unused_qualifications)]

pub mod causal_graph;
pub mod consensus;
pub mod crypto;
pub mod crdt;
pub mod event;
pub mod gossip;
pub mod network;
pub mod vector_clock;

// Re-export commonly used types
pub use causal_graph::{CausalGraph, CausalGraphError, GraphSnapshot, GraphStats};
pub use consensus::{ConsensusConfig, ConsensusEngine, ConsensusError, ConsensusState};
pub use crypto::{generate_keypair, NodeKeypair, NodePublicKey};
pub use crdt::{CvRDT, GCounter, LwwRegister, OrSet};
pub use event::{Event, EventBatch, EventHeader, EventId, EventRequest, EventStatus, EventValidationError};
pub use gossip::{GossipConfig, GossipDigest, GossipError, GossipMessage, GossipProtocol, GossipStats};
pub use network::{NetworkEvent, OmniaNetwork, OmniaBehaviour};
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

    pub async fn submit_event(&mut self, mut event: Event) -> Result<()> {
        event.validate().map_err(SubstrateError::from)?;

        {
            let mut graph = self.graph.write().await;
            graph.insert(event.clone()).map_err(SubstrateError::from)?;
        }

        let graph = self.graph.read().await;
        self.consensus.process_event(&event, &graph).map_err(SubstrateError::from)?;
        drop(graph);

        if let Some(ref mut gossip) = self.gossip {
            gossip.broadcast_event(event).await.map_err(SubstrateError::from)?;
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

    pub fn stats(&self) -> SubstrateStats {
        let graph_stats = futures::executor::block_on(async {
            self.graph.read().await.stats()
        });
        let consensus_stats = self.consensus.stats();

        SubstrateStats {
            graph: graph_stats,
            consensus: consensus_stats,
            running: self.running,
        }
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
