# Vector Clock Reconciliation Strategy

**Task**: 1.3 — Document the vector clock reconciliation strategy
**Date**: 2026-05-14

## Overview

Vector clocks are the foundation of Omnia's causal ordering system. After a network partition heals, vector clocks across nodes may have diverged. This document describes the reconciliation strategy that ensures deterministic ordering is restored post-merge.

## Vector Clock Basics

A `VectorClock` (implemented in `substrate/src/vector_clock.rs`) is a map of `NodeId → LogicalClock`. Each node maintains its own vector clock, incrementing its own entry each time it creates an event. When a node receives an event from another node, it merges the event's vector clock into its own using pointwise maximum:

```rust
pub fn merge(&mut self, other: &Self) {
    for (node, &clock) in other.clocks.iter() {
        let entry = self.clocks.entry(*node).or_insert(0);
        *entry = (*entry).max(clock);
    }
}
```

Vector clocks enable three fundamental operations:

1. **`happened_before()`**: Returns `true` if all entries in `self` are ≤ corresponding entries in `other`, with at least one strict inequality. This means `self` causally precedes `other`.
2. **`concurrent()`**: Returns `true` if neither clock is ≤ the other — i.e., some entries in `self` are greater and some are less. Concurrent events are causally independent and can be processed in parallel.
3. **`merge()`**: Takes the pointwise maximum of both clocks, representing combined causal knowledge.

## Network Partition Scenario

Consider a network of 4 nodes (A, B, C, D) that splits into two partitions: {A, B} and {C, D}.

**Before partition:**
```
Node A: {A:5, B:4, C:3, D:3}
Node B: {A:5, B:4, C:3, D:3}
Node C: {A:5, B:4, C:3, D:3}
Node D: {A:5, B:4, C:3, D:3}
```

**During partition (events continue locally):**
```
Node A: {A:8, B:7, C:3, D:3}   (3 new events from A, 3 from B)
Node B: {A:8, B:7, C:3, D:3}
Node C: {A:5, B:4, C:6, D:5}   (3 new events from C, 2 from D)
Node D: {A:5, B:4, C:6, D:5}
```

**After partition heals (each node merges with the other partition):**
```
Node A: {A:8, B:7, C:6, D:5}   (merged with C/D's clocks)
Node B: {A:8, B:7, C:6, D:5}
Node C: {A:8, B:7, C:6, D:5}   (merged with A/B's clocks)
Node D: {A:8, B:7, C:6, D:5}
```

After merging, all nodes converge to the same vector clock. This is the CRDT property of vector clocks: `merge()` is commutative, associative, and idempotent.

## VectorClock::merge() Uses Pointwise Max (CRDT Semantics)

The `merge()` operation is a CvRDT (state-based CRDT) merge. It takes the pointwise maximum of each entry:

```rust
*entry = (*entry).max(clock);
```

This has several important properties:

1. **Commutativity**: `a.merge(b)` produces the same result as `b.merge(a)`. The order in which partitions merge does not matter.

2. **Associativity**: `a.merge(b).merge(c)` produces the same result as `a.merge(b.merge(c))`. Multi-way merges produce the same result regardless of grouping.

3. **Idempotency**: `a.merge(a)` is a no-op. Merging with yourself changes nothing.

4. **Monotonicity**: `merge()` only increases entries (takes the max). Vector clocks never decrease.

These properties guarantee that after all partitions have exchanged their vector clocks, all nodes converge to the same state — the "least upper bound" of all observed clocks.

## CausalOrder Remains Deterministic Post-Merge

After a partition heals and vector clocks are merged, the `CausalOrder` between any two events remains deterministic. This is because:

**`happened_before` is transitive.** If event X happened before event Y, and event Y happened before event Z, then X happened before Z. This transitivity is preserved after merge because:

- If `X.vc ≤ Y.vc` before merge, then `X.vc ≤ Y.vc` after merge (since merge only adds information, never removes it).
- The `all_less_equal()` check in `VectorClock::compare()` considers all known nodes, so after merge, previously unknown entries (from the other partition) are included in the comparison.

**Concurrent events remain concurrent after merge.** If two events were concurrent before the partition (neither's vector clock is ≤ the other's), they remain concurrent after merge. Merge only adds entries; it never makes a previously-concurrent pair non-concurrent.

