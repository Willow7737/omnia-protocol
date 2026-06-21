# ADR-001: EventProcessor Trait Contract

> 🎯 Audience: Architects
> 🔗 Context: Part of the adr documentation section
> 📅 Last Updated: 2026-05-20

**Status**: Accepted
**Date:** 2026-05-14
**Decision**: Define a minimal, synchronous `EventProcessor` trait as the interface between the substrate's committed event stream and Layer 2 shard processors.

## Context

The Omnia substrate (`substrate/src/lib.rs`) manages a causal graph of events and a BFT consensus engine. Once events achieve consensus finality (i.e., are committed), they must be forwarded to downstream consumers — the domain shard processors. The substrate treats these processors as opaque: it does not know or care about their internal structure, only that they can accept a committed event.

The design must satisfy several constraints:

1. **Only committed events are forwarded.** Shards must never observe un-finalized state that could later be rolled back. The `Substrate::run()` loop explicitly forwards only events returned by `process_consensus()`, which drains `unprocessed_events` and returns only those that achieve `ConsensusState::Committed`.

2. **Shard processor failure must not halt the substrate.** The substrate is the backbone of the entire protocol; if a shard processor panics or returns an error, the substrate must continue processing. Errors are logged via `tracing::warn!` but never propagated upward.

3. **Thread safety is required.** The substrate runs in an async Tokio runtime, and the processor may be called from multiple tasks. The trait must enforce `Send + Sync` bounds.

4. **The interface should be maximally simple.** The `EventProcessor` trait is a boundary between two crates (`omnia-substrate` and downstream consumers). A complex trait with structured error types would impose coupling; a simple `Result<(), String>` keeps the contract minimal.

## Decision

We adopt the following trait definition (as implemented in `substrate/src/lib.rs`):

```rust
pub trait EventProcessor: Send + Sync {
    fn process_event(&mut self, event: &Event) -> std::result::Result<(), String>;
}
```

### Key Design Choices

**Error type is `String`, not a structured error enum.** The `EventProcessor` trait is the boundary between the substrate and arbitrary shard implementations. Using `String` avoids forcing every shard to depend on a shared error crate or to convert between error hierarchies. The substrate does not act on the error content — it only logs it — so structured error information provides no benefit at this interface. If a shard needs richer error handling internally, it can use its own `ShardError` enum (as `shards/src/shard.rs` does) and convert to `String` at the boundary.

**`&mut self` rather than `&self`.** The processor is allowed to mutate its own state when processing an event. This is essential for shards that maintain internal state machines (e.g., `FinancialState` applies balance mutations). The substrate holds the processor as `Option<Box<dyn EventProcessor>>` and calls it sequentially, so no additional synchronization is needed within a single processor.

**`&Event` rather than owned `Event`.** The substrate retains ownership of the event in the `CausalGraph`. Passing a reference avoids cloning overhead and makes it clear that the processor cannot modify or store the event without explicit cloning.

### How the Substrate Run Loop Feeds Events

The `Substrate::run()` method (defined in `substrate/src/lib.rs`) follows this loop on each iteration:

1. **Drain gossip events.** If gossip is active, `gossip.process_pending_events()` is called, which drains the `network_rx` channel, deserializes `Event::from_bytes()`, validates signatures, and inserts events into the `CausalGraph` via `graph.insert()`. Newly inserted event IDs are added to `unprocessed_events`.

2. **Run consensus.** `process_consensus()` drains `unprocessed_events`, feeding each event through `consensus.process_event()`. Events that achieve `ConsensusState::Committed` are returned.

3. **Forward committed events to shard processor.** Only after consensus completes, the substrate iterates over the committed event IDs, looks them up in the graph via `graph.get(event_id)`, and calls `processor.process_event(event)`. Errors are caught and logged:

   ```rust
   if let Err(e) = processor.process_event(event) {
       tracing::warn!("Shard processor error for event {}: {}",
           hex::encode(&event_id[..4]), e);
   }
   ```

This ordering guarantees that shards only ever see finalized events, never pending or gossiped ones.

### Usage in the Node Binary

The `omnia-node` binary does not use the `EventProcessor` trait directly for its REST API. Instead, the API handlers in `node/src/api/` interact with the substrate, shard router, and economics state through the shared `AppState`. The `ShardRouter` in `omnia-shards` acts as the primary event processor in the substrate's `run()` loop, routing events to individual shard implementations.

## Consequences

- **Positive**: Clean separation between substrate and shard logic. The substrate remains unaware of shard internals.
- **Positive**: Error isolation — a misbehaving shard cannot crash or stall the substrate.
- **Positive**: Simple trait signature makes it trivial to implement mock processors for testing.
- **Negative**: `String` errors lose type information at the boundary. Downstream consumers cannot programmatically match on error variants from the `EventProcessor` trait. This is acceptable because the substrate never acts on the error content.
- **Negative**: Sequential processing within a single `Box<dyn EventProcessor>`. If parallel shard processing is needed in the future, the substrate would need to support multiple processors or a processor router that fans out internally.
- **Trade-off**: The `&mut self` receiver means the processor cannot be called concurrently from multiple threads without external synchronization. This is intentional — the substrate processes events in causal order, and concurrent processing would violate ordering guarantees.

---

🔙 **Back**: [ADR Index](./) | 🔄 **Related**: [ADR Index](../reference/adr-index.md)
🚀 **Next**: [ADR Index](../reference/adr-index.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)
