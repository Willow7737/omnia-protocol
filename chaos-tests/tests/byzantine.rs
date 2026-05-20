#![allow(clippy::unwrap_used)]
//! Byzantine fault chaos tests.
//!
//! Tests that the SlashingEngine detects and penalizes Byzantine behavior,
//! including equivocation (double-signing) and liveness violations (inactivity).

use omnia_chaos_tests::ChaosNetwork;
use omnia_substrate::{Event, SlashOutcome, SlashingEngine};

/// Test: 1 of 4 nodes equivocates → SlashingEngine detects and slashes.
///
/// A node creates two different events with the same (creator, sequence) pair
/// but different payloads (and thus different IDs). When both events are
/// submitted to an observer node, the consensus engine detects the
/// equivocation and slashes the offending validator.
#[test]
fn test_equivocation_detection() {
    let mut network = ChaosNetwork::new(4);

    // Verify initial state
    assert!(network.check_safety());
    assert!(network.check_liveness());

    // Get node 0's identity and keypair for crafting equivocating events
    let node0_id = network.nodes[0].node_id;
    let node0_keypair = network.nodes[0].keypair.clone();
    let node0_sequence = network.nodes[0].next_sequence;
    let node0_self_parent = network.nodes[0].last_event_id();
    let node0_vc = network.nodes[0].vector_clock.clone();

    // Create two equivocating events from node 0
    // Both have the same creator, sequence, and self_parent but different payloads
    let mut vc1 = node0_vc.clone();
    vc1.set(node0_id, node0_sequence.saturating_add(1));
    let mut event_a = Event::new(
        node0_id,
        node0_sequence,
        vc1,
        node0_self_parent,
        None,
        vec![0xAA], // Payload A
    );
    event_a.sign_with_keypair(&node0_keypair);

    let mut vc2 = node0_vc.clone();
    vc2.set(node0_id, node0_sequence.saturating_add(1));
    let mut event_b = Event::new(
        node0_id,
        node0_sequence,
        vc2,
        node0_self_parent,
        None,
        vec![0xBB], // Payload B (different from A)
    );
    event_b.sign_with_keypair(&node0_keypair);

    // Verify these are indeed equivocating events
    assert_ne!(event_a.id, event_b.id, "Equivocating events must have different IDs");
    assert_eq!(
        event_a.creator, event_b.creator,
        "Equivocating events must have the same creator"
    );
    assert_eq!(
        event_a.sequence, event_b.sequence,
        "Equivocating events must have the same sequence number"
    );
    assert!(
        SlashingEngine::check_equivocation(&event_a, &event_b),
        "Events should be detected as equivocation"
    );

    // Inject event A into node 1 (observer)
    network
        .inject_event(1, event_a.clone())
        .expect("Should be able to inject event A into node 1");

    // Verify node 0 is NOT slashed yet (only one event seen)
    assert!(
        !network.is_node_slashed(1, &node0_id),
        "Node 0 should not be slashed after only one event"
    );

    // Inject event B into node 1 (the equivocating event)
    network
        .inject_event(1, event_b.clone())
        .expect("Should be able to inject event B into node 1");

    // Node 1's consensus engine should have detected equivocation and slashed node 0
    assert!(
        network.is_node_slashed(1, &node0_id),
        "Node 0 should be slashed for equivocation on node 1"
    );

    // Safety should still hold (no conflicting commits — the equivocating
    // events were detected before they could both be committed)
    assert!(
        network.check_safety(),
        "Safety should be maintained after equivocation detection"
    );

    tracing::info!("Equivocation detection test passed — node 0 was slashed");
}

/// Test: Equivocation detected by multiple observer nodes.
///
/// When equivocating events are gossiped across the network, all
/// observer nodes should detect the equivocation independently.
#[test]
fn test_equivocation_detected_by_multiple_observers() {
    let mut network = ChaosNetwork::new(4);
    assert!(network.check_liveness());

    let node0_id = network.nodes[0].node_id;
    let node0_keypair = network.nodes[0].keypair.clone();
    let node0_sequence = network.nodes[0].next_sequence;
    let node0_self_parent = network.nodes[0].last_event_id();
    let node0_vc = network.nodes[0].vector_clock.clone();

    // Create equivocating events
    let mut vc1 = node0_vc.clone();
    vc1.set(node0_id, node0_sequence.saturating_add(1));
    let mut event_a = Event::new(node0_id, node0_sequence, vc1, node0_self_parent, None, vec![1]);
    event_a.sign_with_keypair(&node0_keypair);

    let mut vc2 = node0_vc.clone();
    vc2.set(node0_id, node0_sequence.saturating_add(1));
    let mut event_b = Event::new(node0_id, node0_sequence, vc2, node0_self_parent, None, vec![2]);
    event_b.sign_with_keypair(&node0_keypair);

    assert!(SlashingEngine::check_equivocation(&event_a, &event_b));

    // Inject both events into all observer nodes
    for observer in &[1, 2, 3] {
        network
            .inject_event(*observer, event_a.clone())
            .expect("Inject event A should succeed");
        network
            .inject_event(*observer, event_b.clone())
            .expect("Inject event B should succeed");
    }

    // All observer nodes should have slashed node 0
    for observer in &[1, 2, 3] {
        assert!(
            network.is_node_slashed(*observer, &node0_id),
            "Observer node {observer} should have slashed node 0 for equivocation"
        );
    }

    assert!(network.check_safety());
}

