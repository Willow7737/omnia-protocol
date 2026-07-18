//! Real P2P networking layer using libp2p 0.56
//!
//! Provides QUIC transport, GossipSub for event propagation, mDNS for local
//! peer discovery, Kademlia DHT for wide-area peer discovery, and
//! request-response for sync operations. Uses tokio::sync primitives
//! exclusively — never std::sync::Mutex across await points.
//!
//! # Protocol Version Negotiation
//!
//! When two Omnia nodes connect, they exchange protocol versions via the
//! request-response protocol. If the major versions differ, the connection
//! is rejected to prevent consensus divergence. Minor and patch version
//! differences are allowed (backward-compatible).
//!
//! # Kademlia DHT (H-4)
//!
//! Wide-area peer discovery via Kademlia DHT with configurable bootstrap
//! peers and periodic routing table maintenance. See [`NetworkConfig`] for
//! NAT traversal options (AutoNAT, relay, DCutr).
//!
//! # GossipSub Peer Scoring (H-5)
//!
//! Custom peer scoring tuned for Omnia's threat model: a heavy penalty for
//! invalid messages plus rewards for first-delivery and time-in-mesh. The
//! mesh-message-deliveries deficit penalty is intentionally disabled (it
//! collapses low-traffic meshes — see [`configure_gossipsub_scoring()`]).
//! See also [`PeerScoreTracker`].

// The libp2p NetworkBehaviour derive macro generates an event enum without
// doc comments on its variants, which triggers missing_docs. Allow it here
// since we cannot annotate the derived code directly.
#![allow(missing_docs)]

use crate::blake3_domain::blake3_hash_domain;
use crate::PROTOCOL_IDENTIFIER;
#[allow(unused_imports)] // PROTOCOL_VERSION used in tests
use crate::PROTOCOL_VERSION;
use libp2p::{
    gossipsub::{
        self, IdentTopic, MessageAuthenticity, PeerScoreParams, PeerScoreThresholds, TopicScoreParams, ValidationMode,
    },
    identity, Multiaddr, PeerId, StreamProtocol, Swarm, SwarmBuilder,
};
use std::collections::HashMap;
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// NetworkConfig — H-4: NAT traversal & Kademlia configuration
// ---------------------------------------------------------------------------

/// Configuration for the network layer with NAT traversal support.
///
/// Controls Kademlia DHT bootstrap peers, relay servers for NAT
/// traversal, and transport fallback options.
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    /// Ed25519 secret-key bytes for a persistent swarm identity.
    ///
    /// When `Some`, the node's libp2p `PeerId` is derived from this key and
    /// survives restarts, so bootstrap multiaddrs pinned with `/p2p/<PeerId>`
    /// stay valid. When `None` a fresh identity is generated on every start
    /// (the previous behaviour), which invalidates pinned addresses on each
    /// restart.
    pub identity: Option<[u8; 32]>,
    /// Multiaddresses of bootstrap/seed peers for initial DHT population.
    pub bootstrap_peers: Vec<Multiaddr>,
    /// Relay server multiaddresses for NAT traversal.
    pub relay_servers: Vec<Multiaddr>,
    /// Kademlia DHT protocol name.
    pub dht_protocol: String,
    /// Enable AutoNAT probing for NAT detection.
    pub enable_autonat: bool,
    /// Enable relay client for NAT traversal.
    pub enable_relay: bool,
    /// Enable DCutr for direct connection upgrade after relay.
    pub enable_dcutr: bool,
    /// Enable TCP transport fallback alongside QUIC.
    pub enable_tcp_fallback: bool,
    /// Listen addresses for the swarm.
    pub listen_addresses: Vec<Multiaddr>,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            identity: None,
            bootstrap_peers: Vec::new(),
            relay_servers: Vec::new(),
            dht_protocol: "/omnia/kad/1.0.0".to_string(),
            enable_autonat: true,
            enable_relay: true,
            enable_dcutr: true,
            enable_tcp_fallback: true,
            listen_addresses: vec!["/ip4/0.0.0.0/udp/0/quic-v1".parse().expect("valid QUIC listen address")],
        }
    }
}

// ---------------------------------------------------------------------------
// PeerScoreTracker — H-5: application-specific peer scoring
// ---------------------------------------------------------------------------

/// Track and update peer scores based on message validation results.
///
/// This is an application-level score tracker that complements the
/// built-in GossipSub peer scoring. Scores can be fed back into the
/// GossipSub behaviour via `set_application_score()`.
#[derive(Debug)]
pub struct PeerScoreTracker {
    scores: HashMap<PeerId, f64>,
}

impl PeerScoreTracker {
    /// Create a new empty peer score tracker.
    pub fn new() -> Self {
        Self { scores: HashMap::new() }
    }

    /// Maximum number of peer scores to retain before pruning.
    const MAX_PEER_SCORES: usize = 10_000;

    /// Record a validation result for a peer's message.
    ///
    /// Valid messages add +1.0 to the peer's score; invalid messages
    /// subtract 10.0.
    pub fn record_validation(&mut self, peer: &PeerId, is_valid: bool) {
        let score = self.scores.entry(*peer).or_insert(0.0);
        if is_valid {
            *score += 1.0;
        } else {
            *score -= 10.0;
        }
        self.prune_if_needed();
    }

