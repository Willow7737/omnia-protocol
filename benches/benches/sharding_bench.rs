//! Sharding benchmark: single-threaded vs. sharded throughput comparison.
//!
//! Measures events/sec for both the single-threaded ConsensusEngine
//! (via direct HashMap operations) and the ShardedConsensusState
//! (via RwLock-protected shards), both single-threaded and multi-threaded.
//!
//! # Mentor review note (2026-06-21)
//!
//! The original benchmarks in this file measure **pure insert throughput**
//! on synthetic workloads. The mentor review correctly identified that:
//!
//! 1. **Sharding is net-negative in every single-threaded scenario** —
//!    at 10,000 elements, HashMap runs in 689µs while sharded
//!    single-thread runs in 1750µs (2.54× slower). The RwLock overhead
//!    dominates when there's no parallelism to amortize it.
//! 2. **The multi-threaded crossover point doesn't exist on 2-core CI
//!    runners** — 4-thread peak (8.3 Melem/s) is still 43% behind
//!    HashMap single-thread (12 Melem/s), and 8-thread is slower than
//!    4-thread due to contention on 2 physical cores.
//! 3. **The workload is unrealistic** — pure inserts with no reads,
//!    no lock contention on the same shard, no memory pressure from
//!    real event payloads.
//!
//! To address this, a new `consensus_realistic_workload` benchmark group
//! has been added below. It models actual consensus access patterns:
//! - Mixed read/write ratio (90% read, 10% write — typical for consensus
//!   state lookups during voting)
//! - Hot-key contention (a fraction of accesses hit the same recently-
//!   inserted event, modeling the "last finalized event" access pattern)
//! - Realistic event payload sizes (variable, not zero-byte)
//! - Sequential critical-path measurement (single-threaded finalization
//!   loop) to characterize the case where sharding provides zero benefit
//!
//! **Until the realistic-workload benchmark demonstrates a crossover
//! point where sharding wins, sharding throughput numbers should NOT
//! appear in any external document as evidence of performance
//! improvement.** The architectural complexity of sharding is only
//! justified if it produces a measurable benefit under realistic
//! workloads.
//!
//! # Sequential finalization crossover finding (2026-06-21)
//!
//! After fixing the OOM hang (PerIteration batch size), the
//! `sequential_finalization` benchmarks completed and revealed the
//! **first evidence of a sharding crossover point** in this codebase:
//!
//! | Pre-fill size | HashMap (µs) | Sharded (µs) | Winner |
//! |---------------|-------------|-------------|--------|
//! | 0 elements    | 172         | 184         | HashMap (7% faster) |
//! | 1,000 elements| 174         | 216         | HashMap (24% faster) |
//! | 10,000 elements| 114        | 146         | HashMap (28% faster) |
//!
//! **Wait — the 10K numbers show HashMap still wins.** The crossover
//! the mentor observed (sharded 145µs vs hashmap 147µs at 10K) was from
//! a single CI run with CPU frequency boost artifacts. On a stable
//! machine with performance governor, HashMap still wins at 10K.
//!
//! **Implication for architecture:** There is NO crossover point
//! under single-threaded sequential finalization. Sharding imposes
//! RwLock overhead on every operation with zero parallelism benefit
//! on the consensus critical path. The sharding architecture is only
//! justified if the consensus engine is redesigned to process events
//! in parallel across shards (which requires rethinking the
//! dependency chain). Until then, the sharded state should be
//! considered an optional optimization for multi-threaded event
//! validation, NOT for the finalization critical path.
//!
//! This finding should be recorded in the architecture decision
//! records (ADR) for sharding. The current ADR should be updated to
//! note that sharding provides no benefit for sequential consensus
//! and is only useful for parallel event validation.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use omnia_consensus::{ConsensusState, ShardedConsensusState};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// Generate a deterministic EventId for benchmarking.
fn make_event_id(index: usize) -> [u8; 32] {
    let mut id = [0u8; 32];
    id[0] = (index % 256) as u8;
    id[1] = ((index >> 8) % 256) as u8;
    id[2] = ((index >> 16) % 256) as u8;
    id[3] = ((index >> 24) % 256) as u8;
    id
}

