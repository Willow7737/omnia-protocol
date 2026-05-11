use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use omnia_substrate::*;
use omnia_substrate::crypto::NodeKeypair;
use rand::rngs::OsRng;
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

    let mut graph = CausalGraph::new();
    let keypair = NodeKeypair::generate(&mut OsRng);
    let creator = [0u8; 32];

    // Pre-create genesis
    let mut genesis = Event::genesis(creator, vec![]);
    genesis.sign_with_keypair(&keypair);
    graph.insert(genesis.clone()).unwrap();

    group.bench_function("insert_chain", |b| {
        let mut seq = 1;
        b.iter(|| {
            let mut vc = VectorClock::with_node(creator, seq + 1);
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

criterion_group!(benches, benchmark_event_creation, benchmark_graph_insertion, benchmark_vector_clock_merge);
criterion_main!(benches);