    /// Update a peer's score directly.
    ///
    /// Also prunes lowest-scored peers when the tracker exceeds
    /// `MAX_PEER_SCORES` entries.
    pub fn update_score(&mut self, peer_id: &PeerId, score: f64) {
        // Guard against NaN poisoning: NaN values break all comparisons
        // (NaN != NaN, NaN < x is false, NaN > x is false), which would
        // corrupt sorting, graylisting, and pruning logic.
        if score.is_nan() {
            tracing::warn!(
                peer = ?peer_id,
                "Rejecting NaN score for peer — treating as 0.0"
            );
            self.scores.insert(*peer_id, 0.0);
        } else {
            self.scores.insert(*peer_id, score);
        }
        self.prune_if_needed();
    }

    /// Prune lowest-scored peers when the tracker exceeds MAX_PEER_SCORES.
    fn prune_if_needed(&mut self) {
        if self.scores.len() > Self::MAX_PEER_SCORES {
            let mut entries: Vec<_> = self.scores.iter().collect();
            entries.sort_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal));
            let to_remove: Vec<_> = entries
                .iter()
                .take(entries.len() - Self::MAX_PEER_SCORES / 2)
                .map(|(peer, _)| **peer)
                .collect();
            for peer in to_remove {
                self.scores.remove(&peer);
            }
        }
    }

    /// Get a peer's application-specific score.
    pub fn get_score(&self, peer: &PeerId) -> f64 {
        *self.scores.get(peer).unwrap_or(&0.0)
    }

    /// Check if a peer should be graylisted.
    ///
    /// A peer is graylisted when its score falls below -100.0.
    pub fn is_graylisted(&self, peer: &PeerId) -> bool {
        self.get_score(peer) < -100.0
    }

    /// Remove a peer's score entry entirely.
    ///
    /// Call this from the libp2p `SwarmEvent::ConnectionClosed` handler so
    /// that disconnected peers don't accumulate in the score map forever.
    /// H-9 fix (audit v0.1.68): previously, peer scores were never cleaned
    /// up, leading to unbounded memory growth for nodes that cycled through
    /// many transient peers (e.g., mobile clients, NAT'd nodes).
    pub fn remove_peer(&mut self, peer: &PeerId) {
        if self.scores.remove(peer).is_some() {
            tracing::debug!(peer = ?peer, "Removed peer score on disconnect");
        }
    }

    /// Periodically clean up stale peer score entries.
    ///
    /// Call this on a fixed interval (e.g., every 10 minutes) to drop
    /// scores for peers that haven't been seen recently. This is a
    /// defense-in-depth measure alongside [`Self::remove_peer`] — it catches
    /// any peers that disconnected without firing `ConnectionClosed`
    /// (e.g., due to a network partition where we never received the
    /// close event).
    ///
    /// H-9 fix (audit v0.1.68).
    pub fn cleanup_stale(&mut self, max_age: std::time::Duration, last_seen: &HashMap<PeerId, std::time::Instant>) {
        let cutoff = std::time::Instant::now()
            .checked_sub(max_age)
            .unwrap_or_else(std::time::Instant::now);
        let stale_peers: Vec<_> = last_seen
            .iter()
            .filter(|(_, seen)| **seen < cutoff)
            .map(|(peer, _)| *peer)
            .collect();
        for peer in &stale_peers {
            self.scores.remove(peer);
        }
        if !stale_peers.is_empty() {
            tracing::debug!(count = stale_peers.len(), "Cleaned up stale peer scores");
        }
    }
}

impl Default for PeerScoreTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// GossipSub scoring configuration — H-5
// ---------------------------------------------------------------------------

/// Configure GossipSub peer scoring for Omnia's threat model.
///
/// Returns `(PeerScoreParams, PeerScoreThresholds)` tuned for:
/// - Heavy penalty for invalid messages (-150 per invalid delivery)
/// - Heavy penalty for mesh delivery failure (-50)
/// - Rewards for first-message deliveries (+1 per delivery)
/// - Graylisting at score -100
pub fn configure_gossipsub_scoring() -> (PeerScoreParams, PeerScoreThresholds) {
    let topic_params = TopicScoreParams {
        topic_weight: 1.0,
        time_in_mesh_weight: 0.01,
        time_in_mesh_quantum: std::time::Duration::from_secs(1),
        time_in_mesh_cap: 3600.0,
        first_message_deliveries_weight: 1.0,
        first_message_deliveries_decay: 0.99,
        first_message_deliveries_cap: 100.0,
        // Mesh-message-deliveries penalty DISABLED (weight 0).
        //
        // This penalty assumes every mesh peer delivers at least
        // `mesh_message_deliveries_threshold` messages per window. On a low-
        // or bursty-traffic topic no honest peer meets that bar, so once the
        // 30s activation elapses every mesh peer is scored
        // `weight * deficit^2` (with the old -50 weight and a full deficit of
        // 10 that is -5000), driven far below `graylist_threshold` (-100), and
        // pruned across all topics — collapsing the mesh ~30s after it forms
        // and never recovering. libp2p documents this parameter as only safe
        // with reliable, high message rates; Omnia's event/heartbeat topics
        // are neither, so it is left off. Anti-spam is retained via the
        // invalid-message penalty below and per-event signature validation.
        //
        // libp2p skips validation of the companion fields when the weight is
        // 0, so the remaining values here are inert.
        mesh_message_deliveries_weight: 0.0,
        mesh_message_deliveries_decay: 0.99,
        mesh_message_deliveries_threshold: 10.0,
        mesh_message_deliveries_cap: 100.0,
        mesh_message_deliveries_window: std::time::Duration::from_millis(100),
        mesh_message_deliveries_activation: std::time::Duration::from_secs(30),
        // Mesh-failure penalty also disabled: it is derived from the same
        // deliveries tracking and would likewise punish quiet honest peers.
        mesh_failure_penalty_weight: 0.0,
        mesh_failure_penalty_decay: 0.99,
        invalid_message_deliveries_weight: -150.0,
        invalid_message_deliveries_decay: 0.999,
    };

    let score_params = PeerScoreParams {
        topics: HashMap::from([
            (IdentTopic::new("omnia_events").hash(), topic_params.clone()),
            (IdentTopic::new("omnia_consensus").hash(), topic_params),
        ]),
        app_specific_weight: 10.0,
        ..Default::default()
    };

    let thresholds = PeerScoreThresholds {
        gossip_threshold: -10.0,
        publish_threshold: -50.0,
        graylist_threshold: -100.0,
        accept_px_threshold: 10.0,
        opportunistic_graft_threshold: 5.0,
    };

    (score_params, thresholds)
}