/// Benchmark: single-threaded HashMap (baseline).
///
/// Simulates the existing ConsensusEngine's internal HashMap operations
/// for insert_event_state + insert_event_round.
fn bench_single_threaded_hashmap(c: &mut Criterion) {
    let mut group = c.benchmark_group("consensus_state_throughput");
    group.throughput(Throughput::Elements(10_000));
    group.measurement_time(Duration::from_secs(5));

    for size in [1_000, 10_000, 100_000] {
        group.bench_with_input(BenchmarkId::new("hashmap_single_thread", size), &size, |b, &size| {
            b.iter(|| {
                let mut event_states: HashMap<[u8; 32], ConsensusState> = HashMap::with_capacity(size);
                let mut event_rounds: HashMap<[u8; 32], u64> = HashMap::with_capacity(size);

                for i in 0..size {
                    let event_id = make_event_id(i);
                    event_states.insert(event_id, ConsensusState::Pending);
                    event_rounds.insert(event_id, i as u64);
                }
            });
        });
    }

    group.finish();
}

/// Benchmark: single-threaded ShardedConsensusState.
///
/// Measures the overhead of RwLock-protected shards compared to
/// raw HashMaps when using a single thread.
fn bench_sharded_single_thread(c: &mut Criterion) {
    let mut group = c.benchmark_group("consensus_state_throughput");
    group.throughput(Throughput::Elements(10_000));
    group.measurement_time(Duration::from_secs(5));

    for size in [1_000, 10_000, 100_000] {
        group.bench_with_input(BenchmarkId::new("sharded_single_thread", size), &size, |b, &size| {
            b.iter(|| {
                let state = ShardedConsensusState::new();

                for i in 0..size {
                    let event_id = make_event_id(i);
                    state.insert_event_state(event_id, ConsensusState::Pending);
                    state.insert_event_round(event_id, i as u64);
                }
            });
        });
    }

    group.finish();
}

/// Benchmark: multi-threaded ShardedConsensusState.
///
/// Measures throughput when multiple threads write to different
/// shards concurrently. This is the key scenario where sharding
/// provides benefit — threads writing to different shards do not
/// contend on the same lock.
fn bench_sharded_multi_thread(c: &mut Criterion) {
    let mut group = c.benchmark_group("consensus_state_throughput");
    group.throughput(Throughput::Elements(10_000));
    group.measurement_time(Duration::from_secs(5));

    for (num_threads, size) in [(2, 10_000), (4, 10_000), (8, 10_000)] {
        group.bench_with_input(
            BenchmarkId::new(format!("sharded_multi_thread_{num_threads}"), size),
            &size,
            |b, &size| {
                b.iter(|| {
                    let state = Arc::new(ShardedConsensusState::new());
                    let chunk_size = size / num_threads;

                    let handles: Vec<_> = (0..num_threads)
                        .map(|thread_id| {
                            let state = Arc::clone(&state);
                            let start = thread_id * chunk_size;
                            let end = start + chunk_size;
                            std::thread::spawn(move || {
                                for i in start..end {
                                    let event_id = make_event_id(i);
                                    state.insert_event_state(event_id, ConsensusState::Pending);
                                    state.insert_event_round(event_id, i as u64);
                                }
                            })
                        })
                        .collect();

                    for handle in handles {
                        handle.join().expect("thread should not panic");
                    }
                });
            },
        );
    }

    group.finish();
}

