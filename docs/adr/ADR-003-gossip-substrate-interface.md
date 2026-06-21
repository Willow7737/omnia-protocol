# ADR-003: Gossip → Substrate Event Flow

> 🎯 Audience: Architects
> 🔗 Context: Part of the adr documentation section
> 📅 Last Updated: 2026-05-20

**Status**: Accepted
**Date:** 2026-05-14
**Decision**: Define the event flow from gossip network reception through the substrate's causal graph insertion and consensus queue population.

## Context

The Omnia Protocol uses an epidemic gossip protocol to propagate events across the network. When a node receives an event from a peer, that event must traverse a well-defined path before it reaches the consensus engine and, ultimately, the shard processors. This flow must satisfy several invariants:

1. **Only valid events enter the causal graph.** Every event must pass signature verification and hash integrity checks before insertion.
2. **Invalid events must never panic.** Malformed or malicious events are logged and dropped, never causing the node to crash.
3. **Backpressure must be handled gracefully.** If events arrive faster than they can be processed, the system must degrade gracefully rather than run out of memory.
4. **The graph and the consensus queue must stay consistent.** Every event ID in `unprocessed_events` must correspond to an event already in the `CausalGraph`.

The gossip protocol is implemented in `substrate/src/gossip.rs`, the causal graph in `substrate/src/causal_graph.rs`, and the substrate coordinator in `substrate/src/lib.rs`.

## Decision

### Event Flow

The complete event flow from gossip to consensus is:

```
Gossip receives raw bytes → validates signature → converts to Event → calls graph.insert() → adds to unprocessed_events
```

Concretely, this flow is implemented as follows:

**Step 1: Network Reception.** The `OmniaNetwork` (in `substrate/src/network.rs`) receives a gossip message via libp2p GossipSub. It emits a `NetworkEvent::GossipReceived { topic, data, propagation_source }` through the `event_tx` channel.

**Step 2: Draining into Pending Queue.** The `GossipProtocol::process_pending_events()` method (in `substrate/src/gossip.rs`) drains the `network_rx` channel. For each `GossipReceived` event, it attempts to deserialize the raw bytes into an `Event`:

```rust
match Event::from_bytes(&data) {
    Ok(event) => {
        if !self.seen_events.contains(&event.id) {
            self.seen_events.insert(event.id);
            self.pending_events.push_back(event);
            self.stats.events_received += 1;
        }
    }
    Err(e) => {
        warn!("Failed to deserialize gossip event: {:?}", e);
        self.stats.events_rejected += 1;
    }
}
```

**Step 3: Graph Insertion.** After draining the network channel, `process_pending_events()` inserts each pending event into the shared `CausalGraph`:

```rust
let mut graph = self.graph.write().await;
match graph.insert(event.clone()) {
    Ok(_) => {
        self.stats.events_accepted += 1;
        inserted_ids.push(event.id);
    }
    Err(CausalGraphError::DuplicateEvent(_)) => {
        self.stats.events_rejected += 1;
    }
    Err(e) => {
        warn!("Failed to insert event: {}", e);
        self.stats.events_rejected += 1;
    }
}
```

The `CausalGraph::insert()` method validates the event hash (`verify_hash()`), checks parent existence, and detects cycles. Duplicate events are silently rejected — `CausalGraphError::DuplicateEvent` is an idempotent no-op.

**Step 4: Queue Population.** The `process_pending_events()` return value (`Vec<EventId>`) is collected by the `Substrate::run()` loop and appended to `unprocessed_events`:

```rust
if let Some(ref mut gossip) = self.gossip {
    match gossip.process_pending_events().await {
        Ok(inserted) => {
            self.unprocessed_events.extend(inserted);
        }
        Err(e) => {
            tracing::warn!("Gossip processing error: {}", e);
        }
    }
}
```

**Step 5: Consensus Processing.** The `process_consensus()` method drains `unprocessed_events` and feeds each event through `consensus.process_event()`, returning committed event IDs.

### Parallel Event Path: REST API

In addition to the gossip path, the `omnia-node` binary provides a second event submission path through the REST API (`POST /api/v1/events`). The API handler in `node/src/api/events.rs`:

1. Decodes the hex payload from the request body
2. Checks payload size against `omnia_substrate::MAX_PAYLOAD_SIZE`
3. Creates a `Event::genesis()` with the node's configured `node_id_bytes()`
4. Signs the event with a fresh `generate_keypair()` keypair
5. Submits the event to the substrate via `substrate.write().await.submit_event(event).await`
6. Stores a simplified `StoredEvent` in the in-memory `event_store` for later retrieval
7. Increments the `omnia_node_events_submitted_total` Prometheus counter

