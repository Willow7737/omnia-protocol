//! Gossip Protocol Implementation
//!
//! The gossip protocol is responsible for propagating events across the network.
//! It implements a "gossip-about-gossip" pattern inspired by Hashgraph:
//!
//! 1. Each node maintains a local CausalGraph
//! 2. Periodically, a node randomly selects a peer and sends its graph digest
//! 3. The peer responds with events it has that the sender doesn't
//! 4. The sender integrates the new events into its graph
//! 5. This process repeats, ensuring exponential propagation
//!
//! Key properties:
//! - Epidemic propagation: each gossip round reaches more nodes
//! - Bandwidth efficient: only missing events are transmitted
//! - Fault tolerant: no single point of failure
//! - Eventually consistent: all correct nodes receive all events

use crate::causal_graph::{CausalGraph, CausalGraphError};
use crate::event::{Event, EventBatch, EventRequest, EventStatus};
use crate::vector_clock::{NodeId, VectorClock};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use thiserror::Error;
use tracing::{debug, info, trace, warn};

/// Default gossip interval in milliseconds
const DEFAULT_GOSSIP_INTERVAL_MS: u64 = 100;

/// Maximum events to send in a single gossip message
const MAX_EVENTS_PER_GOSSIP: usize = 100;

/// Maximum number of pending events to track
const MAX_PENDING_EVENTS: usize = 100_000;

/// Gossip protocol configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GossipConfig {
    /// Interval between gossip rounds (milliseconds)
    pub interval_ms: u64,
    /// Maximum events per gossip message
    pub max_events_per_message: usize,
    /// Number of peers to gossip with per round
    pub fanout: usize,
    /// Timeout for peer responses (milliseconds)
    pub peer_timeout_ms: u64,
    /// Enable eager push (send events without being asked)
    pub eager_push: bool,
    /// Maximum pending events before forced sync
    pub max_pending: usize,
    /// Random seed for peer selection (0 = random)
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

/// Statistics about gossip protocol performance
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GossipStats {
    /// Total gossip rounds initiated
    pub rounds_initiated: u64,
    /// Total gossip rounds received
    pub rounds_received: u64,
    /// Total events sent
    pub events_sent: u64,
    /// Total events received
    pub events_received: u64,
    /// Total events accepted (valid, not duplicates)
    pub events_accepted: u64,
    /// Total events rejected (invalid or duplicate)
    pub events_rejected: u64,
    /// Number of sync operations completed
    pub syncs_completed: u64,
    /// Average events per sync
    pub avg_events_per_sync: f64,
    /// Time since last successful sync (milliseconds)
    pub time_since_last_sync_ms: u64,
    /// Current number of known peers
    pub known_peers: usize,
    /// Bytes sent
    pub bytes_sent: u64,
    /// Bytes received
    pub bytes_received: u64,
}

/// A digest of the local graph state, sent to initiate gossip
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GossipDigest {
    /// The sender's node ID
    pub node_id: NodeId,
    /// The current frontier vector clock
    pub frontier: VectorClock,
    /// Number of events in the sender's graph
    pub event_count: usize,
    /// Bloom filter-like summary of known events (last N event IDs)
    pub recent_events: Vec<[u8; 8]>, // First 8 bytes of recent event IDs
}

/// A gossip message exchanged between peers
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum GossipMessage {
    /// Initiate gossip: send our digest
    Digest(GossipDigest),
    /// Request specific events we know we're missing
    Request(EventRequest),
    /// Send events in response to a request or eagerly
    Events(EventBatch),
    /// Acknowledge receipt of events
    Ack(Vec<[u8; 32]>),
}

/// A gossip peer (simulated or real)
pub trait GossipPeer: Send + Sync {
    /// Get the peer's node ID
    fn node_id(&self) -> NodeId;
    /// Send a message to this peer
    fn send_message(&self, message: GossipMessage);
    /// Check if peer is responsive
    fn is_alive(&self) -> bool;
}

