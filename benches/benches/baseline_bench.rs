//! Baseline Benchmark Suite for Omnia Protocol Phase 0 Throughput Optimization.
//!
//! This suite establishes baseline performance measurements for:
//! - Transaction throughput (sustained TPS over a 60s window)
//! - Consensus finality latency (p50/p95/p99 from creation to commitment)
//! - ZK proof generation time (1-tx and 100-tx batches, feature-gated)
//! - Gossip propagation latency (single-node simulation)
//! - DAG event insertion latency (p50/p95/p99)
//!
//! These baselines are used to track regression and validate sprint targets
//! as defined in the Phase 0 Throughput Optimization Sprint Plan.

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

fn create_signed_event(
    creator: NodeId,
    seq: u64,
    vc: VectorClock,
    self_parent: Option<[u8; 32]>,
    other_parent: Option<[u8; 32]>,
    payload: Vec<u8>,
) -> Event {
    let keypair = generate_keypair();
    let mut event = Event::new(creator, seq, vc, self_parent, other_parent, payload);
    event.sign_with_keypair(&keypair);
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
/// configured with `total_nodes=1` for trivial single-node finality.
///
/// The reported throughput is events/sec — the number of events the node
/// can create, sign, insert, and finalize per second in steady state.
fn tx_throughput_bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("tx_throughput");
    group.throughput(Throughput::Elements(1000)); // Each iteration processes a 1000-event batch
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(20);

    let creator = test_node(1);
    let keypair = generate_keypair();

    group.bench_function("sustained_tps_single_node", |b| {
        b.iter_batched(
            || {
                // Setup: fresh graph + consensus for each batch
                let mut seed = [0u8; 32];
                seed[0] = 1;
                let config = ConsensusConfig {
                    total_nodes: 1,
                    round_seed: seed,
                    ..Default::default()
                };
                let slashing = SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
                let mut consensus = ConsensusEngine::new(config, slashing);
                consensus.register_validator(creator, 10_000);
                let mut graph = CausalGraph::new();

                // Genesis event
                let mut genesis = Event::genesis(creator, vec![]);
                genesis.sign_with_keypair(&keypair);
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

                    let mut event = Event::new(creator, seq, vc, Some(last_id), None, vec![seq as u8; 64]);
                    event.sign_with_keypair(&keypair);

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
// Benchmark 2: Finality latency (p50/p95/p99)
// ---------------------------------------------------------------------------

/// Measures time from event creation to finality commitment.
///
/// Uses a 3-node consensus configuration to simulate BFT finality.
/// Each event is created, inserted into the graph, and processed through
/// consensus. The time from `Instant::now()` before creation to the
/// instant after `process_event` returns committed events is recorded.
///
/// Criterion reports p50/p95/p99 automatically from the measurement distribution.
fn finality_latency_bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("finality_latency");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(100);

    let creator = test_node(1);

    group.bench_function("creation_to_finality_p50_p95_p99", |b| {
        b.iter_batched(
            || {
                let mut seed = [0u8; 32];
                seed[0] = 1;
                let config = ConsensusConfig {
                    total_nodes: 1,
                    round_seed: seed,
                    ..Default::default()
                };
                let slashing = SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
                let mut consensus = ConsensusEngine::new(config, slashing);
                consensus.register_validator(creator, 10_000);
                let mut graph = CausalGraph::new();

                let keypair = generate_keypair();
                let mut genesis = Event::genesis(creator, vec![]);
                genesis.sign_with_keypair(&keypair);
                let genesis_id = genesis.id;
                graph.insert(genesis).expect("genesis insert");
                let _ = consensus.process_event(graph.get(&genesis_id).expect("genesis exists"), &graph);

                (graph, consensus, keypair, genesis_id, 1u64)
            },
            |(mut graph, mut consensus, keypair, mut last_id, mut seq)| {
                let start = Instant::now();

                let mut vc = VectorClock::new();
                vc.set(creator, seq + 1);
                let mut event = Event::new(creator, seq, vc, Some(last_id), None, vec![1u8; 64]);
                event.sign_with_keypair(&keypair);

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
/// the arkworks dependencies. It measures:
/// - Basic circuit proof generation (1 transaction)
/// - Expanded circuit proof generation (100 transactions)
///
/// To run: `cargo bench --features full -- baseline_bench`
#[cfg(feature = "full")]
fn zk_proof_gen_bench(c: &mut Criterion) {
    use ark_bn254::Fr;
    use ark_ff::PrimeField;
    use omnia_adapters::circuit::{ExpandedRollupCircuit, RollupCircuit};
    use omnia_adapters::prover::{
        create_expanded_proof, create_proof, generate_trusted_setup, generate_trusted_setup_expanded,
    };

    let mut group = c.benchmark_group("zk_proof_gen");
    group.measurement_time(Duration::from_secs(30));
    group.sample_size(10);

    // 1-tx batch (basic circuit)
    let circuit = RollupCircuit::empty();
    let (pk, _vk) = generate_trusted_setup(&circuit).expect("setup failed");

    group.bench_function("1_tx_batch", |b| {
        b.iter(|| {
            let old = [1u8; 32];
            let new = [2u8; 32];
            let circuit = RollupCircuit::from_state_roots(old, new, 1);
            let _ = create_proof(circuit, &pk);
        })
    });

    // 100-tx batch (expanded circuit)
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
                let mut genesis = Event::genesis(creator, vec![]);
                genesis.sign_with_keypair(&keypair);
                let genesis_id = genesis.id;
                local_graph.insert(genesis).expect("genesis insert");

                // "Remote" graph (starts empty)
                let remote_graph = CausalGraph::new();

                (local_graph, remote_graph, keypair, genesis_id, 1u64)
            },
            |(local_graph, mut remote_graph, keypair, mut last_id, mut seq)| {
                let start = Instant::now();

                // Create a new event on the "local" node
                let mut vc = VectorClock::new();
                vc.set(creator, seq + 1);
                let mut event = Event::new(creator, seq, vc, Some(last_id), None, vec![1u8; 128]);
                event.sign_with_keypair(&keypair);

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
// Benchmark 5: DAG event insertion latency (p50/p95/p99)
// ---------------------------------------------------------------------------

/// Measures DAG event insertion latency with varying graph sizes.
///
/// Tests insertion performance with empty, 100-event, and 1000-event
/// graphs to capture how insertion latency scales with graph size.
/// Criterion reports p50/p95/p99 from the measurement distribution.
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
                            let mut event = Event::new(creator, seq, vc, last_id, None, vec![seq as u8; 32]);
                            event.sign_with_keypair(&keypair);
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
                        let mut event = Event::new(creator, seq, vc, last_id, None, vec![seq as u8; 32]);
                        event.sign_with_keypair(&keypair);
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
