//! Sharding benchmark: single-threaded vs. sharded throughput comparison.
//!
//! Measures events/sec for both the single-threaded ConsensusEngine
//! (via direct HashMap operations) and the ShardedConsensusState
//! (via RwLock-protected shards), both single-threaded and multi-threaded.

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

criterion_group!(
    sharding_benches,
    bench_single_threaded_hashmap,
    bench_sharded_single_thread,
    bench_sharded_multi_thread,
    bench_sharded_read_heavy,
);
criterion_main!(sharding_benches);
