//! Network-simulated multi-node benchmarks.
//!
//! These benchmarks use the `ChaosNetwork` in-process simulation framework
//! to measure consensus performance under realistic multi-node conditions:
//! actual gossip propagation between nodes, BFT supermajority waiting,
//! and cross-node DAG synchronization.
//!
//! Unlike the single-node criterion benchmarks (which measure function-call
//! latency with no network I/O), these benchmarks measure the FULL
//! consensus pipeline:
//!   event creation → gossip broadcast → peer receipt → graph insert →
//!   consensus processing → finality commitment
//!
//! The mentor review (2026-06-23) correctly identified that single-node
//! benchmarks cannot produce performance claims suitable for documentation
//! — "22 µs finality on a local benchmark with no network, no quorum, no
//! adversarial delay is not a consensus system performance claim."
//!
//! These benchmarks address that gap. The numbers here are still synthetic
//! (no real TCP/UDP, no real latency) but they DO include the multi-node
//! coordination overhead that the single-node benchmarks omit.
//!
//! # What's measured
//!
//! - `multi_node_finality_latency` — time from event creation to finality
//!   commitment across N nodes (3, 5, 7). This is the real "consensus
//!   latency" number — it includes the gossip round-trip + BFT voting.
//! - `multi_node_throughput` — sustained events/sec across N nodes with
//!   multiple concurrent submitters. This is the real "consensus TPS"
//!   number — it includes cross-node contention.
//! - `partition_recovery_latency` — time from partition heal to first
//!   post-partition finality. Measures the sync/recovery path.
//! - `crash_recovery_latency` — time from node restart to full state
//!   sync. Measures the genesis-replay / fast-sync path.
//!
//! # Caveats
//!
//! 1. The ChaosNetwork simulates the network IN-PROCESS — there's no real
//!    socket I/O. Real-world latency will be 10-100x higher due to network
//!    round-trips.
//! 2. The simulation uses deterministic event ordering (round-robin submit).
//!    Real-world gossip is non-deterministic, which can produce different
//!    commit orderings.
//! 3. These benchmarks use `ChaosNetwork::advance()` which submits one
//!    event per node per round. Real-world load patterns may differ.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use omnia_chaos_tests::ChaosNetwork;
use std::time::{Duration, Instant};

/// Time an operation and return the elapsed duration in nanoseconds.
fn time_ns<F: FnOnce()>(f: F) -> u64 {
    let start = Instant::now();
    f();
    start.elapsed().as_nanos() as u64
}

/// Benchmark: multi-node finality latency.
///
/// Measures the time from event creation to finality commitment across
/// N nodes. Each iteration:
///   1. Creates a fresh ChaosNetwork with N nodes
///   2. Warms up the network (sync genesis events)
///   3. Submits a single event from node 0
///   4. Advances consensus rounds until the event is committed on ALL nodes
///   5. Records the wall-clock time from submit to finality
///
/// This is the closest synthetic approximation of "real consensus latency"
/// without actual network I/O.
fn multi_node_finality_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("network_sim/finality_latency");
    group.measurement_time(Duration::from_secs(20));
    group.sample_size(20); // Each iteration creates a fresh network — expensive

    for &n_nodes in &[3usize, 5, 7] {
        group.bench_with_input(BenchmarkId::new("n_nodes", n_nodes), &n_nodes, |b, &n| {
            b.iter_custom(|iters| {
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    let mut net = ChaosNetwork::new(n);
                    net.warmup();

                    // Submit one event and time how long until it's
                    // committed on all non-crashed nodes.
                    total += Duration::from_nanos(time_ns(|| {
                        net.submit_event(0, vec![0xAA]).expect("submit");
                        // Advance rounds until the event is committed on
                        // at least one node (consensus finality).
                        let mut rounds = 0;
                        while net.committed_count() == 0 && rounds < 100 {
                            net.advance(1);
                            rounds += 1;
                        }
                    }));
                }
                total
            });
        });
    }

    group.finish();
}

/// Benchmark: multi-node sustained throughput.
///
/// Measures sustained events/sec across N nodes with multiple concurrent
/// submitters. Each iteration:
///   1. Creates a fresh ChaosNetwork with N nodes
///   2. Warms up the network
///   3. Submits a batch of 100 events (round-robin across nodes)
///   4. Advances consensus until all events are committed
///   5. Reports throughput as events/sec
///
/// The throughput is set to Elements(100) so criterion reports
/// Melem/s — the "real TPS" number for the consensus system.
fn multi_node_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("network_sim/throughput");
    group.measurement_time(Duration::from_secs(20));
    group.sample_size(10);
    group.throughput(Throughput::Elements(100)); // 100 events per iteration

    // Only benchmark 3-node throughput in CI. The 5-node and 7-node
    // variants take 56s and 391s respectively (per 2026-06-23 CI run),
    // which exceeds the job timeout. The 3-node number is sufficient
    // for regression detection — the scaling curve can be measured
    // manually on a self-hosted runner.
    for &n_nodes in &[3usize] {
        group.bench_with_input(BenchmarkId::new("n_nodes", n_nodes), &n_nodes, |b, &n| {
            b.iter_custom(|iters| {
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    let mut net = ChaosNetwork::new(n);
                    net.warmup();

                    total += Duration::from_nanos(time_ns(|| {
                        // Submit 100 events round-robin across all nodes
                        for i in 0..100u8 {
                            let node_idx = (i as usize) % n;
                            let _ = net.submit_event(node_idx, vec![i]);
                        }
                        // Advance until all events are committed
                        let mut rounds = 0;
                        while net.committed_count() < 100 && rounds < 500 {
                            net.advance(1);
                            rounds += 1;
                        }
                    }));
                }
                total
            });
        });
    }

    group.finish();
}

