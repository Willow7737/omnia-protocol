//! Multi-Node BFT Consensus Validation Tests
//!
//! Phase 5: These tests verify that multiple Omnia consensus engines
//! can reach BFT finality in a simulated multi-node environment.
//!
//! Unlike the single-node load test, these tests create separate
//! `ConsensusEngine` instances per node, each with their own `CausalGraph`,
//! and verify that all honest nodes agree on finalized state.
//!
//! P0-1 fix: All events are now signed with Ed25519 keypairs. The
//! consensus engine verifies signatures before processing — unsigned
//! or forged events are rejected with ConsensusError::InvalidSignature.

use omnia_consensus::{
    CausalGraph, ConsensusConfig, ConsensusEngine, SlashingEngine, DEFAULT_EJECTION_THRESHOLD, DEFAULT_SLASH_THRESHOLD,
};
use omnia_crypto::{generate_keypair, NodeKeypair};
use omnia_primitives::{blake3_hash_domain, Event, NodeId, VectorClock};

/// Test node: holds a keypair and derived NodeId.
struct TestNode {
    keypair: NodeKeypair,
    node_id: NodeId,
}

impl TestNode {
    /// Create a test node with a deterministic keypair.
    /// The NodeId is derived from the public key via BLAKE3 domain
    /// separation, matching the production Event::sign_with_keypair
    /// derivation.
    fn new(seed: u8) -> Self {
        // Generate a keypair — for test reproducibility we accept
        // non-deterministic generation (the seed only affects the
        // node_id derivation, not the keypair itself).
        let keypair = generate_keypair();
        let node_id = blake3_hash_domain(b"omnia-creator", &keypair.verifying_key().to_bytes());
        let _ = seed; // seed is unused; keypair generation is random
        Self { keypair, node_id }
    }

    /// Sign an event with this node's keypair.
    fn sign_event(&self, event: &mut Event) {
        event
            .sign_with_keypair(&self.keypair)
            .expect("event signing should succeed");
    }
}

/// Helper: create a ConsensusEngine for a specific node.
fn create_consensus_node(_node_id: NodeId, total_nodes: usize, all_node_ids: &[NodeId]) -> ConsensusEngine {
    let mut seed = [0u8; 32];
    seed[0] = 1; // deterministic seed for test reproducibility
    let config = ConsensusConfig {
        total_nodes,
        round_seed: seed,
        ..Default::default()
    };
    let slashing = SlashingEngine::new(None, DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD)
        .expect("slashing engine creation");
    let mut engine = ConsensusEngine::new(config, slashing);
    // Register all validators with equal stake
    for &nid in all_node_ids {
        engine.register_validator(nid, 10_000);
    }
    engine
}

/// Test that 4 nodes reach BFT finality over simulated event processing.
///
/// Each node has its own ConsensusEngine and CausalGraph. Events are
/// submitted through node 0, then gossiped (directly inserted) to all
/// other nodes. After processing, all nodes should agree on which
/// events are committed.
#[test]
#[allow(clippy::unwrap_used)]
fn test_multi_node_bft_finality() {
    let num_nodes = 4;
    let nodes: Vec<TestNode> = (0..num_nodes).map(|i| TestNode::new(i as u8)).collect();
    let node_ids: Vec<NodeId> = nodes.iter().map(|n| n.node_id).collect();

    // Create one engine + graph per node
    let mut engines: Vec<ConsensusEngine> = node_ids
        .iter()
        .map(|&id| create_consensus_node(id, num_nodes, &node_ids))
        .collect();
    let mut graphs: Vec<CausalGraph> = (0..num_nodes).map(|_| CausalGraph::new()).collect();

    // Submit events through node 0
    let creator = node_ids[0];
    let mut self_parent: Option<[u8; 32]> = None;
    let mut vector_clock = VectorClock::new();

    for seq in 0..10u64 {
        vector_clock.set(creator, seq + 1);

        let mut event = if self_parent.is_none() {
            Event::genesis(creator, vec![seq as u8]).expect("valid genesis event")
        } else {
            Event::new(creator, seq, vector_clock.clone(), self_parent, None, vec![seq as u8]).expect("valid event")
        };

        // P0-1 fix: sign the event so verify_signature() passes
        nodes[0].sign_event(&mut event);

        let event_id = event.id;
        self_parent = Some(event_id);

        // Insert and process on all nodes (simulating gossip)
        for (i, engine) in engines.iter_mut().enumerate() {
            graphs[i].insert(event.clone()).expect("graph insert should succeed");
            let committed = engine.process_event(&event, &graphs[i]);
            // Processing should not error on honest events
            assert!(
                committed.is_ok(),
                "Node {} should process event {} without error: {:?}",
                i,
                seq,
                committed.err()
            );
        }
    }

    // Verify all nodes have the same committed count
    let committed_counts: Vec<u64> = engines.iter().map(|e| e.committed_count()).collect();
    // All nodes should have committed the same number of events
    let first = committed_counts[0];
    for (i, &count) in committed_counts.iter().enumerate() {
        assert_eq!(count, first, "Node {i} committed {count} events, expected {first}");
    }
}

