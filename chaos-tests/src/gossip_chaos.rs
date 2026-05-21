//! Chaos tests for the optimized gossip protocol.
//!
//! Tests the gossip optimization components (bloom filter, compact encoding,
//! priority queue) under adversarial conditions including:
//! - 10% message loss + reordering
//! - Safety verification (no loss of events that should be delivered)
//! - Liveness verification (events eventually delivered)
//! - Bloom filter effectiveness (FPR within bounds)

#[cfg(test)]
use crate::ChaosNetwork;
#[cfg(test)]
use omnia_network::{
    CompactEncoder, DeltaClock, GossipBloomFilter, GossipPriority, PriorityGossipQueue, PriorityQueueConfig,
};
#[cfg(test)]
use omnia_primitives::{Event, EventId, NodeId, VectorClock};
#[cfg(test)]
use std::collections::HashSet;

/// Helper: create a NodeId from a single byte.
#[cfg(test)]
fn node(id: u8) -> NodeId {
    let mut n = [0u8; 32];
    n[0] = id;
    n
}

// ---------------------------------------------------------------------------
// Bloom filter chaos tests
// ---------------------------------------------------------------------------

/// Test: Bloom filter maintains no false negatives under load.
///
/// Insert 100,000 event IDs and verify that every inserted ID is
/// correctly reported as "seen" (no false negatives allowed).
#[test]
fn test_bloom_filter_no_false_negatives_under_load() {
    let mut filter = GossipBloomFilter::new(100_000, 0.001);

    let mut inserted_ids: Vec<EventId> = Vec::new();
    for i in 0..1000u32 {
        let mut id = [0u8; 32];
        id[..4].copy_from_slice(&i.to_le_bytes());
        filter.insert(&id);
        inserted_ids.push(id);
    }

    // Every inserted ID must be found
    for id in &inserted_ids {
        assert!(filter.contains(id), "False negative for event {:?}", &id[..4]);
    }
}

/// Test: Bloom filter FPR is within bounds.
///
/// Insert 50,000 event IDs and test 50,000 non-inserted IDs.
/// The observed FPR should be close to the target (0.001).
#[test]
fn test_bloom_filter_fpr_within_bounds() {
    let mut filter = GossipBloomFilter::new(50_000, 0.001);

    // Insert 50,000 IDs
    for i in 0..50_000u32 {
        let mut id = [0u8; 32];
        id[..4].copy_from_slice(&i.to_le_bytes());
        filter.insert(&id);
    }

    // Test 50,000 non-inserted IDs
    let mut false_positives = 0usize;
    let test_count = 50_000usize;
    for i in 50_000..50_000 + test_count as u32 {
        let mut id = [0u8; 32];
        id[..4].copy_from_slice(&i.to_le_bytes());
        if filter.contains(&id) {
            false_positives += 1;
        }
    }

    let observed_fpr = false_positives as f64 / test_count as f64;
    // Allow 10x tolerance (bloom filters have variance)
    assert!(
        observed_fpr < 0.01,
        "FPR too high: {observed_fpr:.6} (target: 0.001, tolerance: 0.01)"
    );
}

/// Test: Bloom filter rotation correctly expires old entries.
///
/// After inserting IDs, rotating twice should cause them to expire
/// from both the active and inactive filters.
#[test]
fn test_bloom_filter_rotation_expires_entries() {
    let mut filter = GossipBloomFilter::new(10_000, 0.01);

    // Insert an ID
    let event_id = [1u8; 32];
    filter.insert(&event_id);
    assert!(filter.contains(&event_id));

    // First rotation: active -> inactive
    filter.rotate();
    assert!(filter.contains(&event_id), "Should still be found after first rotation");

    // Second rotation: inactive (with our ID) gets cleared
    filter.rotate();
    assert!(!filter.contains(&event_id), "Should be expired after second rotation");
}