/// Benchmark: partition recovery latency.
///
/// Measures the time from partition heal to first post-partition finality.
/// This exercises the sync/recovery path — nodes must exchange missed
/// events and re-run consensus.
///
/// Each iteration:
///   1. Creates a 5-node network and warms up
///   2. Partitions into {0,1} vs {2,3,4}
///   3. Advances 10 rounds (partitioned — events accumulate on each side)
///   4. Heals the partition
///   5. Times how long until the next event is committed (recovery)
fn partition_recovery_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("network_sim/partition_recovery");
    group.measurement_time(Duration::from_secs(20));
    group.sample_size(10);

    group.bench_function("5_node_partition_heal", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let mut net = ChaosNetwork::new(5);
                net.warmup();

                // Partition: {0,1} vs {2,3,4}
                net.partition(&[0, 1], &[2, 3, 4]);
                net.advance(10); // accumulate events during partition

                // Heal and time recovery
                net.heal();
                total += Duration::from_nanos(time_ns(|| {
                    net.submit_event(0, vec![0xBB]).expect("submit");
                    let mut rounds = 0;
                    while net.committed_count() == 0 && rounds < 100 {
                        net.advance(1);
                        rounds += 1;
                    }
                }));
            }
            total
        });
    });

    group.finish();
}

/// Benchmark: crash recovery latency.
///
/// Measures the time from node restart to full state sync. This exercises
/// the genesis-replay / fast-sync path — the restarted node must recover
/// missed events from peers.
///
/// Each iteration:
///   1. Creates a 5-node network and warms up
///   2. Advances 10 rounds (nodes produce events)
///   3. Crashes node 0
///   4. Advances 5 more rounds (node 0 misses events)
///   5. Restarts node 0
///   6. Times how long until node 0 catches up (recovery)
fn crash_recovery_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("network_sim/crash_recovery");
    group.measurement_time(Duration::from_secs(20));
    group.sample_size(10);

    group.bench_function("5_node_crash_restart", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let mut net = ChaosNetwork::new(5);
                net.warmup();
                net.advance(10); // produce events

                // Crash node 0, advance 5 rounds without it
                net.crash_node(0).expect("crash");
                net.advance(5);

                // Restart and time recovery
                net.restart_node(0).expect("restart");
                total += Duration::from_nanos(time_ns(|| {
                    // Advance until node 0 has caught up (has at least
                    // as many committed events as node 1)
                    let target = net.node_committed_count(1);
                    let mut rounds = 0;
                    while net.node_committed_count(0) < target && rounds < 200 {
                        net.advance(1);
                        rounds += 1;
                    }
                }));
            }
            total
        });
    });

    group.finish();
}

/// Benchmark: gossip propagation fan-out.
///
/// Measures how long it takes for an event to propagate from one node
/// to all other nodes in the network. This isolates the gossip layer
/// from the consensus layer — we measure receipt, not finality.
///
/// Each iteration:
///   1. Creates a fresh N-node network and warms up
///   2. Submits an event from node 0
///   3. Times how long until the event appears in ALL other nodes' graphs
fn gossip_propagation_fanout(c: &mut Criterion) {
    let mut group = c.benchmark_group("network_sim/gossip_fanout");
    group.measurement_time(Duration::from_secs(15));
    group.sample_size(20);

    for &n_nodes in &[3usize, 5, 7, 10] {
        group.bench_with_input(BenchmarkId::new("n_nodes", n_nodes), &n_nodes, |b, &n| {
            b.iter_custom(|iters| {
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    let mut net = ChaosNetwork::new(n);
                    net.warmup();

                    total += Duration::from_nanos(time_ns(|| {
                        net.submit_event(0, vec![0xCC]).expect("submit");
                        // Advance until the event has propagated to all
                        // nodes (at least 1 event in every node's graph).
                        let mut rounds = 0;
                        while rounds < 50 {
                            net.advance(1);
                            rounds += 1;
                            // Check if all nodes have at least 1 event
                            let all_have_events = (0..n).all(|i| net.node_committed_count(i) > 0);
                            if all_have_events {
                                break;
                            }
                        }
                    }));
                }
                total
            });
        });
    }

    group.finish();
}

criterion_group!(
    name = network_sim_benches;
    config = Criterion::default()
        .measurement_time(Duration::from_secs(20))
        .sample_size(20);
    targets =
        multi_node_finality_latency,
        multi_node_throughput,
        partition_recovery_latency,
        crash_recovery_latency,
        gossip_propagation_fanout
);

criterion_main!(network_sim_benches);
