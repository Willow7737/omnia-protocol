# Graph Insertion Optimization — Benchmark Comparison

> Sprint 3: Optimized Graph Insertion with Pre-allocated Data Structures

## Overview

This document tracks the performance comparison between the original
`CausalGraph` (HashMap-based) and the new optimized `PruningAwarePool`
(pre-allocated slab allocator + vector clock index).

## Baseline: HashMap-based CausalGraph

The original `CausalGraph` uses:
- `HashMap<EventId, Event>` for event storage → heap allocation on every insert
- `HashMap<EventId, usize>` for depth tracking → heap allocation per event
- `HashMap<NodeId, Vec<EventId>>` for creator index → heap allocation per creator
- Parent resolution via `HashMap::get(&EventId)` → hash + equality on 32-byte key

### Expected Bottlenecks

1. **Per-insert heap allocation**: Each `HashMap::insert()` may allocate
   memory for the new entry (if no spare capacity) and for the `Event`
   value's heap fields (e.g., `payload: Vec<u8>`).
2. **Hash computation**: Every lookup/insert requires hashing the 32-byte
   `EventId` key.
3. **Cache misses**: `HashMap` entries are heap-allocated at arbitrary
   addresses, causing cache misses during traversal.
4. **Rehashing**: When the HashMap exceeds its load factor, all entries
   are rehashed and relocated.

## Optimized: PruningAwarePool (Pre-allocated Slab + VectorClockIndex)

The new `PruningAwarePool` uses:
- `EventPool` (slab allocator) → pre-allocated slots, O(1) free list recycling
- `VectorClockIndex` → O(1) parent resolution via (creator, sequence) indexing
- Free slot reuse after pruning → no allocation for events inserted into
  freed slots

### Expected Improvements

1. **Zero per-insert heap allocation** for events placed in pre-allocated slots
2. **O(1) parent resolution** without hashing EventId keys
3. **Better cache locality** — events stored in contiguous `Vec<Slot>`
4. **No rehashing** — slots are addressed by index, not hash

## Benchmark Results

### Insertion Latency

| Metric | CausalGraph (HashMap) | PruningAwarePool (Slab) | Improvement |
|--------|----------------------|------------------------|-------------|
| p50 | _TBD_ | _TBD_ | _TBD_ |
| p95 | _TBD_ | _TBD_ | _TBD_ |
| p99 | _TBD_ | _TBD_ | ≥60% |
| Mean | _TBD_ | _TBD_ | _TBD_ |

### Throughput

| Metric | CausalGraph (HashMap) | PruningAwarePool (Slab) | Improvement |
|--------|----------------------|------------------------|-------------|
| Events/sec (1 node) | _TBD_ | _TBD_ | _TBD_ |
| Events/sec (10 nodes) | _TBD_ | _TBD_ | _TBD_ |
| Events/sec (100 nodes) | _TBD_ | _TBD_ | _TBD_ |

### Memory Usage

| Metric | CausalGraph (HashMap) | PruningAwarePool (Slab) | Improvement |
|--------|----------------------|------------------------|-------------|
| Peak RSS (10K events) | _TBD_ | _TBD_ | _TBD_ |
| Peak RSS (100K events) | _TBD_ | _TBD_ | _TBD_ |
| Memory after pruning | _TBD_ | _TBD_ | _TBD_ |
| Fragmentation | _TBD_ | _TBD_ | _TBD_ |

### Pool Utilization Stats

| Metric | Value |
|--------|-------|
| Initial capacity | Configurable (default 1024) |
| Growth factor | 1.5x |
| Max capacity | Configurable (default 1M) |
| Steady-state utilization | _TBD_ |
| Growth count (10K inserts) | _TBD_ |

## Methodology

### Hardware

- CPU: _TBD_
- RAM: _TBD_
- OS: _TBD_

### Software

- Rust toolchain: _TBD_
- Profile: `--release` with `lto = "thin"`

### Test Scenarios

1. **Sequential insert**: Single creator, monotonic sequence numbers
2. **Multi-creator insert**: 10/100/1000 concurrent creators
3. **Insert-prune cycle**: Insert 10K events, finalize, prune, repeat
4. **Steady-state**: 1M events with continuous insert/prune at 10K events/sec

## Running the Benchmarks

```bash
# Run all benchmarks
cargo bench --bench throughput -- graph-insert

# Run with specific event count
cargo bench --bench throughput -- graph-insert --events 100000

# Compare before/after
cargo bench --bench baseline -- graph-insert  # baseline (HashMap)
cargo bench --bench throughput -- graph-insert  # optimized (Slab)
```

## Notes

- All measurements use `std::time::Instant` for sub-nanosecond precision
- p50/p95/p99 are computed over 10,000 iterations
- Memory measurements use `/proc/self/status` on Linux (VmRSS)
- Results are averages over 3 runs, discarding the warm-up run
