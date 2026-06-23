//! Iai-callgrind benchmarks for hot-path functions.
//!
//! These benchmarks use deterministic callgrind profiling to measure
//! instruction counts and cache behavior for performance-critical paths.
//! Unlike criterion (statistical), iai-callgrind produces reproducible
//! results ideal for CI regression detection.
//!
//! # Coverage
//!
//! ## Core hot paths (original)
//! - `bench_vector_clock_merge_100` — vector clock merge with 100 nodes
//! - `bench_event_validate` — event creation + Ed25519 signature verification
//! - `bench_causal_graph_insert` — causal graph insertion (genesis + 1 child)
//!
//! ## Slashing hot paths (added 2026-06-23 per mentor review)
//! The slashing module is the highest-risk module for silent correctness
//! divergence — it uses f64→basis-points arithmetic with deferred
//! non-deterministic conversions, and at ~57% function coverage it has
//! the lowest test coverage of any consensus module. The IAI benchmarks
//! below cover the three hot paths that run on EVERY event:
//! - `bench_check_equivocation` — constant-time equivocation detection,
//!   called on every pair of events from the same creator
//! - `bench_record_offense_equivocation` — the main state mutation path
//!   for the most severe offense type (500 points)
//! - `bench_record_offense_liveness` — the periodic liveness check path
//!   (100 points per violation)
//! - `bench_check_liveness` — the liveness threshold comparison path

#![allow(clippy::unwrap_used)]

use iai_callgrind::{black_box, library_benchmark, library_benchmark_group, main};
use omnia_consensus::{CausalGraph, SlashOffense, SlashingEngine};
use omnia_crypto::{generate_keypair, NodeKeypair};
use omnia_primitives::{blake3_hash_domain, Event, NodeId, VectorClock};
use std::sync::OnceLock;

/// Generate the keypair once so it is not re-created on every benchmark
/// iteration. Key generation is expensive and would dominate the measured
/// instruction count if included in the hot path.
static BENCH_KEYPAIR: OnceLock<NodeKeypair> = OnceLock::new();

fn get_keypair() -> &'static NodeKeypair {
    BENCH_KEYPAIR.get_or_init(generate_keypair)
}

/// Helper to create a minimal valid (signed) Event for benchmarking.
fn create_signed_event(seq: u64, parent: Option<[u8; 32]>) -> Event {
    let kp = get_keypair();
    let creator = blake3_hash_domain(b"omnia-creator", &kp.verifying_key().to_bytes());
    let vc = VectorClock::with_node(creator, seq + 1);
    let mut event = Event::new(creator, seq, vc, parent, None, vec![1, 2, 3]).expect("event creation");
    event.sign_with_keypair(kp).expect("signing");
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
    let event = create_signed_event(0, None);
    event.validate().unwrap();
    black_box(());
}

#[library_benchmark]
fn bench_causal_graph_insert() {
    let mut graph = CausalGraph::new();
    let genesis = create_signed_event(0, None);
    let _ = graph.insert(genesis.clone());
    let event = create_signed_event(1, Some(genesis.id));
    let _ = black_box(graph.insert(event));
}

// ── Slashing hot-path benchmarks ─────────────────────────────────
//
// These cover the three methods on SlashingEngine that run on every
// event or every consensus round. A regression in instruction count
// here means the slashing code path changed — which is the highest-
// risk module for silent correctness divergence.

/// Create two events with the same creator and sequence but different
/// payloads (and therefore different EventIds). This is the equivocation
/// pattern that `check_equivocation` is designed to detect.
fn create_equivocating_events() -> (Event, Event) {
    let kp = get_keypair();
    let creator = blake3_hash_domain(b"omnia-creator", &kp.verifying_key().to_bytes());
    let vc = VectorClock::with_node(creator, 1);

    // Same creator, same sequence, different payload → different EventId
    let mut event_a = Event::new(creator, 0, vc.clone(), None, None, vec![1, 2, 3]).expect("event a");
    event_a.sign_with_keypair(kp).expect("signing a");

    let mut event_b = Event::new(creator, 0, vc, None, None, vec![4, 5, 6]).expect("event b");
    event_b.sign_with_keypair(kp).expect("signing b");

    (event_a, event_b)
}

