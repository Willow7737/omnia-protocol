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

use crate::blake3_domain::blake3_hash_domain;
use crate::compact_event_encoding::{decode_compact_wire, encode_compact_wire, is_compact_wire};
use crate::compression::{deserialize_with_compression, serialize_with_compression};
use crate::gossip_bloom_filter::GossipBloomFilter;
use crate::network::{extract_peer_id_from_multiaddr, NetworkCommand, NetworkEvent, OmniaNetwork};
use crate::priority_gossip_queue::{GossipPriority, PriorityGossipQueue, PriorityQueueConfig};
use omnia_consensus::causal_graph::{CausalGraph, CausalGraphError};
use omnia_consensus::rate_limiter::RateLimiter;
use omnia_primitives::{Event, EventBatch, EventId, EventRequest, MAX_PAYLOAD_SIZE};
use omnia_primitives::{NodeId, VectorClock};

const DEFAULT_GOSSIP_INTERVAL_MS: u64 = 100;
const MAX_EVENTS_PER_GOSSIP: usize = 100;
const MAX_PENDING_EVENTS: usize = 100_000;
const DEFAULT_PARTITION_THRESHOLD_MS: u64 = 30_000; // 30 seconds (was 3s)

/// Default interval between gossip heartbeats. One third of the partition
/// threshold, so a healthy connection can lose two consecutive heartbeats
/// before the peer is considered silent.
const DEFAULT_HEARTBEAT_INTERVAL_MS: u64 = DEFAULT_PARTITION_THRESHOLD_MS / 3;

/// Constant topic name for Omnia events gossip. Avoids per-call allocation.
const OMNIA_EVENTS_TOPIC: &str = "omnia_events";

/// Topic for gossip keepalive heartbeats (issue #259).
///
/// Nothing in the protocol generates background traffic, so on an idle
/// network every peer eventually exceeds `partition_threshold_ms` of
/// silence and the mesh dissolves. Heartbeats on this topic keep
/// `last_seen` fresh on healthy connections. Receivers update peer
/// liveness and discard the payload — it never reaches consensus.
pub const HEARTBEAT_TOPIC: &str = "omnia_heartbeat";

/// Maximum buffered auxiliary-topic messages (non-event topics such as
/// Lane 0 ack batches). When full, the oldest message is dropped —
/// auxiliary protocols must tolerate loss (Lane 0 certificates are
/// grow-only CRDTs, so a dropped ack batch only delays finality until
/// the next delivery).
const MAX_AUX_MESSAGES: usize = 4096;

/// Expected number of events per bloom-filter rotation window for dedup.
///
/// The rotating [`GossipBloomFilter`] is sized for this many events at a
/// 0.1% false-positive rate (~350 KiB for both filters combined). When the
/// active filter reaches this count it rotates, expiring entries older
/// than two windows.
const MAX_SEEN_EVENTS: usize = 100_000;

/// Target false-positive rate for the dedup bloom filter.
///
/// A bloom "maybe seen" answer is always confirmed against the pending
/// queue and the causal graph before an event is dropped, so a false
/// positive costs one exact lookup — never a lost event.
const SEEN_FILTER_FP_RATE: f64 = 0.001;

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
    /// Interval in milliseconds between keepalive heartbeats on
    /// [`HEARTBEAT_TOPIC`]. Must be well below `partition_threshold_ms`
    /// or idle peers will still be evicted. `0` disables heartbeats.
    /// Default: 10000 ms (a third of the partition threshold).
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval_ms: u64,
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

fn default_heartbeat_interval() -> u64 {
    DEFAULT_HEARTBEAT_INTERVAL_MS
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
            heartbeat_interval_ms: DEFAULT_HEARTBEAT_INTERVAL_MS,
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
    /// Total number of events received across all completed sync operations.
    ///
    /// To compute the average events per sync, divide by `total_syncs`:
    /// `avg = total_events_in_syncs / total_syncs` (caller handles division).
    pub total_events_in_syncs: u64,
    /// Total number of completed sync operations.
    ///
    /// Combined with `total_events_in_syncs`, this allows the caller to
    /// compute the average events per sync without losing precision to
    /// floating-point arithmetic.
    pub total_syncs: u64,
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
    // Network event receiver — drained into the pending queue by
    // process_pending_events() so that p2p events flow through consensus.
    /// Network event receiver
    pub network_rx: Option<mpsc::Receiver<NetworkEvent>>,
    /// Priority-ordered pending event IDs: consensus-relevant merge events
    /// are dequeued (and therefore inserted into the graph) first.
    pending_queue: PriorityGossipQueue,
    /// Payloads for queued event IDs. Kept in lockstep with `pending_queue`.
    pending_store: HashMap<EventId, Event>,
    /// Buffered messages from non-event topics (e.g. Lane 0 ack batches),
    /// as `(topic, payload)` pairs. Bounded by [`MAX_AUX_MESSAGES`];
    /// drained by [`take_aux_messages`](Self::take_aux_messages).
    aux_messages: VecDeque<(String, Vec<u8>)>,
    stats: GossipStats,
    last_sync: Instant,
    running: bool,
    /// Rotating bloom filter for duplicate suppression (see
    /// [`MAX_SEEN_EVENTS`] / [`SEEN_FILTER_FP_RATE`]). "Maybe seen" answers
    /// are confirmed exactly before dropping an event.
    seen_filter: GossipBloomFilter,
    /// Tracks when each peer was last heard from (for partition detection).
    last_seen: HashMap<PeerId, Instant>,
    /// Peers with a live transport connection (PeerConnected received,
    /// no PeerDisconnected yet). Used as the liveness fallback for
    /// partition detection: a transport-connected peer is never treated
    /// as silent, even if it hasn't sent a message recently (issue #259).
    connected_peers: HashSet<PeerId>,
    /// When this node last published a keepalive heartbeat.
    last_heartbeat: Instant,
    /// Whether a partition is currently detected (to avoid duplicate events).
    partition_active: bool,
    /// Per-peer rate limiter (token bucket).
    rate_limiter: RateLimiter,
    /// Events that exceeded a peer's rate limit, held for retry on the next
    /// processing round instead of being dropped. Bounded by
    /// [`MAX_RATE_DEFERRED`]; beyond that events are dropped as before.
    ///
    /// Rationale: gossipsub delivers a message to a mesh peer exactly once —
    /// an event dropped here is *permanently lost* to this node (no
    /// redelivery), which turned the rate limiter into a data-loss mechanism
    /// for honest bursts (e.g. a 1000-event burst from one peer capped at
    /// `burst_capacity` inserted, the rest gone). Deferring converts the
    /// limiter into backpressure: the burst drains at the refill rate over
    /// subsequent rounds while memory stays bounded.
    rate_deferred: VecDeque<(Box<Event>, PeerId)>,
}

