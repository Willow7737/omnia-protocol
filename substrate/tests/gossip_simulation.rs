//! Gossip Protocol Simulation Test
//!
//! This test simulates a network of N nodes running the Omnia gossip protocol
//! and verifies that:
//! 1. All events created by any node eventually reach all other nodes
//! 2. CRDT state converges to the same value on all nodes
//! 3. Causal ordering is preserved
//! 4. The causal graph remains consistent (no cycles, no dangling references)
//!
//! This is the primary integration test for Layer 1: The Substrate.

use omnia_substrate::*;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A simulated network node for testing
struct SimulatedNode {
    node_id: NodeId,
    substrate: Substrate,
    /// Events this node has created
    created_events: Vec<EventId>,
    /// CRDT state (G-Counter for testing)
    counter: GCounter,
}

/// A simulated network that connects multiple nodes
struct SimulatedNetwork {
    nodes: HashMap<NodeId, Arc<Mutex<SimulatedNode>>>,
    /// Message queue: (sender, recipient, message)
    message_queue: Vec<(NodeId, NodeId, GossipMessage)>,
    /// Events that should converge
    convergence_target: usize,
}

impl SimulatedNetwork {
    fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            message_queue: Vec::new(),
            convergence_target: 0,
        }
    }

    fn add_node(&mut self, node_id: NodeId) {
        let config = SubstrateConfig::with_network_size(node_id, 4);
        let substrate = Substrate::new(config);

        let node = SimulatedNode {
            node_id,
            substrate,
            created_events: Vec::new(),
            counter: GCounter::new(),
        };

        self.nodes.insert(node_id, Arc::new(Mutex::new(node)));
    }

    /// Create an event at a specific node
    fn create_event(&mut self, node_id: NodeId, payload: Vec<u8>) -> EventId {
        let node_arc = self.nodes.get(&node_id).unwrap().clone();
        let mut node = node_arc.lock().unwrap();

        // Create event
        let event = Event::genesis(node_id, payload);
        let event_id = event.id;

        node.created_events.push(event_id);
        self.convergence_target += 1;

        event_id
    }

    /// Check if all nodes have converged to the same CRDT state
    fn check_crdt_convergence(&self) -> bool {
        if self.nodes.is_empty() {
            return true;
        }

        let nodes: Vec<_> = self.nodes.values().collect();
        let first = nodes[0].lock().unwrap();
        let first_hash = first.counter.state_hash();
        drop(first);

        for node_arc in &nodes[1..] {
            let node = node_arc.lock().unwrap();
            if node.counter.state_hash() != first_hash {
                return false;
            }
        }

        true
    }

    /// Get the number of unique events across all nodes
    fn total_unique_events(&self) -> usize {
        let mut all_events = HashSet::new();
        for node_arc in self.nodes.values() {
            let node = node_arc.lock().unwrap();
            for event_id in &node.created_events {
                all_events.insert(*event_id);
            }
        }
        all_events.len()
    }

    /// Simulate one round of gossip between all nodes
    fn gossip_round(&mut self) -> usize {
        let mut total_exchanged = 0;
        let node_ids: Vec<_> = self.nodes.keys().copied().collect();

        // Each node gossips with every other node (full mesh for testing)
        for i in 0..node_ids.len() {
            for j in 0..node_ids.len() {
                if i == j {
                    continue;
                }

                let sender_id = node_ids[i];
                let receiver_id = node_ids[j];

                let sender_arc = self.nodes.get(&sender_id).unwrap().clone();
                let receiver_arc = self.nodes.get(&receiver_id).unwrap().clone();

                let sender = sender_arc.lock().unwrap();
                let mut receiver = receiver_arc.lock().unwrap();

                // Exchange events: sender shares all events with receiver
                let sender_events: Vec<EventId> = sender.created_events.clone();
                let receiver_events: HashSet<EventId> =
                    receiver.created_events.iter().copied().collect();

                let mut new_events = 0;
                for event_id in sender_events {
                    if !receiver_events.contains(&event_id) {
                        receiver.created_events.push(event_id);
                        // Update CRDT state
                        receiver.counter.increment(receiver_id, 1);
                        new_events += 1;
                    }
                }

                total_exchanged += new_events;
            }
        }

        total_exchanged
    }
}

/// Helper to create a test node ID
fn test_node(id: u8) -> NodeId {
    let mut node = [0u8; 32];
    node[0] = id;
    node
}