/// Create two events with the same creator but DIFFERENT sequence
/// numbers. This is NOT equivocation — `check_equivocation` should
/// return false. We benchmark this path too because it's the common
/// case (most event pairs are NOT equivocating).
fn create_non_equivocating_events() -> (Event, Event) {
    let kp = get_keypair();
    let creator = blake3_hash_domain(b"omnia-creator", &kp.verifying_key().to_bytes());

    let mut event_a = Event::new(
        creator,
        0,
        VectorClock::with_node(creator, 1),
        None,
        None,
        vec![1, 2, 3],
    )
    .expect("event a");
    event_a.sign_with_keypair(kp).expect("signing a");

    let mut event_b = Event::new(
        creator,
        1,
        VectorClock::with_node(creator, 2),
        Some(event_a.id),
        None,
        vec![4, 5, 6],
    )
    .expect("event b");
    event_b.sign_with_keypair(kp).expect("signing b");

    (event_a, event_b)
}

#[library_benchmark]
fn bench_check_equivocation_detected() {
    // Equivocation detected: same creator + sequence, different EventId.
    // This exercises the constant-time comparison (ct_eq / ct_ne) and
    // the short-circuit AND logic.
    let (event_a, event_b) = create_equivocating_events();
    let result = SlashingEngine::check_equivocation(&event_a, &event_b);
    black_box(result);
}

#[library_benchmark]
fn bench_check_equivocation_not_detected() {
    // Equivocation NOT detected: same creator, different sequence.
    // This is the common case — most event pairs are not equivocating.
    // The function should return false quickly after the sequence
    // comparison fails.
    let (event_a, event_b) = create_non_equivocating_events();
    let result = SlashingEngine::check_equivocation(&event_a, &event_b);
    black_box(result);
}

#[library_benchmark]
fn bench_record_offense_equivocation() {
    // Record an Equivocation offense (500 points — the most severe).
    // This exercises:
    //   - state.write() lock acquisition (with abort-on-poison)
    //   - state.clone() for the snapshot (used by undo)
    //   - saturating_add on slash_points
    //   - offense_history and typed_offense_history Vec push
    //   - threshold comparison (ejection_threshold, slash_threshold)
    //   - persist_state() (in-memory store → no-op)
    //
    // We use a fresh SlashingEngine per iteration to avoid accumulating
    // points across iterations (which would change the code path as the
    // node approaches ejection threshold).
    let mut engine = SlashingEngine::new_in_memory(500, 2000);
    let node = blake3_hash_domain(b"omnia-node", &[1u8; 32]);
    engine.register_validator(node, 10_000);
    let outcome = engine.record_offense(node, SlashOffense::Equivocation);
    black_box(outcome);
}

#[library_benchmark]
fn bench_record_offense_liveness() {
    // Record a LivenessViolation offense (100 points).
    // Same code path as Equivocation but with a different point value
    // (100 vs 500), which exercises a different branch in the threshold
    // comparison (Warned vs Slashed for a fresh node).
    let mut engine = SlashingEngine::new_in_memory(500, 2000);
    let node = blake3_hash_domain(b"omnia-node", &[2u8; 32]);
    engine.register_validator(node, 10_000);
    let outcome = engine.record_offense(node, SlashOffense::LivenessViolation);
    black_box(outcome);
}

#[library_benchmark]
fn bench_check_liveness_violation() {
    // check_liveness with a violation: last_active=5, current=20,
    // threshold=10 → 15 > 10, so a LivenessViolation is recorded.
    // This exercises:
    //   - round subtraction (current - last_active)
    //   - threshold comparison
    //   - record_offense (which is benchmarked separately above, but
    //     here we measure the full check_liveness entry path including
    //     the read lock on state.stakes)
    let mut engine = SlashingEngine::new_in_memory(500, 2000);
    let node = blake3_hash_domain(b"omnia-node", &[3u8; 32]);
    engine.register_validator(node, 10_000);
    let result = engine.check_liveness(node, 5, 20, 10);
    black_box(result);
}

#[library_benchmark]
fn bench_check_liveness_no_violation() {
    // check_liveness with NO violation: last_active=15, current=20,
    // threshold=10 → 5 <= 10, so None is returned. This is the common
    // case — most liveness checks pass. The function should return
    // quickly after the threshold comparison.
    let mut engine = SlashingEngine::new_in_memory(500, 2000);
    let node = blake3_hash_domain(b"omnia-node", &[4u8; 32]);
    engine.register_validator(node, 10_000);
    let result = engine.check_liveness(node, 15, 20, 10);
    black_box(result);
}

library_benchmark_group!(
    name = hot_path_group;
    benchmarks =
        bench_vector_clock_merge_100,
        bench_event_validate,
        bench_causal_graph_insert,
        bench_check_equivocation_detected,
        bench_check_equivocation_not_detected,
        bench_record_offense_equivocation,
        bench_record_offense_liveness,
        bench_check_liveness_violation,
        bench_check_liveness_no_violation
);

main!(library_benchmark_groups = hot_path_group);
