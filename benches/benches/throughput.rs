//! Unified throughput benchmark suite for the Omnia Protocol.
//!
//! Consolidated from substrate/benches/throughput.rs into the shared
//! omnia-benches crate. Uses the new crate structure directly:
//! - omnia-primitives: Event, VectorClock, NodeId
//! - omnia-consensus: CausalGraph, SlashingEngine, SlashOffense
//! - omnia-crypto: NodeKeypair, deterministic hash operations

use criterion::{criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use omnia_consensus::{slashing::SlashOffense, CausalGraph, SlashingEngine};
use omnia_crypto::{
    generate_keypair,
    vrf::{deterministic_compute, deterministic_verify, select_leader},
    NodeKeypair,
};
use omnia_primitives::{Event, NodeId, VectorClock};
use rand::rngs::OsRng;
use std::collections::HashMap;
use std::hint::black_box;
use std::time::Duration;

fn benchmark_event_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("event_creation");
    group.throughput(Throughput::Elements(1000));
    group.measurement_time(Duration::from_secs(10));

    let keypair = NodeKeypair::generate(&mut OsRng);
    let creator: NodeId = [0u8; 32];
    let vc = VectorClock::with_node(creator, 1);

    group.bench_function("create_and_sign", |b| {
        b.iter(|| {
            let mut event = Event::new(creator, 0, vc.clone(), None, None, vec![1, 2, 3]).expect("event creation");
            event.sign_with_keypair(&keypair);
            black_box(event);
        });
    });

    group.finish();
}

fn benchmark_graph_insertion(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph_insertion");
    group.throughput(Throughput::Elements(1000));

    let mut graph = CausalGraph::new();
    let keypair = NodeKeypair::generate(&mut OsRng);
    let creator: NodeId = [0u8; 32];

    // Pre-create genesis
    let mut genesis = Event::genesis(creator, vec![]).expect("genesis creation");
    genesis.sign_with_keypair(&keypair);
    graph.insert(genesis.clone()).expect("genesis insert should succeed");

    group.bench_function("insert_chain", |b| {
        let mut seq: u64 = 1;
        b.iter(|| {
            let vc = VectorClock::with_node(creator, seq + 1);
            let mut event = Event::new(creator, seq, vc, Some(genesis.id), None, vec![seq as u8]).expect("event creation");
            event.sign_with_keypair(&keypair);
            let _ = graph.insert(event);
            seq += 1;
        });
    });

    group.finish();
}

fn benchmark_vector_clock_merge(c: &mut Criterion) {
    let mut group = c.benchmark_group("vector_clock");

    let mut vc_a = VectorClock::new();
    let mut vc_b = VectorClock::new();

    for i in 0..100 {
        let mut node: NodeId = [0u8; 32];
        node[0] = i;
        vc_a.set(node, i as u64 * 2);
        vc_b.set(node, i as u64 * 3);
    }

    group.bench_function("merge_100_nodes", |b| {
        b.iter(|| {
            let mut result = vc_a.clone();
            result.merge(&vc_b);
            black_box(result);
        });
    });

    group.finish();
}

fn bench_slashing_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("slashing");

    group.bench_function("record_offense", |b| {
        b.iter_batched(
            || {
                let mut engine = SlashingEngine::new_in_memory(500, 2000);
                let node: NodeId = [42u8; 32];
                engine.register_validator(node, 10_000);
                (engine, node)
            },
            |(mut engine, node)| {
                engine.record_offense(node, SlashOffense::LivenessViolation);
            },
            BatchSize::SmallInput,
        )
    });

    group.bench_function("check_equivocation", |b| {
        let keypair = generate_keypair();
        let creator: NodeId = [0u8; 32];
        let vc = VectorClock::with_node(creator, 1);
        let mut event_a = Event::new(creator, 0, vc.clone(), None, None, vec![1, 2, 3]).expect("event creation");
        event_a.sign_with_keypair(&keypair);
        let mut event_b = Event::new(creator, 0, vc.clone(), None, None, vec![4, 5, 6]).expect("event creation");
        event_b.sign_with_keypair(&keypair);

        b.iter(|| {
            let _ = SlashingEngine::check_equivocation(&event_a, &event_b);
        })
    });

    group.finish();
}

fn bench_deterministic_hash(c: &mut Criterion) {
    let mut group = c.benchmark_group("deterministic_hash");

    group.bench_function("deterministic_compute", |b| {
        let keypair = generate_keypair();
        let input = b"round-42-seed";
        b.iter(|| {
            let _ = deterministic_compute(&keypair, input);
        })
    });

    group.bench_function("deterministic_verify", |b| {
        let keypair = generate_keypair();
        let input = b"round-42-seed";
        let output = deterministic_compute(&keypair, input);
        b.iter(|| {
            let _ = deterministic_verify(&keypair.verifying_key(), input, &output);
        })
    });

    group.bench_function("select_leader_100_validators", |b| {
        let mut candidates: HashMap<NodeId, (NodeKeypair, u64)> = HashMap::new();
        for i in 0..100u8 {
            let mut node: NodeId = [0u8; 32];
            node[0] = i;
            let kp = generate_keypair();
            candidates.insert(node, (kp, 100));
        }
        let seed = [0u8; 32];

        b.iter(|| {
            let _ = select_leader(&candidates, &seed, 1);
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    benchmark_event_creation,
    benchmark_graph_insertion,
    benchmark_vector_clock_merge,
    bench_slashing_operations,
    bench_deterministic_hash
);
criterion_main!(benches);
