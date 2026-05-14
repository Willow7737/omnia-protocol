//! Real Multi-Node libp2p Integration Tests
//!
//! These tests use REAL libp2p networking (QUIC transport, GossipSub protocol)
//! to verify multi-node event propagation. They are marked `#[ignore]` because
//! they require network access and are slow.
//!
//! Unlike `gossip_simulation.rs` which uses `Arc<RwLock<>>` shared memory
//! between nodes, these tests spin up real `libp2p::Swarm` instances with
//! real QUIC transports and real GossipSub protocol messages.
//!
//! Run with: `cargo test -p omnia-substrate -- --ignored`

use libp2p::{Multiaddr, PeerId};
use omnia_substrate::{
    generate_keypair, Event, NetworkCommand, NetworkEvent, OmniaNetwork, Substrate, SubstrateConfig,
};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;

/// Duration to wait for GossipSub mesh formation after nodes connect.
///
/// GossipSub requires at least one heartbeat interval (default ~1s) for
/// mesh peers to exchange GRAFT messages after subscribing. We wait
/// 3 seconds to provide a comfortable margin.
const MESH_FORMATION_DELAY: Duration = Duration::from_secs(3);

/// A running libp2p test node with channels for interaction.
///
/// Wraps the spawned network task and provides methods to publish
/// events and wait for incoming gossip messages. The network event loop
/// runs in a background tokio task and communicates through the command
/// channel (`cmd_tx`) and event receiver (`event_rx`).
#[allow(dead_code)]
struct TestNode {
    /// The libp2p PeerId of this node.
    peer_id: PeerId,
    /// The Substrate runtime for this node.
    substrate: Substrate,
    /// Command channel for sending publish/subscribe/dial commands
    /// to the background network task.
    cmd_tx: mpsc::Sender<NetworkCommand>,
    /// Receiver for network events emitted by the background task.
    event_rx: mpsc::Receiver<NetworkEvent>,
}

impl TestNode {
    /// Publish data to a gossipsub topic via the command channel.
    ///
    /// Sends a [`NetworkCommand::Publish`] to the background network task,
    /// which calls [`OmniaNetwork::publish()`] on the swarm.
    ///
    /// # Errors
    ///
    /// Returns an error string if the command channel is closed (i.e., the
    /// background network task has stopped).
    async fn publish(&self, topic: &str, data: Vec<u8>) -> Result<(), String> {
        self.cmd_tx
            .send(NetworkCommand::Publish {
                topic: topic.to_string(),
                data,
            })
            .await
            .map_err(|e| format!("Failed to send publish command: {}", e))
    }

    /// Wait for a [`NetworkEvent::GossipReceived`] event containing the
    /// specified data, within the given timeout duration.
    ///
    /// Skips any `PeerConnected`, `PeerDisconnected`, `MessageReceived`, or
    /// non-matching gossip events while waiting. Returns the propagation
    /// source [`PeerId`] on success.
    ///
    /// # Errors
    ///
    /// Returns an error string if the timeout expires or the event channel
    /// closes unexpectedly.
    async fn wait_for_gossip(
        &mut self,
        expected_data: &[u8],
        timeout_duration: Duration,
    ) -> Result<PeerId, String> {
        timeout(timeout_duration, async {
            loop {
                match self.event_rx.recv().await {
                    Some(NetworkEvent::GossipReceived {
                        data,
                        propagation_source,
                        ..
                    }) => {
                        if data == expected_data {
                            return Ok(propagation_source);
                        }
                        // Different gossip message — skip and keep waiting
                    }
                    Some(NetworkEvent::PeerConnected(_)) => {
                        // Peer connection event — skip
                    }
                    Some(NetworkEvent::PeerDisconnected(_)) => {
                        // Peer disconnection event — skip
                    }
                    Some(NetworkEvent::MessageReceived(_, _)) => {
                        // Direct message event — skip
                    }
                    None => {
                        return Err("Event channel closed unexpectedly".to_string());
                    }
                }
            }
        })
        .await
        .map_err(|_| "Timeout waiting for gossip message".to_string())?
    }

