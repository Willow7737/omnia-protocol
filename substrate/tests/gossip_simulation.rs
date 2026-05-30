#![allow(clippy::unwrap_used)]
#![allow(deprecated)]
//! Real Integration Test: Multi-Node Substrate Network
//!
//! Spins up N Substrate instances in-memory, submits signed events, and verifies:
//! 1. All events reach all nodes' CausalGraphs
//! 2. Consensus commits events (finality)
//! 3. CRDT state converges
//!
//! FIX(bug-4): Rewrites the old hand-rolled SimulatedNetwork test that
//! never called Substrate, CausalGraph::insert(), GossipProtocol, or
//! ConsensusEngine. This version uses real Substrate instances.

use omnia_substrate::*;
use std::sync::Arc;
use tokio::sync::RwLock;

fn test_node(id: u8) -> NodeId {
    let mut node = [0u8; 32];
    node[0] = id;
    node
}

/// In-memory test network that shares events between Substrate instances.
/// Simulates gossip propagation by copying events from a shared graph
/// into each node's local graph.
struct TestNetwork {
    nodes: Vec<Arc<RwLock<Substrate>>>,
    shared_graph: Arc<RwLock<CausalGraph>>,
}

impl TestNetwork {
    async fn new(node_count: usize) -> Self {
        let shared_graph = Arc::new(RwLock::new(CausalGraph::new()));
        let mut nodes = Vec::new();

        for i in 0..node_count {
            let node_id = test_node(i as u8 + 1);
            let config = SubstrateConfig::with_network_size(node_id, node_count);
            let substrate = Substrate::new(config);
            nodes.push(Arc::new(RwLock::new(substrate)));
        }

        Self { nodes, shared_graph }
    }

    /// Submit an event to a specific node AND to the shared graph.
    async fn submit_event(&self, node_idx: usize, event: &Event) {
        let mut node = self.nodes[node_idx].write().await;
        node.submit_event(event.clone()).await.unwrap();

        // Also insert into shared graph for cross-node propagation
        let mut graph = self.shared_graph.write().await;
        let _ = graph.insert(event.clone());
    }

    /// Propagate all events from the shared graph to all nodes.
    /// This simulates gossip: after propagation, every node's CausalGraph
    /// contains every event. Events that are already present are skipped
    /// (CausalGraph returns DuplicateEvent, which we ignore).
    async fn propagate_all(&self) {
        let shared = self.shared_graph.read().await;
        let all_ids: Vec<EventId> = shared.event_ids();

        for node_arc in &self.nodes {
            let mut node = node_arc.write().await;
            for id in &all_ids {
                if let Some(event) = shared.get(id) {
                    // Submit to each node — this validates, inserts into graph,
                    // and processes through consensus
                    let _ = node.submit_event(event.clone()).await;
                }
            }
        }
    }

    /// Process consensus on all nodes
    async fn process_consensus_all(&self) {
        for node_arc in &self.nodes {
            let mut node = node_arc.write().await;
            let _ = node.process_consensus().await;
        }
    }
}

/// Test: 3 nodes creating events, verify all events propagate via shared graph
#[tokio::test]
async fn test_three_node_event_propagation() {
    let network = TestNetwork::new(3).await;
    let keypair = generate_keypair();

    // Node 0 creates genesis event
    let mut event = Event::genesis(test_node(1), vec![1, 2, 3]).expect("valid genesis event");
    event.sign_with_keypair(&keypair);
    let event_id = event.id;

    network.submit_event(0, &event).await;

    // Propagate to all nodes
    network.propagate_all().await;

    // Verify the event is in the shared graph
    let shared = network.shared_graph.read().await;
    assert!(shared.contains(&event_id));
    drop(shared);

    // Verify all nodes have the event
    for (i, node_arc) in network.nodes.iter().enumerate() {
        let node = node_arc.read().await;
        let graph = node.graph().await;
        assert!(graph.contains(&event_id), "Node {i} should have the event");
    }
}

/// Test: 3 nodes each submit events, verify CRDT convergence via GCounter
#[test]
fn test_three_node_crdt_convergence() {
    let n1 = test_node(1);
    let n2 = test_node(2);
    let n3 = test_node(3);

    let mut counter_a = GCounter::new();
    let mut counter_b = GCounter::new();
    let mut counter_c = GCounter::new();

    // Each node increments independently
    for _ in 0..5 {
        counter_a.increment(n1, 1).unwrap();
    }
    for _ in 0..3 {
        counter_b.increment(n2, 1).unwrap();
    }
    for _ in 0..7 {
        counter_c.increment(n3, 1).unwrap();
    }

    // Merge all (simulates gossip propagation)
    counter_a.merge(&counter_b);
    counter_a.merge(&counter_c);
    counter_b.merge(&counter_a);
    counter_c.merge(&counter_a);

    let expected = 5 + 3 + 7;
    assert_eq!(counter_a.value(), expected);
    assert_eq!(counter_b.value(), expected);
    assert_eq!(counter_c.value(), expected);

    // State hashes converge
    assert_eq!(counter_a.state_hash(), counter_b.state_hash());
    assert_eq!(counter_b.state_hash(), counter_c.state_hash());
}

