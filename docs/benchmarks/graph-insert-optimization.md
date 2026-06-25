# Graph Insertion Optimization — Benchmark Comparison

> Sprint 3: Optimized Graph Insertion with Pre-allocated Data Structures

## Overview

This document tracks the performance comparison between the original
`CausalGraph` (HashMap-based) and the new optimized `PruningAwarePool`
(pre-allocated slab allocator + vector clock index).

## Baseline: HashMap-based CausalGraph (v0.1.48 Measured)

The current `CausalGraph` uses HashMap-based storage. Measured performance:

### Measured Insertion Latency (v0.1.48)

| Pre-fill        | p50      | p95      | p99       | mean     |
| --------------- | -------- | -------- | --------- | -------- |
| **0 events**    | 18.09 µs | 22.64 µs | 26.26 µs  | 18.49 µs |
| **100 events**  | 18.21 µs | 23.36 µs | 30.85 µs  | 18.81 µs |
| **1000 events** | 18.28 µs | 25.03 µs | 145.46 µs | 21.36 µs |

### Key Observations

1. **p50 insertion is flat at ~18 µs** across all graph sizes — O(1) amortized as expected
2. **p99 spikes at 1000 events** (145 µs) — likely HashMap rehashing when load factor exceeded
3. **Mean insertion increases slightly** with graph size due to occasional rehashing
4. **Graph insertion in full pipeline** (create genesis + sign + insert child): 39.66 µs p50

### Expected Bottlenecks

1. **Per-insert heap allocation**: Each `HashMap::insert()` may allocate
   memory for the new entry and for the `Event` value's heap fields.
2. **Hash computation**: Every lookup/insert requires hashing the 32-byte `EventId` key.
3. **Cache misses**: `HashMap` entries are heap-allocated at arbitrary
   addresses, causing cache misses during traversal.
4. **Rehashing**: When the HashMap exceeds its load factor, all entries
   are rehashed and relocated (visible in p99 spike at 1000 events).

## Optimized: PruningAwarePool (Pre-allocated Slab + VectorClockIndex)

The new `PruningAwarePool` uses:

- `EventPool` (slab allocator) → pre-allocated slots, O(1) free list recycling
- `VectorClockIndex` → O(1) parent resolution via (creator, sequence) indexing
- Free slot reuse after pruning → no allocation for events inserted into freed slots

### Expected Improvements

1. **Zero per-insert heap allocation** for events placed in pre-allocated slots
2. **O(1) parent resolution** without hashing EventId keys
3. **Better cache locality** — events stored in contiguous `Vec<Slot>`
4. **No rehashing** — slots are addressed by index, not hash
5. **p99 should drop significantly** — no rehashing spikes

### Predicted Performance (Based on HashMap Baseline)

| Metric | CausalGraph (HashMap) | PruningAwarePool (Slab) | Predicted Improvement |
| ------ | --------------------- | ----------------------- | --------------------- |
| p50    | 18.09 µs              | ~10-12 µs               | ~35-45%               |
| p95    | 22.64 µs              | ~12-14 µs               | ~40-45%               |
| p99    | 145.46 µs (at 1K)     | ~15-18 µs               | ~88-90%               |
| Mean   | 21.36 µs (at 1K)      | ~11-13 µs               | ~40-50%               |

## Benchmark Results

### Insertion Latency

| Metric | CausalGraph (HashMap) | PruningAwarePool (Slab) | Improvement  |
| ------ | --------------------- | ----------------------- | ------------ |
| p50    | 18.09 µs              | _TBD_                   | _TBD_        |
| p95    | 22.64 µs              | _TBD_                   | _TBD_        |
| p99    | 26.26–145.46 µs       | _TBD_                   | Target: ≥60% |
| Mean   | 18.49–21.36 µs        | _TBD_                   | _TBD_        |

### Throughput

| Metric                 | CausalGraph (HashMap) | PruningAwarePool (Slab) | Improvement |
| ---------------------- | --------------------- | ----------------------- | ----------- |
| Events/sec (1 node)    | 7,190                 | _TBD_                   | _TBD_       |
| Events/sec (10 nodes)  | _TBD_                 | _TBD_                   | _TBD_       |
| Events/sec (100 nodes) | _TBD_                 | _TBD_                   | _TBD_       |

### Memory Usage

| Metric                 | CausalGraph (HashMap) | PruningAwarePool (Slab) | Improvement |
| ---------------------- | --------------------- | ----------------------- | ----------- |
| Peak RSS (10K events)  | ~22.5 MB              | _TBD_                   | _TBD_       |
| Peak RSS (100K events) | _TBD_                 | _TBD_                   | _TBD_       |
| Memory after pruning   | _TBD_                 | _TBD_                   | _TBD_       |
| Fragmentation          | _TBD_                 | _TBD_                   | _TBD_       |

### Pool Utilization Stats

| Metric                     | Value                       |
| -------------------------- | --------------------------- |
| Initial capacity           | Configurable (default 1024) |
| Growth factor              | 1.5x                        |
| Max capacity               | Configurable (default 1M)   |
| Steady-state utilization   | _TBD_                       |
| Growth count (10K inserts) | _TBD_                       |

## Methodology

### Hardware

- CPU: heterogeneous Intel/AMD (GitHub Actions ubuntu-latest)
- RAM: 8 GiB
- OS: Linux 5.10.134 (x86_64, cloud instance)

### Software

- Rust toolchain: rustc 1.91.0 (59807616e 2026-04-14)
- Profile: `--release` (opt-level=2, no LTO, codegen-units=16)

### Test Scenarios

1. **Sequential insert**: Single creator, monotonic sequence numbers
2. **Multi-creator insert**: 10/100/1000 concurrent creators
3. **Insert-prune cycle**: Insert 10K events, finalize, prune, repeat
4. **Steady-state**: 1M events with continuous insert/prune at 10K events/sec

## Running the Benchmarks

```bash
# Run current CausalGraph baseline
cargo bench --bench baseline_bench -- dag_insert

# Run PruningAwarePool benchmarks (when implemented)
cargo bench --bench throughput -- graph-insert
```

## Notes

- All measurements use `std::time::Instant` for sub-nanosecond precision
- p50/p95/p99 computed over 500 iterations with 50 warmup
- Memory measurements use `/proc/self/status` on Linux (VmRSS)
- Results are averages over multiple runs, discarding the warm-up run
- Baseline measured: 2026-05-23, commit d52b7da (v0.1.48)
