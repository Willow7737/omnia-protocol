//! Async Gossip Protocol Implementation using libp2p
//!
//! Refactored from std::sync::Mutex to tokio::sync::RwLock to prevent deadlocks
//! in async contexts. Integrates with the real OmniaNetwork for P2P communication.

use crate::causal_graph::{CausalGraph, CausalGraphError};
use crate::event::{Event, EventBatch, EventRequest};
use crate::network::{NetworkCommand, NetworkEvent, OmniaNetwork};
use crate::vector_clock::{NodeId, VectorClock};
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::time::Instant;
use thiserror::Error;
use tokio::sync::{mpsc, RwLock};
use tracing::{info, warn};

const DEFAULT_GOSSIP_INTERVAL_MS: u64 = 100;
const MAX_EVENTS_PER_GOSSIP: usize = 100;
const MAX_PENDING_EVENTS: usize = 100_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GossipConfig {
    pub interval_ms: u64,
    pub max_events_per_message: usize,
    pub fanout: usize,
    pub peer_timeout_ms: u64,
    pub eager_push: bool,
    pub max_pending: usize,
    pub seed: u64,
}

impl Default for GossipConfig {
    fn default() -> Self {
        Self {
            interval_ms: DEFAULT_GOSSIP_INTERVAL_MS,
            max_events_per_message: MAX_EVENTS_PER_GOSSIP,
            fanout: 3,
            peer_timeout_ms: 5000,
            eager_push: true,
            max_pending: MAX_PENDING_EVENTS,
            seed: 0,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GossipStats {
    pub rounds_initiated: u64,
    pub rounds_received: u64,
    pub events_sent: u64,
    pub events_received: u64,
    pub events_accepted: u64,
    pub events_rejected: u64,
    pub syncs_completed: u64,
    pub avg_events_per_sync: f64,
    pub time_since_last_sync_ms: u64,
    pub known_peers: usize,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GossipDigest {
    pub node_id: NodeId,
    pub frontier: VectorClock,
    pub event_count: usize,
    pub recent_events: Vec<[u8; 8]>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum GossipMessage {
    Digest(GossipDigest),
    Request(EventRequest),
    Events(EventBatch),
    Ack(Vec<[u8; 32]>),
}

/// Async gossip protocol engine.
/// Uses tokio::sync::RwLock — never std::sync::Mutex across await points.
#[allow(dead_code)]
pub struct GossipProtocol {
    node_id: NodeId,
    config: GossipConfig,
    graph: Arc<RwLock<CausalGraph>>,
    // FIX(bug-1): Command channel to publish through the network task.
    // After start_with_network(), OmniaNetwork is owned by the spawned
    // task. broadcast_event() sends Publish commands through this channel.
    network_cmd_tx: Option<mpsc::Sender<NetworkCommand>>,
    pending_events: VecDeque<Event>,
    recent_gossip: HashSet<[u8; 32]>,
    stats: GossipStats,
    last_sync: Instant,
    running: bool,
    seen_events: HashSet<[u8; 32]>,
}

impl GossipProtocol {
    pub fn new(node_id: NodeId, config: GossipConfig, graph: Arc<RwLock<CausalGraph>>) -> Self {
        Self {
            node_id,
            config,
            graph,
            network_cmd_tx: None,
            pending_events: VecDeque::new(),
            recent_gossip: HashSet::new(),
            stats: GossipStats::default(),
            last_sync: Instant::now(),
            running: false,
            seen_events: HashSet::new(),
        }
    }

    /// Attach a real network layer.
    /// The network is consumed by start_with_network() — this method just
    /// flags that a network is available.
    pub fn attach_network(&mut self, _network: OmniaNetwork) {
        // Cannot hold OmniaNetwork because Swarm is !Send.
        // Use start_with_network() instead.
    }

    /// Start with an OmniaNetwork. Moves the network into a spawned task.
    ///
    /// FIX(bug-1): A command channel is created so broadcast_event() can
    /// publish even after the network is moved into the spawned task.
    ///
    /// FIX(bug-2): The event_rx is consumed by run_with_commands() so the
    /// mpsc channel never fills up and blocks.
    pub async fn start_with_network(&mut self, mut network: OmniaNetwork) {
        self.running = true;

        // Take the event_rx out of the network — it will be consumed by
        // run_with_commands() inside the spawned task.
        let event_rx = network
            .event_rx
            .take()
            .expect("event_rx should be present after OmniaNetwork::new()");

        // Command channel for broadcast_event() → network task
        let (cmd_tx, cmd_rx) = mpsc::channel::<NetworkCommand>(256);
        self.network_cmd_tx = Some(cmd_tx);

        // Clone the graph Arc for the spawned task
        let graph = Arc::clone(&self.graph);

        // Spawn the network event loop. The task owns OmniaNetwork.
        // It processes: swarm events, event_rx (drains channel), cmd_rx (publish).
        tokio::spawn(async move {
            network.run_with_commands(cmd_rx, event_rx, graph).await;
        });

        info!(
            node = ?&self.node_id[..4],
            "Gossip protocol started with network"
        );
    }

    /// Start the gossip protocol without a network (local-only).
    pub async fn start(&mut self) {
        self.running = true;
        info!(
            node = ?&self.node_id[..4],
            "Gossip protocol started"
        );
    }

    pub fn stop(&mut self) {
        self.running = false;
        self.network_cmd_tx = None;
        info!("Gossip protocol stopped");
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Broadcast a locally created event to the network.
    /// FIX(bug-1): Sends a Publish command through the channel. The
    /// network task (running in a spawned task) picks it up and calls
    /// OmniaNetwork::publish(). This works even after start() because
    /// the command channel is independent of the Swarm ownership.
    pub async fn broadcast_event(&mut self, event: Event) -> Result<(), GossipError> {
        // Add to our graph first
        {
            let mut graph = self.graph.write().await;
            graph
                .insert(event.clone())
                .map_err(|e| GossipError::GraphError(e.to_string()))?;
        }

        // Send publish command to the network task
        let bytes = event.to_bytes();
        let bytes_len = bytes.len();
        if let Some(ref cmd_tx) = self.network_cmd_tx {
            cmd_tx
                .send(NetworkCommand::Publish {
                    topic: "omnia_events".to_string(),
                    data: bytes,
                })
                .await
                .map_err(|e| GossipError::NetworkError(e.to_string()))?;
        }

        self.stats.events_sent += 1;
        self.stats.bytes_sent += bytes_len as u64;

        Ok(())
    }

    /// Process an incoming network event.
    pub async fn handle_network_event(
        &mut self,
        event: NetworkEvent,
    ) -> Result<usize, GossipError> {
        match event {
            NetworkEvent::GossipReceived { data, .. } => {
                match Event::from_bytes(&data) {
                    Ok(event) => {
                        if !self.seen_events.contains(&event.id) {
                            self.seen_events.insert(event.id);
                            self.pending_events.push_back(event);
                        }
                    }
                    Err(e) => {
                        warn!("Failed to deserialize gossip event: {:?}", e);
                        self.stats.events_rejected += 1;
                    }
                }
                self.process_pending_events().await
            }
            _ => Ok(0),
        }
    }

    pub fn stats(&self) -> &GossipStats {
        &self.stats
    }

    async fn process_pending_events(&mut self) -> Result<usize, GossipError> {
        let mut processed = 0;
        let to_process: Vec<Event> = self
            .pending_events
            .drain(..self.pending_events.len())
            .collect();

        for event in to_process {
            let mut graph = self.graph.write().await;
            match graph.insert(event.clone()) {
                Ok(_) => {
                    self.stats.events_accepted += 1;
                    processed += 1;
                }
                Err(CausalGraphError::DuplicateEvent(_)) => {
                    self.stats.events_rejected += 1;
                }
                Err(e) => {
                    warn!("Failed to insert event: {}", e);
                    self.stats.events_rejected += 1;
                }
            }
        }

        Ok(processed)
    }
}

#[derive(Error, Debug, Clone)]
pub enum GossipError {
    #[error("Graph error: {0}")]
    GraphError(String),
    #[error("Network error: {0}")]
    NetworkError(String),
    #[error("Serialization error: {0}")]
    SerializationError(String),
    #[error("Protocol not running")]
    NotRunning,
}

impl From<CausalGraphError> for GossipError {
    fn from(e: CausalGraphError) -> Self {
        GossipError::GraphError(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;
    use crate::vector_clock::VectorClock;

    fn node(id: u8) -> NodeId {
        let mut n = [0u8; 32];
        n[0] = id;
        n
    }

    #[tokio::test]
    async fn test_gossip_creation() {
        let graph = Arc::new(RwLock::new(CausalGraph::new()));
        let protocol = GossipProtocol::new(node(1), GossipConfig::default(), graph);
        assert!(!protocol.is_running());
    }

    #[tokio::test]
    async fn test_gossip_start_stop() {
        let graph = Arc::new(RwLock::new(CausalGraph::new()));
        let mut protocol = GossipProtocol::new(node(1), GossipConfig::default(), graph);

        protocol.start().await;
        assert!(protocol.is_running());
        protocol.stop();
        assert!(!protocol.is_running());
    }

    #[tokio::test]
    async fn test_gossip_stats() {
        let graph = Arc::new(RwLock::new(CausalGraph::new()));
        let protocol = GossipProtocol::new(node(1), GossipConfig::default(), graph);

        let stats = protocol.stats();
        assert_eq!(stats.rounds_initiated, 0);
        assert_eq!(stats.events_sent, 0);
        assert_eq!(stats.events_received, 0);
    }
}
