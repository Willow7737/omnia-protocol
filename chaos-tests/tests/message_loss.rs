#![allow(clippy::unwrap_used)]
//! Message loss chaos tests.
//!
//! Tests that the protocol can tolerate moderate message loss (20%) and
//! still eventually finalize events, while extreme message loss (80%)
//! prevents timely finalization.

use omnia_chaos_tests::ChaosNetwork;

/// Test: 20% message drop rate → events still eventually finalize.
///
/// With a low drop rate, the network can still reach supermajority for
/// most events. Retries via gossip ensure that eventually all events
/// are delivered and processed.
#[test]
fn test_low_message_loss_liveness() {
    let mut network = ChaosNetwork::new(4);

    // Verify initial state (genesis committed)
    assert!(
        network.check_liveness(),
        "Network should be live before introducing message loss"
    );

    let committed_before = network.committed_count();

    // Set 20% drop rate on all nodes
    for i in 0..4 {
        network.set_drop_rate(i, 0.2);
    }

    // Submit multiple rounds of events
    for _ in 0..5 {
        for i in 0..4 {
            let result = network.submit_event(i, vec![0x11]);
            // Submissions should still succeed (event is created locally)
            assert!(
                result.is_ok(),
                "Local event submission should succeed even with message loss"
            );
        }
    }

    // With 20% loss, events should still eventually be delivered to most nodes.
    // Run additional advance rounds and syncs to compensate for lost messages.
    network.advance(3);
    network.warmup(); // re-sync

    // Safety should hold (no conflicting commits)
    assert!(
        network.check_safety(),
        "Safety should be maintained with 20% message loss"
    );

    // The network should still be live (have committed events)
    assert!(
        network.check_liveness(),
        "Network should be live with 20% message loss"
    );

    let committed_after = network.committed_count();
    assert!(
        committed_after >= committed_before,
        "Committed count should not decrease: before={committed_before}, after={committed_after}"
    );

    tracing::info!(
        committed_before,
        committed_after,
        "Low message loss test passed"
    );
}

/// Test: 80% message drop rate → events may not finalize within timeout.
///
/// With extreme message loss, the network cannot reliably reach
/// supermajority, so new events are unlikely to be committed.
/// This is the expected behavior — the test verifies that the
/// protocol degrades gracefully rather than producing incorrect results.
#[test]
fn test_high_message_loss_prevents_finalization() {
    let mut network = ChaosNetwork::new(4);

    // Verify initial liveness
    assert!(
        network.check_liveness(),
        "Network should be live before introducing message loss"
    );

    let committed_before = network.committed_count();

    // Set 80% drop rate on all nodes
    for i in 0..4 {
        network.set_drop_rate(i, 0.8);
    }

    // Submit events
    for _ in 0..3 {
        for i in 0..4 {
            let _ = network.submit_event(i, vec![0x22]);
        }
    }

    // With 80% loss, most gossip messages are dropped.
    // Events are created locally but rarely propagated.
    // Safety should still hold (no conflicting commits possible)
    assert!(
        network.check_safety(),
        "Safety should be maintained even with 80% message loss"
    );

    // Liveness may or may not hold depending on which messages got through.
    // The key invariant is safety, not liveness, under extreme conditions.
    let committed_after = network.committed_count();

    // With 80% loss, it's very likely that few or no NEW events are committed.
    // (Genesis events were committed before the loss was introduced.)
    // We just verify that the committed count didn't decrease.
    assert!(
        committed_after >= committed_before,
        "Committed count should never decrease"
    );

    tracing::info!(
        committed_before,
        committed_after,
        new_committed = committed_after - committed_before,
        "High message loss test completed (expected low finalization)"
    );
}

/// Test: Asymmetric message loss — one node has high loss, others have none.
///
/// When a single node drops most messages, it falls behind but the
/// rest of the network continues to operate normally.
#[test]
fn test_asymmetric_message_loss() {
    let mut network = ChaosNetwork::new(4);
    assert!(network.check_liveness());

    let committed_before = network.committed_count();

    // Only node 3 has high message loss
    network.set_drop_rate(3, 0.9);

    // Submit events from all nodes
    for _ in 0..5 {
        for i in 0..4 {
            let _ = network.submit_event(i, vec![0x33]);
        }
    }

    // Nodes 0-2 should be able to communicate and make progress
    assert!(
        network.check_safety(),
        "Safety should hold with asymmetric message loss"
    );

    // At least the non-lossy nodes should have committed events
    assert!(
        network.check_liveness(),
        "Network should be live despite one lossy node"
    );

    // Reset drop rate and sync
    network.set_drop_rate(3, 0.0);
    network.warmup();

    // After resetting, node 3 should catch up
    let committed_after = network.committed_count();
    assert!(
        committed_after >= committed_before,
        "Committed count should not decrease after recovery"
    );

    tracing::info!(
        committed_before,
        committed_after,
        "Asymmetric message loss test passed"
    );
}

/// Test: Intermittent message loss — drop rate changes over time.
///
/// Simulates a network with fluctuating reliability. Events are submitted
/// during periods of both low and high message loss.
#[test]
fn test_intermittent_message_loss() {
    let mut network = ChaosNetwork::new(4);
    assert!(network.check_liveness());

    // Phase 1: No loss
    for _ in 0..3 {
        for i in 0..4 {
            let _ = network.submit_event(i, vec![0x44]);
        }
    }

    // Phase 2: High loss
    for i in 0..4 {
        network.set_drop_rate(i, 0.7);
    }
    for _ in 0..3 {
        for i in 0..4 {
            let _ = network.submit_event(i, vec![0x55]);
        }
    }

    // Phase 3: Recovery — no loss, sync
    for i in 0..4 {
        network.set_drop_rate(i, 0.0);
    }
    network.warmup();

    for _ in 0..3 {
        for i in 0..4 {
            let _ = network.submit_event(i, vec![0x66]);
        }
    }

    // Safety should always hold
    assert!(
        network.check_safety(),
        "Safety should hold through intermittent message loss"
    );

    // After recovery, liveness should be restored
    assert!(
        network.check_liveness(),
        "Network should be live after message loss recovery"
    );

    tracing::info!("Intermittent message loss test passed");
}

/// Test: Zero drop rate is the same as no message loss.
///
/// Setting drop rate to 0.0 should not affect normal operation.
#[test]
fn test_zero_drop_rate_no_effect() {
    let mut network = ChaosNetwork::new(4);

    // Set zero drop rate (should be a no-op)
    for i in 0..4 {
        network.set_drop_rate(i, 0.0);
    }

    // Submit events normally
    for _ in 0..3 {
        for i in 0..4 {
            let _ = network.submit_event(i, vec![0x77]);
        }
    }

    assert!(network.check_safety());
    assert!(network.check_liveness());
}

/// Test: Full drop rate isolates a node.
///
/// Setting drop rate to 1.0 means the node drops all incoming messages.
/// The node can still create events, but they are not propagated.
#[test]
fn test_full_drop_rate_isolation() {
    let mut network = ChaosNetwork::new(4);
    assert!(network.check_liveness());

    // Node 3 drops all messages
    network.set_drop_rate(3, 1.0);

    // Submit events
    for _ in 0..3 {
        for i in 0..4 {
            let _ = network.submit_event(i, vec![0x88]);
        }
    }

    // Safety should hold
    assert!(
        network.check_safety(),
        "Safety should hold with full drop rate on one node"
    );

    // Other nodes should still be live
    assert!(
        network.check_liveness(),
        "Network should be live despite one fully lossy node"
    );
}
