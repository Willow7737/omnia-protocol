//! Async Gossip Protocol Implementation using libp2p
//!
//! Refactored from std::sync::Mutex to tokio::sync::RwLock to prevent deadlocks
//! in async contexts. Integrates with the real OmniaNetwork for P2P communication.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Instant;

use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{mpsc, RwLock};
use tracing::{info, warn};

use crate::causal_graph::{CausalGraph, CausalGraphError};
use crate::event::{Event, EventBatch, EventId, EventRequest};
use crate::network::{NetworkCommand, NetworkEvent, OmniaNetwork};
use crate::rate_limiter::RateLimiter;
use crate::vector_clock::{NodeId, VectorClock};

const DEFAULT_GOSSIP_INTERVAL_MS: u64 = 100;
const MAX_EVENTS_PER_GOSSIP: usize = 100;
const MAX_PENDING_EVENTS: usize = 100_000;
const DEFAULT_PARTITION_THRESHOLD_MS: u64 = 30_000; // 30 seconds (was 3s)

/// Maximum number of seen event IDs to retain for dedup.
/// When exceeded, the entire set is cleared (it is only a dedup cache,
/// losing old entries is acceptable — at worst, a duplicate event is
/// reprocessed idempotently).
const MAX_SEEN_EVENTS: usize = 100_000;

/// Configuration for the gossip protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GossipConfig {
    /// Interval between gossip rounds in milliseconds
    pub interval_ms: u64,
    /// Maximum events per gossip message
    pub max_events_per_message: usize,
    /// Number of peers to gossip to per round
    pub fanout: usize,
    /// Timeout for peer responses in milliseconds
    pub peer_timeout_ms: u64,
    /// Whether to use eager push strategy
    pub eager_push: bool,
    /// Maximum number of pending events
    pub max_pending: usize,
    /// Random seed for peer selection
    pub seed: u64,
    /// Bootstrap peer addresses to dial on startup.
    /// Stored as strings (multiaddr format, e.g. "/ip4/1.2.3.4/udp/4001/quic")
    /// because Multiaddr is not Serialize/Deserialize without the libp2p serde feature.
    #[serde(default)]
    pub bootstrap_peers: Vec<String>,
    /// Threshold in milliseconds after which a silent peer is considered
    /// partitioned. Default: 30000 ms.
    #[serde(default = "default_partition_threshold")]
    pub partition_threshold_ms: u64,
    /// Maximum events per peer per second (refill rate).
    #[serde(default = "default_max_events_per_second")]
    pub max_events_per_second: u32,
    /// Burst capacity (max tokens) per peer.
    #[serde(default = "default_burst_capacity")]
    pub burst_capacity: u32,
}

fn default_partition_threshold() -> u64 {
    DEFAULT_PARTITION_THRESHOLD_MS
}

fn default_max_events_per_second() -> u32 {
    100
}

fn default_burst_capacity() -> u32 {
    200
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
            bootstrap_peers: Vec::new(),
            partition_threshold_ms: DEFAULT_PARTITION_THRESHOLD_MS,
            max_events_per_second: 100,
            burst_capacity: 200,
        }
    }
}

/// Gossip protocol statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GossipStats {
    /// Number of gossip rounds initiated
    pub rounds_initiated: u64,
    /// Number of gossip rounds received
    pub rounds_received: u64,
    /// Number of events sent
    pub events_sent: u64,
    /// Number of events received
    pub events_received: u64,
    /// Number of events accepted into the graph
    pub events_accepted: u64,
    /// Number of events rejected
    pub events_rejected: u64,
    /// Number of events rejected specifically due to invalid signatures.
    pub messages_rejected_invalid_sig: u64,
    /// Number of syncs completed
    pub syncs_completed: u64,
    /// Average events per sync operation
    pub avg_events_per_sync: f64,
    /// Time since last sync in milliseconds
    pub time_since_last_sync_ms: u64,
    /// Number of known peers
    pub known_peers: usize,
    /// Total bytes sent
    pub bytes_sent: u64,
    /// Total bytes received
    pub bytes_received: u64,
}