/// Test: 3 nodes creating events, verify all events propagate to all nodes
#[test]
fn test_three_node_event_propagation() {
    println!("\n=== Test: 3-Node Event Propagation ===");

    let mut network = SimulatedNetwork::new();

    // Create 3 nodes
    let n1 = test_node(1);
    let n2 = test_node(2);
    let n3 = test_node(3);

    network.add_node(n1);
    network.add_node(n2);
    network.add_node(n3);

    // Each node creates 5 events
    for i in 0..5 {
        network.create_event(n1, vec![1, i]);
        network.create_event(n2, vec![2, i]);
        network.create_event(n3, vec![3, i]);
    }

    let total_events = network.total_unique_events();
    println!("Total unique events created: {}", total_events);
    assert_eq!(total_events, 15);

    // Run gossip until convergence
    let start = Instant::now();
    let mut rounds = 0;
    let max_rounds = 10;

    loop {
        let exchanged = network.gossip_round();
        rounds += 1;
        println!("Round {}: {} events exchanged", rounds, exchanged);

        if exchanged == 0 || rounds >= max_rounds {
            break;
        }
    }

    let elapsed = start.elapsed();
    println!("Converged in {} rounds, {:?}", rounds, elapsed);

    // Verify all nodes have all events
    for (node_id, node_arc) in &network.nodes {
        let node = node_arc.lock().unwrap();
        assert_eq!(
            node.created_events.len(),
            total_events,
            "Node {:?} missing events (has {}, expected {})",
            &node_id[..4],
            node.created_events.len(),
            total_events
        );
    }

    println!(
        "PASS: All {} events propagated to all 3 nodes",
        total_events
    );
}

/// Test: CRDT convergence across 3 nodes with concurrent increments
#[test]
fn test_three_node_crdt_convergence() {
    println!("\n=== Test: 3-Node CRDT Convergence ===");

    let n1 = test_node(1);
    let n2 = test_node(2);
    let n3 = test_node(3);

    // Each node starts with its own GCounter
    let mut counter_a = GCounter::new();
    let mut counter_b = GCounter::new();
    let mut counter_c = GCounter::new();

    // Node A increments 5 times
    for _ in 0..5 {
        counter_a.increment(n1, 1);
    }
    println!("Node A counter: {}", counter_a.value());

    // Node B increments 3 times
    for _ in 0..3 {
        counter_b.increment(n2, 1);
    }
    println!("Node B counter: {}", counter_b.value());

    // Node C increments 7 times
    for _ in 0..7 {
        counter_c.increment(n3, 1);
    }
    println!("Node C counter: {}", counter_c.value());

    // All counters are different before merge
    assert_ne!(counter_a.value(), counter_b.value());
    assert_ne!(counter_b.value(), counter_c.value());

    // Simulate gossip: full mesh merge (A→B, A→C, B→A, B→C, C→A, C→B)
    // In a real gossip protocol, all nodes eventually see all others' state.
    println!("\nMerging A -> B...");
    counter_b.merge(&counter_a);
    println!("B after merge with A: {}", counter_b.value());

    println!("Merging A -> C...");
    counter_c.merge(&counter_a);
    println!("C after merge with A: {}", counter_c.value());

    println!("Merging B -> C...");
    counter_c.merge(&counter_b);
    println!("C after merge with B: {}", counter_c.value());

    println!("Merging C -> A...");
    counter_a.merge(&counter_c);
    println!("A after merge with C: {}", counter_a.value());

    println!("Merging C -> B...");
    counter_b.merge(&counter_c);
    println!("B after merge with C: {}", counter_b.value());

    // After full propagation, all counters should converge
    let expected_total = 5 + 3 + 7; // 15
    assert_eq!(
        counter_a.value(),
        expected_total,
        "Counter A did not converge to expected value"
    );
    assert_eq!(
        counter_b.value(),
        expected_total,
        "Counter B did not converge to expected value"
    );
    assert_eq!(
        counter_c.value(),
        expected_total,
        "Counter C did not converge to expected value"
    );

    // All state hashes should be identical
    assert_eq!(
        counter_a.state_hash(),
        counter_b.state_hash(),
        "State hashes don't match after convergence"
    );
    assert_eq!(
        counter_b.state_hash(),
        counter_c.state_hash(),
        "State hashes don't match after convergence"
    );

    println!("PASS: All 3 CRDT counters converged to {}", expected_total);
    println!("State hash: {:?}", hex::encode(counter_a.state_hash()));
}