// ---------------------------------------------------------------------------
// Existing types
// ---------------------------------------------------------------------------

/// Handshake message exchanged during protocol version negotiation.
///
/// When two peers first connect, each sends a `VersionHandshake` message
/// containing their protocol version. If the versions are incompatible,
/// the connection is gracefully terminated.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VersionHandshake {
    /// The protocol version of the sending node (semver string).
    pub protocol_version: String,
    /// The protocol identifier (e.g., "/omnia/1.0.0").
    pub protocol_identifier: String,
    /// The node's identifier in the network.
    pub node_id: [u8; 32],
}

/// Result of a version compatibility check between two nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionCompatibility {
    /// The versions are fully compatible.
    Compatible,
    /// The versions are compatible with a minor difference (newer peer has features).
    MinorDifference {
        /// The local protocol version.
        local: String,
        /// The remote protocol version.
        remote: String,
    },
    /// The versions are incompatible (different major versions).
    Incompatible {
        /// The local protocol version.
        local: String,
        /// The remote protocol version.
        remote: String,
    },
}

/// Check version compatibility between two protocol version strings.
///
/// Parses both versions as semver and compares major versions.
/// - Same major version: `Compatible` or `MinorDifference`
/// - Different major version: `Incompatible`
///
/// # Example
///
/// ```
/// use omnia_network::network::check_version_compatibility;
///
/// let result = check_version_compatibility("4.0.0", "4.1.0");
/// assert!(matches!(result, _compatible if !matches!(result, omnia_network::network::VersionCompatibility::Incompatible { .. })));
///
/// let result = check_version_compatibility("4.0.0", "3.0.0");
/// assert!(matches!(result, omnia_network::network::VersionCompatibility::Incompatible { .. }));
/// ```
pub fn check_version_compatibility(local: &str, remote: &str) -> VersionCompatibility {
    let local_parts: Vec<&str> = local.split('.').collect();
    let remote_parts: Vec<&str> = remote.split('.').collect();

    let local_major = local_parts.first().and_then(|s| s.parse::<u32>().ok());
    let remote_major = remote_parts.first().and_then(|s| s.parse::<u32>().ok());

    // If either version string cannot be parsed, treat as incompatible.
    // A malformed version string (e.g., "not-a-version") indicates a
    // protocol violation and should never be treated as compatible.
    let (local_major, remote_major) = match (local_major, remote_major) {
        (Some(l), Some(r)) => (l, r),
        _ => {
            return VersionCompatibility::Incompatible {
                local: local.to_string(),
                remote: remote.to_string(),
            };
        }
    };

    if local_major != remote_major {
        VersionCompatibility::Incompatible {
            local: local.to_string(),
            remote: remote.to_string(),
        }
    } else if local == remote {
        VersionCompatibility::Compatible
    } else {
        VersionCompatibility::MinorDifference {
            local: local.to_string(),
            remote: remote.to_string(),
        }
    }
}

/// Events emitted by the network layer.
#[derive(Debug)]
pub enum NetworkEvent {
    /// A peer has connected
    PeerConnected(PeerId),
    /// A peer has disconnected
    PeerDisconnected(PeerId),
    /// A direct message was received from a peer
    MessageReceived(PeerId, Vec<u8>),
    /// A gossipsub message was received
    GossipReceived {
        /// The topic the message was published to
        topic: String,
        /// The message payload
        data: Vec<u8>,
        /// The peer that propagated the message
        propagation_source: PeerId,
    },
}