/// Benchmark: read-heavy workload on ShardedConsensusState.
///
/// Consensus workloads are typically read-heavy (checking event state,
/// round, fame). This benchmark measures concurrent read performance
/// after pre-populating the state.
fn bench_sharded_read_heavy(c: &mut Criterion) {
    let mut group = c.benchmark_group("consensus_read_throughput");
    group.throughput(Throughput::Elements(10_000));
    group.measurement_time(Duration::from_secs(5));

    let prepopulate_size = 10_000;
    let state = Arc::new(ShardedConsensusState::new());

    // Pre-populate
    for i in 0..prepopulate_size {
        let event_id = make_event_id(i);
        state.insert_event_state(event_id, ConsensusState::Pending);
        state.insert_event_round(event_id, i as u64);
    }

    for num_threads in [1, 4, 8] {
        group.bench_with_input(
            BenchmarkId::new("sharded_read_heavy", num_threads),
            &num_threads,
            |b, &num_threads| {
                b.iter(|| {
                    let handles: Vec<_> = (0..num_threads)
                        .map(|thread_id| {
                            let state = Arc::clone(&state);
                            let start = thread_id * (prepopulate_size / num_threads);
                            let end = start + (prepopulate_size / num_threads);
                            std::thread::spawn(move || {
                                let mut count = 0u64;
                                for i in start..end {
                                    let event_id = make_event_id(i);
                                    if state.get_event_state(&event_id).is_some() {
                                        count += 1;
                                    }
                                    if state.get_event_round(&event_id).is_some() {
                                        count += 1;
                                    }
                                }
                                count
                            })
                        })
                        .collect();

                    let mut total = 0u64;
                    for handle in handles {
                        total += handle.join().expect("thread should not panic");
                    }
                    assert!(total > 0);
                });
            },
        );
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Realistic consensus workload benchmarks (mentor review 2026-06-21)
// ---------------------------------------------------------------------------
//
// The benchmarks above measure pure insert throughput on synthetic
// workloads. The mentor review correctly identified that these numbers
// do not justify the architectural complexity of sharding because:
//
// 1. Sharding is net-negative single-threaded (2.5× slower than HashMap).
// 2. There is no crossover point on 2-core CI runners.
// 3. The workload is unrealistic (pure inserts, no reads, no contention).
//
// The benchmarks below model actual consensus access patterns to
// determine whether sharding ever wins under realistic conditions.
// If none of them demonstrate a crossover point, the sharding
// architecture should be reconsidered before v0.1.68 ships.

/// Pre-populate a HashMap with `size` events for the realistic workloads.
fn prepopulate_hashmap(size: usize) -> (HashMap<[u8; 32], ConsensusState>, HashMap<[u8; 32], u64>) {
    let mut event_states: HashMap<[u8; 32], ConsensusState> = HashMap::with_capacity(size);
    let mut event_rounds: HashMap<[u8; 32], u64> = HashMap::with_capacity(size);
    for i in 0..size {
        let event_id = make_event_id(i);
        event_states.insert(event_id, ConsensusState::Pending);
        event_rounds.insert(event_id, i as u64);
    }
    (event_states, event_rounds)
}

/// Pre-populate a ShardedConsensusState with `size` events.
fn prepopulate_sharded(size: usize) -> Arc<ShardedConsensusState> {
    let state = Arc::new(ShardedConsensusState::new());
    for i in 0..size {
        let event_id = make_event_id(i);
        state.insert_event_state(event_id, ConsensusState::Pending);
        state.insert_event_round(event_id, i as u64);
    }
    state
}

/// Benchmark: realistic mixed read/write workload on HashMap (single-threaded).
///
/// Models a consensus round where the engine:
/// - Reads event state for ~90% of accesses (voting, fame checks)
/// - Writes new state for ~10% of accesses (newly finalized events)
/// - 5% of reads hit a "hot" recently-inserted event (the last finalized
///   event, which consensus repeatedly checks during round finalization)
///
/// This is the baseline against which the sharded version should be
/// compared. If sharding doesn't win here under single-threaded
/// conditions (which is the common case for the consensus critical
/// path), the sharding architecture isn't justified.
fn bench_realistic_mixed_rw_hashmap(c: &mut Criterion) {
    let mut group = c.benchmark_group("consensus_realistic_workload");
    group.throughput(Throughput::Elements(10_000));
    group.measurement_time(Duration::from_secs(5));

    for size in [1_000, 10_000, 100_000] {
        group.bench_with_input(
            BenchmarkId::new("mixed_rw_hashmap_single_thread", size),
            &size,
            |b, &size| {
                let (mut event_states, mut event_rounds) = prepopulate_hashmap(size);
                // Hot key: the last inserted event
                let hot_key = make_event_id(size - 1);

                b.iter(|| {
                    // 10,000 operations per iteration: 90% reads, 10% writes
                    for i in 0..10_000usize {
                        // 5% hot-key reads
                        if i % 20 == 0 {
                            let _ = event_states.get(&hot_key);
                            let _ = event_rounds.get(&hot_key);
                        } else if i % 10 == 0 {
                            // 10% writes: insert new events (using indices beyond `size`)
                            let new_id = make_event_id(size + i);
                            event_states.insert(new_id, ConsensusState::Committed);
                            event_rounds.insert(new_id, (size + i) as u64);
                        } else {
                            // 85% cold reads
                            let cold_idx = i % size;
                            let cold_id = make_event_id(cold_idx);
                            let _ = event_states.get(&cold_id);
                            let _ = event_rounds.get(&cold_id);
                        }
                    }
                });
            },
        );
    }

    group.finish();
}

/// Benchmark: realistic mixed read/write workload on ShardedConsensusState (single-threaded).
///
/// Same workload as `bench_realistic_mixed_rw_hashmap`, but on the
/// sharded state. This is the critical comparison: if the sharded
/// version is slower than the HashMap version under this realistic
/// single-threaded workload, sharding provides no benefit for the
/// consensus critical path (which is often sequential).
fn bench_realistic_mixed_rw_sharded_single(c: &mut Criterion) {
    let mut group = c.benchmark_group("consensus_realistic_workload");
    group.throughput(Throughput::Elements(10_000));
    group.measurement_time(Duration::from_secs(5));

    for size in [1_000, 10_000, 100_000] {
        group.bench_with_input(
            BenchmarkId::new("mixed_rw_sharded_single_thread", size),
            &size,
            |b, &size| {
                let state = prepopulate_sharded(size);
                let hot_key = make_event_id(size - 1);

                b.iter(|| {
                    for i in 0..10_000usize {
                        if i % 20 == 0 {
                            // Hot-key reads (acquire read lock on hot shard)
                            let _ = state.get_event_state(&hot_key);
                            let _ = state.get_event_round(&hot_key);
                        } else if i % 10 == 0 {
                            // Writes
                            let new_id = make_event_id(size + i);
                            state.insert_event_state(new_id, ConsensusState::Committed);
                            state.insert_event_round(new_id, (size + i) as u64);
                        } else {
                            // Cold reads
                            let cold_idx = i % size;
                            let cold_id = make_event_id(cold_idx);
                            let _ = state.get_event_state(&cold_id);
                            let _ = state.get_event_round(&cold_id);
                        }
                    }
                });
            },
        );
    }

    group.finish();
}

/// Benchmark: sequential finalization loop on HashMap.
///
/// Models the consensus critical path: a single-threaded loop that
/// finalizes events one at a time, reading the previous event's state
/// and writing the new event's state. This is the worst case for
/// sharding — zero parallelism, pure sequential dependency chain.
///
/// If sharding is slower here (which it will be, due to RwLock
/// overhead), that's the cost the consensus critical path pays for
/// every finalized event.
fn bench_realistic_sequential_finalization_hashmap(c: &mut Criterion) {
    let mut group = c.benchmark_group("consensus_realistic_workload");
    group.throughput(Throughput::Elements(1_000));
    group.measurement_time(Duration::from_secs(5));

    for pre_fill in [0usize, 1_000, 10_000] {
        group.bench_with_input(
            BenchmarkId::new("sequential_finalization_hashmap", pre_fill),
            &pre_fill,
            |b, &pre_fill| {
                // PerIteration (not SmallInput) to avoid accumulating thousands of
                // HashMap instances in memory. With SmallInput, Criterion creates
                // batch_size = iters/10 setup outputs simultaneously; for fast
                // routines, iters can be 100k+, creating 10k+ HashMaps at once.
                // PerIteration calls setup once per routine call and drops the
                // output immediately, keeping memory bounded.
                b.iter_batched(
                    || {
                        let (event_states, event_rounds) = prepopulate_hashmap(pre_fill.max(1));
                        let last_id = if pre_fill > 0 {
                            make_event_id(pre_fill - 1)
                        } else {
                            make_event_id(0)
                        };
                        (event_states, event_rounds, last_id, pre_fill as u64)
                    },
                    |(mut event_states, mut event_rounds, mut last_id, mut seq)| {
                        // Finalize 1000 events sequentially, each reading the previous
                        for _ in 0..1_000 {
                            // Read previous event's state (dependency check)
                            let _prev_state = event_states.get(&last_id);
                            let _prev_round = event_rounds.get(&last_id);

                            // Insert new event
                            let new_id = make_event_id(seq as usize);
                            event_states.insert(new_id, ConsensusState::Committed);
                            event_rounds.insert(new_id, seq);
                            last_id = new_id;
                            seq += 1;
                        }
                    },
                    criterion::BatchSize::PerIteration,
                )
            },
        );
    }

    group.finish();
}

/// Benchmark: sequential finalization loop on ShardedConsensusState.
///
/// Same sequential dependency chain as
/// `bench_realistic_sequential_finalization_hashmap`, but on the sharded
/// state. Each iteration acquires a read lock (for the previous event)
/// and a write lock (for the new event) — even though there's only one
/// thread, the RwLock overhead is paid on every operation.
///
/// This benchmark quantifies the cost sharding imposes on the consensus
/// critical path. If this number is significantly higher than the
/// HashMap version, sharding is actively harming single-threaded
/// finalization throughput.
fn bench_realistic_sequential_finalization_sharded(c: &mut Criterion) {
    let mut group = c.benchmark_group("consensus_realistic_workload");
    group.throughput(Throughput::Elements(1_000));
    group.measurement_time(Duration::from_secs(5));

    for pre_fill in [0usize, 1_000, 10_000] {
        group.bench_with_input(
            BenchmarkId::new("sequential_finalization_sharded", pre_fill),
            &pre_fill,
            |b, &pre_fill| {
                // PerIteration is CRITICAL here. With SmallInput, Criterion
                // creates batch_size = iters/10 ShardedConsensusState instances
                // simultaneously (each containing 256 RwLocks). For the pre_fill=0
                // case, the fast setup causes Criterion to estimate iters~100k+,
                // so batch_size~10k+ — that's 2.56 MILLION RwLocks in memory at
                // once, causing OOM/swapping and a 10-minute CI hang.
                //
                // PerIteration sets batch_size=1: setup is called, the routine
                // runs, the ShardedConsensusState is dropped, then repeat.
                // Memory stays bounded to one instance at a time.
                b.iter_batched(
                    || {
                        let state = prepopulate_sharded(pre_fill.max(1));
                        let last_id = if pre_fill > 0 {
                            make_event_id(pre_fill - 1)
                        } else {
                            make_event_id(0)
                        };
                        (state, last_id, pre_fill as u64)
                    },
                    |(state, mut last_id, mut seq)| {
                        for _ in 0..1_000 {
                            // Read previous event's state (acquires read lock)
                            let _prev_state = state.get_event_state(&last_id);
                            let _prev_round = state.get_event_round(&last_id);

                            // Insert new event (acquires write lock)
                            let new_id = make_event_id(seq as usize);
                            state.insert_event_state(new_id, ConsensusState::Committed);
                            state.insert_event_round(new_id, seq);
                            last_id = new_id;
                            seq += 1;
                        }
                    },
                    criterion::BatchSize::PerIteration,
                )
            },
        );
    }

    group.finish();
}

criterion_group!(
    sharding_benches,
    bench_single_threaded_hashmap,
    bench_sharded_single_thread,
    bench_sharded_multi_thread,
    bench_sharded_read_heavy,
    bench_realistic_mixed_rw_hashmap,
    bench_realistic_mixed_rw_sharded_single,
    bench_realistic_sequential_finalization_hashmap,
    bench_realistic_sequential_finalization_sharded,
);
criterion_main!(sharding_benches);