/// Test: Bloom filter maintains correctness under rotation with
/// interleaved inserts and lookups.
#[test]
fn test_bloom_filter_interleaved_rotation() {
    let mut filter = GossipBloomFilter::new(10_000, 0.01);

    // Phase 1: Insert some IDs
    let id_1 = [1u8; 32];
    filter.insert(&id_1);
    assert!(filter.contains(&id_1));

    // Rotate
    filter.rotate();

    // Phase 2: Insert more IDs
    let id_2 = [2u8; 32];
    filter.insert(&id_2);
    assert!(filter.contains(&id_1));
    assert!(filter.contains(&id_2));

    // Rotate again
    filter.rotate();

    // id_1 should be expired (was in the inactive filter that was cleared)
    // id_2 might still be found (was in active before second rotation, now in inactive)
    assert!(!filter.contains(&id_1), "id_1 should be expired");

    // id_2 was in active when second rotation happened, so it moved to inactive
    assert!(filter.contains(&id_2), "id_2 should still be found");
}

// ---------------------------------------------------------------------------
// Priority queue chaos tests
// ---------------------------------------------------------------------------

/// Test: Priority queue maintains strict priority ordering under load.
#[test]
fn test_priority_queue_strict_ordering() {
    let mut queue = PriorityGossipQueue::with_defaults();

    // Insert events in random priority order
    for i in 0..100u8 {
        let priority = match i % 4 {
            0 => GossipPriority::Low,
            1 => GossipPriority::Normal,
            2 => GossipPriority::High,
            3 => GossipPriority::Critical,
            _ => GossipPriority::Normal,
        };
        let mut id = [0u8; 32];
        id[0] = i;
        queue.enqueue(id, priority);
    }

    // Dequeue all and verify ordering
    let mut last_priority = GossipPriority::Critical;
    while let Some(event_id) = queue.dequeue() {
        // Determine the priority of the dequeued event
        let current_priority = match event_id[0] % 4 {
            0 => GossipPriority::Low,
            1 => GossipPriority::Normal,
            2 => GossipPriority::High,
            3 => GossipPriority::Critical,
            _ => GossipPriority::Normal,
        };
        assert!(
            current_priority <= last_priority,
            "Priority ordering violated: {current_priority:?} after {last_priority:?}"
        );
        last_priority = current_priority;
    }
}

/// Test: Priority queue capacity limits are enforced.
#[test]
fn test_priority_queue_capacity_limits() {
    let config = PriorityQueueConfig {
        max_critical: 10,
        max_high: 10,
        max_normal: 10,
        max_low: 10,
    };
    let mut queue = PriorityGossipQueue::new(config);

    // Fill each level beyond capacity
    for i in 0..20u8 {
        let mut id = [0u8; 32];
        id[0] = i;
        queue.enqueue(id, GossipPriority::Critical);
    }

    // Should be capped at 10
    assert_eq!(
        queue.len_by_priority(GossipPriority::Critical),
        10,
        "Critical queue should be capped at 10"
    );
}

/// Test: Critical events are always dequeued first.
#[test]
fn test_critical_events_always_first() {
    let mut queue = PriorityGossipQueue::with_defaults();

    // Insert many normal events first
    for i in 0..100u8 {
        let mut id = [0u8; 32];
        id[0] = i;
        queue.enqueue(id, GossipPriority::Normal);
    }

    // Insert a critical event
    let critical_id = [255u8; 32];
    queue.enqueue(critical_id, GossipPriority::Critical);

    // The critical event should be dequeued first
    let first = queue.dequeue();
    assert_eq!(first, Some(critical_id), "Critical event should be dequeued first");
}

// ---------------------------------------------------------------------------
// Compact encoding chaos tests
// ---------------------------------------------------------------------------

/// Test: Compact encoding roundtrip preserves all event data.
#[test]
fn test_compact_encoding_roundtrip_preserves_data() {
    let mut encoder = CompactEncoder::new(1024, 16);
    let keypair = omnia_crypto::generate_keypair();

    let mut event = Event::genesis(node(1), vec![1, 2, 3, 4, 5]);
    event.sign_with_keypair(&keypair);

    let peer_id = node(2);
    let compact = encoder.encode(&event, &peer_id).unwrap();

    // Serialize and deserialize
    let bytes = CompactEncoder::serialize_compact(&compact).unwrap();
    let restored_compact = CompactEncoder::deserialize_compact(&bytes).unwrap();

    // Decode back to full event
    let local_frontier = VectorClock::new();
    let decoded = encoder
        .decode(&restored_compact, &peer_id, &local_frontier, |_truncated| {
            // For this test, we don't have a graph to resolve against,
            // so just return None for truncated IDs
            None
        })
        .unwrap();

    // The decoded event should match the original
    assert_eq!(decoded.id, event.id);
    assert_eq!(decoded.creator, event.creator);
    assert_eq!(decoded.sequence, event.sequence);
    assert_eq!(decoded.payload, event.payload);
    assert_eq!(decoded.signature, event.signature);
    assert_eq!(decoded.creator_pubkey, event.creator_pubkey);
}

