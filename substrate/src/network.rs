//! Real P2P networking layer using libp2p
//!
//! Provides QUIC transport, GossipSub for event propagation, mDNS for peer discovery,
//! and request-response for sync operations. Uses tokio::sync primitives exclusively
//! — never std::sync::Mutex across await points.

use libp2p::{
    identity, PeerId, Swarm, SwarmBuilder,
    gossipsub::{self, IdentTopic, MessageAuthenticity, ValidationMode},
    mdns::{tokio::Behaviour as MdnsBehaviour, Config as MdnsConfig},
    request_response::{self, ProtocolSupport, ResponseChannel},
    Multiaddr, StreamProtocol,
};
use tokio::sync::mpsc;
use std::collections::HashMap;
use std::io;

/// Events emitted by the network layer.
#[derive(Debug)]
pub enum NetworkEvent {
    PeerConnected(PeerId),
    PeerDisconnected(PeerId),
    MessageReceived(PeerId, Vec<u8>),
    GossipReceived { topic: String, data: Vec<u8>, propagation_source: PeerId },
}

/// Combined libp2p behaviour for Omnia.
#[derive(libp2p::swarm::NetworkBehaviour)]
pub struct OmniaBehaviour {
    pub gossipsub: gossipsub::Behaviour,
    pub mdns: MdnsBehaviour,
    pub req_res: request_response::cbor::Behaviour<Vec<u8>, Vec<u8>>,
}

/// The Omnia P2P network handle.
pub struct OmniaNetwork {
    swarm: Swarm<OmniaBehaviour>,
    local_peer_id: PeerId,
    event_tx: mpsc::Sender<NetworkEvent>,
    pub event_rx: mpsc::Receiver<NetworkEvent>,
    known_peers: HashMap<PeerId, Multiaddr>,
}

impl OmniaNetwork {
    /// Create a new network instance listening on the given address.
    pub async fn new(listen_addr: Multiaddr) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let local_key = identity::Keypair::generate_ed25519();
        let local_peer_id = PeerId::from(local_key.public());

        // QUIC transport
        let transport = libp2p::quic::tokio::Transport::new(libp2p::quic::Config::default(), local_key.clone())
            .map(|(peer_id, conn), _| (peer_id, libp2p::core::muxing::StreamMuxerBox::new(conn)))
            .boxed();

        // GossipSub configuration
        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .validation_mode(ValidationMode::Strict)
            .message_id_fn(|msg| {
                let hash = blake3::hash(&msg.data);
                gossipsub::MessageId::from(hash.as_bytes())
            })
            .build()?;

        let gossipsub = gossipsub::Behaviour::new(
            MessageAuthenticity::Signed(local_key.clone()),
            gossipsub_config,
        )?;

        let mdns = MdnsBehaviour::new(MdnsConfig::default(), local_peer_id)?;

        let req_res = request_response::cbor::Behaviour::new(
            [(StreamProtocol::new("/omnia/1.0.0"), ProtocolSupport::Full)],
            request_response::Config::default(),
        );

        let behaviour = OmniaBehaviour {
            gossipsub,
            mdns,
            req_res,
        };

        let mut swarm = SwarmBuilder::with_tokio_executor(transport, behaviour, local_peer_id).build();
        swarm.listen_on(listen_addr)?;

        let (event_tx, event_rx) = mpsc::channel(1000);

        Ok(Self {
            swarm,
            local_peer_id,
            event_tx,
            event_rx,
            known_peers: HashMap::new(),
        })
    }

    pub fn local_peer_id(&self) -> PeerId {
        self.local_peer_id
    }

    pub fn dial(&mut self, peer_id: PeerId, addr: Multiaddr) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let p2p_addr = addr.with(libp2p::multiaddr::Protocol::P2p(peer_id));
        self.swarm.dial(p2p_addr)?;
        Ok(())
    }

    pub fn subscribe(&mut self, topic: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let topic = IdentTopic::new(topic);
        self.swarm.behaviour_mut().gossipsub.subscribe(&topic)?;
        Ok(())
    }

    pub fn publish(&mut self, topic: &str, data: Vec<u8>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let topic = IdentTopic::new(topic);
        self.swarm.behaviour_mut().gossipsub.publish(topic, data)?;
        Ok(())
    }

    /// Run the network event loop. This should be spawned on a tokio task.
    pub async fn run(&mut self) {
        loop {
            tokio::select! {
                event = self.swarm.select_next_some() => {
                    self.handle_swarm_event(event).await;
                }
            }
        }
    }

    async fn handle_swarm_event(
        &mut self,
        event: libp2p::swarm::SwarmEvent<OmniaBehaviourEvent>,
    ) {
        use libp2p::swarm::SwarmEvent;
        match event {
            SwarmEvent::Behaviour(OmniaBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                propagation_source,
                message,
                ..
            })) => {
                let _ = self.event_tx.send(NetworkEvent::GossipReceived {
                    topic: message.topic.into_string(),
                    data: message.data,
                    propagation_source,
                }).await;
            }
            SwarmEvent::NewListenAddr { address, .. } => {
                tracing::info!("Listening on {}", address);
            }
            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                let _ = self.event_tx.send(NetworkEvent::PeerConnected(peer_id)).await;
            }
            SwarmEvent::ConnectionClosed { peer_id, .. } => {
                let _ = self.event_tx.send(NetworkEvent::PeerDisconnected(peer_id)).await;
            }
            _ => {}
        }
    }
}
