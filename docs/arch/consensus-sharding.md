# Architecture RFC: Consensus State Sharding

| RFC ID   | Title                    | Status   | Sprint   |
| -------- | ------------------------ | -------- | -------- |
| ARCH-001 | Consensus State Sharding | Approved | Sprint 1 |

## Summary

Shard the consensus engine's event state by EventId hash to enable parallel
event processing, targeting ≥2× throughput improvement on single-node benchmarks.

## Motivation

The current `ConsensusEngine<S>` holds all consensus state in a single struct
with `HashMap` fields accessed via `&mut self`. This means all event processing
is serialized through a single mutable reference, creating a throughput bottleneck
even on multi-core systems.

Key observations:

- Event processing is **embarrassingly parallel** for events that don't share
  ancestry — they can be validated, assigned rounds, and tracked independently.
- The consensus state is **read-heavy**: most operations look up existing state
  (event_states, event_rounds) rather than modifying it.
- Cross-shard coordination is only needed for **witness recording** and
  **finality determination**, which are relatively infrequent.

## Sharding Strategy

### Shard Key: First Byte of EventId

The `EventId` is a 32-byte SHA-256 hash. We use the **first byte** as the
shard key, giving us 256 shards.

**Why the first byte?**

- SHA-256 output is uniformly distributed, so events spread evenly across shards.
- Simple to compute (no additional hashing needed).
- 256 shards provides fine granularity for parallelism while keeping per-shard
  memory overhead low (one `RwLock` + three `HashMap`s per shard).

**Why not more or fewer shards?**