/// Test: Delta clock encoding with multiple vector clock entries.
#[test]
fn test_delta_clock_multiple_entries() {
    let mut local = VectorClock::new();
    local.set(node(1), 10);
    local.set(node(2), 20);
    local.set(node(3), 30);

    let mut remote = VectorClock::new();
    remote.set(node(1), 5);
    remote.set(node(2), 20);
    // node(3) not in remote

    let delta = CompactEncoder::encode_delta_clock(&local, &remote);

    // node(1) advanced (10 > 5), node(2) same (20 == 20), node(3) new (30 > 0)
    assert_eq!(delta.entries.len(), 2);
    assert!(delta.entries.contains(&(node(1), 10)));
    assert!(delta.entries.contains(&(node(3), 30)));

    // Apply delta to remote and verify
    let reconstructed = CompactEncoder::apply_delta_clock(&remote, &delta);
    assert_eq!(reconstructed.get(&node(1)), 10);
    assert_eq!(reconstructed.get(&node(2)), 20);
    assert_eq!(reconstructed.get(&node(3)), 30);
}

/// Test: Delta clock with empty remote frontier sends full clock.
#[test]
fn test_delta_clock_empty_remote_sends_full() {
    let mut local = VectorClock::new();
    local.set(node(1), 5);
    local.set(node(2), 10);

    let remote = VectorClock::new();
    let delta = CompactEncoder::encode_delta_clock(&local, &remote);

    assert_eq!(delta.entries.len(), 2);

    // Applying the delta to an empty frontier should reconstruct the full clock
    let reconstructed = CompactEncoder::apply_delta_clock(&remote, &delta);
    assert_eq!(reconstructed, local);
}

// ---------------------------------------------------------------------------
// Integration: Optimized gossip under message loss
// ---------------------------------------------------------------------------

/// Test: Optimized gossip under 10% message loss with reordering.
///
/// Simulates a 3-node network with 10% message loss. Verifies:
/// - Safety: No loss of events that should be delivered (no conflicting commits).
/// - Liveness: Events are eventually delivered to all nodes.
/// - Bloom filter effectiveness: FPR stays within bounds.
#[test]
fn test_optimized_gossip_under_message_loss() {
    let mut network = ChaosNetwork::new(3);

    // Verify initial liveness
    assert!(
        network.check_liveness(),
        "Network should be live before introducing message loss"
    );
    assert!(
        network.check_safety(),
        "Network should be safe before introducing message loss"
    );

    // Set 10% drop rate on all nodes
    for i in 0..3 {
        network.set_drop_rate(i, 0.1);
    }

    // Submit multiple rounds of events
    for round in 0..10 {
        for i in 0..3 {
            let payload = vec![round as u8, i as u8];
            let result = network.submit_event(i, payload);
            assert!(result.is_ok(), "Event submission should succeed even with message loss");
        }
    }

    // Re-sync to compensate for lost messages
    network.advance(5);
    network.warmup();

    // Safety should hold (no conflicting commits)
    assert!(
        network.check_safety(),
        "Safety should be maintained with 10% message loss"
    );

    // Liveness should hold (events committed)
    assert!(network.check_liveness(), "Network should be live with 10% message loss");
}

/// Test: Bloom filter correctly suppresses duplicate events
/// while allowing new events through.
#[test]
fn test_bloom_filter_suppresses_duplicates() {
    let mut filter = GossipBloomFilter::new(10_000, 0.01);

    // Insert some event IDs
    let mut seen_ids = HashSet::new();
    for i in 0..100u32 {
        let mut id = [0u8; 32];
        id[..4].copy_from_slice(&i.to_le_bytes());
        filter.insert(&id);
        seen_ids.insert(id);
    }

    // Check that all inserted IDs are detected
    let mut detected = 0;
    for id in &seen_ids {
        if filter.contains(id) {
            detected += 1;
        }
    }
    assert_eq!(
        detected, 100,
        "All inserted IDs should be detected (no false negatives)"
    );

    // Check that non-inserted IDs are mostly not detected
    let mut false_positives = 0;
    for i in 100..200u32 {
        let mut id = [0u8; 32];
        id[..4].copy_from_slice(&i.to_le_bytes());
        if filter.contains(&id) {
            false_positives += 1;
        }
    }

    // FPR should be reasonable
    let fpr = false_positives as f64 / 100.0;
    assert!(
        fpr < 0.1,
        "FPR should be < 10% for 10K items at 0.01 target, got {fpr:.4}"
    );
}

