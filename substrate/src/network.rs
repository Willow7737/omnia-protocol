//! Real P2P networking layer using libp2p 0.53
//!
//! Provides QUIC transport, GossipSub for event propagation, mDNS for peer discovery,
//! and request-response for sync operations. Uses tokio::sync primitives exclusively
//! — never std::sync::Mutex across await points.

use libp2p::{
    gossipsub::{self, IdentTopic, MessageAuthenticity, ValidationMode},
    identity, Multiaddr, PeerId, StreamProtocol, Swarm, SwarmBuilder,
};
use std::collections::HashMap;
use tokio::sync::mpsc;

/// Events emitted by the network layer.
#[derive(Debug)]
pub enum NetworkEvent {
    PeerConnected(PeerId),
    PeerDisconnected(PeerId),
    MessageReceived(PeerId, Vec<u8>),
    GossipReceived {
        topic: String,
        data: Vec<u8>,
        propagation_source: PeerId,
    },
}

/// Combined libp2p behaviour for Omnia.
#[derive(libp2p::swarm::NetworkBehaviour)]
pub struct OmniaBehaviour {
    pub gossipsub: gossipsub::Behaviour,
    pub mdns: libp2p::mdns::tokio::Behaviour,
    pub req_res: libp2p::request_response::cbor::Behaviour<Vec<u8>, Vec<u8>>,
}

/// Command sent from external callers to the network event loop.
#[derive(Debug)]
pub enum NetworkCommand {
    /// Publish data to a gossipsub topic.
    Publish { topic: String, data: Vec<u8> },
    /// Subscribe to a gossipsub topic.
    Subscribe { topic: String },
}

/// The Omnia P2P network handle.
#[allow(dead_code)]
pub struct OmniaNetwork {
    swarm: Swarm<OmniaBehaviour>,
    local_peer_id: PeerId,
    event_tx: mpsc::Sender<NetworkEvent>,
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
                StreamProtocol::new("/omnia/1.0.0"),
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

    pub fn local_peer_id(&self) -> PeerId {
        self.local_peer_id
    }

    pub fn dial(
        &mut self,
        peer_id: PeerId,
        addr: Multiaddr,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let p2p_addr = addr.with(libp2p::multiaddr::Protocol::P2p(peer_id));
        self.swarm.dial(p2p_addr)?;
        Ok(())
    }

    pub fn subscribe(&mut self, topic: &str) -> Result<bool, gossipsub::SubscriptionError> {
        let topic = IdentTopic::new(topic);
        self.swarm.behaviour_mut().gossipsub.subscribe(&topic)
    }

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
    pub async fn run_with_commands(
        &mut self,
        mut cmd_rx: mpsc::Receiver<NetworkCommand>,
    ) {
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