- Fewer shards (e.g., 16) would limit parallelism.
- More shards (e.g., 65536) would increase memory overhead without proportional
  benefit (we're limited by CPU cores, not by lock contention on 256 shards).

### Shard Contents

Each shard holds the per-event data that can be partitioned by EventId:

| Field                      | Shard Key     | Lock             |
| -------------------------- | ------------- | ---------------- |
| `event_states`             | EventId       | Per-shard RwLock |
| `event_rounds`             | EventId       | Per-shard RwLock |
| `fame_status`              | EventId       | Per-shard RwLock |
| `round_witnesses`          | Round         | Global RwLock    |
| `node_info`                | NodeId        | Global RwLock    |
| `first_event_for_sequence` | (NodeId, u64) | Global RwLock    |
| `committed_count`          | —             | Global RwLock    |

## Locking Strategy

### Per-Shard RwLock

Each shard is protected by a `std::sync::RwLock`. This allows:

- **Concurrent reads**: Multiple threads can read from the same shard simultaneously.
- **Exclusive writes**: Only one thread can write to a shard at a time.
- **Cross-shard parallelism**: Threads accessing different shards never contend.

### Global RwLock

Cross-shard state (`round_witnesses`, `node_info`, `first_event_for_sequence`,
`committed_count`) is protected by a single global `RwLock`. This is a
bottleneck for cross-shard operations, but:

1. **Witness recording** is relatively infrequent (once per round per node).
2. **Node info updates** are infrequent (once per event per node).
3. **Equivocation tracking** is rare (only on equivocation, which is exceptional).

The global lock is not on the critical path for the common case of event
state lookups and insertions.

### Poison Recovery

All `RwLock` acquisitions use `unwrap_or_else(|e| e.into_inner())` to recover
from lock poisoning. If a thread panics while holding a lock, we recover the
lock rather than propagating the panic. This prevents a single bug from
deadlocking the entire system.

## Thread Pool Design

### Architecture

```
                    ┌──────────────┐
  Submit Task ────> │  Round-Robin  │ ────> Worker 0 ──> Shard RwLocks
                    │  Distributor  │ ────> Worker 1 ──> Shard RwLocks
                    └──────────────┘ ────> Worker 2 ──> Shard RwLocks
                                              ...
                    ┌──────────────┐ ────> Worker N ──> Shard RwLocks
  Results    <──── │ Result Vec   │ <──── Workers write here
                    │ (RwLock)     │
                    └──────────────┘
```

### Design Decisions

1. **One channel per worker**: Each worker has its own `std::sync::mpsc::Receiver`.
   This avoids contention on a shared work queue.

2. **Round-robin distribution**: Tasks are assigned to workers in round-robin
   fashion using an atomic counter. This provides even distribution without
   requiring work-stealing complexity.

3. **Shared result collector**: Workers write results to a shared `RwLock<Vec<ValidationResult>>`.
   This is simple and sufficient for the validation workload.

4. **Configurable worker count**: Defaults to `num_cpus`, but can be overridden
   for testing or resource-constrained environments.

## Cross-Shard Finality Coordination

Finality determination requires cross-shard coordination because it involves
witnesses from multiple shards. The current design handles this through the
global `round_witnesses` map:

1. **Recording a witness**: When a shard determines an event is a witness,
   it records the event in `round_witnesses[round]` under the global lock.

2. **Checking fame**: When determining if a witness is famous, the engine
   reads `round_witnesses[round + delay]` under the global read lock and
   checks ancestry against each witness.

3. **Committing events**: When events are committed, each shard updates
   its own `event_states` and the global `committed_count`.

This design means that finality is a **sequential step** after parallel
validation. The throughput gain comes from parallelizing the validation
work (hash checks, signature verification, state insertion), which is
the bulk of the CPU time.

## Performance Expectations

### Target: ≥2× Throughput

The sharding design targets ≥2× throughput improvement on single-node
benchmarks compared to the single-threaded `ConsensusEngine`.

### Why 2× is Achievable

1. **Read-heavy workload**: Most event processing involves reading existing
   state (checking if an event exists, looking up round assignments).
   RwLock allows concurrent reads.

2. **Uniform shard distribution**: SHA-256 hashes distribute events evenly
   across 256 shards, so parallel work is well-balanced.

3. **Low global lock contention**: The global lock is only needed for witness
   recording and node info updates, which are infrequent relative to
   per-event state lookups.

4. **Multi-core utilization**: On a 4-core machine, 4 workers can process
   events in parallel with minimal contention (each worker hits a different
   shard 99.6% of the time).

### Benchmark Methodology

We measure **events/sec** for three scenarios:

1. **Single-threaded HashMap**: Baseline using plain `HashMap` (simulates
   the existing `ConsensusEngine` internals).
2. **Single-threaded ShardedConsensusState**: Measures RwLock overhead
   in the single-threaded case.
3. **Multi-threaded ShardedConsensusState**: Measures throughput with
   2, 4, and 8 worker threads.

The 2× target is: `multi_threaded_throughput / single_threaded_throughput >= 2`.

## Risks and Mitigations

### Risk 1: Global Lock Contention

**Risk**: The global `RwLock` for `round_witnesses`, `node_info`, etc.
could become a bottleneck under high load.

**Mitigation**:

- The global lock is only taken for cross-shard operations (witness recording,
  node info updates), which are O(1) per event.
- Read operations (checking witnesses for a round) use a read lock, allowing
  concurrent reads.
- If contention becomes measurable, we can shard `node_info` by NodeId
  and use fine-grained locking for `round_witnesses`.

### Risk 2: Lock Poisoning

**Risk**: A panic in one thread could poison an `RwLock`, causing
subsequent accesses to fail.

**Mitigation**:

- All lock acquisitions use `unwrap_or_else(|e| e.into_inner())` to recover
  from poisoning. The data may be in an inconsistent state, but the system
  continues operating.
- The consensus state can always be rebuilt from the causal graph, so
  temporary inconsistency after a panic is acceptable.
- `#![deny(unsafe_code) (see SAFETY.md)]` and `#![deny(clippy::unwrap_used)]` reduce
  the likelihood of panics in the first place.

### Risk 3: Memory Overhead

**Risk**: 256 shards × 3 HashMaps each = 768 HashMap objects, which
may increase memory usage compared to the single-engine design.

**Mitigation**:

- Empty HashMaps use minimal memory (typically 0 bytes for the allocation
  plus a small fixed overhead for the struct).
- In practice, most shards will be populated in a running system, so
  the per-shard overhead is negligible relative to the event data.
- We can reduce to 64 shards if memory overhead is a concern (still
  6.25× more granular than a single lock).

### Risk 4: Cross-Shard Consistency

**Risk**: Events in different shards may see inconsistent state if
one shard is being written to while another is being read.

**Mitigation**:

- Within a shard, `RwLock` guarantees that readers see a consistent
  snapshot (either the old or new state, never a torn read).
- Cross-shard operations (finality) are sequenced through the global
  lock, ensuring that commitment decisions are based on a consistent
  view of the witness set.
- The sharded state is designed as a **cache** of the consensus engine's
  state, not the source of truth. The causal graph is the source of
  truth and can always be used to rebuild the state.

## Future Work

1. **Adaptive sharding**: Dynamically adjust the number of shards based
   on load and CPU core count.

2. **Lock-free shards**: Replace `RwLock` with lock-free data structures
   (e.g., `dashmap`) for even lower contention.

3. **NUMA-aware placement**: Pin shards to NUMA nodes for reduced memory
   access latency on multi-socket systems.

4. **Shard-level cleanup**: Run cleanup (`cleanup_old_committed`) on
   individual shards in parallel rather than sequentially.

## References

- [AlephBFT: An Asynchronous and Byzantine Fault Tolerant Protocol](https://arxiv.org/abs/1909.11436)
- [Hashgraph Consensus: Detailed Overview](https://www.swirlds.com/downloads/SWIRLDS-TR-2016-01.pdf)
- [RwLock vs Mutex: When to use which](https://doc.rust-lang.org/std/sync/struct.RwLock.html)