/// Test: 4-node consensus finality — genesis events should be committed
#[tokio::test]
async fn test_consensus_finality() {
    let network = TestNetwork::new(4).await;

    // Create and submit genesis events from all 4 nodes
    let mut genesis_ids = Vec::new();
    for i in 0..4 {
        let keypair = generate_keypair();
        let mut event = Event::genesis(test_node(i as u8 + 1), vec![i as u8]).expect("valid genesis event");
        event.sign_with_keypair(&keypair);
        genesis_ids.push(event.id);
        network.submit_event(i, &event).await;
    }

    // Propagate all events to all nodes (simulates gossip)
    network.propagate_all().await;

    // Process consensus on each node
    network.process_consensus_all().await;

    // Verify genesis events are finalized on all nodes
    for (i, node_arc) in network.nodes.iter().enumerate() {
        let node = node_arc.read().await;
        for id in &genesis_ids {
            assert!(
                node.is_finalized(id),
                "Node {}: genesis event {:?} not finalized",
                i,
                &id[..4]
            );
        }
    }
}

/// Test: Causal ordering is preserved across events
#[test]
fn test_causal_ordering_preserved() {
    let n1 = test_node(1);

    let mut vc = VectorClock::with_node(n1, 1);
    let e1 = Event::new(n1, 0, vc.clone(), None, None, vec![1]).expect("valid event");
    let e1_id = e1.id;

    vc.increment(n1).unwrap();
    let e2 = Event::new(n1, 1, vc.clone(), Some(e1_id), None, vec![2]).expect("valid event");
    let e2_id = e2.id;

    vc.increment(n1).unwrap();
    let e3 = Event::new(n1, 2, vc.clone(), Some(e2_id), None, vec![3]).expect("valid event");

    // Verify causal chain
    assert!(e1.vector_clock.happened_before(&e2.vector_clock));
    assert!(e2.vector_clock.happened_before(&e3.vector_clock));
    assert!(e1.vector_clock.happened_before(&e3.vector_clock));

    // No cycles
    assert!(!e2.vector_clock.happened_before(&e1.vector_clock));
    assert!(!e3.vector_clock.happened_before(&e2.vector_clock));
}

/// Test: Concurrent events are correctly identified
#[test]
fn test_concurrent_event_detection() {
    let n1 = test_node(1);
    let n2 = test_node(2);

    let vc1 = VectorClock::with_node(n1, 1);
    let e1 = Event::new(n1, 0, vc1.clone(), None, None, vec![1]).expect("valid event");

    let vc2 = VectorClock::with_node(n2, 1);
    let e2 = Event::new(n2, 0, vc2.clone(), None, None, vec![2]).expect("valid event");

    // Events from different nodes with no parent relation should be concurrent
    assert!(e1.vector_clock.concurrent(&e2.vector_clock));
    assert!(!e1.vector_clock.happened_before(&e2.vector_clock));
    assert!(!e2.vector_clock.happened_before(&e1.vector_clock));
}

/// Test: OR-Set convergence across nodes
#[test]
fn test_or_set_convergence() {
    let n1 = test_node(1);
    let n2 = test_node(2);

    let mut set_a: OrSet<String> = OrSet::new();
    let mut set_b: OrSet<String> = OrSet::new();

    set_a.add(n1, "apple".to_string());
    set_a.add(n1, "banana".to_string());

    set_b.add(n2, "cherry".to_string());
    set_b.add(n2, "apple".to_string());

    // Merge both directions
    set_a.merge(&set_b);
    set_b.merge(&set_a);

    let elements_a = set_a.elements();
    assert!(elements_a.contains(&"apple".to_string()));
    assert!(elements_a.contains(&"banana".to_string()));
    assert!(elements_a.contains(&"cherry".to_string()));

    // Remove then concurrent add
    set_a.remove(&"apple".to_string());
    assert!(!set_a.elements().contains(&"apple".to_string()));

    set_b.add(n2, "apple".to_string());
    set_a.merge(&set_b);
    // New add wins
    assert!(set_a.contains(&"apple".to_string()));
}

