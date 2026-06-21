# Consensus Queue Invariants

> 🎯 Audience: Developers
> 🔗 Context: Documents the consensus queue invariants and O(new_events) processing guarantee
> 📅 Last Updated: 2026-05-20

**Task**: 1.1 — Document the consensus queue invariants
**Date**: 2026-05-14

## Overview

The consensus queue (`Substrate::unprocessed_events`) is the mechanism that makes consensus processing O(new_events) instead of O(total_events). This document describes the invariants that the queue maintains, how it interacts with the causal graph, and what guarantees it provides.

## Why the Queue Makes Consensus O(new_events) Instead of O(n)

Without the queue, the consensus engine would need to scan the entire causal graph on each iteration to find unprocessed events. For a graph with N events, this would be O(N) per iteration, which is unacceptable for a protocol targeting 10,000 TPS.

The `unprocessed_events` queue tracks only the events that have been inserted into the graph but not yet processed by consensus. When a new event arrives (either locally via `submit_event()` or from the network via gossip), its `EventId` is appended to the queue. When `process_consensus()` runs, it drains the queue entirely:

```rust
pub async fn process_consensus(&mut self) -> Vec<EventId> {
    let graph = self.graph.read().await;
    let mut all_committed = Vec::new();

    // Drain only unprocessed events (topologically ordered)
    let to_process: Vec<EventId> = self.unprocessed_events.drain(..).collect();

    for id in &to_process {
        if let Some(event) = graph.get(id) {
            if let Ok(committed) = self.consensus.process_event(event, &graph) {
                all_committed.extend(committed);
            }
        }
    }

    all_committed
}
```

This makes each consensus iteration O(k), where k is the number of new events since the last iteration, rather than O(N), where N is the total number of events in the graph.

## What Happens When Events Arrive Out of Topological Order

Events may arrive out of topological order because:

1. **Gossip propagation is non-deterministic.** Events from different nodes may arrive in any order, depending on network latency and gossip fanout.
2. **Local events are created in order, but network events may interleave.** A node creates events with monotonically increasing sequence numbers, but events from other nodes may arrive with gaps.

When an out-of-order event arrives, the following happens:

1. **Graph insertion succeeds.** The `CausalGraph::insert()` method checks that the event's parents exist in the graph. If a parent is missing, the insertion fails with `CausalGraphError::MissingParent`. This means that events are only inserted when all their parents are already in the graph, which is a natural form of topological ordering.

2. **The event is added to the queue.** Even if the event arrives "out of order" relative to events from other nodes, it is added to `unprocessed_events` because its parents (and thus its causal predecessors) are already in the graph.

3. **Consensus processes events in queue order.** The `process_consensus()` method iterates over the queue in insertion order. However, the `ConsensusEngine::process_event()` method assigns rounds and determines fame based on the event's actual causal relationships (parent links, ancestry checks), not on insertion order. So even if events are processed in a different order than their topological sort, the consensus outcome is the same.

4. **Already-processed events are skipped.** The `ConsensusEngine::process_event()` method checks `if self.event_states.contains_key(&event_id)` and returns `Ok(Vec::new())` for already-processed events. This provides an additional layer of idempotency.

## How Duplicate Events Are Handled

Duplicate events can arise from gossip retransmission. The system handles them at two levels:

**Level 1: Gossip deduplication.** The `GossipProtocol` maintains a `seen_events: HashSet<[u8; 32]>` that tracks event IDs already received. If a duplicate arrives from the network, it is silently dropped before reaching the pending queue:

```rust
if !self.seen_events.contains(&event.id) {
    self.seen_events.insert(event.id);
    self.pending_events.push_back(event);
}
```

**Level 2: CausalGraph idempotency.** If a duplicate event somehow reaches the graph (e.g., via `submit_event()` called twice with the same event), `CausalGraph::insert()` returns `CausalGraphError::DuplicateEvent`. The caller (either `GossipProtocol::process_pending_events()` or `Substrate::submit_event()`) handles this gracefully:

```rust
Err(CausalGraphError::DuplicateEvent(_)) => {
    self.stats.events_rejected += 1;
}
```

**Level 3: Consensus idempotency.** The `ConsensusEngine::process_event()` method checks if the event has already been processed and returns early:

```rust
if self.event_states.contains_key(&event_id) {
    return Ok(Vec::new());
}
```