/// Combined libp2p behaviour for Omnia.
///
/// Includes GossipSub for event propagation, mDNS for LAN discovery,
/// request-response for sync operations, Kademlia DHT for
/// wide-area peer discovery, AutoNAT for NAT detection, relay client
/// for NAT traversal, and DCutr for direct connection upgrades.
#[allow(missing_docs)]
#[derive(libp2p::swarm::NetworkBehaviour)]
pub struct OmniaBehaviour {
    /// GossipSub event propagation behaviour
    pub gossipsub: gossipsub::Behaviour,
    /// mDNS peer discovery behaviour
    pub mdns: libp2p::mdns::tokio::Behaviour,
    /// Request-response sync behaviour
    pub req_res: libp2p::request_response::cbor::Behaviour<Vec<u8>, Vec<u8>>,
    /// Kademlia DHT for wide-area peer discovery
    pub kademlia: libp2p::kad::Behaviour<libp2p::kad::store::MemoryStore>,
    /// Identify — exchanges listen addresses and supported protocols so
    /// Kademlia can populate its routing table (without it Kademlia has no
    /// known peers and DHT discovery never bootstraps).
    pub identify: libp2p::identify::Behaviour,
    /// AutoNAT for NAT type detection
    pub autonat: libp2p::autonat::Behaviour,
    /// Relay client for NAT traversal
    pub relay_client: libp2p::relay::client::Behaviour,
    /// DCutr for direct connection upgrade after relay
    pub dcutr: libp2p::dcutr::Behaviour,
}

/// Command sent from external callers to the network event loop.
#[derive(Debug)]
pub enum NetworkCommand {
    /// Publish data to a gossipsub topic.
    Publish {
        /// Topic to publish to
        topic: String,
        /// Data payload
        data: Vec<u8>,
    },
    /// Subscribe to a gossipsub topic.
    Subscribe {
        /// Topic to subscribe to
        topic: String,
    },
    /// Dial an address without a known peer ID (e.g. a bootstrap
    /// multiaddr lacking a `/p2p` component). The remote identity is
    /// learned during the connection handshake.
    DialAddress {
        /// Multiaddress to dial
        addr: Multiaddr,
    },
    /// Dial a specific peer at the given address.
    Dial {
        /// Peer to dial
        peer_id: PeerId,
        /// Address to dial
        addr: Multiaddr,
    },
}

/// The Omnia P2P network handle.
#[allow(dead_code)]
pub struct OmniaNetwork {
    swarm: Swarm<OmniaBehaviour>,
    local_peer_id: PeerId,
    event_tx: mpsc::Sender<NetworkEvent>,
    /// Network event receiver
    pub event_rx: Option<mpsc::Receiver<NetworkEvent>>,
    known_peers: HashMap<PeerId, Multiaddr>,
    /// Application-level peer score tracker (H-5).
    pub peer_score_tracker: PeerScoreTracker,
    /// Interval for periodic Kademlia bootstrap (H-4).
    kademlia_bootstrap_interval: tokio::time::Interval,
}