    /// Wait for any [`NetworkEvent::PeerConnected`] event, within the given
    /// timeout duration.
    ///
    /// Returns the connected [`PeerId`] on success.
    ///
    /// # Errors
    ///
    /// Returns an error string if the timeout expires or the event channel
    /// closes unexpectedly.
    #[allow(dead_code)]
    async fn wait_for_any_peer_connected(
        &mut self,
        timeout_duration: Duration,
    ) -> Result<PeerId, String> {
        timeout(timeout_duration, async {
            loop {
                match self.event_rx.recv().await {
                    Some(NetworkEvent::PeerConnected(peer_id)) => {
                        return Ok(peer_id);
                    }
                    Some(_) => {
                        // Skip non-connection events
                    }
                    None => {
                        return Err("Event channel closed unexpectedly".to_string());
                    }
                }
            }
        })
        .await
        .map_err(|_| "Timeout waiting for peer connection".to_string())?
    }

    /// Collect all currently buffered gossip events matching the expected data.
    ///
    /// Drains the event receiver non-blockingly and returns the number of
    /// matching gossip events found. Useful for checking that multiple
    /// events were received.
    ///
    /// # Errors
    ///
    /// Returns an error string if the event channel is closed.
    #[allow(dead_code)]
    fn drain_matching_gossip(&mut self, expected_data: &[u8]) -> Result<usize, String> {
        let mut count = 0;
        loop {
            match self.event_rx.try_recv() {
                Ok(NetworkEvent::GossipReceived { data, .. }) => {
                    if data == expected_data {
                        count += 1;
                    }
                }
                Ok(_) => {
                    // Skip non-gossip events
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    return Err("Event channel closed unexpectedly".to_string());
                }
            }
        }
        Ok(count)
    }
}

/// Spawn a libp2p node listening on the given port, optionally dialing
/// bootstrap peers.
///
/// Creates a real [`OmniaNetwork`] with QUIC transport and a fresh Ed25519
/// keypair, subscribes to the `"omnia_events"` gossipsub topic, dials any
/// provided bootstrap peers, and spawns the network event loop in a
/// background tokio task.
///
/// Also creates a [`Substrate`] instance whose [`NodeId`](omnia_substrate::NodeId)
/// is derived from the port number (for deterministic, collision-free node IDs
/// within a test).
///
/// Returns a [`TestNode`] handle with the peer ID, substrate, command channel,
/// and event receiver for interacting with the spawned node.
///
/// # Arguments
///
/// * `port` — UDP port for the QUIC listener (e.g., `7001`).
/// * `bootstrap_peers` — List of `(PeerId, Multiaddr)` tuples for bootstrap
///   peers to dial immediately after creation.
///
/// # Errors
///
/// Returns an error if the listen address is invalid, OmniaNetwork creation
/// fails, GossipSub subscription fails, or the event receiver is missing.
async fn spawn_node(
    port: u16,
    bootstrap_peers: Vec<(PeerId, Multiaddr)>,
) -> Result<TestNode, Box<dyn std::error::Error + Send + Sync>> {
    // Build the listen address for QUIC transport on localhost
    let listen_addr: Multiaddr = format!("/ip4/127.0.0.1/udp/{}/quic-v1", port)
        .parse()
        .map_err(|e| format!("Invalid listen address for port {}: {}", port, e))?;

    // Create OmniaNetwork — generates a fresh Ed25519 keypair internally
    let mut network = OmniaNetwork::new(listen_addr).await?;
    let peer_id = network.local_peer_id();

    // Subscribe to the gossipsub topic before spawning the event loop.
    // The actual SUBSCRIBE control message will be sent to peers when
    // the swarm starts polling and connections are established.
    network
        .subscribe("omnia_events")
        .map_err(|e| format!("Subscribe failed: {:?}", e))?;

    // Create command channel for external control of the network task
    let (cmd_tx, cmd_rx) = mpsc::channel(256);

    // Take the event receiver before moving OmniaNetwork into the spawned task.
    // After this, the network task will send events through event_tx, and
    // we consume them through event_rx.
    let event_rx = network
        .event_rx
        .take()
        .ok_or("event_rx already taken from OmniaNetwork")?;

    // Dial bootstrap peers (queued on the swarm; processed when the event
    // loop starts polling). We do NOT fail the whole spawn if a dial fails —
    // the bootstrap peer may not be listening yet.
    for (pid, addr) in bootstrap_peers {
        if let Err(e) = network.dial(pid, addr) {
            eprintln!("Warning: failed to dial bootstrap peer {}: {:?}", pid, e);
        }
    }

    // Spawn the network event loop in a background task.
    // The task owns OmniaNetwork and runs until cmd_rx is closed
    // (i.e., when TestNode and its cmd_tx are dropped).
    tokio::spawn(async move {
        network.run_with_commands(cmd_rx).await;
    });

    // Brief sleep to let the swarm start processing (listening, dialing, etc.)
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Create Substrate with a NodeId derived from the port number.
    // This ensures deterministic, collision-free node IDs within a test.
    let node_id = {
        let mut id = [0u8; 32];
        id[0] = (port & 0xFF) as u8;
        id[1] = ((port >> 8) & 0xFF) as u8;
        id
    };
    let config = SubstrateConfig::with_network_size(node_id, 4);
    let substrate = Substrate::new(config);

    Ok(TestNode {
        peer_id,
        substrate,
        cmd_tx,
        event_rx,
    })
}