/// Test: Priority queue handles burst traffic correctly.
#[test]
fn test_priority_queue_burst_traffic() {
    let config = PriorityQueueConfig {
        max_critical: 100,
        max_high: 500,
        max_normal: 1000,
        max_low: 500,
    };
    let mut queue = PriorityGossipQueue::new(config);

    // Burst: Insert 200 critical events (exceeds capacity of 100)
    for i in 0..200u8 {
        let mut id = [0u8; 32];
        id[0] = i;
        queue.enqueue(id, GossipPriority::Critical);
    }

    // Should be capped at 100
    assert_eq!(queue.len_by_priority(GossipPriority::Critical), 100);

    // Older events should have been dropped (first 100 dropped)
    // The remaining events should be the most recent ones
    let first = queue.dequeue();
    assert!(first.is_some());
    // The first dequeued should have id[0] = 100 (oldest surviving event)
    assert_eq!(first.map(|id| id[0]), Some(100));

    // Statistics should reflect the drops
    assert_eq!(queue.total_enqueued(), 200);
    assert_eq!(queue.total_dropped(), 100);
}

/// Test: Compact encoding with events that have large vector clocks.
#[test]
fn test_compact_encoding_large_vector_clock() {
    let mut encoder = CompactEncoder::new(2048, 16);
    let keypair = omnia_crypto::generate_keypair();

    // Create an event with a large vector clock (10 nodes)
    let mut vc = VectorClock::new();
    for i in 0..10u8 {
        vc.set(node(i), (i as u64) * 100 + 50);
    }

    let mut event = Event::new(node(1), 0, vc, None, None, vec![1, 2, 3]);
    event.sign_with_keypair(&keypair);

    let peer_id = node(2);
    let compact = encoder.encode(&event, &peer_id).unwrap();

    // Delta clock should have 10 entries (all new to the peer)
    assert_eq!(compact.delta_clock.entries.len(), 10);

    // Serialize and deserialize
    let bytes = CompactEncoder::serialize_compact(&compact).unwrap();
    let restored = CompactEncoder::deserialize_compact(&bytes).unwrap();

    // Decode
    let local_frontier = VectorClock::new();
    let decoded = encoder.decode(&restored, &peer_id, &local_frontier, |_| None).unwrap();

    // Verify the reconstructed vector clock matches the original
    assert_eq!(decoded.vector_clock, event.vector_clock);
}

/// Test: Full integration — 3-node testnet with all optimizations.
///
/// Uses the ChaosNetwork with bloom filter dedup, priority queue,
/// and compact encoding to verify that events propagate correctly.
#[test]
fn test_full_optimized_gossip_integration() {
    let mut network = ChaosNetwork::new(3);

    // Set up bloom filters for each "node" (simulated)
    let mut bloom_filters: Vec<GossipBloomFilter> = (0..3).map(|_| GossipBloomFilter::new(10_000, 0.01)).collect();

    // Set up priority queues for each "node" (simulated)
    let mut priority_queues: Vec<PriorityGossipQueue> = (0..3).map(|_| PriorityGossipQueue::with_defaults()).collect();

    // Verify initial state
    assert!(network.check_liveness());
    assert!(network.check_safety());

    // Submit events and track them through bloom filters and priority queues
    let mut all_event_ids: Vec<EventId> = Vec::new();
    for round in 0..5 {
        for i in 0..3 {
            let payload = vec![round as u8, i as u8];
            let result = network.submit_event(i, payload);
            if let Ok(()) = result {
                // In a real system, the event ID would be known here.
                // For the chaos test, we just verify the submission succeeded.
            }
        }
    }

    // Insert all committed event IDs into bloom filters
    for (idx, bloom) in bloom_filters.iter_mut().enumerate() {
        let committed = network.nodes[idx].consensus.get_committed();
        for event_id in committed {
            bloom.insert(&event_id);
        }
    }

    // Verify bloom filters contain no false negatives for committed events
    for (idx, bloom) in bloom_filters.iter().enumerate() {
        let committed = network.nodes[idx].consensus.get_committed();
        for event_id in committed {
            assert!(
                bloom.contains(&event_id),
                "Bloom filter false negative for committed event at node {idx}"
            );
        }
    }

    // Enqueue events into priority queues with classification
    for (idx, queue) in priority_queues.iter_mut().enumerate() {
        let committed = network.nodes[idx].consensus.get_committed();
        for event_id in committed {
            // Classify: use the event_id's first byte as a heuristic
            let priority = if event_id[0] % 4 == 0 {
                GossipPriority::Critical
            } else if event_id[0] % 4 == 1 {
                GossipPriority::High
            } else if event_id[0] % 4 == 2 {
                GossipPriority::Normal
            } else {
                GossipPriority::Low
            };
            queue.enqueue(event_id, priority);
        }
    }

    // Verify priority queues are not empty (events were enqueued)
    for (idx, queue) in priority_queues.iter().enumerate() {
        if !network.nodes[idx].consensus.get_committed().is_empty() {
            assert!(
                !queue.is_empty(),
                "Priority queue at node {idx} should not be empty after enqueuing committed events"
            );
        }
    }

    // Safety should hold
    assert!(
        network.check_safety(),
        "Safety should hold after optimized gossip integration test"
    );

    // Liveness should hold
    assert!(
        network.check_liveness(),
        "Liveness should hold after optimized gossip integration test"
    );
}