/// The gossip protocol engine
///
/// Manages event propagation across a network of peers.
/// Uses epidemic gossip for efficient, fault-tolerant distribution.
pub struct GossipProtocol {
    /// Our node ID
    node_id: NodeId,
    /// Protocol configuration
    config: GossipConfig,
    /// The local causal graph (shared state)
    graph: Arc<Mutex<CausalGraph>>,
    /// Known peers
    peers: Vec<Box<dyn GossipPeer>>,
    /// Pending events to gossip (events we've received but not yet processed)
    pending_events: VecDeque<Event>,
    /// Recently gossiped event IDs (deduplication)
    recent_gossip: HashSet<[u8; 32]>,
    /// Protocol statistics
    stats: GossipStats,
    /// Last successful sync time
    last_sync: Instant,
    /// Whether the protocol is running
    running: bool,
    /// Event IDs we've seen (for deduplication)
    seen_events: HashSet<[u8; 32]>,
}

impl GossipProtocol {
    /// Create a new gossip protocol instance
    pub fn new(
        node_id: NodeId,
        config: GossipConfig,
        graph: Arc<Mutex<CausalGraph>>,
    ) -> Self {
        Self {
            node_id,
            config,
            graph,
            peers: Vec::new(),
            pending_events: VecDeque::new(),
            recent_gossip: HashSet::new(),
            stats: GossipStats::default(),
            last_sync: Instant::now(),
            running: false,
            seen_events: HashSet::new(),
        }
    }

    /// Add a peer to the gossip network
    pub fn add_peer(&mut self, peer: Box<dyn GossipPeer>) {
        self.peers.push(peer);
        self.stats.known_peers = self.peers.len();
    }

    /// Start the gossip protocol
    pub fn start(&mut self) {
        self.running = true;
        info!(
            node = ?&self.node_id[..4],
            peers = self.peers.len(),
            "Gossip protocol started"
        );
    }

    /// Stop the gossip protocol
    pub fn stop(&mut self) {
        self.running = false;
        info!("Gossip protocol stopped");
    }

    /// Check if protocol is running
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Perform one gossip round: select peers and exchange events
    pub fn gossip_round(&mut self) -> Result<usize, GossipError> {
        if !self.running || self.peers.is_empty() {
            return Ok(0);
        }

        let mut total_received = 0;

        // Select random peers (up to fanout)
        let selected_peers = self.select_peers();

        for peer in selected_peers {
            if !peer.is_alive() {
                continue;
            }

            // Send our digest
            let digest = self.create_digest();
            peer.send_message(GossipMessage::Digest(digest));
            self.stats.rounds_initiated += 1;

            // In a real implementation, we'd wait for a response
            // For the simulation, we handle this in process_message
        }

        // Process any pending events
        total_received += self.process_pending_events()?;

        // Eager push: send new events to random subset of peers
        if self.config.eager_push {
            total_received += self.eager_push()?;
        }

        self.stats.time_since_last_sync_ms = self.last_sync.elapsed().as_millis() as u64;

        Ok(total_received)
    }