impl OmniaNetwork {
    /// Create a new network instance listening on the given address with
    /// default [`NetworkConfig`].
    pub async fn new(listen_addr: Multiaddr) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Self::with_config(listen_addr, NetworkConfig::default()).await
    }

    /// Create a new network instance with the given listen address and
    /// [`NetworkConfig`].
    ///
    /// This initialises all behaviours (GossipSub, mDNS, request-response,
    /// Kademlia DHT, AutoNAT, relay client, DCutr) and applies custom
    /// GossipSub peer scoring.
    pub async fn with_config(
        listen_addr: Multiaddr,
        config: NetworkConfig,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let local_key = match config.identity {
            Some(secret) => identity::Keypair::ed25519_from_bytes(secret)
                .map_err(|e| format!("invalid persistent identity key: {e}"))?,
            None => identity::Keypair::generate_ed25519(),
        };
        let local_peer_id = PeerId::from(local_key.public());

        // ── GossipSub config ─────────────────────────────────────────
        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .validation_mode(ValidationMode::Strict)
            .message_id_fn(|msg: &gossipsub::Message| {
                let hash = blake3_hash_domain(b"omnia-commitment", &msg.data);
                gossipsub::MessageId::from(hash.to_vec())
            })
            .build()?;

        // ── Kademlia DHT config (H-4) ────────────────────────────────
        let stream_protocol = StreamProtocol::try_from_owned(config.dht_protocol.clone())
            .map_err(|e| format!("Invalid DHT protocol name: {e}"))?;
        let kademlia_config = libp2p::kad::Config::new(stream_protocol);

        // ── AutoNAT config ───────────────────────────────────────────
        let autonat_config = libp2p::autonat::Config {
            timeout: std::time::Duration::from_secs(30),
            ..Default::default()
        };

        // ── Bootstrap peers (for Kademlia) ───────────────────────────
        let bootstrap_peers = config.bootstrap_peers.clone();

        // Build the swarm with relay client support.
        // The relay client behaviour is created by `with_relay_client()` and then
        // passed into the behaviour constructor, so that DCutr can reference it.
        let mut swarm = SwarmBuilder::with_existing_identity(local_key)
            .with_tokio()
            .with_quic()
            // Wrap the transport in a DNS resolver so `/dns4/…` and
            // `/dnsaddr/…` bootstrap multiaddrs resolve. Without this, the
            // stock docker-compose testnet (whose peers dial the bootstrap by
            // service name, e.g. `/dns4/omnia-bootstrap/udp/4001/quic-v1`)
            // can never connect — the dial fails to resolve the hostname, so
            // no gossip mesh forms and Kademlia reports `NoKnownPeers`.
            .with_dns()?
            .with_relay_client(libp2p::noise::Config::new, libp2p::yamux::Config::default)?
            .with_behaviour(move |key, relay_client| {
                let local_pid = PeerId::from(key.public());

                // GossipSub with custom peer scoring
                let mut gossipsub =
                    gossipsub::Behaviour::new(MessageAuthenticity::Signed(key.clone()), gossipsub_config)?;
                let (score_params, thresholds) = configure_gossipsub_scoring();
                gossipsub.with_peer_score(score_params, thresholds)?;

                // mDNS for LAN discovery
                let mdns = libp2p::mdns::tokio::Behaviour::new(libp2p::mdns::Config::default(), local_pid)?;

                // Request-response for sync operations
                let req_res = libp2p::request_response::cbor::Behaviour::new(
                    [(
                        StreamProtocol::new(PROTOCOL_IDENTIFIER),
                        libp2p::request_response::ProtocolSupport::Full,
                    )],
                    libp2p::request_response::Config::default(),
                );

                // Kademlia DHT for wide-area peer discovery
                let store = libp2p::kad::store::MemoryStore::new(local_pid);
                let mut kademlia = libp2p::kad::Behaviour::with_config(local_pid, store, kademlia_config);
                // Add bootstrap peers to the routing table
                for addr in &bootstrap_peers {
                    if let Some(peer_id) = extract_peer_id_from_multiaddr(addr) {
                        kademlia.add_address(&peer_id, addr.clone());
                    }
                }

                // Identify: advertise our protocols/addresses and learn peers'
                // so Kademlia can route. Uses the same libp2p keypair.
                let identify = libp2p::identify::Behaviour::new(
                    libp2p::identify::Config::new("/omnia/id/1.0.0".to_string(), key.public())
                        .with_agent_version(format!("omnia-node/{}", env!("CARGO_PKG_VERSION"))),
                );

                // AutoNAT for NAT detection
                let autonat = libp2p::autonat::Behaviour::new(local_pid, autonat_config);

                // DCutr for direct connection upgrade after relay
                let dcutr = libp2p::dcutr::Behaviour::new(local_pid);

                Ok(OmniaBehaviour {
                    gossipsub,
                    mdns,
                    req_res,
                    kademlia,
                    identify,
                    autonat,
                    relay_client,
                    dcutr,
                })
            })?
            .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(std::time::Duration::from_secs(60)))
            .build();

        swarm.listen_on(listen_addr)?;

        // Also listen on any additional configured addresses
        for addr in &config.listen_addresses {
            if swarm.listen_on(addr.clone()).is_ok() {
                tracing::info!("Listening on additional address: {}", addr);
            }
        }

        let (event_tx, event_rx) = mpsc::channel(10_000);

        // Periodic Kademlia bootstrap every 5 minutes
        let kademlia_bootstrap_interval = tokio::time::interval(std::time::Duration::from_secs(300));

        Ok(Self {
            swarm,
            local_peer_id,
            event_tx,
            event_rx: Some(event_rx),
            known_peers: HashMap::new(),
            peer_score_tracker: PeerScoreTracker::new(),
            kademlia_bootstrap_interval,
        })
    }

    /// Get the local peer ID
    pub fn local_peer_id(&self) -> PeerId {
        self.local_peer_id
    }

    /// Dial an address whose peer ID is not yet known. The remote
    /// identity is verified during the transport handshake and learned
    /// via the identify/kademlia behaviours.
    pub fn dial_addr(&mut self, addr: Multiaddr) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.swarm.dial(addr)?;
        Ok(())
    }

    /// Dial a peer at the given address
    pub fn dial(&mut self, peer_id: PeerId, addr: Multiaddr) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let p2p_addr = addr.with(libp2p::multiaddr::Protocol::P2p(peer_id));
        self.swarm.dial(p2p_addr)?;
        Ok(())
    }

    /// Subscribe to a gossipsub topic
    pub fn subscribe(&mut self, topic: &str) -> Result<bool, gossipsub::SubscriptionError> {
        let topic = IdentTopic::new(topic);
        self.swarm.behaviour_mut().gossipsub.subscribe(&topic)
    }

    /// Publish data to a gossipsub topic
    pub fn publish(&mut self, topic: &str, data: Vec<u8>) -> Result<(), gossipsub::PublishError> {
        let topic = IdentTopic::new(topic);
        self.swarm.behaviour_mut().gossipsub.publish(topic, data)?;
        Ok(())
    }

    /// Run the network event loop. This should be spawned on a tokio task.
    ///
    /// The `shutdown` channel allows graceful termination: when the sender
    /// side is dropped or sends `true`, the loop exits cleanly.
    pub async fn run(mut self, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        use futures::StreamExt;
        loop {
            tokio::select! {
                _ = shutdown.changed() => {
                    tracing::info!("Network event loop shutting down");
                    break;
                }
                event = self.swarm.select_next_some() => {
                    self.handle_swarm_event(event).await;
                }
                _ = self.kademlia_bootstrap_interval.tick() => {
                    // H-4: Periodic bootstrap for routing table maintenance
                    if let Err(e) = self.swarm.behaviour_mut().kademlia.bootstrap() {
                        tracing::warn!("Kademlia bootstrap failed: {:?}", e);
                    }
                }
            }
        }
    }

    /// Run the network event loop with a command channel.
    ///
    /// The network layer only: runs the libp2p swarm, sends NetworkEvents
    /// to `event_tx` (for external consumers like GossipProtocol), and
    /// receives NetworkCommands from `cmd_rx` to publish/subscribe.
    ///
    /// Event consumption (graph insertion + consensus) is handled by
    /// GossipProtocol::process_pending_events(), not here.
    pub async fn run_with_commands(&mut self, mut cmd_rx: mpsc::Receiver<NetworkCommand>) {
        use futures::StreamExt;
        loop {
            tokio::select! {
                event = self.swarm.select_next_some() => {
                    self.handle_swarm_event(event).await;
                }
                // H-4: Periodic Kademlia bootstrap
                _ = self.kademlia_bootstrap_interval.tick() => {
                    if let Err(e) = self.swarm.behaviour_mut().kademlia.bootstrap() {
                        tracing::warn!("Kademlia bootstrap failed: {:?}", e);
                    }
                }
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(NetworkCommand::Publish { topic, data }) => {
                            if let Err(e) = self.publish(&topic, data) {
                                tracing::warn!("Publish failed: {:?}", e);
                            }
                        }
                        Some(NetworkCommand::Subscribe { topic }) => {
                            if let Err(e) = self.subscribe(&topic) {
                                tracing::warn!("Subscribe failed: {:?}", e);
                            }
                        }
                        Some(NetworkCommand::DialAddress { addr }) => {
                            if let Err(e) = self.dial_addr(addr) {
                                tracing::warn!("Dial (no peer ID) failed: {:?}", e);
                            }
                        }
                        Some(NetworkCommand::Dial { peer_id, addr }) => {
                            if let Err(e) = self.dial(peer_id, addr) {
                                tracing::warn!("Dial failed: {:?}", e);
                            }
                        }
                        None => {
                            // Command channel closed — shut down
                            return;
                        }
                    }
                }
            }
        }
    }

    async fn handle_swarm_event(&mut self, event: libp2p::swarm::SwarmEvent<OmniaBehaviourEvent>) {
        use libp2p::swarm::SwarmEvent;
        match event {
            SwarmEvent::Behaviour(OmniaBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                propagation_source,
                message,
                ..
            })) => {
                if let Err(e) = self
                    .event_tx
                    .send(NetworkEvent::GossipReceived {
                        topic: message.topic.to_string(),
                        data: message.data,
                        propagation_source,
                    })
                    .await
                {
                    tracing::warn!("Dropped gossip event - channel full: {}", e);
                }
            }
            // H-4: Kademlia DHT event handling
            SwarmEvent::Behaviour(OmniaBehaviourEvent::Kademlia(event)) => {
                match event {
                    libp2p::kad::Event::RoutingUpdated {
                        peer,
                        addresses,
                        is_new_peer,
                        ..
                    } => {
                        if is_new_peer {
                            tracing::info!("Kademlia routing updated: new peer {}", peer);
                        }
                        // Track known addresses for this peer
                        for addr in addresses.iter() {
                            self.known_peers.insert(peer, addr.clone());
                        }
                    }
                    libp2p::kad::Event::OutboundQueryProgressed { result, .. } => {
                        // Log DHT query results for monitoring
                        match result {
                            libp2p::kad::QueryResult::GetClosestPeers(Ok(ok)) => {
                                tracing::debug!("Kademlia closest peers query completed: {} peers", ok.peers.len());
                            }
                            libp2p::kad::QueryResult::Bootstrap(Ok(ok)) => {
                                tracing::debug!("Kademlia bootstrap completed, remaining: {}", ok.num_remaining);
                            }
                            libp2p::kad::QueryResult::GetClosestPeers(Err(e)) => {
                                tracing::warn!("Kademlia closest peers query failed: {:?}", e);
                            }
                            libp2p::kad::QueryResult::Bootstrap(Err(e)) => {
                                tracing::warn!("Kademlia bootstrap query failed: {:?}", e);
                            }
                            _ => {}
                        }
                    }
                    libp2p::kad::Event::RoutablePeer { peer, address } => {
                        tracing::debug!("Kademlia routable peer: {} at {}", peer, address);
                        self.known_peers.insert(peer, address);
                    }
                    libp2p::kad::Event::UnroutablePeer { peer } => {
                        tracing::debug!("Kademlia unroutable peer: {}", peer);
                    }
                    _ => {}
                }
            }
            // Identify event handling — feed a peer's advertised listen
            // addresses into Kademlia's routing table. Without this the
            // routing table stays empty (`bootstrap()` → NoKnownPeers) and
            // peers never discover each other beyond the initial dial, so the
            // gossip mesh cannot grow past a star around the bootstrap.
            SwarmEvent::Behaviour(OmniaBehaviourEvent::Identify(libp2p::identify::Event::Received {
                peer_id,
                info,
                ..
            })) => {
                for addr in info.listen_addrs {
                    self.swarm.behaviour_mut().kademlia.add_address(&peer_id, addr.clone());
                    self.known_peers.insert(peer_id, addr);
                }
                tracing::debug!(peer = %peer_id, "Identify: added peer addresses to Kademlia");
            }
            // AutoNAT event handling — log NAT status changes
            SwarmEvent::Behaviour(OmniaBehaviourEvent::Autonat(libp2p::autonat::Event::StatusChanged { old, new })) => {
                tracing::info!("AutoNAT status changed: {:?} -> {:?}", old, new);
            }
            // DCutr event handling — log direct connection upgrades
            SwarmEvent::Behaviour(OmniaBehaviourEvent::Dcutr(event)) => {
                tracing::debug!("DCutr event: {:?}", event);
            }
            SwarmEvent::NewListenAddr { address, .. } => {
                tracing::info!("Listening on {}", address);
            }
            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                if let Err(e) = self.event_tx.send(NetworkEvent::PeerConnected(peer_id)).await {
                    tracing::warn!("Dropped peer connected event - channel full: {}", e);
                }
            }
            SwarmEvent::ConnectionClosed { peer_id, .. } => {
                // H-9 fix (audit v0.1.68): clean up the peer's application
                // score on disconnect so the score map doesn't grow
                // unboundedly as peers churn.
                self.peer_score_tracker.remove_peer(&peer_id);
                if let Err(e) = self.event_tx.send(NetworkEvent::PeerDisconnected(peer_id)).await {
                    tracing::warn!("Dropped peer disconnected event - channel full: {}", e);
                }
            }
            _ => {}
        }
    }
}

