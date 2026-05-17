#![allow(clippy::unwrap_used)]
//! Network partition chaos test.
//!
//! Tests that a 4-node network maintains safety and eventually achieves
//! liveness when a partition separates {0,1} from {2,3}. Each partition
//! continues to create events but cannot reach cross-partition supermajority.
//! After healing, all nodes converge and finalize events.

use omnia_chaos_tests::ChaosNetwork;

/// Test: 4 nodes, partition {0,1} from {2,3} → each partition creates events
/// independently → heal → all nodes converge.
///
/// During the partition, neither side can reach the 3-of-4 supermajority
/// required to commit new witnesses, so no new events are committed beyond
/// the initial genesis round. After healing, the full network can reach
/// supermajority and events from both partitions become visible to all nodes.
#[test]
fn test_partition_and_heal() {
    let mut network = ChaosNetwork::new(4);

    // Verify initial state: genesis events should be committed
    assert!(
        network.check_liveness(),
        "Network should have committed genesis events after construction"
    );
    assert!(
        network.check_safety(),
        "No conflicting commits should exist initially"
    );

    let committed_before = network.committed_count();
    assert!(
        committed_before > 0,
        "Should have committed genesis events, got {}",
        committed_before
    );

    // Create partition: {0,1} cannot communicate with {2,3}
    network.partition(&[0, 1], &[2, 3]);

    // Submit events in each partition
    for _ in 0..3 {
        for &i in &[0, 1] {
            let result = network.submit_event(i, vec![0xAA]);
            assert!(
                result.is_ok(),
                "Node {} should be able to submit events during partition",
                i
            );
        }
        for &i in &[2, 3] {
            let result = network.submit_event(i, vec![0xBB]);
            assert!(
                result.is_ok(),
                "Node {} should be able to submit events during partition",
                i
            );
        }
    }

    // Safety should be maintained during partition
    assert!(
        network.check_safety(),
        "Safety should be maintained during partition"
    );

    // Heal the partition
    network.heal();

    // Submit advance events to help consensus progress
    network.advance(3);

    // After healing and advancing, safety should still hold
    assert!(
        network.check_safety(),
        "Safety should be maintained after healing"
    );

    // Liveness should be confirmed
    assert!(
        network.check_liveness(),
        "Network should be live after healing"
    );

    // All nodes should have events in their graphs
    for i in 0..4 {
        let count = network.node_committed_count(i);
        assert!(
            count > 0,
            "Node {} should have committed events after healing, got {}",
            i,
            count
        );
    }

    tracing::info!(
        committed_before,
        committed_after = network.committed_count(),
        "Partition test completed successfully"
    );
}

/// Test: Multiple overlapping partitions are handled correctly.
///
/// Creates two partitions: {0,1}|{2,3} and then {0,2}|{1,3}.
/// The second partition makes node 0 unable to talk to node 1
/// (and node 2 unable to talk to node 3), effectively isolating
/// all nodes from each other. After healing, convergence is restored.
#[test]
fn test_overlapping_partitions() {
    let mut network = ChaosNetwork::new(4);
    assert!(network.check_safety());
    assert!(network.check_liveness());

    // First partition
    network.partition(&[0, 1], &[2, 3]);

    // Submit some events
    for i in 0..4 {
        let _ = network.submit_event(i, vec![1]);
    }

    // Add a second partition that cuts across the first
    network.partition(&[0, 2], &[1, 3]);

    // Now node 0 can't talk to 1 (2nd partition) or 2,3 (1st partition puts
    // 0 in group with 1, but 2nd partition puts 0 against 1).
    // Effectively: 0 can only talk to 0 (itself).

    // Submit more events
    for i in 0..4 {
        let _ = network.submit_event(i, vec![2]);
    }

    assert!(
        network.check_safety(),
        "Safety should hold with overlapping partitions"
    );

    // Heal everything
    network.heal();
    network.advance(3);

    assert!(network.check_safety());
    assert!(network.check_liveness());
}

/// Test: A single node is partitioned away from the rest.
///
/// Node 3 is isolated from {0,1,2}. The remaining 3 nodes can still
/// reach supermajority (3 of 4) and should be able to finalize events.
#[test]
fn test_single_node_partitioned() {
    let mut network = ChaosNetwork::new(4);
    assert!(network.check_liveness());

    // Partition: {3} isolated from {0,1,2}
    network.partition(&[3], &[0, 1, 2]);

    // Submit events from the majority partition
    for _ in 0..3 {
        for &i in &[0, 1, 2] {
            let _ = network.submit_event(i, vec![0xCC]);
        }
    }

    // Node 3 can still submit events (to itself)
    let _ = network.submit_event(3, vec![0xDD]);

    assert!(
        network.check_safety(),
        "Safety should hold with single node partitioned"
    );

    // Heal and verify convergence
    network.heal();
    network.advance(3);

    assert!(network.check_safety());
    assert!(network.check_liveness());
}
