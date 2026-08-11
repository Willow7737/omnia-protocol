# Consensus Pipeline Design

> 🎯 Audience: Developers
> 🔗 Context: Consensus pipeline, mempool, leader selection, and queue invariants
> 📅 Last Updated: 2026-08-11

## Overview

This document describes the consensus pipeline — the mechanism that makes consensus processing O(new_events) instead of O(total_events), the mempool for pending events, and the leader selection process.

## The Consensus Queue

The consensus queue (`Substrate::unprocessed_events`) is the mechanism that makes consensus processing O(new_events) instead of O(n). Without the queue, the consensus engine would need to scan the entire causal graph on each iteration to find unprocessed events.

### Why the Queue Makes Consensus O(new_events) Instead of O(n)

When a new event arrives (either locally via `submit_event()` or from the network via gossip), its `EventId` is appended to the queue. When `process_consensus()` runs, it drains the queue entirely:

```rust
pub async fn process_consensus(&mut self) -> Vec<EventId> {
    let graph = self.graph.read().await;
    let mut all_committed = Vec::new();
    let to_process: Vec<EventId> = self.unprocessed_events.drain(..).collect();
    for id in &to_process {
        if let Some(event) = graph.get_checked(id) {
            if let Ok(committed) = self.consensus.process_event(event, &graph) {
                all_committed.extend(committed);
            }
        }
    }
    all_committed
}
```

This makes each consensus iteration O(k), where k is the number of new events since the last iteration.

> Note: The actual consensus loop calls `process_consensus_round()` (not just `process_consensus()`), which: (1) drains gossip events, (2) checks leader duty and calls `propose_block()`, (3) calls `process_consensus()`, (4) forwards committed events to the shard processor.

## Queue Invariants

### Invariant 1: Every Event in `unprocessed_events` Is Already in the Graph

For every `EventId` in `unprocessed_events`, there exists a corresponding `Event` in `CausalGraph`. Events are inserted into the graph _before_ their IDs are added to the queue.

### Invariant 2: `process_consensus()` Drains the Queue Completely Each Call

After `process_consensus()` returns, `unprocessed_events` is empty. The method uses `self.unprocessed_events.drain(..).collect()`.

### Invariant 3: No Event Is Processed by Consensus Before It Is in the Graph

The `ConsensusEngine::process_event()` method is never called with an event that is not in the `CausalGraph`.

### Invariant 4: The Queue May Contain Duplicate Event IDs

There is no deduplication at the queue level. This is safe because `ConsensusEngine::process_event()` is idempotent — it returns `Ok(Vec::new())` for already-processed events.

## Duplicate Event Handling (3 Levels)

1. **Gossip deduplication**: `seen_events: HashSet<[u8; 32]>` — duplicates dropped before reaching the pending queue
2. **CausalGraph idempotency**: `CausalGraph::insert()` returns `CausalGraphError::DuplicateEvent`
3. **Consensus idempotency**: `ConsensusEngine::process_event()` checks `event_states.contains_key()` and returns early

## Mempool

A mempool for pending events with bounded size (default 10,000). Located in: `substrate/src/mempool.rs`.

## Leader Selection

VRF-based leader selection with stake weighting. `compute_leader()` called every round in the main consensus loop. Leader nodes produce proposal events via `propose_block()`.

- V1: Ed25519 signature + BLAKE3 derivation (legacy)
- V2: ECVRF with Fiat-Shamir + Ed25519 signatures (standard, target)
- `select_leader_v2()` — Version-aware leader selection

The 100ms sleep poll loop was replaced with `tokio::select!` + round timer. `process_consensus_round()` was extracted for clarity.

Located in: `substrate/src/vrf.rs`, `substrate/src/lib.rs`

See [ADR-012](../reference/adr-index.md#adr-012-vrf-construction-choice) and [ADR-015](../reference/adr-index.md#adr-015-leader-selection-consensus-loop) for the decision records.

## Consensus State Persistence

- `ConsensusStore` trait with `save_state()`, `load_state()`, `save_round()`, `load_round()`
- `RedbConsensusStore` using redb embedded database with ACID guarantees
- `ConsensusEngine::load_or_new()` restores from persisted state if available
- `persist_state()` called after every round advancement

Located in: `substrate/src/consensus_store.rs`

See [ADR-018](../reference/adr-index.md#adr-018-consensus-state-persistence) for the decision record.

## Performance Characteristics

| Operation           | Complexity      | Notes                                        |
| ------------------- | --------------- | -------------------------------------------- |
| Append to queue     | O(1)            | `Vec::push()` amortized                      |
| Drain queue         | O(k)            | k = number of unprocessed events             |
| Consensus per event | O(k × ancestry) | Ancestry checks are O(k) in worst case       |
| Total per iteration | O(k)            | Dominated by consensus, not queue management |

The queue ensures that consensus processing scales with the rate of new events, not with the total history size.

---

🔙 **Back**: [architecture/](./) | 🔄 **Related**: [trait-boundaries.md](./trait-boundaries.md)
🚀 **Next**: [crdt-convergence.md](./crdt-convergence.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)