/// Create a signed genesis event for testing.
///
/// Generates a fresh Ed25519 keypair, creates a genesis event with the
/// given payload, and signs it with the keypair. Returns the signed event.
///
/// # Arguments
///
/// * `creator_byte` — First byte of the creator NodeId (for identification).
/// * `payload` — Event payload bytes.
fn create_signed_event(creator_byte: u8, payload: Vec<u8>) -> Event {
    let keypair = generate_keypair();
    let mut node_id = [0u8; 32];
    node_id[0] = creator_byte;
    let mut event = Event::genesis(node_id, payload);
    event.sign_with_keypair(&keypair);
    event
}

/// Test: Events published by one node propagate to all connected nodes
/// via real GossipSub protocol over QUIC transport.
///
/// Spawns three nodes (A, B, C). B and C connect to A as bootstrap.
/// After the GossipSub mesh forms, A publishes a signed event.
/// B and C should both receive the event within 10 seconds.
///
/// # Network Topology
///
/// ```text
/// A (7001) ← B (7002)
///   ↖
///     C (7003)
/// ```
#[tokio::test]
#[ignore]
async fn event_propagation() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Spawn Node A on port 7001 (bootstrap node, no peers to dial)
    let node_a = spawn_node(7001, Vec::new()).await?;

    // Build the multiaddr for Node A that other nodes will dial
    let addr_a: Multiaddr = "/ip4/127.0.0.1/udp/7001/quic-v1".parse()?;

    // Spawn Node B on port 7002, dialing A as bootstrap
    let mut node_b = spawn_node(7002, vec![(node_a.peer_id, addr_a.clone())]).await?;

    // Spawn Node C on port 7003, dialing A as bootstrap
    let mut node_c = spawn_node(7003, vec![(node_a.peer_id, addr_a.clone())]).await?;

    // Wait for GossipSub mesh to form (requires heartbeat exchange)
    tokio::time::sleep(MESH_FORMATION_DELAY).await;

    // Create a signed event on Node A
    let event = create_signed_event(1, vec![1, 2, 3]);
    let event_id = event.id;
    let event_bytes = event.to_bytes();

    // Publish from Node A
    node_a.publish("omnia_events", event_bytes.clone()).await?;

    // Wait for B to receive the event (timeout 10s)
    let result_b = node_b
        .wait_for_gossip(&event_bytes, Duration::from_secs(10))
        .await;
    assert!(
        result_b.is_ok(),
        "Node B should receive the event from A: {:?}",
        result_b
    );

    // Wait for C to receive the event (timeout 10s)
    let result_c = node_c
        .wait_for_gossip(&event_bytes, Duration::from_secs(10))
        .await;
    assert!(
        result_c.is_ok(),
        "Node C should receive the event from A: {:?}",
        result_c
    );

    // Verify the event can be deserialized and its ID matches
    let received_event = Event::from_bytes(&event_bytes)?;
    assert_eq!(
        received_event.id, event_id,
        "Received event ID should match published event ID"
    );

    // Verify signature is valid
    assert!(
        received_event.verify_signature(),
        "Received event signature should be valid"
    );

    Ok(())
}