**Important difference from gossip path:** The API path creates a new keypair for each event submission rather than reusing a persistent identity. This means each API-submitted event has a different `creator` field (derived from `blake3(pubkey)`), which affects equivocation detection and slashing.

### Error Handling: Invalid Events Are Logged and Dropped

At every stage of the pipeline, errors are caught and logged without propagating:

- **Deserialization failure**: Logged via `tracing::warn!`, event rejected, stats incremented.
- **Graph insertion failure** (invalid hash, missing parent, cycle): Logged via `tracing::warn!`, event rejected.
- **Duplicate event**: Silently accepted (idempotent), stats incremented as rejected.
- **Gossip processing error** (returned from `process_pending_events()`): Logged at the substrate level, but the substrate loop continues.
- **API payload too large**: Returns HTTP 413 (Payload Too Large) to the client.
- **Invalid hex payload**: Returns HTTP 400 (Bad Request) to the client.
- **Event submission failure**: Event is still stored in the event store with `status: "submission_failed"`.

At no point does an invalid event cause a panic or halt the substrate. This is a critical safety invariant for a network-facing component.

### Backpressure: Queue Bounds

The `GossipConfig` (in `substrate/src/gossip.rs`) defines a `max_pending` field with a default of `MAX_PENDING_EVENTS` (100,000). This bounds the `pending_events` deque. If the pending queue exceeds `max_pending`, the oldest events are dropped. This is configurable per-node, allowing operators to tune memory usage vs. event delivery guarantees.

The `seen_events` HashSet provides deduplication at the gossip level, preventing the same event from being processed twice. This set grows unboundedly in the current implementation, which is a known limitation — future work should add periodic pruning of old entries.

### Shared Graph Ownership

The `GossipProtocol` holds `Arc<RwLock<CausalGraph>>`, shared with the `Substrate`. This allows the gossip protocol to insert events directly into the graph (for network-received events) without going through the substrate's `submit_event()` method. The `RwLock` ensures that concurrent reads (e.g., consensus ancestry checks) are not blocked by insertions.

The substrate also inserts events into the graph via `submit_event()` (for locally created events), which acquires a write lock on the same `Arc<RwLock<CausalGraph>>`.

### The `process_pending_events()` Bridge

The `process_pending_events()` method is the bridge between the P2P network and consensus. It is called once per iteration of the `Substrate::run()` loop. It:

1. Drains all available events from the `network_rx` channel (non-blocking, using `try_recv()`).
2. Inserts all pending events into the graph.
3. Returns the IDs of successfully inserted events.

This design ensures that network events land in the graph where `Substrate::process_consensus()` can pick them up. The substrate's `unprocessed_events` queue is then populated with these IDs, making them available for the next consensus round.

## Consequences

- **Positive**: Clean separation of concerns — the gossip protocol handles network I/O and deserialization, the causal graph handles structural validation, and the substrate handles consensus routing.
- **Positive**: Error isolation — malformed or malicious events cannot crash the node.
- **Positive**: Idempotency — duplicate events (from gossip retransmission) are silently handled by `CausalGraph::insert()` returning `DuplicateEvent`.
- **Positive**: Dual event path — events can enter the system via gossip (network) or the REST API (local), both feeding into the same substrate pipeline.
- **Negative**: The `seen_events` HashSet grows unboundedly. In a long-running node with high throughput, this could consume significant memory. A pruning strategy (e.g., LRU eviction) is needed.
- **Negative**: Backpressure via dropping oldest events means that under extreme load, events may be silently lost. For a BFT system, this is acceptable because events are gossiped to multiple nodes — another node will process them.
- **Negative**: The REST API event path creates a fresh keypair per submission, which means each API-submitted event has a unique creator. This prevents equivocation detection for API-submitted events (since each uses a different key), but also means these events cannot be attributed to a persistent identity.
- **Trade-off**: The `Arc<RwLock<CausalGraph>>` pattern means that graph insertion requires an async write lock. Under high contention (many concurrent insertions), this could become a bottleneck. However, the lock is held only for the duration of `insert()`, which is O(1) for the HashMap lookup plus O(k) for ancestry checks.

---

🔙 **Back**: [ADR Index](./) | 🔄 **Related**: [ADR Index](../reference/adr-index.md)
🚀 **Next**: [ADR Index](../reference/adr-index.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)