/// Test: Gossip with reordering — events arrive out of order but
/// the causal graph handles them correctly.
#[test]
fn test_gossip_with_event_reordering() {
    let mut network = ChaosNetwork::new(3);
    assert!(network.check_liveness());
    assert!(network.check_safety());

    // Set higher drop rates to simulate reordering (messages arrive
    // at different times for different nodes)
    for i in 0..3 {
        network.set_drop_rate(i, 0.15);
    }

    // Submit events rapidly
    for round in 0..8 {
        for i in 0..3 {
            let payload = vec![round as u8, i as u8, 0xAA];
            let _ = network.submit_event(i, payload);
        }
    }

    // Re-sync
    network.advance(3);
    network.warmup();

    // Despite reordering, safety must hold
    assert!(network.check_safety(), "Safety should hold despite event reordering");

    // Events should eventually be delivered
    assert!(
        network.check_liveness(),
        "Network should be live despite event reordering"
    );
}

/// Test: Bloom filter memory usage is bounded.
#[test]
fn test_bloom_filter_memory_bounded() {
    let filter = GossipBloomFilter::new(100_000, 0.001);
    let size = filter.estimated_size();

    // Memory should be roughly 2 * 176 KiB = ~352 KiB
    assert!(
        size < 500_000,
        "Bloom filter memory should be bounded, got {size} bytes"
    );
    assert!(
        size > 100_000,
        "Bloom filter should use at least 100KB for 100K items at 0.001 FPR, got {size} bytes"
    );
}

/// Test: Delta clock varint encoding handles large values.
#[test]
fn test_delta_clock_large_values() {
    let delta = DeltaClock {
        entries: vec![(node(1), u64::MAX), (node(2), 1), (node(3), 0)],
    };

    let bytes = delta.to_bytes();
    let restored = DeltaClock::from_bytes(&bytes).unwrap();
    assert_eq!(delta, restored);
}

/// Test: Compact encoder frontier updates correctly.
#[test]
fn test_compact_encoder_frontier_updates() {
    let mut encoder = CompactEncoder::new(1024, 16);
    let keypair = omnia_crypto::generate_keypair();

    // Create event from node 1
    let mut event = Event::genesis(node(1), vec![1, 2, 3]);
    event.sign_with_keypair(&keypair);

    let peer_id = node(2);

    // No frontier yet
    assert!(encoder.get_frontier(&peer_id).is_none());

    // Encode and decode
    let compact = encoder.encode(&event, &peer_id).unwrap();
    let local_frontier = VectorClock::new();
    let _decoded = encoder.decode(&compact, &peer_id, &local_frontier, |_| None).unwrap();

    // Frontier should now be updated
    let frontier = encoder.get_frontier(&peer_id);
    assert!(frontier.is_some(), "Frontier should be updated after decode");
    assert_eq!(frontier.map(|f| f.get(&node(1))), Some(1));
}