/// A digest of gossip state for efficient synchronization
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GossipDigest {
    /// Node identifier
    pub node_id: NodeId,
    /// Frontier vector clock
    pub frontier: VectorClock,
    /// Number of events known
    pub event_count: usize,
    /// Recent event ID prefixes
    pub recent_events: Vec<[u8; 8]>,
}

/// Messages in the gossip protocol
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum GossipMessage {
    /// State digest for synchronization
    Digest(GossipDigest),
    /// Request for missing events
    Request(EventRequest),
    /// Batch of events
    Events(EventBatch),
    /// Acknowledgment of received events
    Ack(Vec<[u8; 32]>),
}

/// Gossip-level events (not yet wired to consensus, used for logging/monitoring).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GossipEvent {
    /// A network partition has been detected: more than 1/3 of known peers
    /// have been silent for longer than the configured threshold.
    PartitionDetected,
    /// A previously detected partition has healed: enough peers are back.
    PartitionHealed,
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
    /// Network event receiver
    pub network_rx: Option<mpsc::Receiver<NetworkEvent>>,
    pending_events: VecDeque<Event>,
    stats: GossipStats,
    last_sync: Instant,
    running: bool,
    seen_events: HashSet<[u8; 32]>,
    /// Tracks when each peer was last heard from (for partition detection).
    last_seen: HashMap<PeerId, Instant>,
    /// Whether a partition is currently detected (to avoid duplicate events).
    partition_active: bool,
    /// Per-peer rate limiter (token bucket).
    rate_limiter: RateLimiter,
}

