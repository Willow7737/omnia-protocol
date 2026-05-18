# Task 7 — H-3: Wire Leader Selection into Consensus Block Production

**Date**: 2026-03-05
**Agent**: code-agent
**Status**: Completed

## Summary

Wired VRF-based leader selection into the consensus block production loop. Previously, `ConsensusEngine::compute_leader()` existed but was never called from the main `run()` loop. There was no "produce block as leader" logic. Now the substrate checks VRF-selected leaders each round and drains the mempool to produce block proposals.

## Files Created

1. **`substrate/src/mempool.rs`** — New module implementing a bounded mempool for pending events:
   - `Mempool` struct with FIFO ordering and configurable max capacity
   - `insert()` — adds events, returns `MempoolError::Full` when at capacity
   - `drain_up_to()` — drains up to N events for block production
   - `remove_by_id()` — removes a specific event by ID
   - `contains()` — checks if an event is in the mempool
   - `len()`, `is_empty()`, `max_size()` — standard accessors
   - 8 unit tests covering insert/drain, partial drain, full capacity, empty drain, remove_by_id, contains, max_size, and drain-more-than-available

## Files Modified

1. **`substrate/src/lib.rs`** — Core integration changes:
   - Added `pub mod mempool;` module declaration
   - Added `pub use mempool::{Mempool, MempoolError};` re-export
   - Added `use std::collections::HashMap;` import
   - Added to `SubstrateConfig`:
     - `mempool_size: usize` (default: 10_000)
     - `max_block_events: usize` (default: 500)
   - Updated `SubstrateConfig::new()` and `with_network_size()` to include new defaults
   - Added to `Substrate` struct:
     - `mempool: Mempool` — pending events awaiting block inclusion
     - `max_block_events: usize` — max events per block proposal
     - `validator_candidates: HashMap<NodeId, (NodeKeypair, u64)>` — VRF candidate set
   - Updated `Substrate::new()` to initialize new fields
   - Added `with_validator_candidates()` — builder method for VRF candidates
   - Added `add_validator()` — convenience method for single validator registration
   - Added `mempool()` and `mempool_mut()` — accessors for the mempool
   - Added `propose_block()` — drains mempool, inserts events into graph, tracks unprocessed
   - Updated `run()` — added leader check step between gossip drain and consensus processing
   - Updated `submit_event()` — also adds events to mempool for block proposal tracking
   - Updated `process_consensus()` doc comment to mention `propose_block()` as a source

## Key Design Decisions

- **Mempool as secondary staging**: Events submitted via `submit_event()` are inserted into both the graph (for immediate propagation/consensus) and the mempool (for block proposal tracking). When `propose_block()` drains the mempool, events already in the graph are silently skipped via `CausalGraph::insert()` returning `DuplicateEvent`.

- **Validator candidates via builder pattern**: `validator_candidates` is populated via `with_validator_candidates()` builder method, keeping `SubstrateConfig` simple. If no validators are registered, the leader check is skipped (backward compatible).

- **Leader check in run loop**: Between gossip drain (step 1) and consensus processing (step 3), the run loop checks if this node is the VRF-selected leader. If so, it calls `propose_block()` to drain pending events.

- **Graceful duplicate handling**: `propose_block()` uses `if let Ok(()) = graph.insert(...)` to silently skip events already in the graph (from `submit_event()` or gossip), preventing double-processing.

## Verification

- `cargo check` — clean compilation, no errors
- `cargo test --lib` — all 363 tests pass including 8 new mempool tests
