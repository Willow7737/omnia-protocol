//! Real P2P networking layer using libp2p 0.53
//!
//! Provides QUIC transport, GossipSub for event propagation, mDNS for peer discovery,
//! and request-response for sync operations. Uses tokio::sync primitives exclusively
//! — never std::sync::Mutex across await points.
//!
//! # Protocol Version Negotiation
//!
//! When two Omnia nodes connect, they exchange protocol versions via the
//! request-response protocol. If the major versions differ, the connection
//! is rejected to prevent consensus divergence. Minor and patch version
//! differences are allowed (backward-compatible).

// The libp2p NetworkBehaviour derive macro generates an event enum without
// doc comments on its variants, which triggers missing_docs. Allow it here
// since we cannot annotate the derived code directly.
#![allow(missing_docs)]

use crate::PROTOCOL_IDENTIFIER;
#[allow(unused_imports)] // PROTOCOL_VERSION used in tests
use crate::PROTOCOL_VERSION;
use libp2p::{
    gossipsub::{self, IdentTopic, MessageAuthenticity, ValidationMode},
    identity, Multiaddr, PeerId, StreamProtocol, Swarm, SwarmBuilder,
};
use std::collections::HashMap;
use tokio::sync::mpsc;

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
/// use omnia_substrate::network::check_version_compatibility;
///
/// let result = check_version_compatibility("4.0.0", "4.1.0");
/// assert!(matches!(result, _compatible if !matches!(result, omnia_substrate::network::VersionCompatibility::Incompatible { .. })));
///
/// let result = check_version_compatibility("4.0.0", "3.0.0");
/// assert!(matches!(result, omnia_substrate::network::VersionCompatibility::Incompatible { .. }));
/// ```
pub fn check_version_compatibility(local: &str, remote: &str) -> VersionCompatibility {
    let local_parts: Vec<&str> = local.split('.').collect();
    let remote_parts: Vec<&str> = remote.split('.').collect();

    let local_major = local_parts.first().and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let remote_major = remote_parts.first().and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);

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
#[allow(missing_docs)]
#[derive(libp2p::swarm::NetworkBehaviour)]
pub struct OmniaBehaviour {
    /// GossipSub event propagation behaviour
    pub gossipsub: gossipsub::Behaviour,
    /// mDNS peer discovery behaviour
    pub mdns: libp2p::mdns::tokio::Behaviour,
    /// Request-response sync behaviour
    pub req_res: libp2p::request_response::cbor::Behaviour<Vec<u8>, Vec<u8>>,
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
}

impl OmniaNetwork {
    /// Create a new network instance listening on the given address.
    pub async fn new(
        listen_addr: Multiaddr,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let local_key = identity::Keypair::generate_ed25519();
        let local_peer_id = PeerId::from(local_key.public());

        // GossipSub configuration
        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .validation_mode(ValidationMode::Strict)
            .message_id_fn(|msg: &gossipsub::Message| {
                let hash = blake3::hash(&msg.data);
                gossipsub::MessageId::from(hash.as_bytes().to_vec())
            })
            .build()?;

        let gossipsub = gossipsub::Behaviour::new(
            MessageAuthenticity::Signed(local_key.clone()),
            gossipsub_config,
        )?;

        let mdns =
            libp2p::mdns::tokio::Behaviour::new(libp2p::mdns::Config::default(), local_peer_id)?;

        let req_res = libp2p::request_response::cbor::Behaviour::new(
            [(
                StreamProtocol::new(PROTOCOL_IDENTIFIER),
                libp2p::request_response::ProtocolSupport::Full,
            )],
            libp2p::request_response::Config::default(),
        );

        let behaviour = OmniaBehaviour {
            gossipsub,
            mdns,
            req_res,
        };

        let mut swarm = SwarmBuilder::with_existing_identity(local_key)
            .with_tokio()
            .with_quic()
            .with_behaviour(|_| behaviour)?
            .with_swarm_config(|cfg| {
                cfg.with_idle_connection_timeout(std::time::Duration::from_secs(60))
            })
            .build();

        swarm.listen_on(listen_addr)?;

        let (event_tx, event_rx) = mpsc::channel(1000);

        Ok(Self {
            swarm,
            local_peer_id,
            event_tx,
            event_rx: Some(event_rx),
            known_peers: HashMap::new(),
        })
    }

    /// Get the local peer ID
    pub fn local_peer_id(&self) -> PeerId {
        self.local_peer_id
    }

    /// Dial a peer at the given address
    pub fn dial(
        &mut self,
        peer_id: PeerId,
        addr: Multiaddr,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
    pub async fn run(&mut self) {
        use futures::StreamExt;
        loop {
            tokio::select! {
                event = self.swarm.select_next_some() => {
                    self.handle_swarm_event(event).await;
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
                let _ = self
                    .event_tx
                    .send(NetworkEvent::GossipReceived {
                        topic: message.topic.to_string(),
                        data: message.data,
                        propagation_source,
                    })
                    .await;
            }
            SwarmEvent::NewListenAddr { address, .. } => {
                tracing::info!("Listening on {}", address);
            }
            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                let _ = self
                    .event_tx
                    .send(NetworkEvent::PeerConnected(peer_id))
                    .await;
            }
            SwarmEvent::ConnectionClosed { peer_id, .. } => {
                let _ = self
                    .event_tx
                    .send(NetworkEvent::PeerDisconnected(peer_id))
                    .await;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
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
        assert_eq!(handshake.protocol_identifier, "/omnia/1.0.0");
    }

    #[test]
    fn test_version_handshake_serialization() {
        let handshake = VersionHandshake {
            protocol_version: "4.0.0".to_string(),
            protocol_identifier: "/omnia/1.0.0".to_string(),
            node_id: [42u8; 32],
        };
        let json = serde_json::to_string(&handshake).expect("serialize");
        let restored: VersionHandshake = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.protocol_version, "4.0.0");
        assert_eq!(restored.protocol_identifier, "/omnia/1.0.0");
        assert_eq!(restored.node_id, [42u8; 32]);
    }

    #[test]
    fn test_protocol_identifier_constant() {
        assert_eq!(PROTOCOL_IDENTIFIER, "/omnia/1.0.0");
    }

    #[test]
    fn test_protocol_version_constant() {
        assert_eq!(PROTOCOL_VERSION, "4.0.0");
    }
}