/// Maximum events held for rate-limit retry (DoS bound for the internal
/// `GossipProtocol::rate_deferred` queue). Oversized payloads are rejected
/// before deferral, so worst-case memory is bounded by well-formed events.
pub const MAX_RATE_DEFERRED: usize = 4096;

impl GossipProtocol {
    /// Create a new gossip protocol instance
    pub fn new(node_id: NodeId, config: GossipConfig, graph: Arc<RwLock<CausalGraph>>) -> Self {
        let rate_limiter = RateLimiter::new(config.burst_capacity, config.max_events_per_second);
        // Map the single `max_pending` knob onto per-priority capacities:
        // Normal (regular events) keeps the full configured bound; the
        // other levels get a tenth each, so worst-case memory stays within
        // 1.3× of the pre-priority-queue bound.
        let per_level = (config.max_pending / 10).max(1024);
        let queue_config = PriorityQueueConfig {
            max_critical: per_level,
            max_high: per_level,
            max_normal: config.max_pending,
            max_low: per_level,
        };
        Self {
            node_id,
            config,
            graph,
            network_cmd_tx: None,
            network_rx: None,
            pending_queue: PriorityGossipQueue::new(queue_config),
            pending_store: HashMap::new(),
            aux_messages: VecDeque::new(),
            stats: GossipStats::default(),
            last_sync: Instant::now(),
            running: false,
            seen_filter: GossipBloomFilter::new(MAX_SEEN_EVENTS, SEEN_FILTER_FP_RATE),
            last_seen: HashMap::new(),
            connected_peers: HashSet::new(),
            last_heartbeat: Instant::now(),
            partition_active: false,
            rate_limiter,
            rate_deferred: VecDeque::new(),
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
        let event_rx = network.event_rx.take().ok_or(GossipError::ChannelNotInitialized)?;
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
    ///
    /// F-21 fix: This method is called by Substrate::submit_event AFTER
    /// the event has already been inserted into the graph. The previous
    /// implementation called graph.insert(event.clone()) again here,
    /// which returned Err(DuplicateEvent) and caused every local-event
    /// submission to fail when gossip was initialized.
    ///
    /// The graph insert has been removed. Callers that need to insert
    /// AND broadcast should call Substrate::submit_event (which does
    /// both in the correct order). Callers that need to broadcast an
    /// already-inserted event (e.g., relaying a remote event) can call
    /// this method directly.
    pub async fn broadcast_event(&mut self, event: Event) -> Result<(), GossipError> {
        // F-21 fix: do NOT re-insert the event into the graph here.
        // Substrate::submit_event already inserted it. Re-inserting
        // returns Err(DuplicateEvent) which propagates as a GossipError
        // and fails the entire submit_event call.
        //
        // The original comment "Add to our graph first" was correct
        // when broadcast_event was the only insertion path, but
        // Substrate::submit_event was later wired to insert first,
        // creating a double-insert.

        // Serialize with the compact wire format (delta-encoded vector
        // clock, ~40% smaller); fall back to the full event format for the
        // rare clock shapes compact encoding cannot represent losslessly.
        let bytes = match encode_compact_wire(&event) {
            Ok(bytes) => bytes,
            Err(_) => event
                .to_bytes()
                .map_err(|e| GossipError::SerializationError(e.to_string()))?,
        };
        let bytes_len = bytes.len();
        let event_id_prefix = &event.id[..4];

        // Mark our own broadcast as seen so a relayed echo of it is
        // suppressed by dedup instead of paying validation again.
        self.seen_filter.insert(&event.id);
        if let Some(ref cmd_tx) = self.network_cmd_tx {
            let result = cmd_tx
                .send(NetworkCommand::Publish {
                    topic: OMNIA_EVENTS_TOPIC.to_string(),
                    data: bytes,
                })
                .await;
            if let Err(e) = result {
                tracing::error!(
                    "Event {} inserted into local graph but FAILED to publish to network: {e}. \
                     Local node has event but peers may not. Manual intervention may be required.",
                    hex::encode(event_id_prefix)
                );
            }
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
                            (None, dial_addr) => {
                                // No /p2p component — dial the raw address and
                                // learn the peer's identity during the handshake.
                                // Previously these addresses were silently
                                // skipped, which meant the stock docker-compose
                                // testnet (whose bootstrap address has no peer
                                // ID) could never form a network.
                                info!(addr = %addr_str, "Dialing bootstrap peer (identity learned on handshake)");
                                if let Err(e) = cmd_tx.send(NetworkCommand::DialAddress { addr: dial_addr }).await {
                                    warn!("Failed to send DialAddress command for bootstrap peer: {}", e);
                                }
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
    ///
    /// A peer with a live transport connection is never counted as
    /// silent, regardless of message recency: on an idle network no
    /// messages flow, but the QUIC connection state still proves the
    /// peer is reachable (issue #259).
    pub fn detect_partition(&self) -> bool {
        if self.last_seen.is_empty() {
            return false;
        }
        let now = Instant::now();
        let threshold = std::time::Duration::from_millis(self.config.partition_threshold_ms);
        let silent_count = self
            .last_seen
            .iter()
            .filter(|&(peer, &last)| !self.connected_peers.contains(peer) && now.duration_since(last) > threshold)
            .count();
        let total = self.last_seen.len();
        // More than 1/3 silent => partition
        silent_count * 3 > total
    }

    /// Return the number of peers we have observed recently enough to
    /// consider them "connected" for `/readyz` and node-info purposes.
    ///
    /// A peer is considered connected if it has a live transport
    /// connection, or has been heard from within the partition-detection
    /// threshold. This includes peers that sent us gossip messages or
    /// that the network layer reported as PeerConnected.
    pub fn connected_peer_count(&self) -> usize {
        if self.last_seen.is_empty() {
            return 0;
        }
        let now = Instant::now();
        let threshold = std::time::Duration::from_millis(self.config.partition_threshold_ms);
        self.last_seen
            .iter()
            .filter(|&(peer, &last)| self.connected_peers.contains(peer) || now.duration_since(last) <= threshold)
            .count()
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
            Gossip {
                event: Box<Event>,
                source: PeerId,
            },
            Aux {
                topic: String,
                data: Vec<u8>,
                source: PeerId,
            },
            PeerConnected(PeerId),
            PeerDisconnected(PeerId),
        }
        let mut drained: Vec<DrainedEvent> = Vec::new();
        let mut channel_disconnected = false;

        if let Some(ref mut rx) = self.network_rx {
            loop {
                match rx.try_recv() {
                    Ok(NetworkEvent::GossipReceived {
                        topic,
                        data,
                        propagation_source,
                    }) if topic != OMNIA_EVENTS_TOPIC => {
                        // Non-event topic (e.g. Lane 0 acks): buffer the raw
                        // payload for the substrate layer to decode.
                        drained.push(DrainedEvent::Aux {
                            topic,
                            data,
                            source: propagation_source,
                        });
                    }
                    Ok(NetworkEvent::GossipReceived {
                        data,
                        propagation_source,
                        ..
                    }) => {
                        // Dispatch on the wire-format version byte: compact
                        // events (version 2) reconstruct the full event; any
                        // other prefix takes the full-event path.
                        let parsed = if is_compact_wire(&data) {
                            decode_compact_wire(&data).map_err(|e| e.to_string())
                        } else {
                            Event::from_bytes(&data).map_err(|e| e.to_string())
                        };
                        match parsed {
                            Ok(event) => {
                                drained.push(DrainedEvent::Gossip {
                                    event: Box::new(event),
                                    source: propagation_source,
                                });
                            }
                            Err(e) => {
                                warn!("Failed to deserialize gossip event: {e}");
                                self.stats.events_rejected += 1;
                            }
                        }
                    }
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

        // Phase 2: Process events — rate-limit-deferred retries from earlier
        // rounds first (their tokens may have refilled; FIFO keeps ordering
        // fair), then the freshly drained batch.
        let mut work: Vec<DrainedEvent> = self
            .rate_deferred
            .drain(..)
            .map(|(event, source)| DrainedEvent::Gossip { event, source })
            .collect();
        work.extend(drained);
        for evt in work {
            match evt {
                DrainedEvent::Gossip { event, source } => {
                    // Update last_seen for partition detection
                    self.last_seen.insert(source, Instant::now());

                    // Defense-in-depth: reject oversized payloads BEFORE the
                    // rate limiter (so junk never occupies deferred slots)
                    // and before any expensive crypto validation.
                    // Event::validate() also checks this, but the gossip
                    // layer enforces it early to avoid wasted CPU.
                    if event.payload.len() > MAX_PAYLOAD_SIZE {
                        warn!(
                            size = event.payload.len(),
                            max = MAX_PAYLOAD_SIZE,
                            "Gossip event rejected: payload exceeds MAX_PAYLOAD_SIZE"
                        );
                        self.stats.events_rejected += 1;
                        continue;
                    }

                    // Rate limit check: hash PeerId bytes with domain
                    // separation. Over-limit events are DEFERRED to the next
                    // processing round (token bucket refills at
                    // `max_events_per_second`), not dropped: gossipsub never
                    // redelivers, so a drop here would lose the event on this
                    // node permanently. Only when the bounded deferral queue
                    // is itself full does the old drop behaviour kick in.
                    let peer_id_bytes = blake3_hash_domain(b"omnia-nonce", &source.to_bytes());
                    if !self.rate_limiter.allow(&peer_id_bytes) {
                        if self.rate_deferred.len() < MAX_RATE_DEFERRED {
                            tracing::debug!(peer = ?source, "Rate limiting peer — deferring event to next round");
                            self.rate_deferred.push_back((event, source));
                        } else {
                            warn!(peer = ?source, "Rate limiting peer — deferral queue full, dropping event");
                            self.stats.events_rejected += 1;
                        }
                        continue;
                    }

                    // Task 3.1: Validate event BEFORE adding to pending queue
                    if let Err(e) = self.validate_event(&event) {
                        warn!("Gossip event rejected (validation): {:?}", e);
                        // Track signature-specific rejections
                        // TODO: Replace with structured error types from libp2p for reliable detection
                        if matches!(
                            e,
                            GossipError::ValidationFailed(ref msg)
                            if msg.to_lowercase().contains("signature") || msg.to_lowercase().contains("invalid")
                        ) {
                            self.stats.messages_rejected_invalid_sig += 1;
                        }
                        self.stats.events_rejected += 1;
                        continue;
                    }

                    // Duplicate suppression: a bloom "not seen" answer is
                    // authoritative (no false negatives) — the event is new.
                    // A bloom "maybe seen" answer is confirmed against the
                    // pending queue and the causal graph before dropping, so
                    // a false positive costs one exact lookup, never a lost
                    // event. This also recovers events that were evicted
                    // from an overflowing queue: they stay absent from both
                    // exact stores, so a retransmission is re-admitted.
                    if self.seen_filter.contains(&event.id)
                        && (self.pending_queue.contains(&event.id) || self.graph.read().await.contains(&event.id))
                    {
                        continue;
                    }
                    self.seen_filter.insert(&event.id);
                    if self.seen_filter.active_count() >= self.seen_filter.expected_items() {
                        // Rotation expires entries older than two windows,
                        // keeping the false-positive rate bounded.
                        self.seen_filter.rotate();
                    }

                    let priority = Self::classify_priority(&event);
                    if let Some(evicted) = self.pending_queue.enqueue(event.id, priority) {
                        // FIX 8 (carried over): bounded pending queue —
                        // capacity overflow drops the oldest event at the
                        // same priority level.
                        self.pending_store.remove(&evicted);
                        tracing::warn!("Pending events queue overflow - dropping event {:?}", evicted);
                    }
                    self.pending_store.insert(event.id, *event);
                    self.stats.events_received += 1;
                }
                DrainedEvent::Aux { topic, data, source } => {
                    self.last_seen.insert(source, Instant::now());
                    // Keepalive heartbeats only refresh peer liveness —
                    // never buffer them for the substrate layer (#259).
                    if topic == HEARTBEAT_TOPIC {
                        continue;
                    }
                    // Bounded buffer: drop the oldest message when full.
                    if self.aux_messages.len() >= MAX_AUX_MESSAGES {
                        self.aux_messages.pop_front();
                    }
                    self.aux_messages.push_back((topic, data));
                }
                DrainedEvent::PeerConnected(peer_id) => {
                    info!("Peer connected: {:?}", peer_id);
                    self.last_seen.insert(peer_id, Instant::now());
                    self.connected_peers.insert(peer_id);
                }
                DrainedEvent::PeerDisconnected(peer_id) => {
                    info!("Peer disconnected: {:?}", peer_id);
                    // Don't remove from last_seen — partition detection
                    // needs to know about recently-disconnected peers.
                    // Do drop the transport-liveness exemption: a
                    // disconnected peer must be eligible for eviction.
                    self.connected_peers.remove(&peer_id);
                }
            }
        }

        // Process all pending events in priority order: consensus-relevant
        // merge events are inserted into the graph before regular events.
        {
            let mut graph = self.graph.write().await;
            while let Some(event_id) = self.pending_queue.dequeue() {
                let Some(event) = self.pending_store.remove(&event_id) else {
                    continue;
                };
                match graph.insert(event) {
                    Ok(ids) => {
                        self.stats.events_accepted += 1;
                        inserted_ids.extend(ids);
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
        }

        Ok(inserted_ids)
    }

    /// Drain buffered messages from non-event topics (e.g. Lane 0 ack
    /// batches), as `(topic, payload)` pairs in arrival order.
    ///
    /// Aux messages are buffered by [`process_pending_events`](Self::process_pending_events);
    /// call this after it to pick them up.
    pub fn take_aux_messages(&mut self) -> Vec<(String, Vec<u8>)> {
        self.aux_messages.drain(..).collect()
    }

    /// Publish a raw payload on an arbitrary gossipsub topic.
    ///
    /// Used for auxiliary protocols (e.g. Lane 0 finality acks) that ride
    /// their own topic beside the event topic. The caller is responsible
    /// for the payload's wire format and any subscription to the topic.
    pub async fn publish_raw(&mut self, topic: &str, data: Vec<u8>) -> Result<(), GossipError> {
        let bytes_len = data.len();
        if let Some(ref cmd_tx) = self.network_cmd_tx {
            cmd_tx
                .send(NetworkCommand::Publish {
                    topic: topic.to_string(),
                    data,
                })
                .await
                .map_err(|e| GossipError::NetworkError(format!("publish_raw({topic}): {e}")))?;
            self.stats.bytes_sent += bytes_len as u64;
        }
        Ok(())
    }

    /// Publish a keepalive heartbeat on [`HEARTBEAT_TOPIC`] if one is due.
    ///
    /// Call this from the periodic consensus loop. It is a no-op when:
    /// - heartbeats are disabled (`heartbeat_interval_ms == 0`),
    /// - no network is wired,
    /// - no transport-connected peers exist (publishing to an empty
    ///   mesh only produces "insufficient peers" warnings), or
    /// - the last heartbeat was sent less than `heartbeat_interval_ms` ago.
    ///
    /// The payload is `node_id ‖ unix_millis` — the timestamp makes each
    /// heartbeat unique so gossipsub's message-ID dedup never suppresses
    /// consecutive keepalives (issue #259).
    pub async fn maybe_send_heartbeat(&mut self) {
        if self.config.heartbeat_interval_ms == 0 || self.network_cmd_tx.is_none() || self.connected_peers.is_empty() {
            return;
        }
        let interval = std::time::Duration::from_millis(self.config.heartbeat_interval_ms);
        if self.last_heartbeat.elapsed() < interval {
            return;
        }
        self.last_heartbeat = Instant::now();

        let unix_millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let mut payload = Vec::with_capacity(40);
        payload.extend_from_slice(&self.node_id);
        payload.extend_from_slice(&unix_millis.to_le_bytes());

        if let Err(e) = self.publish_raw(HEARTBEAT_TOPIC, payload).await {
            tracing::debug!("Heartbeat publish failed: {e}");
        }
    }

    /// Classify an incoming event's gossip priority.
    ///
    /// Merge events (those carrying an `other_parent`) knit the causal DAG
    /// across creators and are what round/witness structure advances on,
    /// so they are processed before regular single-chain events. `Critical`
    /// and `Low` are reserved for consensus-flagged messages and sync
    /// retransmissions respectively (retransmissions are currently
    /// suppressed by dedup before they reach the queue).
    fn classify_priority(event: &Event) -> GossipPriority {
        if event.other_parent.is_some() {
            GossipPriority::High
        } else {
            GossipPriority::Normal
        }
    }
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
    #[error("Payload too large for gossip: {size} bytes (max {max})")]
    /// Gossip event payload exceeds the maximum allowed size
    PayloadTooLarge {
        /// Actual payload size in bytes.
        size: usize,
        /// Maximum allowed payload size in bytes.
        max: usize,
    },
    /// Compression/decompression error.
    #[error("compression error: {0}")]
    Compression(String),
    /// Invalid message format.
    #[error("invalid message format: {0}")]
    InvalidMessageFormat(String),
}

/// Serialize an event with optional snappy compression.
///
/// Delegates to [`crate::compression::serialize_with_compression`] and
/// converts the error type to [`GossipError`].
pub fn serialize_compressed<T: Serialize>(value: &T) -> Result<Vec<u8>, GossipError> {
    serialize_with_compression(value).map_err(GossipError::SerializationError)
}

/// Deserialize an event with optional snappy decompression.
///
/// Delegates to [`crate::compression::deserialize_with_compression`] and
/// converts the error type to [`GossipError`].
pub fn deserialize_compressed<T: serde::de::DeserializeOwned>(data: &[u8]) -> Result<T, GossipError> {
    deserialize_with_compression(data).map_err(|e| match e {
        s if s.starts_with("unknown compression") => GossipError::InvalidMessageFormat(s),
        s if s.starts_with("empty payload") => GossipError::InvalidMessageFormat(s),
        s if s.contains("decompress") || s.contains("exceeds limit") => GossipError::Compression(s),
        s => GossipError::SerializationError(s),
    })
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
    use omnia_crypto::generate_keypair;
    use omnia_primitives::Event;

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
        let mut event = Event::genesis(node(2), vec![1, 2, 3]).expect("valid genesis event");
        event.sign_with_keypair(&keypair).expect("signing");

        assert!(protocol.validate_event(&event).is_ok());
    }

    #[test]
    fn test_validate_event_unsigned() {
        let graph = Arc::new(RwLock::new(CausalGraph::new()));
        let protocol = GossipProtocol::new(node(1), GossipConfig::default(), graph);

        // Event::new creates events with all-zero signature and pubkey
        let event = Event::genesis(node(2), vec![1, 2, 3]).expect("valid genesis event");

        let result = protocol.validate_event(&event);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("unsigned"), "Expected unsigned error, got: {err_msg}");
    }

    #[test]
    fn test_validate_event_invalid_signature() {
        let graph = Arc::new(RwLock::new(CausalGraph::new()));
        let protocol = GossipProtocol::new(node(1), GossipConfig::default(), graph);

        let keypair = generate_keypair();
        let mut event = Event::genesis(node(2), vec![1, 2, 3]).expect("valid genesis event");
        event.sign_with_keypair(&keypair).expect("signing");

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
        let mut event = Event::genesis(node(2), vec![1, 2, 3]).expect("valid genesis event");
        event.sign_with_keypair(&keypair).expect("signing");

        // Tamper with the ID
        let mut tampered = event.clone();
        tampered.id = [99u8; 32];

        let result = protocol.validate_event(&tampered);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("hash"), "Expected hash error");
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
        let event = Event::genesis(node(2), vec![1, 2, 3]).expect("valid genesis event");
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
        let inserted = gossip.process_pending_events().await.expect("process should succeed");
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
        let mut event = Event::genesis(node(2), vec![1, 2, 3]).expect("valid genesis event");
        event.sign_with_keypair(&keypair).expect("signing");
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

        let inserted = gossip.process_pending_events().await.expect("process should succeed");
        assert_eq!(inserted.len(), 1);
        assert_eq!(gossip.stats().events_received, 1);
        assert_eq!(gossip.stats().events_accepted, 1);
        assert_eq!(gossip.stats().events_rejected, 0);

        let g = gossip.graph().read().await;
        assert!(g.contains(&event_id));
    }

    #[tokio::test]
    async fn test_rate_limited_events_deferred_not_dropped() {
        use tokio::sync::mpsc;

        let graph = Arc::new(RwLock::new(CausalGraph::new()));
        let config = GossipConfig {
            // Tiny burst so 5 events from one peer overflow immediately. The
            // refill must be much slower than the drain loop (1 token per
            // 100ms vs ~µs per event) or tokens regenerate mid-drain and the
            // limiter never trips; the sleeps below then span whole refills.
            burst_capacity: 2,
            max_events_per_second: 10,
            ..Default::default()
        };
        let mut gossip = GossipProtocol::new(node(1), config, graph);

        let (tx, rx) = mpsc::channel(10);
        gossip.network_rx = Some(rx);

        let peer = PeerId::random();
        for i in 0..5u8 {
            let keypair = generate_keypair();
            let mut event = Event::genesis(node(10 + i), vec![i]).expect("valid genesis event");
            event.sign_with_keypair(&keypair).expect("signing");
            tx.send(NetworkEvent::GossipReceived {
                topic: "omnia_events".to_string(),
                data: event.to_bytes().expect("serialize"),
                propagation_source: peer,
            })
            .await
            .expect("send should succeed");
        }
        drop(tx);

        // Round 1: burst of 2 admitted, 3 deferred — and crucially NOT
        // counted as rejected (they are not lost).
        let inserted = gossip.process_pending_events().await.expect("process");
        assert_eq!(inserted.len(), 2, "burst capacity admitted");
        assert_eq!(gossip.rate_deferred.len(), 3, "overflow deferred, not dropped");
        assert_eq!(gossip.stats().events_rejected, 0);

        // Round 2 (after refill; tokens cap at burst_capacity=2).
        std::thread::sleep(std::time::Duration::from_millis(250));
        let inserted = gossip.process_pending_events().await.expect("process");
        assert_eq!(inserted.len(), 2, "deferred events retried after refill");
        assert_eq!(gossip.rate_deferred.len(), 1);

        // Round 3: the last one lands. Nothing was ever lost.
        std::thread::sleep(std::time::Duration::from_millis(250));
        let inserted = gossip.process_pending_events().await.expect("process");
        assert_eq!(inserted.len(), 1, "final deferred event delivered");
        assert!(gossip.rate_deferred.is_empty());
        assert_eq!(gossip.stats().events_rejected, 0, "no event was dropped");
        assert_eq!(gossip.graph().read().await.len(), 5, "all 5 events inserted");
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

        assert!(!protocol.detect_partition(), "All peers recently seen => no partition");
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
        assert_eq!(protocol.check_partition(), Some(GossipEvent::PartitionDetected));

        // Already in partition state — no new event
        assert_eq!(protocol.check_partition(), None);

        // Heal: update all peers to recent
        let now = Instant::now();
        for peer_id in protocol.last_seen.keys().copied().collect::<Vec<_>>() {
            protocol.last_seen.insert(peer_id, now);
        }

        // Should detect healing
        assert_eq!(protocol.check_partition(), Some(GossipEvent::PartitionHealed));

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

    // ── Issue #259: keepalive heartbeats + transport liveness ──────────

    #[test]
    fn test_gossip_config_heartbeat_interval_default() {
        let config = GossipConfig::default();
        assert_eq!(
            config.heartbeat_interval_ms, 10_000,
            "heartbeat interval must default to a third of the partition threshold"
        );
    }

    #[test]
    fn test_transport_connected_peer_never_silent() {
        let graph = Arc::new(RwLock::new(CausalGraph::new()));
        let mut protocol = GossipProtocol::new(
            node(1),
            GossipConfig {
                partition_threshold_ms: 100,
                ..Default::default()
            },
            graph,
        );

        // All peers silent past the threshold — but transport-connected.
        let long_ago = Instant::now() - std::time::Duration::from_millis(500);
        let peers: Vec<PeerId> = (0..3).map(|_| PeerId::random()).collect();
        for p in &peers {
            protocol.last_seen.insert(*p, long_ago);
            protocol.connected_peers.insert(*p);
        }
        assert!(
            !protocol.detect_partition(),
            "transport-connected peers must never count as silent"
        );
        assert_eq!(protocol.connected_peer_count(), 3);

        // Connections drop → the same silence now means eviction.
        for p in &peers {
            protocol.connected_peers.remove(p);
        }
        assert!(protocol.detect_partition());
        assert_eq!(protocol.connected_peer_count(), 0);
    }

    /// An incoming heartbeat must refresh the sender's liveness without
    /// leaking into the aux-message buffer (it is not consensus data).
    #[tokio::test]
    async fn test_heartbeat_refreshes_liveness_and_is_not_buffered() {
        use tokio::sync::mpsc;

        let graph = Arc::new(RwLock::new(CausalGraph::new()));
        let mut gossip = GossipProtocol::new(
            node(1),
            GossipConfig {
                partition_threshold_ms: 100,
                ..Default::default()
            },
            graph,
        );

        let (tx, rx) = mpsc::channel(10);
        gossip.network_rx = Some(rx);

        // A known peer, silent long enough to trip partition detection.
        let peer = PeerId::random();
        gossip
            .last_seen
            .insert(peer, Instant::now() - std::time::Duration::from_millis(500));
        assert!(gossip.detect_partition());

        // The peer's heartbeat arrives.
        let mut payload = Vec::with_capacity(40);
        payload.extend_from_slice(&node(2));
        payload.extend_from_slice(&0u64.to_le_bytes());
        tx.send(NetworkEvent::GossipReceived {
            topic: HEARTBEAT_TOPIC.to_string(),
            data: payload,
            propagation_source: peer,
        })
        .await
        .expect("send should succeed");
        drop(tx);

        gossip.process_pending_events().await.expect("process should succeed");

        assert!(!gossip.detect_partition(), "heartbeat must refresh last_seen");
        assert_eq!(gossip.connected_peer_count(), 1);
        assert!(
            gossip.take_aux_messages().is_empty(),
            "heartbeats must not be buffered as aux messages"
        );
    }

    /// A due heartbeat publishes `node_id ‖ unix_millis` on the
    /// heartbeat topic through the network command channel.
    #[tokio::test]
    async fn test_maybe_send_heartbeat_publishes_when_due() {
        use tokio::sync::mpsc;

        let graph = Arc::new(RwLock::new(CausalGraph::new()));
        let mut gossip = GossipProtocol::new(
            node(1),
            GossipConfig {
                heartbeat_interval_ms: 1,
                ..Default::default()
            },
            graph,
        );
        let (cmd_tx, mut cmd_rx) = mpsc::channel(4);
        gossip.network_cmd_tx = Some(cmd_tx);
        gossip.connected_peers.insert(PeerId::random());

        // Let the 1 ms interval elapse so a heartbeat is due.
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        gossip.maybe_send_heartbeat().await;

        match cmd_rx.try_recv() {
            Ok(NetworkCommand::Publish { topic, data }) => {
                assert_eq!(topic, HEARTBEAT_TOPIC);
                assert_eq!(data.len(), 40, "payload = node_id (32) + unix millis (8)");
                assert_eq!(&data[..32], &node(1)[..]);
            }
            other => panic!("expected heartbeat Publish command, got {other:?}"),
        }
    }

    /// Heartbeats are suppressed with no connected peers (publishing to
    /// an empty mesh only produces warnings) and when disabled via
    /// `heartbeat_interval_ms == 0`.
    #[tokio::test]
    async fn test_maybe_send_heartbeat_gating() {
        use tokio::sync::mpsc;

        // No connected peers → no heartbeat.
        let graph = Arc::new(RwLock::new(CausalGraph::new()));
        let mut gossip = GossipProtocol::new(
            node(1),
            GossipConfig {
                heartbeat_interval_ms: 1,
                ..Default::default()
            },
            graph,
        );
        let (cmd_tx, mut cmd_rx) = mpsc::channel(4);
        gossip.network_cmd_tx = Some(cmd_tx);
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        gossip.maybe_send_heartbeat().await;
        assert!(cmd_rx.try_recv().is_err(), "no heartbeat without connected peers");

        // Disabled (interval 0) → no heartbeat even with connected peers.
        let graph = Arc::new(RwLock::new(CausalGraph::new()));
        let mut gossip = GossipProtocol::new(
            node(1),
            GossipConfig {
                heartbeat_interval_ms: 0,
                ..Default::default()
            },
            graph,
        );
        let (cmd_tx, mut cmd_rx) = mpsc::channel(4);
        gossip.network_cmd_tx = Some(cmd_tx);
        gossip.connected_peers.insert(PeerId::random());
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        gossip.maybe_send_heartbeat().await;
        assert!(
            cmd_rx.try_recv().is_err(),
            "heartbeat_interval_ms = 0 must disable heartbeats"
        );
    }

    // ── AUDIT-14: idle component integration tests ─────────────────────

    /// The dedup path must suppress a duplicate delivery: same event sent
    /// twice through the network channel is inserted exactly once and
    /// counted as received exactly once.
    #[tokio::test]
    async fn test_duplicate_event_suppressed() {
        use tokio::sync::mpsc;

        let graph = Arc::new(RwLock::new(CausalGraph::new()));
        let mut gossip = GossipProtocol::new(node(1), GossipConfig::default(), graph);

        let (tx, rx) = mpsc::channel(10);
        gossip.network_rx = Some(rx);

        let keypair = generate_keypair();
        let mut event = Event::genesis(node(2), vec![1, 2, 3]).expect("valid genesis event");
        event.sign_with_keypair(&keypair).expect("signing");
        let bytes = event.to_bytes().expect("test event serialization");

        for _ in 0..2 {
            tx.send(NetworkEvent::GossipReceived {
                topic: "omnia_events".to_string(),
                data: bytes.clone(),
                propagation_source: PeerId::random(),
            })
            .await
            .expect("send should succeed");
        }
        drop(tx);

        let inserted = gossip.process_pending_events().await.expect("process should succeed");
        assert_eq!(inserted.len(), 1, "Duplicate must be suppressed");
        assert_eq!(gossip.stats().events_received, 1);
        assert_eq!(gossip.stats().events_accepted, 1);
    }

    /// A duplicate arriving in a LATER processing round (after the first
    /// copy is already in the graph) must be suppressed via the
    /// bloom-positive → graph-confirm path.
    #[tokio::test]
    async fn test_duplicate_across_rounds_suppressed() {
        use tokio::sync::mpsc;

        let graph = Arc::new(RwLock::new(CausalGraph::new()));
        let mut gossip = GossipProtocol::new(node(1), GossipConfig::default(), graph);

        let (tx, rx) = mpsc::channel(10);
        gossip.network_rx = Some(rx);

        let keypair = generate_keypair();
        let mut event = Event::genesis(node(2), vec![7]).expect("valid genesis event");
        event.sign_with_keypair(&keypair).expect("signing");
        let bytes = event.to_bytes().expect("test event serialization");

        let send = |data: Vec<u8>| {
            let tx = tx.clone();
            async move {
                tx.send(NetworkEvent::GossipReceived {
                    topic: "omnia_events".to_string(),
                    data,
                    propagation_source: PeerId::random(),
                })
                .await
                .expect("send should succeed");
            }
        };

        send(bytes.clone()).await;
        let first = gossip.process_pending_events().await.expect("first round");
        assert_eq!(first.len(), 1);

        send(bytes).await;
        let second = gossip.process_pending_events().await.expect("second round");
        assert_eq!(
            second.len(),
            0,
            "Retransmission of an inserted event must be suppressed"
        );
        assert_eq!(gossip.stats().events_received, 1);
    }

    /// A compact-encoded (wire version 2) event must be decoded, validated,
    /// and inserted exactly like a full-format event.
    #[tokio::test]
    async fn test_compact_wire_event_accepted() {
        use tokio::sync::mpsc;

        let graph = Arc::new(RwLock::new(CausalGraph::new()));
        let mut gossip = GossipProtocol::new(node(1), GossipConfig::default(), graph);

        let (tx, rx) = mpsc::channel(10);
        gossip.network_rx = Some(rx);

        let keypair = generate_keypair();
        let mut event = Event::genesis(node(3), vec![4, 5, 6]).expect("valid genesis event");
        event.sign_with_keypair(&keypair).expect("signing");
        let event_id = event.id;
        let bytes = encode_compact_wire(&event).expect("compact encoding");
        assert!(is_compact_wire(&bytes));

        tx.send(NetworkEvent::GossipReceived {
            topic: "omnia_events".to_string(),
            data: bytes,
            propagation_source: PeerId::random(),
        })
        .await
        .expect("send should succeed");
        drop(tx);

        let inserted = gossip.process_pending_events().await.expect("process should succeed");
        assert_eq!(inserted.len(), 1);
        assert_eq!(gossip.stats().events_accepted, 1);
        assert_eq!(gossip.stats().events_rejected, 0);

        let g = gossip.graph().read().await;
        assert!(g.contains(&event_id));
    }

    /// Merge events (other_parent set) classify as High priority; regular
    /// events classify as Normal.
    #[test]
    fn test_classify_priority() {
        let keypair = generate_keypair();
        let mut regular = Event::genesis(node(2), vec![1]).expect("valid genesis event");
        regular.sign_with_keypair(&keypair).expect("signing");
        assert_eq!(GossipProtocol::classify_priority(&regular), GossipPriority::Normal);

        let mut clock = VectorClock::new();
        clock.set(node(2), 2);
        let mut merge = Event::new(node(2), 1, clock, Some([1u8; 32]), Some([2u8; 32]), vec![1]).expect("valid event");
        merge.sign_with_keypair(&keypair).expect("signing");
        assert_eq!(GossipProtocol::classify_priority(&merge), GossipPriority::High);
    }

    /// The dedup filter must stay memory-bounded: its estimated size is a
    /// function of the configured window, not of how many events pass by.
    #[test]
    fn test_seen_filter_memory_bounded() {
        let graph = Arc::new(RwLock::new(CausalGraph::new()));
        let protocol = GossipProtocol::new(node(1), GossipConfig::default(), graph);

        // ~1.44 MiB * items/1e5 for both filters at 0.1% FPR — well under
        // the ~4.4 MiB the old exact HashMap needed for the same window.
        assert!(
            protocol.seen_filter.estimated_size() < 1_000_000,
            "bloom filter pair should be well under 1 MB, got {}",
            protocol.seen_filter.estimated_size()
        );
        assert_eq!(protocol.seen_filter.expected_items(), MAX_SEEN_EVENTS);
    }
}
