//! Async Gossip Protocol Implementation using libp2p
//!
//! Refactored from std::sync::Mutex to tokio::sync::RwLock to prevent deadlocks
//! in async contexts. Integrates with the real OmniaNetwork for P2P communication.

use crate::causal_graph::{CausalGraph, CausalGraphError};
use crate::event::{Event, EventBatch, EventId, EventRequest};
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
    // Command channel to publish through the network task.
    network_cmd_tx: Option<mpsc::Sender<NetworkCommand>>,
    // Network event receiver — drained into pending_events by
    // process_pending_events() so that p2p events flow through consensus.
    pub network_rx: Option<mpsc::Receiver<NetworkEvent>>,
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
            network_rx: None,
            pending_events: VecDeque::new(),
            recent_gossip: HashSet::new(),
            stats: GossipStats::default(),
            last_sync: Instant::now(),
            running: false,
            seen_events: HashSet::new(),
        }
    }

    /// Deprecated — use start_with_network() instead.
    /// Cannot hold OmniaNetwork because Swarm is !Send.
    pub fn attach_network(&mut self, _network: OmniaNetwork) {
        // No-op. Use start_with_network() which moves the network into
        // a spawned task and wires event_rx into process_pending_events().
    }

    /// Start with an OmniaNetwork. Moves the network into a spawned task.
    ///
    /// The event_rx is stored in self.network_rx so that
    /// process_pending_events() can drain network events into the
    /// pending queue and process them through the graph + consensus.
    pub async fn start_with_network(&mut self, mut network: OmniaNetwork) {
        self.running = true;

        // Take the event_rx out of the network — we consume it in
        // process_pending_events() instead of letting the network task
        // insert into the graph directly.
        let event_rx = network
            .event_rx
            .take()
            .expect("event_rx should be present after OmniaNetwork::new()");
        self.network_rx = Some(event_rx);

        // Command channel for broadcast_event() → network task
        let (cmd_tx, cmd_rx) = mpsc::channel::<NetworkCommand>(256);
        self.network_cmd_tx = Some(cmd_tx);

        // Spawn the network event loop. The task owns OmniaNetwork.
        // It only processes swarm events + command channel (publish/subscribe).
        // Event consumption is handled by GossipProtocol.
        tokio::spawn(async move {
            network.run_with_commands(cmd_rx).await;
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

    pub fn stats(&self) -> &GossipStats {
        &self.stats
    }

    /// Read access to the underlying graph (for testing/inspection).
    pub fn graph(&self) -> &Arc<RwLock<CausalGraph>> {
        &self.graph
    }

    /// Process pending events: first drain network_rx into the pending queue,
    /// then insert all pending events into the graph.
    ///
    /// This is the bridge between p2p network events and consensus —
    /// network events land in the graph where Substrate::process_consensus()
    /// can pick them up.
    pub async fn process_pending_events(&mut self) -> Result<Vec<EventId>, GossipError> {
        let mut inserted_ids = Vec::new();

        // Drain network events into pending queue
        if let Some(ref mut rx) = self.network_rx {
            loop {
                match rx.try_recv() {
                    Ok(NetworkEvent::GossipReceived { data, .. }) => {
                        match Event::from_bytes(&data) {
                            Ok(event) => {
                                if !self.seen_events.contains(&event.id) {
                                    self.seen_events.insert(event.id);
                                    self.pending_events.push_back(event);
                                    self.stats.events_received += 1;
                                }
                            }
                            Err(e) => {
                                warn!("Failed to deserialize gossip event: {:?}", e);
                                self.stats.events_rejected += 1;
                            }
                        }
                    }
                    Ok(NetworkEvent::PeerConnected(peer_id)) => {
                        info!("Peer connected: {:?}", peer_id);
                    }
                    Ok(NetworkEvent::PeerDisconnected(peer_id)) => {
                        info!("Peer disconnected: {:?}", peer_id);
                    }
                    Ok(_) => {}
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        self.network_rx = None;
                        break;
                    }
                }
            }
        }

        // Process all pending events (both locally created and network received)
        let to_process: Vec<Event> = self.pending_events.drain(..).collect();

        for event in to_process {
            let mut graph = self.graph.write().await;
            match graph.insert(event.clone()) {
                Ok(_) => {
                    self.stats.events_accepted += 1;
                    inserted_ids.push(event.id);
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

        Ok(inserted_ids)
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