These three levels of idempotency ensure that duplicate events are always handled gracefully, with no risk of double-processing or state corruption.

## The Relationship Between `graph.insert()` and Queue Population

The causal graph and the consensus queue are populated in tandem:

**For locally created events** (via `Substrate::submit_event()`):

1. The event is validated (`event.validate()`).
2. The event is inserted into the graph (`graph.insert(event.clone())`).
3. The event ID is appended to `unprocessed_events`.
4. The event is also processed by consensus immediately (`consensus.process_event()`).
5. The event is gossiped to the network.

**For network-received events** (via `GossipProtocol::process_pending_events()`):

1. The event is deserialized from gossip bytes (`Event::from_bytes()`).
2. The event is inserted into the graph (`graph.insert(event.clone())`).
3. The event ID is returned from `process_pending_events()`.
4. The substrate appends the returned IDs to `unprocessed_events`.

In both cases, the event is in the graph before its ID is added to the queue. This ordering is critical for the invariants below.

## Invariants

### Invariant 1: Every Event in `unprocessed_events` Is Already in the Graph

**Statement**: For every `EventId` in `unprocessed_events`, there exists a corresponding `Event` in `CausalGraph`.

**Why it holds**: Events are inserted into the graph _before_ their IDs are added to the queue. If graph insertion fails (invalid hash, missing parent, duplicate), the event ID is never added to the queue.

**Why it matters**: `process_consensus()` looks up events in the graph by ID. If an ID in the queue had no corresponding graph entry, `graph.get(id)` would return `None`, and the event would be silently skipped — but this would mean that consensus missed an event, potentially violating finality guarantees.

### Invariant 2: `process_consensus()` Drains the Queue Completely Each Call

**Statement**: After `process_consensus()` returns, `unprocessed_events` is empty.

**Why it holds**: The method uses `self.unprocessed_events.drain(..).collect()` to take all elements from the queue. The `drain()` method removes all elements from the `Vec`, leaving it empty.

**Why it matters**: If the queue were not fully drained, events would accumulate and be reprocessed on the next iteration. While consensus idempotency (Level 3 above) prevents double-processing from causing errors, it would waste CPU time on redundant `event_states.contains_key()` checks.

### Invariant 3: No Event Is Processed by Consensus Before It Is in the Graph

**Statement**: The `ConsensusEngine::process_event()` method is never called with an event that is not in the `CausalGraph`.

**Why it holds**: For locally created events, `submit_event()` inserts into the graph before calling `consensus.process_event()`. For network events, `process_pending_events()` inserts into the graph before returning the ID to the substrate. The substrate only calls `consensus.process_event()` for events retrieved from the graph via `graph.get(id)`.

**Why it matters**: The `ConsensusEngine::process_event()` method uses `graph.is_ancestor_of()` to determine rounds and fame. If the event or its ancestors were not in the graph, these ancestry checks would return incorrect results, potentially violating consensus safety.

### Invariant 4: The Queue May Contain Duplicate Event IDs

**Statement**: The same `EventId` may appear in `unprocessed_events` more than once.

**Why it holds**: There is no deduplication at the queue level. If `submit_event()` is called and then the same event arrives via gossip before `process_consensus()` runs, the ID would be in the queue twice.

**Why it is safe**: The `ConsensusEngine::process_event()` method is idempotent — it returns `Ok(Vec::new())` for already-processed events. Duplicate IDs in the queue are simply no-ops during consensus processing.

## Performance Characteristics

| Operation           | Complexity      | Notes                                        |
| ------------------- | --------------- | -------------------------------------------- |
| Append to queue     | O(1)            | `Vec::push()` amortized                      |
| Drain queue         | O(k)            | k = number of unprocessed events             |
| Consensus per event | O(k × ancestry) | Ancestry checks are O(k) in worst case       |
| Total per iteration | O(k)            | Dominated by consensus, not queue management |

The queue ensures that consensus processing scales with the rate of new events, not with the total history size. For a protocol targeting 10,000 TPS with a 100ms consensus interval, k ≈ 1,000 events per iteration — a manageable workload.

---

🔙 **Back**: [Architecture Index](./) | 🔄 **Related**: [Pipeline Design](./pipeline-design.md)
🚀 **Next**: [CRDT Convergence](./crdt-convergence.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)