/// Test: 1 of 4 nodes is silent → liveness maintained, eventually slashed for inactivity.
///
/// A node stops creating events (goes silent). The remaining nodes continue
/// to operate and maintain liveness. The SlashingEngine's `check_liveness`
/// method detects the inactivity and records a liveness violation.
#[test]
fn test_silent_node_liveness_slashing() {
    let mut network = ChaosNetwork::new(4);
    assert!(network.check_liveness());

    let silent_node_id = network.nodes[2].node_id;

    // Record the silent node's last active round
    // The node starts at round 0 after warmup
    let silent_last_round: u64 = 0;

    // Submit events from the active nodes (0, 1, 3) but NOT from node 2
    for _ in 0..5 {
        for &i in &[0, 1, 3] {
            let _ = network.submit_event(i, vec![0xCC]);
        }
    }

    // Liveness should be maintained by the active nodes
    assert!(
        network.check_liveness(),
        "Network should remain live without the silent node"
    );

    // Safety should also hold
    assert!(
        network.check_safety(),
        "Safety should be maintained without the silent node"
    );

    // Now check liveness of node 2 via the slashing engine on node 0
    // Use a low threshold so the inactivity is flagged
    let current_round: u64 = 10;
    let threshold: u64 = 3; // 3 rounds of inactivity triggers violation

    let outcome = network.check_node_liveness(0, silent_node_id, silent_last_round, current_round, threshold);

    assert!(
        outcome.is_some(),
        "Silent node should be flagged for liveness violation"
    );

    match outcome {
        Some(SlashOutcome::Warned { node, points }) => {
            tracing::info!(
                node = ?&node[..4],
                points,
                "Silent node received liveness warning"
            );
            // Points should be 100 (LivenessViolation)
            assert_eq!(points, 100, "Liveness violation should add 100 points");
        }
        Some(SlashOutcome::Slashed { node, amount }) => {
            tracing::info!(
                node = ?&node[..4],
                amount,
                "Silent node was slashed for inactivity"
            );
        }
        Some(SlashOutcome::Ejected { node }) => {
            tracing::info!(
                node = ?&node[..4],
                "Silent node was ejected for inactivity"
            );
        }
        None => {
            panic!("Expected a liveness violation outcome");
        }
    }

    tracing::info!("Silent node liveness test passed");
}

/// Test: Multiple liveness violations accumulate and eventually lead to slashing.
///
/// A node that is repeatedly inactive accumulates slash points. After enough
/// violations, the node crosses the slash threshold and is fully slashed.
#[test]
fn test_repeated_liveness_violations_lead_to_slashing() {
    let mut network = ChaosNetwork::new(4);

    let silent_node_id = network.nodes[2].node_id;

    // Simulate multiple rounds of inactivity checks
    // Each violation adds 100 points; slash threshold is 500
    // So 5 violations should trigger slashing
    let mut violation_count = 0;
    let mut is_slashed = false;

    for round in (1..=20).step_by(2) {
        let outcome = network.check_node_liveness(
            0,
            silent_node_id,
            0, // last active at round 0
            round,
            1, // threshold of 1 round
        );

        if let Some(result) = outcome {
            violation_count += 1;
            tracing::info!(
                round,
                violation_count,
                outcome = ?result,
                "Liveness violation recorded"
            );

            if matches!(result, SlashOutcome::Slashed { .. } | SlashOutcome::Ejected { .. }) {
                is_slashed = true;
                break;
            }
        }
    }

    assert!(
        violation_count >= 5,
        "Should have at least 5 liveness violations, got {violation_count}"
    );
    assert!(
        is_slashed,
        "Node should be slashed after {violation_count} violations with 100 points each (threshold 500)"
    );

    tracing::info!(
        violation_count,
        "Repeated liveness violation test passed — node slashed"
    );
}

/// Test: Honest node is never slashed.
///
/// A node that participates regularly should never accumulate slash points.
#[test]
fn test_honest_node_never_slashed() {
    let mut network = ChaosNetwork::new(4);

    let honest_node_id = network.nodes[0].node_id;

    // Submit events from all nodes (everyone participates)
    for _ in 0..5 {
        for i in 0..4 {
            let _ = network.submit_event(i, vec![0xDD]);
        }
    }

    // Check liveness for the honest node — should not be flagged
    // Use the node's current round as last_active to simulate active participation
    let outcome = network.check_node_liveness(1, honest_node_id, 5, 6, 3);

    assert!(
        outcome.is_none(),
        "Active node should not be flagged for liveness violation"
    );

    // Verify not slashed on any observer
    for observer in 1..4 {
        assert!(
            !network.is_node_slashed(observer, &honest_node_id),
            "Honest node should not be slashed on observer {observer}"
        );
    }
}