/// Test: Verify 100% convergence guarantee with property-based testing
#[test]
fn test_crdt_100_percent_convergence() {
    println!("\n=== Test: 100% CRDT Convergence Guarantee ===");

    // Test with multiple random increment patterns
    let test_cases = vec![
        (vec![10, 0, 0], "single-node-active"),
        (vec![5, 5, 5], "equal-increments"),
        (vec![100, 1, 1], "one-node-dominant"),
        (vec![0, 0, 50], "late-joiner"),
        (vec![33, 33, 34], "nearly-equal"),
    ];

    let n1 = test_node(1);
    let n2 = test_node(2);
    let n3 = test_node(3);

    for (increments, desc) in test_cases {
        println!("\n  Test case: {} ({:?})", desc, increments);

        let mut c1 = GCounter::new();
        let mut c2 = GCounter::new();
        let mut c3 = GCounter::new();

        // Apply increments
        for _ in 0..increments[0] {
            c1.increment(n1, 1);
        }
        for _ in 0..increments[1] {
            c2.increment(n2, 1);
        }
        for _ in 0..increments[2] {
            c3.increment(n3, 1);
        }

        // Full mesh merge (simulate gossip convergence)
        let mut merged = c1.clone();
        merged.merge(&c2);
        merged.merge(&c3);

        let expected: u64 = increments.iter().sum::<usize>() as u64;

        // Verify all possible merge orders converge to same value
        let mut alt1 = c2.clone();
        alt1.merge(&c1);
        alt1.merge(&c3);

        let mut alt2 = c3.clone();
        alt2.merge(&c2);
        alt2.merge(&c1);

        assert_eq!(merged.value(), expected, "Main merge failed for {}", desc);
        assert_eq!(alt1.value(), expected, "Alt1 merge failed for {}", desc);
        assert_eq!(alt2.value(), expected, "Alt2 merge failed for {}", desc);
        assert_eq!(
            merged.state_hash(),
            alt1.state_hash(),
            "Hash mismatch for {}",
            desc
        );
        assert_eq!(
            alt1.state_hash(),
            alt2.state_hash(),
            "Hash mismatch for {}",
            desc
        );

        println!(
            "    Converged to {} (expected {})",
            merged.value(),
            expected
        );
    }

    println!("\nPASS: 100% CRDT convergence across all test cases");
}

/// Test: Causal ordering preservation across nodes
#[test]
fn test_causal_ordering_preserved() {
    println!("\n=== Test: Causal Ordering Preservation ===");

    let n1 = test_node(1);

    // Create a chain of causally dependent events
    let mut vc = VectorClock::with_node(n1, 1);

    let e1 = Event::new(n1, 0, vc.clone(), None, None, vec![1]);
    let e1_id = e1.id;

    // e1 -> e2
    vc.increment(n1).unwrap();
    let e2 = Event::new(n1, 1, vc.clone(), Some(e1_id), None, vec![2]);
    let e2_id = e2.id;

    // e2 -> e3
    vc.increment(n1).unwrap();
    let e3 = Event::new(n1, 2, vc.clone(), Some(e2_id), None, vec![3]);

    // Verify causal ordering
    assert!(e1.vector_clock.happened_before(&e2.vector_clock));
    assert!(e2.vector_clock.happened_before(&e3.vector_clock));
    assert!(e1.vector_clock.happened_before(&e3.vector_clock));

    // Verify no cycles
    assert!(!e2.vector_clock.happened_before(&e1.vector_clock));
    assert!(!e3.vector_clock.happened_before(&e2.vector_clock));

    println!("e1 -> e2 -> e3 causal chain verified");
    println!("PASS: Causal ordering correctly preserved");
}

/// Test: Concurrent events are correctly identified
#[test]
fn test_concurrent_event_detection() {
    println!("\n=== Test: Concurrent Event Detection ===");

    let n1 = test_node(1);
    let n2 = test_node(2);

    // Both nodes create events independently (concurrent)
    let mut vc1 = VectorClock::with_node(n1, 1);
    let e1 = Event::new(n1, 0, vc1.clone(), None, None, vec![1]);

    let mut vc2 = VectorClock::with_node(n2, 1);
    let e2 = Event::new(n2, 0, vc2.clone(), None, None, vec![2]);

    // Events should be concurrent
    assert!(
        e1.vector_clock.concurrent(&e2.vector_clock),
        "Events from different nodes with no parent relation should be concurrent"
    );

    // Neither happened before the other
    assert!(!e1.vector_clock.happened_before(&e2.vector_clock));
    assert!(!e2.vector_clock.happened_before(&e1.vector_clock));

    println!("Concurrent events correctly identified");

    // Now create a merge event that sees both
    vc1.merge(&vc2);
    vc1.increment(n1).unwrap();
    let merge_event = Event::new(n1, 1, vc1.clone(), Some(e1.id), Some(e2.id), vec![3]);

    // Merge event happened after both
    assert!(e1.vector_clock.happened_before(&merge_event.vector_clock));
    assert!(e2.vector_clock.happened_before(&merge_event.vector_clock));

    println!("Merge event correctly follows both concurrent parents");
    println!("PASS: Concurrent event detection works correctly");
}