**Example:**
```
Before partition:
  Event E1: {A:2, B:0}  (created by A)
  Event E2: {A:0, B:2}  (created by B)
  E1.concurrent(E2) → true  (neither ≤ the other)

After partition heals and clocks merge:
  Event E1: {A:2, B:0}  (unchanged — events are immutable)
  Event E2: {A:0, B:2}  (unchanged)
  E1.concurrent(E2) → true  (still concurrent)

Node's frontier clock: {A:2, B:2}  (merged knowledge)
```

The events' vector clocks are immutable — they were set when the event was created and never change. The merge only affects the node's *view* of the network's causal knowledge, not the events themselves.

## How Concurrent Events After Merge Are Handled

After a partition heals, some events from the two partitions will be concurrent — they were created independently during the partition. These concurrent events are handled by topological sort:

The `CausalGraph::topological_order()` method produces a deterministic ordering of events that respects causal dependencies:

1. Events are ordered so that every event appears after all its causal predecessors (parents).
2. Concurrent events are ordered deterministically by timestamp, then by event hash.

```rust
queue_vec.sort_by(|a, b| {
    let event_a = self.events.get(a).unwrap();
    let event_b = self.events.get(b).unwrap();
    event_a
        .timestamp
        .cmp(&event_b.timestamp)
        .then_with(|| a.cmp(b))
});
```

This ensures that all nodes produce the same topological order for the same set of events, even when some events are concurrent. The tiebreaker is the event ID (a SHA-256 hash), which is globally unique and deterministic.

**Important caveat**: The timestamp tiebreaker uses wall-clock time, which may differ across nodes. In Phase 0, this is acceptable because the BFT consensus engine provides finality regardless of topological order. In future phases, the timestamp ordering should be replaced with a purely deterministic tiebreaker (e.g., lexicographic comparison of event IDs) to ensure all nodes produce identical orderings.

## Partition Detection

Partition detection is **not** part of the vector clock reconciliation strategy. It is the domain of Agent 03, scheduled for Sprint 3. The current design assumes that:

1. Partitions are detected eventually (via gossip failure or timeout).
2. Healing occurs when network connectivity is restored.
3. Vector clock reconciliation happens automatically through the gossip protocol's `process_pending_events()` mechanism.

When a partition heals and events from the other partition start arriving, the `GossipProtocol` processes them normally:

1. Events are deserialized from gossip bytes.
2. Events are inserted into the `CausalGraph` (which validates parent links and detects duplicates).
3. Event IDs are added to `unprocessed_events`.
4. `process_consensus()` processes the newly arrived events.

The vector clocks embedded in these events carry the causal context from the other partition, and the `CausalGraph` merges them into its frontier:

```rust
self.frontier.merge(&event.vector_clock);
```

This automatic reconciliation means that no special "partition healing" protocol is needed at the vector clock level. The CRDT properties of `merge()` guarantee convergence.

## Guarantees and Limitations

### Guarantees

1. **Convergence**: After all partitions have exchanged events, all nodes' `CausalGraph::frontier` clocks converge to the same value.
2. **Causality preservation**: The `happened_before` relationship is never violated by merge — if X causally preceded Y before the partition, X still causally precedes Y after merge.
3. **Deterministic ordering**: `topological_order()` produces a consistent ordering of concurrent events across all nodes (with the caveat about timestamps).

### Limitations

1. **No automatic partition detection**: The system does not proactively detect partitions. It relies on the gossip protocol's timeout and retry mechanisms. Active partition detection is planned for Sprint 3.
2. **Timestamp-based tiebreaker**: The `topological_order()` method uses wall-clock timestamps as a primary tiebreaker for concurrent events. In a partitioned network, clocks may drift, leading to inconsistent orderings. This is mitigated by the BFT consensus engine, which provides finality regardless of topological order.
3. **Unbounded vector clock growth**: Each node adds an entry to the vector clock. Over time, the `BTreeMap` in `VectorClock` grows linearly with the number of nodes. The `prune_below()` method can reclaim entries for nodes that are no longer active, but this must be done carefully to avoid losing causal information.
4. **Missing parent handling**: If a partition heals and events arrive before their parents (because the parent events are still being gossiped), the `CausalGraph::insert()` method will reject them with `CausalGraphError::MissingParent`. The gossip protocol must retry or request missing events. This is handled by the `EventRequest` / `EventBatch` gossip messages, but the retry logic is not yet implemented in Phase 0.
