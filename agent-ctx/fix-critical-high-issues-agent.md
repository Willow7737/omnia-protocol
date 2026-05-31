# Task: Fix CRITICAL and HIGH severity issues in omnia-consensus, fuzz, benches, and tests crates

## Summary of Changes

### omnia-consensus crate

1. **consensus.rs - Remove default all-zero round_seed**: Changed `ConsensusConfig::default()` to use `with_random_seed(4)` instead of hard-coded `[0u8; 32]`. Falls back to all-zero only if `getrandom` fails (should never happen). Added `default_round_seed()` function for serde deserialization backward compatibility. Updated doc comments.

2. **consensus.rs - Add TODO about persisting first_event_for_sequence**: Added comprehensive TODO comment at the `restore_state()` method about persisting `first_event_for_sequence`, covering bounding, rebuilding, and incremental persistence.

3. **consensus.rs - Fix is_witness regression**: Reverted `>` back to `>=` in `is_witness()` method. The previous agent's change from `>=` to `>` broke round-0 witness detection because `0 > 0` is false, preventing genesis events from being classified as witnesses.

4. **batch.rs - Re-buffer events on batch creation failure**: Added pre-validation of batch parameters before draining the buffer. If the buffer exceeds `max_batch_size`, it returns `None` without draining (events remain buffered). Added `BatchTooLarge` check and rollback of `batch_sequence` counter on unexpected failures.

5. **causal_graph.rs - Move depth check before mutations**: Moved the depth overflow check (`depth > MAX_ANCESTRY_DEPTH`) to BEFORE the graph mutations (removing parents from tips, updating creator index, node sequences, frontier, etc.). Previously, if the depth check failed, all prior mutations had already been applied but the event was not stored, leaving the graph in an inconsistent state.

### Already-fixed issues (no changes needed)
- mempool.rs: Already has `HashSet<EventId>` for O(1) membership + duplicate check
- or_set.rs: `state_hash()` already includes `removes`; `merge()` already takes max of sequence counters; `len()` already avoids allocation
- rate_limiter.rs: Already uses `saturating_add` for token refill
- event_pool.rs: Growth factor already uses `self.slots.len()` instead of `initial_capacity`

### fuzz crate

6. **shard_route.rs**: Replaced `Event::genesis(creator, payload).unwrap()` with `if let Ok(event) = Event::genesis(...)` pattern.

7. **causal_graph_insert.rs**: Replaced both `.unwrap()` calls on `Event::genesis()` and `Event::new()` with `if let Ok(...)` pattern.

8. **event_validate.rs**: Replaced both `.unwrap()` calls on `Event::genesis()` and `Event::new()` with `if let Ok(...)` pattern.

9. **fuzz_consensus_state_transition.rs**: Made `CausalGraph` mutable and insert events into it before calling `process_event`, so that ancestry queries work correctly. Falls back to using the original event if graph insertion fails.

10. **vector_clock_merge.rs**: Added comment explaining why this target is intentionally separate from `fuzz_vector_clock_merge.rs` (different input grammar and code paths).

### benches crate

11. **throughput.rs - Use iter_batched for merge benchmark**: Changed `merge_100_nodes` to use `iter_batched` with `vc_a.clone()` as setup, excluding clone time from the measured iteration.

12. **throughput.rs - Fix insert_chain benchmark topology**: Changed `Some(genesis.id)` to `Some(last_id)` so each event in the chain references the previous event as self_parent, creating an actual chain topology instead of all events referencing genesis.

13. **hot_path_iai.rs - Generate keypair once**: Replaced per-call `generate_keypair()` with `OnceLock<NodeKeypair>` static, so the keypair is generated only once and reused across all benchmark iterations. This prevents expensive key generation from dominating instruction counts.

14. **baseline_bench.rs - Change total_nodes from 1 to 3**: Changed `total_nodes: 1` to `total_nodes: 3` in both the tx_throughput and finality_latency benchmarks to match documentation describing 3-node BFT finality.

15. **sharding_bench.rs - Pre-allocate HashMap**: Changed `HashMap::new()` to `HashMap::with_capacity(size)` for `event_states` and `event_rounds` in the single-threaded hashmap benchmark to avoid rehashing overhead.

### Unrelated fix
- substrate/src/lib.rs: Removed `aggregate_signatures_unchecked` from the re-export list since it doesn't exist in `bls` module (was causing a compile error).