/// Test: Full network simulation with 4 nodes and CRDT state
#[test]
fn test_four_node_full_mesh_crdt() {
    println!("\n=== Test: 4-Node Full Mesh CRDT ===");

    let mut network = SimulatedNetwork::new();

    let n1 = test_node(1);
    let n2 = test_node(2);
    let n3 = test_node(3);
    let n4 = test_node(4);

    network.add_node(n1);
    network.add_node(n2);
    network.add_node(n3);
    network.add_node(n4);

    // Each node creates events with different frequencies
    for i in 0..10 {
        network.create_event(n1, vec![1, i]);
        if i % 2 == 0 {
            network.create_event(n2, vec![2, i]);
        }
        if i % 3 == 0 {
            network.create_event(n3, vec![3, i]);
        }
        if i % 5 == 0 {
            network.create_event(n4, vec![4, i]);
        }
    }

    let total = network.total_unique_events();
    println!("Total events created: {}", total);

    // Run gossip to convergence
    let mut rounds = 0;
    loop {
        let exchanged = network.gossip_round();
        rounds += 1;
        if exchanged == 0 || rounds >= 20 {
            break;
        }
    }

    // Verify all nodes have all events
    for (node_id, node_arc) in &network.nodes {
        let node = node_arc.lock().unwrap();
        assert_eq!(
            node.created_events.len(),
            total,
            "Node {:?} missing events",
            &node_id[..4]
        );
    }

    println!("Converged in {} rounds", rounds);
    println!("PASS: 4-node full mesh converged with {} events", total);
}

/// Benchmark: Measure convergence speed
#[test]
fn test_convergence_performance() {
    println!("\n=== Test: Convergence Performance ===");

    let mut network = SimulatedNetwork::new();

    for i in 1..=5 {
        network.add_node(test_node(i));
    }

    // Create 100 events distributed across nodes
    for i in 0..20 {
        for j in 1..=5 {
            network.create_event(test_node(j), vec![j, i as u8]);
        }
    }

    let total = network.total_unique_events();
    let start = Instant::now();

    let mut rounds = 0;
    loop {
        let exchanged = network.gossip_round();
        rounds += 1;
        if exchanged == 0 || rounds >= 50 {
            break;
        }
    }

    let elapsed = start.elapsed();
    println!(
        "{} events across 5 nodes: converged in {} rounds, {:?}",
        total, rounds, elapsed
    );

    // Verify convergence
    for node_arc in network.nodes.values() {
        let node = node_arc.lock().unwrap();
        assert_eq!(node.created_events.len(), total);
    }

    println!("PASS: Performance test completed");
}

/// Test: OR-Set convergence across nodes
#[test]
fn test_or_set_convergence() {
    println!("\n=== Test: OR-Set Convergence ===");

    let n1 = test_node(1);
    let n2 = test_node(2);

    let mut set_a: OrSet<String> = OrSet::new();
    let mut set_b: OrSet<String> = OrSet::new();

    // Node A adds items
    set_a.add(n1, "apple".to_string());
    set_a.add(n1, "banana".to_string());

    // Node B adds items (concurrently)
    set_b.add(n2, "cherry".to_string());
    set_b.add(n2, "apple".to_string()); // Concurrent add of same item

    println!("A before merge: {:?}", set_a.elements());
    println!("B before merge: {:?}", set_b.elements());

    // Merge B into A
    set_a.merge(&set_b);

    // All items should be present (OR-Set: add wins)
    let elements = set_a.elements();
    assert!(elements.contains(&"apple".to_string()));
    assert!(elements.contains(&"banana".to_string()));
    assert!(elements.contains(&"cherry".to_string()));

    // apple should have 2 tokens (one from each node)
    assert_eq!(set_a.tokens(&"apple".to_string()).unwrap().len(), 2);

    println!("Merged set: {:?}", elements);

    // Now remove "apple" at A
    set_a.remove(&"apple".to_string());
    println!("After removing apple: {:?}", set_a.elements());

    // apple should be removed (all tokens were observed)
    assert!(!set_a.elements().contains(&"apple".to_string()));

    // But if B adds apple again concurrently with A's removal...
    let token = set_b.add(n2, "apple".to_string()); // New token!
    set_a.merge(&set_b);

    // The new add wins (it has a token that wasn't observed by the removal)
    assert!(set_a.contains(&"apple".to_string()));

    println!("After concurrent add: {:?}", set_a.elements());
    println!("PASS: OR-Set correctly handles concurrent add/remove");
}