/// Test: 100% CRDT convergence across varied increment patterns
#[test]
fn test_crdt_100_percent_convergence() {
    let n1 = test_node(1);
    let n2 = test_node(2);
    let n3 = test_node(3);

    let test_cases = vec![
        (vec![10, 0, 0], "single-node-active"),
        (vec![5, 5, 5], "equal-increments"),
        (vec![100, 1, 1], "one-node-dominant"),
        (vec![0, 0, 50], "late-joiner"),
        (vec![33, 33, 34], "nearly-equal"),
    ];

    for (increments, desc) in test_cases {
        let mut c1 = GCounter::new();
        let mut c2 = GCounter::new();
        let mut c3 = GCounter::new();

        for _ in 0..increments[0] {
            c1.increment(n1, 1).unwrap();
        }
        for _ in 0..increments[1] {
            c2.increment(n2, 1).unwrap();
        }
        for _ in 0..increments[2] {
            c3.increment(n3, 1).unwrap();
        }

        // Full mesh merge
        let mut merged = c1.clone();
        merged.merge(&c2);
        merged.merge(&c3);

        let expected: u64 = increments.iter().sum::<usize>() as u64;

        // Verify different merge orders converge
        let mut alt1 = c2.clone();
        alt1.merge(&c1);
        alt1.merge(&c3);

        let mut alt2 = c3.clone();
        alt2.merge(&c2);
        alt2.merge(&c1);

        assert_eq!(merged.value(), expected, "Main merge failed for {desc}");
        assert_eq!(alt1.value(), expected, "Alt1 merge failed for {desc}");
        assert_eq!(alt2.value(), expected, "Alt2 merge failed for {desc}");
        assert_eq!(merged.state_hash(), alt1.state_hash(), "Hash mismatch for {desc}");
        assert_eq!(alt1.state_hash(), alt2.state_hash(), "Hash mismatch for {desc}");
    }
}

/// Test: Network-received events flow through consensus to finality.
///
/// Verifies that when events arrive via the simulated network,
/// they get inserted into the graph, processed through consensus,
/// and reach finality — the core fix for the p2p consensus gap.
#[tokio::test]
async fn test_network_event_processed_through_consensus() {
    let network = TestNetwork::new(4).await;

    // Create events from all 4 nodes as if they came from the network
    let mut event_ids = Vec::new();
    for i in 0..4 {
        let keypair = generate_keypair();
        let mut event = Event::genesis(test_node(i as u8 + 1), vec![i as u8 + 10]).expect("valid genesis event");
        event.sign_with_keypair(&keypair);
        event_ids.push(event.id);

        // Insert into shared graph (simulates network receive)
        {
            let mut shared = network.shared_graph.write().await;
            shared.insert(event.clone()).unwrap();
        }
    }

    // Propagate to all nodes (simulates gossip)
    network.propagate_all().await;

    // Process consensus on all nodes
    network.process_consensus_all().await;

    // Verify all nodes have all events
    for (i, node_arc) in network.nodes.iter().enumerate() {
        let node = node_arc.read().await;
        let graph = node.graph().await;
        for id in &event_ids {
            assert!(graph.contains(id), "Node {i} missing event");
        }
    }

    // Verify events are finalized (4 witnesses in round 0 => supermajority)
    let node0 = network.nodes[0].read().await;
    for id in &event_ids {
        assert!(node0.is_finalized(id), "Event {:?} should be finalized", &id[..4]);
    }
}

/// Test: process_pending_events() correctly drains network_rx into the
/// pending queue and inserts events into the graph.
#[tokio::test]
async fn test_process_pending_events_drains_network_rx() {
    use libp2p::PeerId;
    use tokio::sync::mpsc;

    let graph = Arc::new(RwLock::new(CausalGraph::new()));
    let mut gossip = GossipProtocol::new(test_node(1), GossipConfig::default(), graph);

    // Create a fake network receiver
    let (tx, rx) = mpsc::channel(10);
    gossip.network_rx = Some(rx);

    // Create and serialize an event
    let keypair = generate_keypair();
    let mut event = Event::genesis(test_node(2), vec![1, 2, 3]).expect("valid genesis event");
    event.sign_with_keypair(&keypair);
    let event_id = event.id;
    let bytes = event.to_bytes().expect("test event serialization");

    // Send it as a network event
    // Build a dummy PeerId from a public key
    let dummy_peer_id = PeerId::random();
    tx.send(NetworkEvent::GossipReceived {
        topic: "omnia_events".to_string(),
        data: bytes,
        propagation_source: dummy_peer_id,
    })
    .await
    .unwrap();

    // Drop tx so recv doesn't hang
    drop(tx);

    // Process pending events
    let inserted = gossip.process_pending_events().await.unwrap();
    assert_eq!(inserted.len(), 1, "Should process 1 network event");

    // Verify event is in graph
    let g = gossip.graph().read().await;
    assert!(g.contains(&event_id));

    // Verify stats
    assert_eq!(gossip.stats().events_received, 1);
    assert_eq!(gossip.stats().events_accepted, 1);
}
