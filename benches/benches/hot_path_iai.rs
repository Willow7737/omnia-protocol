//! Iai-callgrind benchmarks for hot-path functions.
//!
//! These benchmarks use deterministic callgrind profiling to measure
//! instruction counts and cache behavior for performance-critical paths.
//! Unlike criterion (statistical), iai-callgrind produces reproducible
//! results ideal for CI regression detection.

use iai_callgrind::{black_box, library_benchmark, library_benchmark_group, main};
use omnia_primitives::{Event, NodeId, VectorClock};
use omnia_consensus::CausalGraph;
use omnia_crypto::generate_keypair;

/// Helper to create a minimal valid (signed) Event for benchmarking.
fn create_signed_event(creator: NodeId, seq: u64, parent: Option<[u8; 32]>) -> Event {
    let keypair = generate_keypair();
    let vc = VectorClock::with_node(creator, seq + 1);
    let mut event = Event::new(creator, seq, vc, parent, None, vec![1, 2, 3]);
    event.sign_with_keypair(&keypair);
    event
}

#[library_benchmark]
fn bench_vector_clock_merge_100() {
    let mut vc = VectorClock::new();
    for i in 0..100u8 {
        let mut node: NodeId = [0u8; 32];
        node[0] = i;
        let _ = vc.increment(node);
    }
    let mut other = VectorClock::new();
    for i in 50..150u8 {
        let mut node: NodeId = [0u8; 32];
        node[0] = i;
        let _ = other.increment(node);
    }
    black_box(vc.merged(&other));
}

#[library_benchmark]
fn bench_event_validate() {
    let creator: NodeId = [0u8; 32];
    let event = create_signed_event(creator, 0, None);
    black_box(event.validate().unwrap());
}

#[library_benchmark]
fn bench_causal_graph_insert() {
    let creator: NodeId = [0u8; 32];
    let mut graph = CausalGraph::new();
    let genesis = create_signed_event(creator, 0, None);
    let _ = graph.insert(genesis.clone());
    let event = create_signed_event(creator, 1, Some(genesis.id));
    black_box(graph.insert(event));
}

library_benchmark_group!(
    name = hot_path_group;
    benchmarks = bench_vector_clock_merge_100, bench_event_validate, bench_causal_graph_insert
);

main!(library_benchmark_groups = (hot_path_group));