/// Try to extract a PeerId from a Multiaddr that ends with `/p2p/<peer-id>`.
pub(crate) fn extract_peer_id_from_multiaddr(addr: &Multiaddr) -> Option<PeerId> {
    use libp2p::multiaddr::Protocol;
    addr.iter().find_map(|proto| match proto {
        Protocol::P2p(peer_id) => Some(peer_id),
        _ => None,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_version_compatibility_same_version() {
        let result = check_version_compatibility("4.0.0", "4.0.0");
        assert_eq!(result, VersionCompatibility::Compatible);
    }

    #[test]
    fn test_version_compatibility_minor_difference() {
        let result = check_version_compatibility("4.0.0", "4.1.0");
        assert_eq!(
            result,
            VersionCompatibility::MinorDifference {
                local: "4.0.0".to_string(),
                remote: "4.1.0".to_string(),
            }
        );
    }

    #[test]
    fn test_version_compatibility_patch_difference() {
        let result = check_version_compatibility("4.0.0", "4.0.1");
        assert_eq!(
            result,
            VersionCompatibility::MinorDifference {
                local: "4.0.0".to_string(),
                remote: "4.0.1".to_string(),
            }
        );
    }

    #[test]
    fn test_version_compatibility_major_incompatible() {
        let result = check_version_compatibility("4.0.0", "3.0.0");
        assert_eq!(
            result,
            VersionCompatibility::Incompatible {
                local: "4.0.0".to_string(),
                remote: "3.0.0".to_string(),
            }
        );
    }

    #[test]
    fn test_version_compatibility_major_5_vs_4() {
        let result = check_version_compatibility("5.0.0", "4.0.0");
        assert!(matches!(result, VersionCompatibility::Incompatible { .. }));
    }

    #[test]
    fn test_version_compatibility_malformed_local() {
        let result = check_version_compatibility("not-a-version", "4.0.0");
        // Malformed version has major = 0 (from parse failure)
        assert!(matches!(result, VersionCompatibility::Incompatible { .. }));
    }

    #[test]
    fn test_version_compatibility_malformed_remote() {
        let result = check_version_compatibility("4.0.0", "bad");
        assert!(matches!(result, VersionCompatibility::Incompatible { .. }));
    }

    #[test]
    fn test_version_handshake_creation() {
        let handshake = VersionHandshake {
            protocol_version: PROTOCOL_VERSION.to_string(),
            protocol_identifier: PROTOCOL_IDENTIFIER.to_string(),
            node_id: [0u8; 32],
        };
        assert_eq!(handshake.protocol_version, "4.0.0");
        assert_eq!(handshake.protocol_identifier, "/omnia/4.0.0");
    }

    #[test]
    fn test_version_handshake_serialization() {
        let handshake = VersionHandshake {
            protocol_version: "4.0.0".to_string(),
            protocol_identifier: "/omnia/4.0.0".to_string(),
            node_id: [42u8; 32],
        };
        let json = serde_json::to_string(&handshake).expect("serialize");
        let restored: VersionHandshake = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.protocol_version, "4.0.0");
        assert_eq!(restored.protocol_identifier, "/omnia/4.0.0");
        assert_eq!(restored.node_id, [42u8; 32]);
    }

    #[test]
    fn test_protocol_identifier_constant() {
        assert_eq!(PROTOCOL_IDENTIFIER, "/omnia/4.0.0");
    }

    #[test]
    fn test_protocol_version_constant() {
        assert_eq!(PROTOCOL_VERSION, "4.0.0");
    }

    // ── H-4: NetworkConfig tests ─────────────────────────────────────

    #[test]
    fn test_network_config_defaults() {
        let config = NetworkConfig::default();
        assert!(config.bootstrap_peers.is_empty());
        assert!(config.relay_servers.is_empty());
        assert_eq!(config.dht_protocol, "/omnia/kad/1.0.0");
        assert!(config.enable_autonat);
        assert!(config.enable_relay);
        assert!(config.enable_dcutr);
        assert!(config.enable_tcp_fallback);
        assert!(!config.listen_addresses.is_empty());
    }

    #[tokio::test]
    async fn test_persistent_identity_yields_stable_peer_id() {
        // Two networks built from the same secret must present the same
        // PeerId — this is what lets operators pin `/p2p/<PeerId>` in
        // bootstrap multiaddrs across restarts. A `None` identity must
        // still produce a fresh (random) PeerId.
        let secret = [7u8; 32];
        let expected = PeerId::from(
            identity::Keypair::ed25519_from_bytes(secret)
                .expect("valid secret")
                .public(),
        );

        for _ in 0..2 {
            let config = NetworkConfig {
                identity: Some(secret),
                ..Default::default()
            };
            let net =
                OmniaNetwork::with_config("/ip4/127.0.0.1/udp/0/quic-v1".parse().expect("valid multiaddr"), config)
                    .await
                    .expect("network construction");
            assert_eq!(
                net.local_peer_id(),
                expected,
                "PeerId must be derived from the persistent key"
            );
        }

        let ephemeral = OmniaNetwork::with_config(
            "/ip4/127.0.0.1/udp/0/quic-v1".parse().expect("valid multiaddr"),
            NetworkConfig::default(),
        )
        .await
        .expect("network construction");
        assert_ne!(
            ephemeral.local_peer_id(),
            expected,
            "no identity configured → random PeerId"
        );
    }

    #[test]
    fn test_network_config_custom_bootstrap_peers() {
        // Use a PeerId to generate a valid multiaddr for testing
        let peer_id = PeerId::random();
        let addr: Multiaddr = format!("/ip4/1.2.3.4/udp/4001/quic-v1/p2p/{peer_id}")
            .parse()
            .expect("valid multiaddr");
        let config = NetworkConfig {
            bootstrap_peers: vec![addr.clone()],
            ..Default::default()
        };
        assert_eq!(config.bootstrap_peers.len(), 1);
        assert_eq!(config.bootstrap_peers[0], addr);
    }

    #[test]
    fn test_kademlia_protocol_name() {
        let protocol = StreamProtocol::try_from_owned("/omnia/kad/1.0.0".to_string()).expect("valid protocol name");
        assert_eq!(protocol.to_string(), "/omnia/kad/1.0.0");
    }

    // ── H-5: Peer scoring tests ──────────────────────────────────────

    #[test]
    fn test_peer_scoring_penalizes_invalid_messages() {
        let mut tracker = PeerScoreTracker::new();
        let peer = PeerId::random();

        tracker.record_validation(&peer, false);
        assert!(tracker.get_score(&peer) < 0.0);

        // Multiple invalid messages push toward graylist (need 11 to go below -100)
        for _ in 0..10 {
            tracker.record_validation(&peer, false);
        }
        // Total: 11 invalid = -110, which is < -100
        assert!(tracker.is_graylisted(&peer));
    }

    #[test]
    fn test_peer_scoring_rewards_valid_messages() {
        let mut tracker = PeerScoreTracker::new();
        let peer = PeerId::random();

        tracker.record_validation(&peer, true);
        assert!(tracker.get_score(&peer) > 0.0);
    }

    #[test]
    fn test_graylist_threshold() {
        let mut tracker = PeerScoreTracker::new();
        let peer = PeerId::random();

        // Score at -90 should not be graylisted (9 * -10 = -90, not < -100)
        for _ in 0..9 {
            tracker.record_validation(&peer, false);
        }
        assert!(!tracker.is_graylisted(&peer));

        // One more invalid message → -100 → still not < -100, need one more
        tracker.record_validation(&peer, false);
        assert!(!tracker.is_graylisted(&peer));

        // One more → -110 → graylisted
        tracker.record_validation(&peer, false);
        assert!(tracker.is_graylisted(&peer));
    }

    #[test]
    fn test_peer_score_tracker_default() {
        let tracker = PeerScoreTracker::default();
        let peer = PeerId::random();
        assert_eq!(tracker.get_score(&peer), 0.0);
        assert!(!tracker.is_graylisted(&peer));
    }

    #[test]
    fn test_configure_gossipsub_scoring_params() {
        let (params, thresholds) = configure_gossipsub_scoring();

        // Check that topic scoring is configured for both Omnia topics
        assert!(params.topics.contains_key(&IdentTopic::new("omnia_events").hash()));
        assert!(params.topics.contains_key(&IdentTopic::new("omnia_consensus").hash()));
        assert_eq!(params.app_specific_weight, 10.0);

        // Check thresholds
        assert_eq!(thresholds.gossip_threshold, -10.0);
        assert_eq!(thresholds.publish_threshold, -50.0);
        assert_eq!(thresholds.graylist_threshold, -100.0);
        assert_eq!(thresholds.accept_px_threshold, 10.0);
        assert_eq!(thresholds.opportunistic_graft_threshold, 5.0);
    }

    #[test]
    fn test_configure_gossipsub_scoring_validates() {
        let (params, thresholds) = configure_gossipsub_scoring();
        assert!(params.validate().is_ok(), "PeerScoreParams should validate");
        assert!(thresholds.validate().is_ok(), "PeerScoreThresholds should validate");
    }

    #[test]
    fn test_extract_peer_id_from_multiaddr() {
        // Multiaddr with /p2p suffix — use a generated PeerId for a valid address
        let peer_id = PeerId::random();
        let addr: Multiaddr = format!("/ip4/1.2.3.4/udp/4001/quic-v1/p2p/{peer_id}")
            .parse()
            .expect("valid multiaddr");
        let extracted = extract_peer_id_from_multiaddr(&addr);
        assert!(extracted.is_some());
        assert_eq!(extracted.unwrap(), peer_id);

        // Multiaddr without /p2p suffix
        let addr_no_p2p: Multiaddr = "/ip4/1.2.3.4/udp/4001/quic-v1".parse().expect("valid multiaddr");
        assert!(extract_peer_id_from_multiaddr(&addr_no_p2p).is_none());
    }
}