/// Test: A late-joining node can receive events after connecting to the
/// network via real GossipSub over QUIC.
///
/// Starts nodes A and B, exchanges events between them, then starts
/// node D connecting to A. After the mesh forms, A publishes a new event
/// which D should receive within 15 seconds.
///
/// This tests the "late join" scenario: a new node enters the network
/// after the mesh is already established and can participate in ongoing
/// gossip propagation.
///
/// # Network Topology
///
/// ```text
/// Phase 1: A (7011) ↔ B (7012)
/// Phase 2: A (7011) ↔ B (7012)
///          A (7011) ↔ D (7014)
/// ```
#[tokio::test]
#[ignore]
async fn late_join_sync() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Spawn Node A on port 7011
    let node_a = spawn_node(7011, Vec::new()).await?;
    let addr_a: Multiaddr = "/ip4/127.0.0.1/udp/7011/quic-v1".parse()?;

    // Spawn Node B on port 7012, dialing A
    let mut node_b = spawn_node(7012, vec![(node_a.peer_id, addr_a.clone())]).await?;

    // Wait for GossipSub mesh to form between A and B
    tokio::time::sleep(MESH_FORMATION_DELAY).await;

    // Create and publish the first event from A (before D joins)
    let event1 = create_signed_event(1, vec![42]);
    let event1_bytes = event1.to_bytes();

    node_a.publish("omnia_events", event1_bytes.clone()).await?;

    // Wait for B to receive the first event
    let result_b = node_b
        .wait_for_gossip(&event1_bytes, Duration::from_secs(5))
        .await;
    assert!(
        result_b.is_ok(),
        "Node B should receive the first event: {:?}",
        result_b
    );

    // --- D joins the network ---

    // Spawn Node D on port 7014, dialing A as bootstrap
    let mut node_d = spawn_node(7014, vec![(node_a.peer_id, addr_a.clone())]).await?;

    // Wait for D to join the GossipSub mesh
    tokio::time::sleep(MESH_FORMATION_DELAY).await;

    // Publish a new event from A that D should receive
    let event2 = create_signed_event(1, vec![99]);
    let event2_bytes = event2.to_bytes();
    let event2_id = event2.id;

    node_a.publish("omnia_events", event2_bytes.clone()).await?;

    // Wait for D to receive the event (timeout 15s)
    let result_d = node_d
        .wait_for_gossip(&event2_bytes, Duration::from_secs(15))
        .await;
    assert!(
        result_d.is_ok(),
        "Node D (late joiner) should receive the event from A: {:?}",
        result_d
    );

    // Verify the event can be deserialized and matches
    let received_event = Event::from_bytes(&event2_bytes)?;
    assert_eq!(
        received_event.id, event2_id,
        "Received event ID should match published event ID"
    );

    Ok(())
}

/// Test: Multiple events published in sequence are all delivered to
/// connected peers via real GossipSub over QUIC.
///
/// Spawns two nodes (A, B). A publishes three distinct signed events.
/// B should receive all three events within the timeout.
///
/// This tests that GossipSub reliably delivers multiple messages, not
/// just the first one.
///
/// # Network Topology
///
/// ```text
/// A (7021) ↔ B (7022)
/// ```
#[tokio::test]
#[ignore]
async fn multi_event_propagation() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Spawn Node A on port 7021
    let node_a = spawn_node(7021, Vec::new()).await?;
    let addr_a: Multiaddr = "/ip4/127.0.0.1/udp/7021/quic-v1".parse()?;

    // Spawn Node B on port 7022, dialing A
    let mut node_b = spawn_node(7022, vec![(node_a.peer_id, addr_a)]).await?;

    // Wait for GossipSub mesh to form
    tokio::time::sleep(MESH_FORMATION_DELAY).await;

    // Create and publish three distinct signed events from A
    let event1 = create_signed_event(1, vec![10, 20, 30]);
    let event1_bytes = event1.to_bytes();

    let event2 = create_signed_event(1, vec![40, 50, 60]);
    let event2_bytes = event2.to_bytes();

    let event3 = create_signed_event(1, vec![70, 80, 90]);
    let event3_bytes = event3.to_bytes();

    // Publish all three events from A
    node_a.publish("omnia_events", event1_bytes.clone()).await?;
    node_a.publish("omnia_events", event2_bytes.clone()).await?;
    node_a.publish("omnia_events", event3_bytes.clone()).await?;

    // Wait for B to receive all three events (timeout 10s each)
    let result1 = node_b
        .wait_for_gossip(&event1_bytes, Duration::from_secs(10))
        .await;
    assert!(
        result1.is_ok(),
        "Node B should receive event 1: {:?}",
        result1
    );

    let result2 = node_b
        .wait_for_gossip(&event2_bytes, Duration::from_secs(10))
        .await;
    assert!(
        result2.is_ok(),
        "Node B should receive event 2: {:?}",
        result2
    );

    let result3 = node_b
        .wait_for_gossip(&event3_bytes, Duration::from_secs(10))
        .await;
    assert!(
        result3.is_ok(),
        "Node B should receive event 3: {:?}",
        result3
    );

    // Verify all events deserialize correctly and have valid signatures
    for (i, bytes) in [&event1_bytes, &event2_bytes, &event3_bytes]
        .iter()
        .enumerate()
    {
        let event = Event::from_bytes(bytes)?;
        assert!(
            event.verify_signature(),
            "Event {} should have a valid signature",
            i + 1
        );
    }

    Ok(())
}
