use criterion::{criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use omnia_substrate::crypto::NodeKeypair;
use omnia_substrate::*;
use rand::rngs::OsRng;
use std::collections::HashMap;
use std::hint::black_box;
use std::time::Duration;

fn benchmark_event_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("event_creation");
    group.throughput(Throughput::Elements(1000));
    group.measurement_time(Duration::from_secs(10));

    let keypair = NodeKeypair::generate(&mut OsRng);
    let creator = [0u8; 32];
    let vc = VectorClock::with_node(creator, 1);

    group.bench_function("create_and_sign", |b| {
        b.iter(|| {
            let mut event = Event::new(creator, 0, vc.clone(), None, None, vec![1, 2, 3]);
            event.sign_with_keypair(&keypair);
            black_box(event);
        });
    });

    group.finish();
}

fn benchmark_graph_insertion(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph_insertion");
    group.throughput(Throughput::Elements(1000));

    let mut graph = CausalGraph::new(); // mut needed for insert
    let keypair = NodeKeypair::generate(&mut OsRng);
    let creator = [0u8; 32];

    // Pre-create genesis
    let mut genesis = Event::genesis(creator, vec![]);
    genesis.sign_with_keypair(&keypair);
    graph
        .insert(genesis.clone())
        .expect("genesis insert should succeed");

    group.bench_function("insert_chain", |b| {
        let mut seq: u64 = 1;
        b.iter(|| {
            let vc = VectorClock::with_node(creator, seq + 1);
            let mut event = Event::new(creator, seq, vc, Some(genesis.id), None, vec![seq as u8]);
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
        let mut node = [0u8; 32];
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
                let node = [42u8; 32];
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
        // Use pre-built events for equivocation check
        let keypair = omnia_substrate::crypto::generate_keypair();
        let creator = [0u8; 32];
        let vc = VectorClock::with_node(creator, 1);
        let mut event_a = Event::new(creator, 0, vc.clone(), None, None, vec![1, 2, 3]);
        event_a.sign_with_keypair(&keypair);
        let mut event_b = Event::new(creator, 0, vc.clone(), None, None, vec![4, 5, 6]);
        event_b.sign_with_keypair(&keypair);

        b.iter(|| {
            let _ = SlashingEngine::check_equivocation(&event_a, &event_b);
        })
    });

    group.finish();
}

fn bench_vrf_compute(c: &mut Criterion) {
    let mut group = c.benchmark_group("vrf");

    group.bench_function("vrf_compute", |b| {
        let keypair = omnia_substrate::crypto::generate_keypair();
        let input = b"round-42-seed";
        b.iter(|| {
            let _ = omnia_substrate::vrf::vrf_compute(&keypair, input);
        })
    });

    group.bench_function("vrf_verify", |b| {
        let keypair = omnia_substrate::crypto::generate_keypair();
        let input = b"round-42-seed";
        let vrf_output = omnia_substrate::vrf::vrf_compute(&keypair, input);
        b.iter(|| {
            let _ = omnia_substrate::vrf::vrf_verify(&keypair.verifying_key(), input, &vrf_output);
        })
    });

    group.bench_function("select_leader_100_validators", |b| {
        let mut candidates: HashMap<[u8; 32], (NodeKeypair, u64)> = HashMap::new();
        for i in 0..100u8 {
            let mut node = [0u8; 32];
            node[0] = i;
            let kp = omnia_substrate::crypto::generate_keypair();
            candidates.insert(node, (kp, 100));
        }
        let seed = [0u8; 32];

        b.iter(|| {
            let _ = omnia_substrate::vrf::select_leader(&candidates, &seed, 1);
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
    bench_vrf_compute
);
criterion_main!(benches);
