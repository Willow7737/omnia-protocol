//! Baseline Benchmark Suite for Omnia Protocol Phase 0 Throughput Optimization.
//!
//! This suite establishes baseline performance measurements for:
//! - Transaction throughput (sustained TPS over a 60s window)
//! - Consensus finality latency (mean time from creation to commitment)
//! - ZK proof generation time (1-tx and 100-tx batches, feature-gated)
//! - Gossip propagation latency (single-node simulation)
//! - DAG event insertion latency (mean over varying graph sizes)
//!
//! These baselines are used to track regression and validate sprint targets
//! as defined in the Phase 0 Throughput Optimization Sprint Plan.
//!
//! # On percentile reporting (correction note, mentor review)
//!
//! Previous versions of this file claimed "p50/p95/p99" in benchmark names
//! and doc comments. That was misleading: Criterion's default measurement
//! harness reports **mean** latency (with a confidence interval) and the
//! full sample distribution is available in the JSON output, but Criterion
//! does not surface p50/p95/p99 in its human-readable summary unless you
//! implement a custom `Measurement` harness. The bench names have been
//! corrected to drop the `_p50_p95_p99` suffix. To obtain true tail
//! percentiles, run with `--message-format=json` and post-process the
//! `samples` array, or wrap the bench in a custom percentile harness
//! (see Criterion's `Measurement` trait).
//!
//! # On `sustained_tps_single_node` outlier rate (correction note, mentor review)
//!
//! Previous versions ran this bench with `sample_size(20)`, which produced
//! a ~20% severe-outlier rate on shared CI runners (2 physical cores,
//! noisy neighbors). The TPS number quoted from such a run is not stable.
//! The sample size has been raised to 100 and the measurement window
//! extended; even so, numbers from shared CI runners should be treated
//! as preliminary until reproduced on a dedicated pinned-CPU machine.
//! See the bench's inline doc comment for details.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use omnia_consensus::{
    CausalGraph, ConsensusConfig, ConsensusEngine, SlashingEngine, DEFAULT_EJECTION_THRESHOLD, DEFAULT_SLASH_THRESHOLD,
};
use omnia_crypto::generate_keypair;
use omnia_primitives::{Event, NodeId, VectorClock};
use std::hint::black_box;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Helper: create a signed event for benchmarking
// ---------------------------------------------------------------------------

#[allow(dead_code)]
fn create_signed_event(
    creator: NodeId,
    seq: u64,
    vc: VectorClock,
    self_parent: Option<[u8; 32]>,
    other_parent: Option<[u8; 32]>,
    payload: Vec<u8>,
) -> Event {
    let keypair = generate_keypair();
    let mut event = Event::new(creator, seq, vc, self_parent, other_parent, payload).expect("event creation");
    event.sign_with_keypair(&keypair).expect("signing");
    event
}

fn test_node(id: u8) -> NodeId {
    let mut node = [0u8; 32];
    node[0] = id;
    node
}

// ---------------------------------------------------------------------------
// Benchmark 1: Transaction throughput (sustained TPS)
// ---------------------------------------------------------------------------

