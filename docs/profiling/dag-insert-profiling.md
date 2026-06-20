# DAG Insert Profiling Guide

> Sprint 3: How to profile and verify zero heap allocations in the optimized DAG insertion path

## Overview

This document explains how to generate flamegraphs, profile allocation
hot paths, and verify that the `PruningAwarePool` achieves zero heap
allocations in the steady-state insertion path.

## 1. Flamegraph Generation

### Prerequisites

```bash
# Install perf (Linux)
sudo apt-get install linux-perf

# Install flamegraph tools
cargo install flamegraph
```

### Generating a Flamegraph for DAG Insertion

```bash
# Build with debug info in release mode
CARGO_PROFILE_RELEASE_DEBUG=true cargo flamegraph \
    --bench throughput \
    -- graph-insert \
    --flamegraph dag-insert.svg
```

### Expected Hot Path

In the optimized path, the flamegraph should show:

1. `PruningAwarePool::insert` → `EventPool::insert` → slot assignment
2. `VectorClockIndex::index_event` → HashMap entry + Vec push
3. Minimal time in `alloc::alloc` — only during initial pool growth

In the **unoptimized** path (HashMap), you'll see significant time in:

1. `HashMap::insert` → hash computation → allocation → rehashing
2. `HashMap::get` → hash computation → collision resolution

## 2. Allocation Profiling with dhat

[dhat](https://crates.io/crates/dhat) is a Rust allocation profiler that
tracks every allocation and produces a detailed report.

### Setup

Add to `Cargo.toml`:

```toml
[dev-dependencies]
dhat = "0.3"
```

### Usage

```rust
#[cfg(test)]
mod allocation_tests {
    use dhat::DhatAlloc;

    #[global_allocator]
    static ALLOC: DhatAlloc = DhatAlloc;

    #[test]
    fn test_zero_allocations_in_steady_state() {
        let _profiler = dhat::Profiler::builder().testing().build();

        // Pre-warm: fill pool to capacity
        let mut pool = PruningAwarePool::new(10000, 100000);
        for i in 0..10000 {
            pool.insert(make_event(i)).unwrap();
        }

        // Prune some events to create free slots
        // ... (finalize + prune)

        // Now measure: inserting into free slots should be zero-alloc
        let stats_before = dhat::HeapStats::get();

        for i in 10000..10100 {
            pool.insert(make_event(i)).unwrap();
        }

        let stats_after = dhat::HeapStats::get();

        // Verify: no new heap allocations
        assert_eq!(
            stats_after.total_blocks - stats_before.total_blocks,
            0,
            "Expected zero heap allocations in steady state"
        );
    }
}
```

### Expected Results

| Phase                                    | Allocations per Insert |
| ---------------------------------------- | ---------------------- |
| Initial fill (no free slots, pool grows) | 1-2 (Vec resize)       |
| Steady state (free slots available)      | **0**                  |
| After pruning (slots recycled)           | **0**                  |

## 3. Heap Profiling with Massif (Valgrind)

Massif measures heap memory usage over time, showing allocation
breakdown by call site.

### Usage

```bash
# Build in release with debug info
CARGO_PROFILE_RELEASE_DEBUG=true cargo build --release --bench throughput

# Run under Massif
valgrind --tool=massif --stacks=yes \
    target/release/deps/throughput-* --test graph-insert

# Visualize
ms_print massif.out.*
```

### Expected Patterns

**CausalGraph (HashMap):**

- Heap grows monotonically with each insert
- Periodic jumps when HashMap rehashes
- No significant decrease after pruning (HashMap doesn't shrink)

**PruningAwarePool (Slab):**

- Initial allocation for pre-allocated slots
- Growth events are smooth (1.5x factor)
- After pruning: heap doesn't grow (slots are reused)
- Steady-state: flat heap profile

## 4. Counting Allocations with `std::alloc::GlobalAlloc`

For precise allocation counting without external tools:

```rust
use std::alloc::{GlobalAlloc, Layout};
use std::sync::atomic::{AtomicUsize, Ordering};

struct AllocCounter;

static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for AllocCounter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.fetch_add(1, Ordering::SeqCst);
        std::alloc::System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        std::alloc::System.dealloc(ptr, layout)
    }
}

#[global_allocator]
static ALLOCATOR: AllocCounter = AllocCounter;
```

### Expected Allocation Count

| Operation             | CausalGraph | PruningAwarePool            |
| --------------------- | ----------- | --------------------------- |
| Insert (cold)         | 3-5 allocs  | 1-2 allocs (pool growth)    |
| Insert (steady-state) | 3-5 allocs  | **0 allocs**                |
| Lookup                | 0 allocs    | 0 allocs                    |
| Remove/Prune          | 0 allocs    | 0 allocs (free list update) |

## 5. Cache Performance with `perf stat`

```bash
# Measure cache performance
perf stat -e cache-misses,cache-references,L1-dcache-load-misses \
    cargo run --release --bench throughput -- graph-insert
```

### Expected Cache Improvement

The slab allocator stores events in a contiguous `Vec<Slot>`, which
should produce significantly fewer cache misses than the HashMap's
scattered heap allocations.

| Metric                | CausalGraph | PruningAwarePool |
| --------------------- | ----------- | ---------------- |
| L1-dcache load misses | _TBD_       | _TBD_            |
| Cache miss rate       | _TBD_       | _TBD_            |

## 6. Criterion Benchmarks

For statistically rigorous latency measurements, use Criterion.rs:

```bash
cargo bench --bench throughput -- graph-insert
```

The benchmark reports p50, p95, p99, and mean latency with confidence
intervals. Look for:

- p99 latency reduction ≥ 60% (Sprint 3 target)
- No regression in p50 latency
- Stable results across multiple runs

## Verification Checklist

- [ ] Flamegraph shows `PruningAwarePool::insert` as hot path (not `HashMap::insert`)
- [ ] dhat reports **zero allocations** in steady-state insert
- [ ] Massif shows **flat heap profile** during steady-state operation
- [ ] `perf stat` shows **fewer cache misses** than HashMap baseline
- [ ] Criterion reports **≥60% p99 latency reduction**