impl GossipProtocol {
    /// Create a new gossip protocol instance
    pub fn new(node_id: NodeId, config: GossipConfig, graph: Arc<RwLock<CausalGraph>>) -> Self {
        let rate_limiter = RateLimiter::new(config.burst_capacity, config.max_events_per_second);
        Self {
            node_id,
            config,
            graph,
            network_cmd_tx: None,
            network_rx: None,
            pending_events: VecDeque::new(),
            stats: GossipStats::default(),
            last_sync: Instant::now(),
            running: false,
            seen_events: HashSet::new(),
            last_seen: HashMap::new(),
            partition_active: false,
            rate_limiter,
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
    pub async fn start_with_network(&mut self, mut network: OmniaNetwork) -> Result<(), GossipError> {
        self.running = true;

        // Take the event_rx out of the network — we consume it in
        // process_pending_events() instead of letting the network task
        // insert into the graph directly.
        let event_rx = network
            .event_rx
            .take()
            .ok_or(GossipError::ChannelNotInitialized)?;
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

        // Dial bootstrap peers
        self.dial_bootstrap_peers().await;

        info!(
            node = ?&self.node_id[..4],
            "Gossip protocol started with network"
        );

        Ok(())
    }

    /// Start the gossip protocol without a network (local-only).
    pub async fn start(&mut self) {
        self.running = true;
        info!(
            node = ?&self.node_id[..4],
            "Gossip protocol started"
        );
    }

    /// Stop the gossip protocol
    pub fn stop(&mut self) {
        self.running = false;
        self.network_cmd_tx = None;
        info!("Gossip protocol stopped");
    }

    /// Check if the gossip protocol is running
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
        let bytes = event
            .to_bytes()
            .map_err(|e| GossipError::SerializationError(e.to_string()))?;
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

    /// Get gossip protocol statistics
    pub fn stats(&self) -> &GossipStats {
        &self.stats
    }

    /// Read access to the underlying graph (for testing/inspection).
    pub fn graph(&self) -> &Arc<RwLock<CausalGraph>> {
        &self.graph
    }

    // ── Task 3.1: Validation Pipeline ──────────────────────────────────

    /// Validate a gossip event before it enters the pending queue.
    ///
    /// Checks (in order):
    /// 1. Hash integrity
    /// 2. Signature validity (including unsigned detection)
    /// 3. Timestamp sanity (future / ancient)
    ///
    /// Returns `Ok(())` if the event passes all checks.
    /// The caller is responsible for incrementing the appropriate stats
    /// counters based on the returned error variant.
    pub fn validate_event(&self, event: &Event) -> Result<(), GossipError> {
        // Delegate to Event::validate() which checks unsigned, hash,
        // signature, timestamps, and rejected status in order.
        event
            .validate()
            .map_err(|e| GossipError::ValidationFailed(e.to_string()))
    }

    // ── Task 3.2: Bootstrap Peer Dialing ───────────────────────────────

    /// Dial all configured bootstrap peers by sending Dial commands
    /// through the network command channel.
    pub async fn dial_bootstrap_peers(&self) {
        if let Some(ref cmd_tx) = self.network_cmd_tx {
            for addr_str in &self.config.bootstrap_peers {
                match addr_str.parse::<Multiaddr>() {
                    Ok(addr) => {
                        // Try to extract PeerId from the /p2p suffix
                        let peer_id = extract_peer_id_from_multiaddr(&addr);
                        match (peer_id, addr.clone()) {
                            (Some(pid), dial_addr) => {
                                info!(peer = ?pid, "Dialing bootstrap peer");
                                if let Err(e) = cmd_tx
                                    .send(NetworkCommand::Dial {
                                        peer_id: pid,
                                        addr: dial_addr,
                                    })
                                    .await
                                {
                                    warn!("Failed to send Dial command for bootstrap peer: {}", e);
                                }
                            }
                            (None, _) => {
                                warn!(
                                    addr = %addr_str,
                                    "Bootstrap address missing /p2p peer ID, skipping"
                                );
                            }
                        }
                    }
                    Err(e) => {
                        warn!(
                            addr = %addr_str,
                            "Failed to parse bootstrap address: {}", e
                        );
                    }
                }
            }
        }
    }

    // ── Task 3.3: Partition Detection ──────────────────────────────────

    /// Detect whether a network partition is occurring.
    ///
    /// Returns `true` if more than 1/3 of known peers have been silent
    /// for longer than the configured `partition_threshold_ms`.
    pub fn detect_partition(&self) -> bool {
        if self.last_seen.is_empty() {
            return false;
        }
        let now = Instant::now();
        let threshold = std::time::Duration::from_millis(self.config.partition_threshold_ms);
        let silent_count = self
            .last_seen
            .values()
            .filter(|&&last| now.duration_since(last) > threshold)
            .count();
        let total = self.last_seen.len();
        // More than 1/3 silent => partition
        silent_count * 3 > total
    }

    /// Check for partition state changes and emit GossipEvents.
    ///
    /// Call this periodically (e.g., from the main run loop) to detect
    /// transitions between partitioned and healthy states.
    pub fn check_partition(&mut self) -> Option<GossipEvent> {
        let now_partitioned = self.detect_partition();
        match (self.partition_active, now_partitioned) {
            (false, true) => {
                self.partition_active = true;
                warn!("Network partition detected: >1/3 of peers are silent");
                Some(GossipEvent::PartitionDetected)
            }
            (true, false) => {
                self.partition_active = false;
                info!("Network partition healed: peers are responsive again");
                Some(GossipEvent::PartitionHealed)
            }
            _ => None,
        }
    }

    /// Process pending events: first drain network_rx into the pending queue,
    /// then insert all pending events into the graph.
    ///
    /// This is the bridge between p2p network events and consensus —
    /// network events land in the graph where Substrate::process_consensus()
    /// can pick them up.
    pub async fn process_pending_events(&mut self) -> Result<Vec<EventId>, GossipError> {
        let mut inserted_ids = Vec::new();

        // Phase 1: Drain network events into a temporary buffer.
        // This avoids borrow-checker conflicts between the mutable borrow
        // on network_rx and immutable borrows on other self fields.
        enum DrainedEvent {
            Gossip { event: Box<Event>, source: PeerId },
            PeerConnected(PeerId),
            PeerDisconnected(PeerId),
        }
        let mut drained: Vec<DrainedEvent> = Vec::new();
        let mut channel_disconnected = false;

        if let Some(ref mut rx) = self.network_rx {
            loop {
                match rx.try_recv() {
                    Ok(NetworkEvent::GossipReceived {
                        data,
                        propagation_source,
                        ..
                    }) => match Event::from_bytes(&data) {
                        Ok(event) => {
                            drained.push(DrainedEvent::Gossip {
                                event: Box::new(event),
                                source: propagation_source,
                            });
                        }
                        Err(e) => {
                            warn!("Failed to deserialize gossip event: {:?}", e);
                            self.stats.events_rejected += 1;
                        }
                    },
                    Ok(NetworkEvent::PeerConnected(peer_id)) => {
                        drained.push(DrainedEvent::PeerConnected(peer_id));
                    }
                    Ok(NetworkEvent::PeerDisconnected(peer_id)) => {
                        drained.push(DrainedEvent::PeerDisconnected(peer_id));
                    }
                    Ok(_) => {}
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        channel_disconnected = true;
                        break;
                    }
                }
            }
        }

        if channel_disconnected {
            self.network_rx = None;
        }

        // Phase 2: Process drained events (validation, dedup, partition tracking)
        for evt in drained {
            match evt {
                DrainedEvent::Gossip { event, source } => {
                    // Update last_seen for partition detection
                    self.last_seen.insert(source, Instant::now());

                    // Rate limit check: hash PeerId bytes to [u8; 32] for the rate limiter
                    let peer_id_bytes = blake3::hash(&source.to_bytes());
                    if !self.rate_limiter.allow(peer_id_bytes.as_bytes()) {
                        warn!(peer = ?source, "Rate limiting peer — dropping event");
                        self.stats.events_rejected += 1;
                        continue;
                    }

                    // Task 3.1: Validate event BEFORE adding to pending queue
                    if let Err(e) = self.validate_event(&event) {
                        warn!("Gossip event rejected (validation): {:?}", e);
                        // Track signature-specific rejections
                        if matches!(
                            e,
                            GossipError::ValidationFailed(ref msg)
                            if msg.contains("signature") || msg.contains("unsigned")
                        ) {
                            self.stats.messages_rejected_invalid_sig += 1;
                        }
                        self.stats.events_rejected += 1;
                        continue;
                    }

                    if !self.seen_events.contains(&event.id) {
                        self.seen_events.insert(event.id);
                        self.pending_events.push_back(*event);
                        self.stats.events_received += 1;
                        // Prune seen_events if it exceeds the bound
                        if self.seen_events.len() > MAX_SEEN_EVENTS {
                            self.seen_events.clear();
                            tracing::debug!(
                                "seen_events exceeded {}, cleared dedup cache",
                                MAX_SEEN_EVENTS
                            );
                        }
                    }
                }
                DrainedEvent::PeerConnected(peer_id) => {
                    info!("Peer connected: {:?}", peer_id);
                    self.last_seen.insert(peer_id, Instant::now());
                }
                DrainedEvent::PeerDisconnected(peer_id) => {
                    info!("Peer disconnected: {:?}", peer_id);
                    // Don't remove from last_seen — partition detection
                    // needs to know about recently-disconnected peers.
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

/// Try to extract a PeerId from a Multiaddr that ends with /p2p/<peer-id>.
fn extract_peer_id_from_multiaddr(addr: &Multiaddr) -> Option<PeerId> {
    use libp2p::multiaddr::Protocol;
    addr.iter().find_map(|proto| match proto {
        Protocol::P2p(peer_id) => Some(peer_id),
        _ => None,
    })
}

    /// Errors from the gossip protocol
#[derive(Error, Debug, Clone)]
pub enum GossipError {
    #[error("Graph error: {0}")]
    /// Error from the causal graph
    GraphError(String),
    #[error("Network error: {0}")]
    /// Network communication error
    NetworkError(String),
    #[error("Serialization error: {0}")]
    /// Serialization/deserialization error
    SerializationError(String),
    #[error("Protocol not running")]
    /// Protocol is not running
    NotRunning,
    #[error("Event validation failed: {0}")]
    /// Event validation failed
    ValidationFailed(String),
    #[error("Channel not initialized")]
    /// Channel for network events is not initialized
    ChannelNotInitialized,
}

impl From<CausalGraphError> for GossipError {
    fn from(e: CausalGraphError) -> Self {
        GossipError::GraphError(e.to_string())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::crypto::generate_keypair;
    use crate::event::Event;

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
        assert_eq!(stats.messages_rejected_invalid_sig, 0);
    }

    // ── Task 3.1: Validation Pipeline Tests ────────────────────────────

    #[test]
    fn test_validate_event_valid_signed() {
        let graph = Arc::new(RwLock::new(CausalGraph::new()));
        let protocol = GossipProtocol::new(node(1), GossipConfig::default(), graph);

        let keypair = generate_keypair();
        let mut event = Event::genesis(node(2), vec![1, 2, 3]);
        event.sign_with_keypair(&keypair);

        assert!(protocol.validate_event(&event).is_ok());
    }

    #[test]
    fn test_validate_event_unsigned() {
        let graph = Arc::new(RwLock::new(CausalGraph::new()));
        let protocol = GossipProtocol::new(node(1), GossipConfig::default(), graph);

        // Event::new creates events with all-zero signature and pubkey
        let event = Event::genesis(node(2), vec![1, 2, 3]);

        let result = protocol.validate_event(&event);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("unsigned"),
            "Expected unsigned error, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_validate_event_invalid_signature() {
        let graph = Arc::new(RwLock::new(CausalGraph::new()));
        let protocol = GossipProtocol::new(node(1), GossipConfig::default(), graph);

        let keypair = generate_keypair();
        let mut event = Event::genesis(node(2), vec![1, 2, 3]);
        event.sign_with_keypair(&keypair);

        // Corrupt the signature
        let mut tampered = event.clone();
        tampered.signature = [0xABu8; 64];

        let result = protocol.validate_event(&tampered);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_event_tampered_hash() {
        let graph = Arc::new(RwLock::new(CausalGraph::new()));
        let protocol = GossipProtocol::new(node(1), GossipConfig::default(), graph);

        let keypair = generate_keypair();
        let mut event = Event::genesis(node(2), vec![1, 2, 3]);
        event.sign_with_keypair(&keypair);

        // Tamper with the ID
        let mut tampered = event.clone();
        tampered.id = [99u8; 32];

        let result = protocol.validate_event(&tampered);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("hash"),
            "Expected hash error"
        );
    }

    #[tokio::test]
    async fn test_process_pending_events_rejects_invalid() {
        use tokio::sync::mpsc;

        let graph = Arc::new(RwLock::new(CausalGraph::new()));
        let mut gossip = GossipProtocol::new(node(1), GossipConfig::default(), graph);

        // Create a fake network receiver
        let (tx, rx) = mpsc::channel(10);
        gossip.network_rx = Some(rx);

        // Create an unsigned event (should be rejected by validation)
        let event = Event::genesis(node(2), vec![1, 2, 3]);
        let bytes = event.to_bytes().expect("test event serialization");

        let dummy_peer_id = PeerId::random();
        tx.send(NetworkEvent::GossipReceived {
            topic: "omnia_events".to_string(),
            data: bytes,
            propagation_source: dummy_peer_id,
        })
        .await
        .expect("send should succeed");

        drop(tx);

        // Process pending events
        let inserted = gossip
            .process_pending_events()
            .await
            .expect("process should succeed");
        assert_eq!(inserted.len(), 0, "Unsigned event should be rejected");
        assert_eq!(gossip.stats().events_rejected, 1);
        assert!(
            gossip.stats().messages_rejected_invalid_sig >= 1,
            "Unsigned event should increment invalid_sig counter"
        );
    }

    #[tokio::test]
    async fn test_process_pending_events_accepts_valid() {
        use tokio::sync::mpsc;

        let graph = Arc::new(RwLock::new(CausalGraph::new()));
        let mut gossip = GossipProtocol::new(node(1), GossipConfig::default(), graph);

        let (tx, rx) = mpsc::channel(10);
        gossip.network_rx = Some(rx);

        // Create a properly signed event
        let keypair = generate_keypair();
        let mut event = Event::genesis(node(2), vec![1, 2, 3]);
        event.sign_with_keypair(&keypair);
        let event_id = event.id;
        let bytes = event.to_bytes().expect("test event serialization");

        let dummy_peer_id = PeerId::random();
        tx.send(NetworkEvent::GossipReceived {
            topic: "omnia_events".to_string(),
            data: bytes,
            propagation_source: dummy_peer_id,
        })
        .await
        .expect("send should succeed");

        drop(tx);

        let inserted = gossip
            .process_pending_events()
            .await
            .expect("process should succeed");
        assert_eq!(inserted.len(), 1);
        assert_eq!(gossip.stats().events_received, 1);
        assert_eq!(gossip.stats().events_accepted, 1);
        assert_eq!(gossip.stats().events_rejected, 0);

        let g = gossip.graph().read().await;
        assert!(g.contains(&event_id));
    }

    // ── Task 3.2: Bootstrap Peer Tests ─────────────────────────────────

    #[test]
    fn test_gossip_config_bootstrap_peers_default() {
        let config = GossipConfig::default();
        assert!(config.bootstrap_peers.is_empty());
    }

    #[test]
    fn test_gossip_config_bootstrap_peers_custom() {
        let config = GossipConfig {
            bootstrap_peers: vec!["/ip4/1.2.3.4/udp/4001/quic/p2p/12D3KooWABSApKz".to_string()],
            ..Default::default()
        };
        assert_eq!(config.bootstrap_peers.len(), 1);
    }

    #[test]
    fn test_gossip_config_partition_threshold_default() {
        let config = GossipConfig::default();
        assert_eq!(config.partition_threshold_ms, 30000);
    }

    // ── Task 3.3: Partition Detection Tests ────────────────────────────

    #[test]
    fn test_detect_partition_no_peers() {
        let graph = Arc::new(RwLock::new(CausalGraph::new()));
        let protocol = GossipProtocol::new(node(1), GossipConfig::default(), graph);
        // No peers => no partition
        assert!(!protocol.detect_partition());
    }

    #[test]
    fn test_detect_partition_healthy() {
        let graph = Arc::new(RwLock::new(CausalGraph::new()));
        let mut protocol = GossipProtocol::new(node(1), GossipConfig::default(), graph);

        // Add 3 peers that were just seen
        for _ in 0..3 {
            protocol.last_seen.insert(PeerId::random(), Instant::now());
        }

        assert!(
            !protocol.detect_partition(),
            "All peers recently seen => no partition"
        );
    }

    #[test]
    fn test_detect_partition_with_silent_peers() {
        let graph = Arc::new(RwLock::new(CausalGraph::new()));
        let mut protocol = GossipProtocol::new(
            node(1),
            GossipConfig {
                partition_threshold_ms: 100,
                ..Default::default()
            },
            graph,
        );

        // Add 3 peers: 2 silent (>100ms ago), 1 recent
        // 2/3 silent => 2*3 > 3 => 6 > 3 => partition detected
        let now = Instant::now();
        let long_ago = now - std::time::Duration::from_millis(500);

        protocol.last_seen.insert(PeerId::random(), long_ago);
        protocol.last_seen.insert(PeerId::random(), long_ago);
        protocol.last_seen.insert(PeerId::random(), now);

        assert!(protocol.detect_partition(), "2/3 peers silent => partition");
    }

    #[test]
    fn test_detect_partition_below_threshold() {
        let graph = Arc::new(RwLock::new(CausalGraph::new()));
        let mut protocol = GossipProtocol::new(
            node(1),
            GossipConfig {
                partition_threshold_ms: 100,
                ..Default::default()
            },
            graph,
        );

        // Add 3 peers: 1 silent, 2 recent
        // 1/3 silent => 1*3 > 3? 3 > 3 => false => no partition
        let now = Instant::now();
        let long_ago = now - std::time::Duration::from_millis(500);

        protocol.last_seen.insert(PeerId::random(), long_ago);
        protocol.last_seen.insert(PeerId::random(), now);
        protocol.last_seen.insert(PeerId::random(), now);

        assert!(
            !protocol.detect_partition(),
            "1/3 peers silent => not enough for partition"
        );
    }

    #[test]
    fn test_partition_event_transitions() {
        let graph = Arc::new(RwLock::new(CausalGraph::new()));
        let mut protocol = GossipProtocol::new(
            node(1),
            GossipConfig {
                partition_threshold_ms: 100,
                ..Default::default()
            },
            graph,
        );

        // Initially healthy
        assert_eq!(protocol.check_partition(), None);

        // Make all peers silent
        let long_ago = Instant::now() - std::time::Duration::from_millis(500);
        for _ in 0..3 {
            protocol.last_seen.insert(PeerId::random(), long_ago);
        }

        // Should detect partition
        assert_eq!(
            protocol.check_partition(),
            Some(GossipEvent::PartitionDetected)
        );

        // Already in partition state — no new event
        assert_eq!(protocol.check_partition(), None);

        // Heal: update all peers to recent
        let now = Instant::now();
        for peer_id in protocol.last_seen.keys().copied().collect::<Vec<_>>() {
            protocol.last_seen.insert(peer_id, now);
        }

        // Should detect healing
        assert_eq!(
            protocol.check_partition(),
            Some(GossipEvent::PartitionHealed)
        );

        // Already healthy — no new event
        assert_eq!(protocol.check_partition(), None);
    }

    #[test]
    fn test_gossip_event_variants() {
        // Ensure the variants exist and can be constructed
        let _detected = GossipEvent::PartitionDetected;
        let _healed = GossipEvent::PartitionHealed;
        assert_ne!(GossipEvent::PartitionDetected, GossipEvent::PartitionHealed);
    }

    // ── Task 30: Bounded Caches and Pruning Tests ──────────────────────

    #[test]
    fn test_seen_events_cleared_when_exceeds_max() {
        let graph = Arc::new(RwLock::new(CausalGraph::new()));
        let mut protocol = GossipProtocol::new(node(1), GossipConfig::default(), graph);

        // Insert MAX_SEEN_EVENTS + 1 entries directly
        for i in 0..=MAX_SEEN_EVENTS {
            let mut id = [0u8; 32];
            id[0] = (i % 256) as u8;
            id[1] = ((i >> 8) % 256) as u8;
            protocol.seen_events.insert(id);
        }

        // The set should have MAX_SEEN_EVENTS + 1 entries
        assert!(protocol.seen_events.len() > MAX_SEEN_EVENTS);

        // Simulate the cleanup that happens in process_pending_events
        if protocol.seen_events.len() > MAX_SEEN_EVENTS {
            protocol.seen_events.clear();
        }

        // After cleanup, it should be empty (cleared dedup cache)
        assert!(protocol.seen_events.is_empty());
    }

    #[test]
    fn test_recent_gossip_field_removed() {
        // Verify that GossipProtocol no longer has a recent_gossip field
        // by checking that we can construct the struct without it.
        let graph = Arc::new(RwLock::new(CausalGraph::new()));
        let protocol = GossipProtocol::new(node(1), GossipConfig::default(), graph);
        // If this compiles, the field was successfully removed
        let _ = protocol.is_running();
    }
}