/// Measures sustained transaction throughput over a simulated 60s window.
///
/// Creates events in a chain (each referencing the previous as self-parent),
/// inserts them into a CausalGraph, and processes them through a ConsensusEngine
/// configured with `total_nodes=3` for BFT finality.
///
/// The reported throughput is events/sec — the number of events the node
/// can create, sign, insert, and finalize per second in steady state.
///
/// # Sample size and outlier rate (mentor review correction)
///
/// Previously this bench used `sample_size(20)` with a 10s measurement
/// window. On shared CI runners (2 physical cores, noisy neighbors) this
/// produced a ~20% severe-outlier rate, making the reported TPS number
/// unstable and unreliable for quoting in external materials.
///
/// The sample size has been raised to 100 and the measurement window
/// extended to 15s. Even with these changes, **numbers from shared CI
/// runners should be treated as preliminary** — for publication-grade
/// numbers, run on a dedicated machine with pinned CPU affinity
/// (`taskset -c 0-1 cargo bench ...`) and a longer measurement window
/// (`--measurement-time 60`).
fn tx_throughput_bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("tx_throughput");
    group.throughput(Throughput::Elements(1000)); // Each iteration processes a 1000-event batch
    group.measurement_time(Duration::from_secs(15));
    group.sample_size(100);

    let creator = test_node(1);
    let keypair = generate_keypair();

    group.bench_function("sustained_tps_single_node", |b| {
        b.iter_batched(
            || {
                // Setup: fresh graph + consensus for each batch
                let mut seed = [0u8; 32];
                seed[0] = 1;
                let config = ConsensusConfig {
                    total_nodes: 3,
                    round_seed: seed,
                    ..Default::default()
                };
                let slashing = SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
                let mut consensus = ConsensusEngine::new(config, slashing);
                consensus.register_validator(creator, 10_000);
                let mut graph = CausalGraph::new();

                // Genesis event
                let mut genesis = Event::genesis(creator, vec![]).expect("genesis creation");
                genesis.sign_with_keypair(&keypair).expect("signing");
                let genesis_id = genesis.id;
                graph.insert(genesis).expect("genesis insert");
                let _ = consensus.process_event(graph.get(&genesis_id).expect("genesis exists"), &graph);

                (graph, consensus, genesis_id, 1u64)
            },
            |(mut graph, mut consensus, mut last_id, mut seq)| {
                // Run a batch of events to measure throughput
                let batch_size = 1000usize;
                let mut finalized = 0usize;

                for _ in 0..batch_size {
                    let mut vc = VectorClock::new();
                    vc.set(creator, seq + 1);

                    let mut event =
                        Event::new(creator, seq, vc, Some(last_id), None, vec![seq as u8; 64]).expect("event creation");
                    event.sign_with_keypair(&keypair).expect("signing");

                    let event_id = event.id;
                    if graph.insert(event).is_ok() {
                        if let Ok(graph_event) = graph.get_checked(&event_id) {
                            if let Ok(committed) = consensus.process_event(graph_event, &graph) {
                                finalized += committed.len();
                            }
                        }
                    }

                    last_id = event_id;
                    seq += 1;
                }

                black_box((finalized, seq, last_id));
            },
            criterion::BatchSize::LargeInput,
        );
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Benchmark 2: Finality latency (mean time from creation to commitment)
// ---------------------------------------------------------------------------

/// Measures mean time from event creation to finality commitment.
///
/// Uses a 3-node consensus configuration to simulate BFT finality.
/// Each event is created, inserted into the graph, and processed through
/// consensus. The time from `Instant::now()` before creation to the
/// instant after `process_event` returns committed events is recorded.
///
/// **What this number means**: Criterion's default harness reports the
/// **mean** of the per-iteration elapsed times, with a 95% confidence
/// interval. It does NOT report p50/p95/p99 in the human-readable
/// summary. The full sample distribution is available in Criterion's
/// JSON output (`--message-format=json` → `samples` array) for
/// post-hoc percentile computation.
///
/// To obtain true tail-latency percentiles for publication, either:
/// (a) post-process the JSON `samples` array, or
/// (b) wrap this bench in a custom Criterion `Measurement` that records
///     per-iteration latencies and computes percentiles explicitly.
///
/// The bench function name (`creation_to_finality_mean`) reflects what
/// is actually reported. Previous versions used the misleading name
/// `creation_to_finality_p50_p95_p99`, which implied percentile
/// reporting that the default harness does not provide.
fn finality_latency_bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("finality_latency");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(100);

    let creator = test_node(1);

    group.bench_function("creation_to_finality_mean", |b| {
        b.iter_batched(
            || {
                let mut seed = [0u8; 32];
                seed[0] = 1;
                let config = ConsensusConfig {
                    total_nodes: 3,
                    round_seed: seed,
                    ..Default::default()
                };
                let slashing = SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
                let mut consensus = ConsensusEngine::new(config, slashing);
                consensus.register_validator(creator, 10_000);
                let mut graph = CausalGraph::new();

                let keypair = generate_keypair();
                let mut genesis = Event::genesis(creator, vec![]).expect("genesis creation");
                genesis.sign_with_keypair(&keypair).expect("signing");
                let genesis_id = genesis.id;
                graph.insert(genesis).expect("genesis insert");
                let _ = consensus.process_event(graph.get(&genesis_id).expect("genesis exists"), &graph);

                (graph, consensus, keypair, genesis_id, 1u64)
            },
            |(mut graph, mut consensus, keypair, last_id, seq)| {
                let start = Instant::now();

                let mut vc = VectorClock::new();
                vc.set(creator, seq + 1);
                let mut event =
                    Event::new(creator, seq, vc, Some(last_id), None, vec![1u8; 64]).expect("event creation");
                event.sign_with_keypair(&keypair).expect("signing");

                let event_id = event.id;
                let _ = graph.insert(event);
                if let Ok(graph_event) = graph.get_checked(&event_id) {
                    let _ = consensus.process_event(graph_event, &graph);
                }

                let elapsed = start.elapsed();
                black_box(elapsed);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Benchmark 3: ZK proof generation (feature-gated)
// ---------------------------------------------------------------------------

/// Measures ZK proof generation time for 1-tx and 100-tx batches.
///
/// This benchmark is feature-gated under `full` because it requires
/// the arkworks dependencies.
///
/// To run: `cargo bench --features full -- baseline_bench`
///
/// # Important: do NOT compare 1_tx_batch vs 100_tx_batch as a scaling curve
///
/// These two benchmarks use **fundamentally different circuits**, not the
/// same circuit at different batch sizes:
///
/// - `1_tx_batch` uses `RollupCircuit` (the basic circuit) — a minimal
///   circuit with no per-event Merkle path verification, no per-event
///   state-transition constraints. It proves only that two state roots
///   are linked by a single transition.
/// - `100_tx_batch` uses `ExpandedRollupCircuit` (the expanded circuit)
///   with 100 placeholder events and Merkle depth 8. Each event adds:
///   (a) a Merkle path verification gadget (8 Poseidon hashes per level),
///   (b) a state-transition constraint (1 Poseidon hash per event),
///   (c) an operation-type bit-decomposition range check (3 boolean
///       witnesses + reconstruction),
///   (d) a payload-hash binding constraint (1 Poseidon hash per event).
///
/// So the per-event constraint count of the expanded circuit is roughly
/// 8 + 1 + 3 + 1 = 13 Poseidon hashes plus allocation overhead — and
/// Poseidon is the dominant cost in Groth16 proving. The 100-tx bench
/// is therefore ~1300 Poseidon hashes plus 100 event allocations,
/// versus the 1-tx basic circuit which has zero Poseidon hashes.
///
/// **The 27× worse-than-linear scaling is expected** given the circuit
/// design difference. It is NOT a bug in batching — `create_expanded_proof`
/// does generate a single proof for the entire batch (not 100 sequential
/// proofs). The super-linear cost comes from the expanded circuit's
/// per-event constraint count, which grows as O(events × merkle_depth).
///
/// To properly characterize scaling, compare `expanded_circuit/{1,4,16}`
/// in `zk_benchmarks.rs` — those use the same `ExpandedRollupCircuit`
/// at different event counts and produce a meaningful scaling curve.
///
/// # Coverage caveat
///
/// `batch_proof_circuit.rs` has ~41% region coverage and `prover.rs`
/// has ~34.62% function coverage. The code path that produces these
/// 7.78s proofs is among the least-tested in the project. Treat the
/// absolute numbers as preliminary until coverage is raised.
#[cfg(feature = "full")]
fn zk_proof_gen_bench(c: &mut Criterion) {
    use omnia_adapters::circuit::{ExpandedRollupCircuit, RollupCircuit};
    use omnia_adapters::prover::{
        create_expanded_proof, create_proof, generate_trusted_setup, generate_trusted_setup_expanded,
    };

    let mut group = c.benchmark_group("zk_proof_gen");
    group.measurement_time(Duration::from_secs(30));
    group.sample_size(10);

    // 1-tx batch (basic circuit — no per-event constraints)
    let circuit = RollupCircuit::empty();
    let (pk, _vk) = generate_trusted_setup(&circuit).expect("setup failed");

    group.bench_function("1_tx_batch", |b| {
        b.iter(|| {
            let old = [1u8; 32];
            let new = [2u8; 32];
            let circuit = RollupCircuit::from_state_roots(old, new, 1, old, new);
            let _ = create_proof(circuit, &pk);
        })
    });

    // 100-tx batch (expanded circuit — 100 events × merkle_depth 8).
    // NOTE: This is NOT the same circuit as 1_tx_batch scaled up. See the
    // doc comment above for why the two numbers are not directly comparable.
    let num_events = 100;
    let merkle_depth = 8;
    let (pk_expanded, _vk) = generate_trusted_setup_expanded(num_events, merkle_depth).expect("expanded setup failed");

    group.bench_function("100_tx_batch", |b| {
        b.iter(|| {
            let circuit = ExpandedRollupCircuit::empty(num_events, merkle_depth);
            let _ = create_expanded_proof(circuit, &pk_expanded);
        })
    });

    group.finish();
}

#[cfg(not(feature = "full"))]
fn zk_proof_gen_bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("zk_proof_gen");
    group.bench_function("skipped_no_full_feature", |b| {
        b.iter(|| {
            // Placeholder — ZK benchmarks require `--features full`
        })
    });
    group.finish();
}

// ---------------------------------------------------------------------------
// Benchmark 4: Gossip propagation latency (single-node simulation)
// ---------------------------------------------------------------------------

/// Measures end-to-end event propagation latency in a single-node simulation.
///
/// Simulates the gossip pipeline: create event → serialize → deserialize →
/// insert into a second (simulated remote) graph. This measures the
/// computational overhead of the gossip path without actual network I/O.
fn gossip_latency_bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("gossip_latency");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(100);

    let creator = test_node(1);

    group.bench_function("propagation_single_node_sim", |b| {
        b.iter_batched(
            || {
                // "Local" graph with a genesis event
                let keypair = generate_keypair();
                let mut local_graph = CausalGraph::new();
                let mut genesis = Event::genesis(creator, vec![]).expect("genesis creation");
                genesis.sign_with_keypair(&keypair).expect("signing");
                let genesis_id = genesis.id;
                local_graph.insert(genesis).expect("genesis insert");

                // "Remote" graph (starts empty)
                let remote_graph = CausalGraph::new();

                (local_graph, remote_graph, keypair, genesis_id, 1u64)
            },
            |(_local_graph, mut remote_graph, keypair, last_id, seq)| {
                let start = Instant::now();

                // Create a new event on the "local" node
                let mut vc = VectorClock::new();
                vc.set(creator, seq + 1);
                let mut event =
                    Event::new(creator, seq, vc, Some(last_id), None, vec![1u8; 128]).expect("event creation");
                event.sign_with_keypair(&keypair).expect("signing");

                // Simulate serialization + deserialization (postcard wire format)
                let serialized = postcard::to_allocvec(&event).expect("serialize");
                let deserialized: Event = postcard::from_bytes(&serialized).expect("deserialize");

                // Insert into "remote" graph
                let _ = remote_graph.insert(deserialized);

                let elapsed = start.elapsed();
                black_box(elapsed);
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Benchmark 5: DAG event insertion latency (mean over varying graph sizes)
// ---------------------------------------------------------------------------

/// Measures mean DAG event insertion latency with varying graph sizes.
///
/// Tests insertion performance with empty, 100-event, and 1000-event
/// graphs to capture how insertion latency scales with graph size.
///
/// **What this number means**: Criterion's default harness reports the
/// **mean** of the per-iteration insertion times, with a 95% confidence
/// interval. For true tail percentiles, post-process the JSON `samples`
/// array or implement a custom `Measurement` harness.
fn dag_insert_bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("dag_insert");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(100);

    let creator = test_node(1);
    let keypair = generate_keypair();

    // Benchmark with different pre-existing graph sizes
    for &pre_fill in &[0usize, 100, 1000] {
        group.bench_with_input(
            BenchmarkId::new("insert_latency", pre_fill),
            &pre_fill,
            |b, &pre_fill| {
                b.iter_batched(
                    || {
                        let mut graph = CausalGraph::new();
                        let mut last_id: Option<[u8; 32]> = None;
                        let mut seq = 0u64;

                        // Pre-fill graph
                        for _ in 0..pre_fill {
                            let mut vc = VectorClock::new();
                            vc.set(creator, seq + 1);
                            let mut event = Event::new(creator, seq, vc, last_id, None, vec![seq as u8; 32])
                                .expect("event creation");
                            event.sign_with_keypair(&keypair).expect("signing");
                            last_id = Some(event.id);
                            graph.insert(event).expect("pre-fill insert");
                            seq += 1;
                        }

                        (graph, last_id, seq)
                    },
                    |(mut graph, last_id, seq)| {
                        let start = Instant::now();

                        let mut vc = VectorClock::new();
                        vc.set(creator, seq + 1);
                        let mut event =
                            Event::new(creator, seq, vc, last_id, None, vec![seq as u8; 32]).expect("event creation");
                        event.sign_with_keypair(&keypair).expect("signing");
                        let _ = graph.insert(event);

                        let elapsed = start.elapsed();
                        black_box(elapsed);
                    },
                    criterion::BatchSize::SmallInput,
                )
            },
        );
    }

    group.finish();
}

criterion_group!(
    name = baseline_benches;
    config = Criterion::default()
        .measurement_time(Duration::from_secs(10))
        .sample_size(50);
    targets =
        tx_throughput_bench,
        finality_latency_bench,
        zk_proof_gen_bench,
        gossip_latency_bench,
        dag_insert_bench
);

criterion_main!(baseline_benches);
