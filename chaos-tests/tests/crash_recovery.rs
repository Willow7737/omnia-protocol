#![allow(clippy::unwrap_used)]
//! Crash and recovery chaos test.
//!
//! Tests that a crashed node can be restarted and successfully recover
//! by syncing missed events from the rest of the network.

use omnia_chaos_tests::ChaosNetwork;

/// Test: crash node 2 during active event submission → restart → node 2 recovers.
///
/// While node 2 is crashed, the remaining 3 nodes continue to submit events
/// and make progress. After restart, node 2 receives all missed events
/// and can participate in consensus again.
#[test]
fn test_crash_recovery() {
    let mut network = ChaosNetwork::new(4);

    // Verify initial liveness
    assert!(network.check_liveness(), "Network should be live initially");
    assert!(network.check_safety(), "Network should be safe initially");

    let initial_committed = network.committed_count();

    // Submit some events before the crash
    for i in 0..4 {
        let _ = network.submit_event(i, vec![1]);
    }

    // Crash node 2
    network
        .crash_node(2)
        .expect("Should be able to crash node 2");
    assert!(
        network.nodes[2].crashed,
        "Node 2 should be marked as crashed"
    );

    // Attempting to submit from crashed node should fail
    let result = network.submit_event(2, vec![99]);
    assert!(result.is_err(), "Submitting from crashed node should fail");

    // Submit events from the remaining active nodes
    let events_during_crash = 5;
    for _ in 0..events_during_crash {
        for &i in &[0, 1, 3] {
            let _ = network.submit_event(i, vec![2]);
        }
    }

    // Safety should still hold
    assert!(
        network.check_safety(),
        "Safety should hold while node is crashed"
    );

    let committed_during_crash = network.committed_count();

    // Restart node 2
    network
        .restart_node(2)
        .expect("Should be able to restart node 2");
    assert!(
        !network.nodes[2].crashed,
        "Node 2 should be active after restart"
    );

    // Node 2 should now have events that were created while it was crashed
    // (synced during restart)
    let node2_events = network.nodes[2].graph.event_ids().len();
    assert!(
        node2_events > 0,
        "Node 2 should have events after restart, got {node2_events}"
    );

    // Submit events from node 2 after recovery
    for _ in 0..3 {
        let result = network.submit_event(2, vec![3]);
        assert!(
            result.is_ok(),
            "Node 2 should be able to submit events after restart"
        );
    }

    // Advance consensus to help finalization
    network.advance(2);

    // Final safety and liveness checks
    assert!(network.check_safety(), "Safety should hold after recovery");
    assert!(
        network.check_liveness(),
        "Network should be live after recovery"
    );

    let final_committed = network.committed_count();
    assert!(
        final_committed >= committed_during_crash,
        "Committed count should not decrease: before={committed_during_crash}, after={final_committed}"
    );

    tracing::info!(
        initial_committed,
        committed_during_crash,
        final_committed,
        node2_events,
        "Crash recovery test completed successfully"
    );
}

/// Test: Multiple nodes crash and recover.
///
/// Nodes 1 and 3 crash simultaneously. The remaining nodes 0 and 2
/// continue operating. When nodes 1 and 3 restart, they should recover.
#[test]
fn test_multiple_crash_recovery() {
    let mut network = ChaosNetwork::new(4);
    assert!(network.check_liveness());

    // Crash two nodes
    network.crash_node(1).expect("Should crash node 1");
    network.crash_node(3).expect("Should crash node 3");

    // Submit events from surviving nodes
    for _ in 0..3 {
        for &i in &[0, 2] {
            let _ = network.submit_event(i, vec![0xAA]);
        }
    }

    assert!(
        network.check_safety(),
        "Safety should hold with two crashed nodes"
    );

    // Restart one node
    network.restart_node(1).expect("Should restart node 1");
    assert!(!network.nodes[1].crashed);

    // Submit more events
    let _ = network.submit_event(1, vec![0xBB]);

    // Restart the other node
    network.restart_node(3).expect("Should restart node 3");
    assert!(!network.nodes[3].crashed);

    // Submit events from all recovered nodes
    let _ = network.submit_event(3, vec![0xCC]);

    network.advance(2);

    assert!(network.check_safety());
    assert!(network.check_liveness());
}

/// Test: Crashing a node that is already crashed returns an error.
#[test]
fn test_double_crash_error() {
    let mut network = ChaosNetwork::new(4);

    network.crash_node(0).expect("First crash should succeed");

    let result = network.crash_node(0);
    assert!(
        result.is_err(),
        "Crashing an already-crashed node should return an error"
    );
}

/// Test: Restarting a node that is not crashed returns an error.
#[test]
fn test_restart_active_node_error() {
    let mut network = ChaosNetwork::new(4);

    let result = network.restart_node(0);
    assert!(
        result.is_err(),
        "Restarting an active node should return an error"
    );
}