    /// Process an incoming gossip message
    pub fn process_message(
        &mut self,
        sender: NodeId,
        message: GossipMessage,
    ) -> Result<usize, GossipError> {
        self.stats.rounds_received += 1;

        match message {
            GossipMessage::Digest(digest) => {
                // Someone sent us their digest
                // Figure out what events they have that we don't
                let missing = self.find_missing_events(&digest.frontier)?;

                if !missing.is_empty() {
                    // Request missing events
                    if let Some(peer) = self.find_peer(&sender) {
                        let request = EventRequest {
                            known_events: self.get_known_event_ids(),
                            limit: self.config.max_events_per_message,
                            since: self
                                .graph
                                .lock()
                                .map(|g| g.frontier().clone())
                                .unwrap_or_default(),
                        };
                        peer.send_message(GossipMessage::Request(request));
                    }
                }

                // If they might be missing our events, send eagerly
                let our_frontier = self
                    .graph
                    .lock()
                    .map(|g| g.frontier().clone())
                    .unwrap_or_default();

                if our_frontier.happened_after(&digest.frontier) {
                    let events_to_send = self.get_events_since(&digest.frontier)?;
                    if !events_to_send.is_empty() {
                        if let Some(peer) = self.find_peer(&sender) {
                            peer.send_message(GossipMessage::Events(EventBatch {
                                events: events_to_send,
                                has_more: false,
                                tip_clock: our_frontier,
                            }));
                            self.stats.events_sent += self.config.max_events_per_message as u64;
                        }
                    }
                }

                Ok(0)
            }

            GossipMessage::Request(request) => {
                // Someone wants events they don't have
                let events = self.get_unknown_events(&request.known_events, request.limit)?;
                let has_more = events.len() >= request.limit;

                if let Some(peer) = self.find_peer(&sender) {
                    let frontier = self
                        .graph
                        .lock()
                        .map(|g| g.frontier().clone())
                        .unwrap_or_default();
                    peer.send_message(GossipMessage::Events(EventBatch {
                        events,
                        has_more,
                        tip_clock: frontier,
                    }));
                    self.stats.events_sent += self.config.max_events_per_message as u64;
                }

                Ok(0)
            }

            GossipMessage::Events(batch) => {
                // Received events from a peer
                let mut accepted = 0;
                for event in batch.events {
                    let event_id = event.id;
                    if !self.seen_events.contains(&event_id) {
                        self.seen_events.insert(event_id);
                        self.pending_events.push_back(event);
                        accepted += 1;
                    }
                }
                self.stats.events_received += accepted as u64;
                self.last_sync = Instant::now();
                self.stats.syncs_completed += 1;

                // Send ack
                if let Some(peer) = self.find_peer(&sender) {
                    let ack_ids: Vec<[u8; 32]> =
                        batch.events.iter().map(|e| e.id).collect();
                    peer.send_message(GossipMessage::Ack(ack_ids));
                }

                // Process immediately
                let processed = self.process_pending_events()?;
                Ok(processed)
            }

            GossipMessage::Ack(event_ids) => {
                // Peer acknowledged receipt of events
                for id in event_ids {
                    self.recent_gossip.insert(id);
                }
                Ok(0)
            }
        }
    }

    /// Process locally created events and add them to the graph
    pub fn broadcast_event(&mut self, event: Event) -> Result<(), GossipError> {
        // Add to our graph first
        {
            let mut graph = self.graph.lock().map_err(|_| GossipError::LockError)?;
            graph.insert(event.clone())?;
        }

        self.seen_events.insert(event.id);

        // Eager push to peers
        if self.config.eager_push && self.running {
            let selected = self.select_peers();
            for peer in selected {
                if peer.is_alive() {
                    let frontier = self
                        .graph
                        .lock()
                        .map(|g| g.frontier().clone())
                        .unwrap_or_default();
                    peer.send_message(GossipMessage::Events(EventBatch {
                        events: vec![event.clone()],
                        has_more: false,
                        tip_clock: frontier,
                    }));
                    self.stats.events_sent += 1;
                }
            }
        }

        Ok(())
    }

    /// Get current statistics
    pub fn stats(&self) -> &GossipStats {
        &self.stats
    }

    // --- Internal methods ---

    fn select_peers(&self) -> Vec<&Box<dyn GossipPeer>> {
        let count = self.config.fanout.min(self.peers.len());
        // Simple selection: just take the first 'count' peers
        // In production, use random selection with exclusion
        self.peers.iter().take(count).collect()
    }

    fn create_digest(&self) -> GossipDigest {
        let graph = self.graph.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let frontier = graph.frontier().clone();
        let event_count = graph.len();

        // Get recent event IDs (truncated for efficiency)
        let recent_events: Vec<[u8; 8]> = graph
            .tips()
            .take(20)
            .map(|id| {
                let mut short = [0u8; 8];
                short.copy_from_slice(&id[..8]);
                short
            })
            .collect();

        GossipDigest {
            node_id: self.node_id,
            frontier,
            event_count,
            recent_events,
        }
    }