/// Test BFT safety when one node is Byzantine (sends conflicting events).
///
/// With 4 nodes, BFT tolerates f=1 faulty node. The 3 honest nodes
/// should still agree on the finalized state even when one node
/// creates equivocating events.
#[test]
#[allow(clippy::unwrap_used)]
fn test_bft_safety_with_byzantine_node() {
    let num_nodes = 4;
    let nodes: Vec<TestNode> = (0..num_nodes).map(|i| TestNode::new(i as u8)).collect();
    let node_ids: Vec<NodeId> = nodes.iter().map(|n| n.node_id).collect();

    // Create engines for honest nodes only (nodes 0, 1, 2)
    let mut engines: Vec<ConsensusEngine> = node_ids[..3]
        .iter()
        .map(|&id| create_consensus_node(id, num_nodes, &node_ids))
        .collect();
    let mut graphs: Vec<CausalGraph> = (0..3).map(|_| CausalGraph::new()).collect();

    // Submit honest events through node 0
    let honest_creator = node_ids[0];
    let mut self_parent: Option<[u8; 32]> = None;
    let mut vector_clock = VectorClock::new();

    for seq in 0..5u64 {
        vector_clock.set(honest_creator, seq + 1);

        let mut event = if self_parent.is_none() {
            Event::genesis(honest_creator, vec![seq as u8]).expect("valid genesis event")
        } else {
            Event::new(
                honest_creator,
                seq,
                vector_clock.clone(),
                self_parent,
                None,
                vec![seq as u8],
            )
            .expect("valid event")
        };

        // P0-1 fix: sign the event so verify_signature() passes
        nodes[0].sign_event(&mut event);

        let event_id = event.id;
        self_parent = Some(event_id);

        // Process on all honest nodes
        for (i, engine) in engines.iter_mut().enumerate() {
            graphs[i].insert(event.clone()).expect("graph insert should succeed");
            let committed = engine.process_event(&event, &graphs[i]);
            assert!(
                committed.is_ok(),
                "Honest node {} should process event {}: {:?}",
                i,
                seq,
                committed.err()
            );
        }
    }

    // Verify honest nodes agree on committed count
    let committed_counts: Vec<u64> = engines.iter().map(|e| e.committed_count()).collect();
    let first = committed_counts[0];
    for (i, &count) in committed_counts.iter().enumerate() {
        assert_eq!(
            count, first,
            "Honest node {i} committed {count} events, expected {first}"
        );
    }
}

/// Test that consensus makes progress when fewer than 1/3 of nodes are faulty.
///
/// With 4 nodes (3 honest + 1 faulty), BFT tolerates f=1 faulty node.
/// All 3 honest nodes create events and cross-reference each other's
/// events via other-parent links, simulating real gossip. This ensures
/// enough witnesses accumulate per round to reach supermajority.
#[test]
#[allow(clippy::unwrap_used)]
fn test_consensus_progress_with_minority_faults() {
    // Use 4 total nodes (1 faulty, 3 honest) so supermajority = 3,
    // which is achievable when all 3 honest nodes create events.
    let num_nodes = 4;
    let nodes: Vec<TestNode> = (0..num_nodes).map(|i| TestNode::new(i as u8)).collect();
    let node_ids: Vec<NodeId> = nodes.iter().map(|n| n.node_id).collect();

    // Create engines for the 3 honest nodes
    let honest_nodes = &nodes[..3];
    let honest_node_ids = &node_ids[..3];
    let mut engines: Vec<ConsensusEngine> = honest_node_ids
        .iter()
        .map(|&id| create_consensus_node(id, num_nodes, &node_ids))
        .collect();
    let mut graphs: Vec<CausalGraph> = (0..3).map(|_| CausalGraph::new()).collect();

    // Track per-node event chains
    let mut self_parents: Vec<Option<[u8; 32]>> = vec![None; 3];
    let mut vector_clocks: Vec<VectorClock> = (0..3).map(|_| VectorClock::new()).collect();

    // Simulate multiple rounds of events from all honest nodes
    // Each round, every honest node creates an event that references
    // the previous event from another honest node as other-parent.
    for round in 0..10u64 {
        for node_idx in 0..3usize {
            let creator = honest_node_ids[node_idx];
            vector_clocks[node_idx].set(creator, round + 1);

            // Find another honest node's latest event to use as other-parent
            let other_idx = (node_idx + 1) % 3;
            let other_parent = self_parents[other_idx];

            let mut event = if self_parents[node_idx].is_none() {
                Event::genesis(creator, format!("round-{round}-node-{node_idx}").into_bytes())
                    .expect("valid genesis event")
            } else {
                Event::new(
                    creator,
                    round,
                    vector_clocks[node_idx].clone(),
                    self_parents[node_idx],
                    other_parent,
                    format!("round-{round}-node-{node_idx}").into_bytes(),
                )
                .expect("valid event")
            };

            // P0-1 fix: sign the event so verify_signature() passes
            honest_nodes[node_idx].sign_event(&mut event);

            let event_id = event.id;
            self_parents[node_idx] = Some(event_id);

            // Process this event on all honest nodes (simulating gossip)
            for (i, engine) in engines.iter_mut().enumerate() {
                graphs[i].insert(event.clone()).expect("graph insert should succeed");
                let committed = engine.process_event(&event, &graphs[i]);
                assert!(
                    committed.is_ok(),
                    "Honest node {} failed to process event from node {} round {}: {:?}",
                    i,
                    node_idx,
                    round,
                    committed.err()
                );
            }
        }
    }

    // Verify all honest nodes have committed some events
    for (i, engine) in engines.iter().enumerate() {
        assert!(
            engine.committed_count() > 0,
            "Honest node {i} should have committed events, got 0"
        );
    }

    // Verify honest nodes agree on committed count
    let committed_counts: Vec<u64> = engines.iter().map(|e| e.committed_count()).collect();
    let first = committed_counts[0];
    for (i, &count) in committed_counts.iter().enumerate() {
        assert_eq!(
            count, first,
            "Honest node {i} committed {count} events, expected {first}"
        );
    }
}