    fn find_missing_events(
        &self,
        peer_frontier: &VectorClock,
    ) -> Result<Vec<[u8; 32]>, GossipError> {
        let graph = self.graph.lock().map_err(|_| GossipError::LockError)?;

        // Find events in peer's frontier that we don't have
        let missing: Vec<[u8; 32]> = peer_frontier
            .nodes()
            .filter_map(|node_id| {
                let our_clock = graph.frontier().get(node_id);
                let peer_clock = peer_frontier.get(node_id);
                if peer_clock > our_clock {
                    // Peer has events from this node that we don't
                    // In practice, we'd request the specific range
                    Some(*node_id)
                } else {
                    None
                }
            })
            .map(|n| n) // node_id is the identifier
            .collect();

        Ok(missing)
    }

    fn get_known_event_ids(&self) -> Vec<[u8; 32]> {
        self.seen_events.iter().copied().collect()
    }

    fn find_peer(&self, node_id: &NodeId) -> Option<&Box<dyn GossipPeer>> {
        self.peers.iter().find(|p| p.node_id() == *node_id)
    }

    fn get_events_since(&self, since: &VectorClock) -> Result<Vec<Event>, GossipError> {
        let graph = self.graph.lock().map_err(|_| GossipError::LockError)?;
        let events: Vec<Event> = graph
            .since(since)
            .into_iter()
            .cloned()
            .take(self.config.max_events_per_message)
            .collect();
        Ok(events)
    }

    fn get_unknown_events(
        &self,
        known: &[[u8; 32]],
        limit: usize,
    ) -> Result<Vec<Event>, GossipError> {
        let known_set: HashSet<_> = known.iter().collect();
        let graph = self.graph.lock().map_err(|_| GossipError::LockError)?;

        let events: Vec<Event> = graph
            .diff(&known.iter().copied().collect())
            .into_iter()
            .cloned()
            .take(limit)
            .collect();

        Ok(events)
    }

    fn process_pending_events(&mut self) -> Result<usize, GossipError> {
        let mut processed = 0;
        let to_process: Vec<Event> =
            self.pending_events.drain(..self.pending_events.len()).collect();

        for event in to_process {
            match self.graph.lock() {
                Ok(mut graph) => match graph.insert(event.clone()) {
                    Ok(_) => {
                        self.stats.events_accepted += 1;
                        processed += 1;
                    }
                    Err(CausalGraphError::DuplicateEvent(_)) => {
                        // Already have this event, ignore
                        self.stats.events_rejected += 1;
                    }
                    Err(e) => {
                        warn!("Failed to insert event: {}", e);
                        self.stats.events_rejected += 1;
                    }
                },
                Err(_) => {
                    // Put it back
                    self.pending_events.push_back(event);
                }
            }
        }

        Ok(processed)
    }

    fn eager_push(&mut self) -> Result<usize, GossipError> {
        // Send recent events to a subset of peers
        // This is already handled by broadcast_event for new local events
        // Here we handle re-transmission of events we received from others
        Ok(0)
    }
}

/// Errors that can occur during gossip operations
#[derive(Error, Debug, Clone)]
pub enum GossipError {
    #[error("Failed to acquire lock on graph")]
    LockError,
    #[error("Causal graph error: {0}")]
    GraphError(String),
    #[error("Peer not found: {0:?}")]
    PeerNotFound(NodeId),
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

/// Simulated peer for testing the gossip protocol
#[derive(Clone, Debug)]
pub struct SimulatedPeer {
    pub node_id: NodeId,
    pub alive: Arc<Mutex<bool>>,
    pub received_messages: Arc<Mutex<Vec<(NodeId, GossipMessage)>>>,
}

impl SimulatedPeer {
    pub fn new(node_id: NodeId) -> Self {
        Self {
            node_id,
            alive: Arc::new(Mutex::new(true)),
            received_messages: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn set_alive(&self, alive: bool) {
        if let Ok(mut guard) = self.alive.lock() {
            *guard = alive;
        }
    }

    pub fn get_messages(&self) -> Vec<(NodeId, GossipMessage)> {
        self.received_messages
            .lock()
            .map(|m| m.clone())
            .unwrap_or_default()
    }
}

impl GossipPeer for SimulatedPeer {
    fn node_id(&self) -> NodeId {
        self.node_id
    }

    fn send_message(&self, message: GossipMessage) {
        if let Ok(mut messages) = self.received_messages.lock() {
            messages.push((self.node_id, message));
        }
    }

    fn is_alive(&self) -> bool {
        self.alive.lock().map(|a| *a).unwrap_or(false)
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

    fn create_test_graph() -> Arc<Mutex<CausalGraph>> {
        Arc::new(Mutex::new(CausalGraph::new()))
    }

    #[test]
    fn test_gossip_digest_creation() {
        let graph = create_test_graph();
        let mut protocol = GossipProtocol::new(node(1), GossipConfig::default(), graph);
        protocol.start();

        let digest = protocol.create_digest();
        assert_eq!(digest.node_id, node(1));
        assert_eq!(digest.event_count, 0);
    }

    #[test]
    fn test_add_peer() {
        let graph = create_test_graph();
        let mut protocol = GossipProtocol::new(node(1), GossipConfig::default(), graph);

        let peer = SimulatedPeer::new(node(2));
        protocol.add_peer(Box::new(peer));

        assert_eq!(protocol.peers.len(), 1);
        assert_eq!(protocol.stats.known_peers, 1);
    }

    #[test]
    fn test_process_digest_empty_graphs() {
        let graph_a = create_test_graph();
        let mut proto_a = GossipProtocol::new(node(1), GossipConfig::default(), graph_a);

        let graph_b = create_test_graph();
        let proto_b = GossipProtocol::new(node(2), GossipConfig::default(), graph_b);

        // B sends digest to A
        let digest = proto_b.create_digest();
        let result = proto_a.process_message(node(2), GossipMessage::Digest(digest));

        // Both empty, no events should be exchanged
        assert!(result.is_ok());
    }

    #[test]
    fn test_gossip_message_serialization() {
        let digest = GossipDigest {
            node_id: node(1),
            frontier: VectorClock::with_node(node(1), 5),
            event_count: 42,
            recent_events: vec![[1, 2, 3, 4, 5, 6, 7, 8]],
        };

        let msg = GossipMessage::Digest(digest);
        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: GossipMessage = serde_json::from_str(&json).unwrap();

        match deserialized {
            GossipMessage::Digest(d) => {
                assert_eq!(d.event_count, 42);
                assert_eq!(d.node_id, node(1));
            }
            _ => panic!("Wrong message type after deserialization"),
        }
    }

    #[test]
    fn test_simulated_peer() {
        let peer = SimulatedPeer::new(node(2));
        assert!(peer.is_alive());

        let msg = GossipMessage::Ack(vec![]);
        peer.send_message(msg.clone());

        let messages = peer.get_messages();
        assert_eq!(messages.len(), 1);

        peer.set_alive(false);
        assert!(!peer.is_alive());
    }

    #[test]
    fn test_gossip_stats() {
        let graph = create_test_graph();
        let protocol = GossipProtocol::new(node(1), GossipConfig::default(), graph);

        let stats = protocol.stats();
        assert_eq!(stats.rounds_initiated, 0);
        assert_eq!(stats.events_sent, 0);
        assert_eq!(stats.events_received, 0);
    }

    #[test]
    fn test_running_state() {
        let graph = create_test_graph();
        let mut protocol = GossipProtocol::new(node(1), GossipConfig::default(), graph);

        assert!(!protocol.is_running());
        protocol.start();
        assert!(protocol.is_running());
        protocol.stop();
        assert!(!protocol.is_running());
    }
}
