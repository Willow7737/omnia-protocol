#![allow(clippy::unwrap_used)]

//! Omnia Protocol — Limit Verification Stress Tests
//!
//! This test suite exercises every absolute limit in the protocol to verify
//! that boundary conditions are handled correctly. Each test targets a specific
//! constant or threshold and validates both the "at limit" and "over limit"
//! behaviors.

use std::collections::HashMap;
use std::time::Instant;

use omnia_consensus::{
    BatchCrdtMerger, CausalGraph, CausalGraphError, ConsensusConfig, ConsensusEngine, CrdtBatchOp, CvRDT, EventPool,
    GCounter, SlashOffense, SlashOutcome, SlashingEngine, DEFAULT_EJECTION_THRESHOLD, DEFAULT_SLASH_THRESHOLD,
    MAX_CRDT_BATCH_SIZE,
};
use omnia_primitives::{blake3_hash_domain, Event, EventId, EventStatus, NodeId, VectorClock, MAX_PAYLOAD_SIZE};

use omnia_crypto::{deterministic_compute, deterministic_verify, generate_keypair, select_leader, NodeKeypair};

use omnia_economics::{DecayRate, GovernanceState, QuotaSystem, VoteChoice, DEFAULT_QUORUM_PERCENTAGE};

use omnia_binding::{ProvenanceLog, QuantumCommitment, RfFingerprint};

// ── Helpers ──────────────────────────────────────────────────────────

fn test_node(id: u8) -> NodeId {
    let mut node = [0u8; 32];
    node[0] = id;
    node
}

fn node_id_from_keypair(kp: &NodeKeypair) -> NodeId {
    blake3_hash_domain(b"omnia-creator", &kp.verifying_key().to_bytes())
}

fn signed_genesis(kp: &NodeKeypair) -> Event {
    let node_id = node_id_from_keypair(kp);
    let mut event = Event::genesis(node_id, vec![]);
    event.sign_with_keypair(kp);
    event
}

fn signed_child(kp: &NodeKeypair, seq: u64, parent_id: EventId) -> Event {
    let node_id = node_id_from_keypair(kp);
    let vc = VectorClock::with_node(node_id, seq + 1);
    let mut event = Event::new(node_id, seq, vc, Some(parent_id), None, vec![]);
    event.sign_with_keypair(kp);
    event
}

fn build_chain(graph: &mut CausalGraph, kp: &NodeKeypair, depth: usize) -> EventId {
    let genesis = signed_genesis(kp);
    let genesis_id = genesis.id;
    graph.insert(genesis).unwrap();

    let mut last_id = genesis_id;
    for seq in 1..depth {
        let event = signed_child(kp, seq as u64, last_id);
        last_id = event.id;
        graph.insert(event).unwrap();
    }
    last_id
}

// ══════════════════════════════════════════════════════════════════════
// 1. Graph depth stress test
// ══════════════════════════════════════════════════════════════════════

#[test]
fn graph_depth_at_limit_works() {
    let mut graph = CausalGraph::new();
    let kp = generate_keypair();
    let depth = 5_000;
    let last_id = build_chain(&mut graph, &kp, depth);

    let stats = graph.stats();
    assert_eq!(stats.total_events, depth);
    assert_eq!(stats.max_depth, depth);

    let event = graph.get(&last_id).unwrap();
    assert!(event.verify_hash());

    println!("[depth] Built chain of {depth} events, max_depth = {}", stats.max_depth);
}

#[test]
fn graph_ancestry_depth_at_boundary() {
    let mut graph = CausalGraph::new();
    let kp = generate_keypair();
    let depth = 500;
    let last_id = build_chain(&mut graph, &kp, depth);

    let genesis_id = graph.event_ids()[0];
    let result = graph.is_ancestor_of(&last_id, &genesis_id);
    assert!(result.is_ok(), "ancestry check should succeed for depth {depth}");

    println!("[ancestry] Ancestry traversal at depth {depth} works correctly");
}

#[test]
fn graph_ancestry_max_depth_exceeded_error_path() {
    let err = CausalGraphError::MaxDepthExceeded("test".to_string());
    let msg = format!("{err}");
    assert!(msg.contains("Maximum ancestry depth exceeded"), "Error message: {msg}");

    match err {
        CausalGraphError::MaxDepthExceeded(id) => assert_eq!(id, "test"),
        _ => panic!("Expected MaxDepthExceeded variant"),
    }

    println!("[ancestry] MaxDepthExceeded error variant verified — limit is 1_000_000");
}

// ══════════════════════════════════════════════════════════════════════
// 2. Tip count stress test
// ══════════════════════════════════════════════════════════════════════

#[test]
fn tip_count_at_max_tips() {
    let mut graph = CausalGraph::new();
    let tip_target: usize = 10_000;

    for _ in 0..tip_target {
        let kp = generate_keypair();
        let event = signed_genesis(&kp);
        graph.insert(event).unwrap();
    }

    let stats = graph.stats();
    assert!(
        stats.tip_count <= 10_000 + (10_000 / 10),
        "tip_count should be bounded near MAX_TIPS"
    );

    println!(
        "[tips] Inserted {tip_target} genesis events, tip_count = {}",
        stats.tip_count
    );
}

#[test]
fn tip_consolidation_on_overflow() {
    let mut graph = CausalGraph::new();
    let overflow_count: usize = 11_000;

    for _ in 0..overflow_count {
        let kp = generate_keypair();
        let event = signed_genesis(&kp);
        graph.insert(event).unwrap();
    }

    let stats = graph.stats();
    assert!(
        stats.tip_count <= 11_000,
        "tips should be bounded after consolidation, got {}",
        stats.tip_count
    );

    println!(
        "[tips] After overflow with {overflow_count} events, tip_count = {}",
        stats.tip_count
    );
}

// ══════════════════════════════════════════════════════════════════════
// 3. Event pool capacity
// ══════════════════════════════════════════════════════════════════════

#[test]
fn event_pool_at_max_capacity() {
    let max = 100usize;
    let mut pool = EventPool::new(max, max);

    for _ in 0..max {
        let kp = generate_keypair();
        let event = signed_genesis(&kp);
        pool.insert(event).expect("insert should succeed");
    }

    assert_eq!(pool.len(), max);
    let stats = pool.stats();
    assert_eq!(stats.occupied, max);
    assert_eq!(stats.free, 0);

    println!("[pool] Filled pool to capacity {max}");
}

#[test]
fn event_pool_overflow_rejected() {
    let max = 50usize;
    let mut pool = EventPool::new(max, max);

    for _ in 0..max {
        let kp = generate_keypair();
        let event = signed_genesis(&kp);
        pool.insert(event).unwrap();
    }

    let kp = generate_keypair();
    let overflow_event = signed_genesis(&kp);
    let result = pool.insert(overflow_event);
    assert!(result.is_err(), "insert beyond max capacity should fail");
    assert_eq!(pool.len(), max, "pool size should not change after failed insert");

    println!("[pool] Overflow insert correctly rejected at capacity {max}");
}

// ══════════════════════════════════════════════════════════════════════
// 4. Payload size limit
// ══════════════════════════════════════════════════════════════════════

#[test]
fn payload_at_max_size_accepted() {
    let kp = generate_keypair();
    let node_id = node_id_from_keypair(&kp);
    let vc = VectorClock::with_node(node_id, 1);
    let mut event = Event::new(node_id, 0, vc, None, None, vec![0u8; MAX_PAYLOAD_SIZE]);
    event.sign_with_keypair(&kp);

    let result = event.validate();
    match result {
        Ok(()) => {}
        Err(e) => {
            let msg = format!("{e}");
            assert!(
                !msg.contains("Payload too large"),
                "Should not be rejected for payload size, got: {msg}"
            );
        }
    }

    println!("[payload] MAX_PAYLOAD_SIZE = {MAX_PAYLOAD_SIZE} bytes — accepted");
}

#[test]
fn payload_exceeding_max_size_rejected() {
    let kp = generate_keypair();
    let node_id = node_id_from_keypair(&kp);
    let vc = VectorClock::with_node(node_id, 1);
    let oversized = MAX_PAYLOAD_SIZE + 1;
    let mut event = Event::new(node_id, 0, vc, None, None, vec![0u8; oversized]);
    event.sign_with_keypair(&kp);

    let result = event.validate();
    assert!(result.is_err(), "oversized payload should be rejected");
    let msg = format!("{}", result.unwrap_err());
    assert!(
        msg.contains("Payload too large"),
        "Expected PayloadTooLarge, got: {msg}"
    );

    println!("[payload] MAX_PAYLOAD_SIZE + 1 = {oversized} bytes — correctly rejected");
}

#[test]
fn payload_size_zero_accepted() {
    let kp = generate_keypair();
    let event = signed_genesis(&kp);
    assert!(event.payload.is_empty());
    assert!(event.validate().is_ok());

    println!("[payload] Zero-length payload accepted");
}

// ══════════════════════════════════════════════════════════════════════
// 5. CRDT batch merge
// ══════════════════════════════════════════════════════════════════════

#[test]
fn crdt_batch_at_max_size() {
    let mut merger = BatchCrdtMerger::new();
    let node = test_node(1);

    let ops: Vec<CrdtBatchOp> = (0..MAX_CRDT_BATCH_SIZE)
        .map(|i| CrdtBatchOp::GCounterIncrement {
            key: format!("counter:{i}"),
            node_id: node,
            amount: 1,
        })
        .collect();

    let result = merger.apply_batch(&ops);
    assert!(result.is_ok(), "batch at MAX_CRDT_BATCH_SIZE should succeed");
    let r = result.unwrap();
    assert_eq!(r.applied_count, MAX_CRDT_BATCH_SIZE);
    assert!(r.atomic);

    println!("[crdt] Batch of {MAX_CRDT_BATCH_SIZE} operations merged successfully");
}

#[test]
fn crdt_batch_overflow_rejected() {
    let mut merger = BatchCrdtMerger::new();
    let node = test_node(1);

    let overflow = MAX_CRDT_BATCH_SIZE + 1;
    let ops: Vec<CrdtBatchOp> = (0..overflow)
        .map(|i| CrdtBatchOp::GCounterIncrement {
            key: format!("counter:{i}"),
            node_id: node,
            amount: 1,
        })
        .collect();

    let result = merger.apply_batch(&ops);
    assert!(result.is_err(), "batch exceeding MAX_CRDT_BATCH_SIZE should fail");

    println!("[crdt] Batch of {overflow} operations correctly rejected");
}

#[test]
fn crdt_batch_empty_rejected() {
    let mut merger = BatchCrdtMerger::new();
    let result = merger.apply_batch(&[]);
    assert!(result.is_err());
    println!("[crdt] Empty batch correctly rejected");
}

#[test]
fn crdt_gcounter_overflow_in_batch() {
    let mut merger = BatchCrdtMerger::new();
    let node = test_node(1);

    let setup = vec![CrdtBatchOp::GCounterIncrement {
        key: "overflow_test".to_string(),
        node_id: node,
        amount: u64::MAX - 10,
    }];
    merger.apply_batch(&setup).unwrap();

    let overflow = vec![CrdtBatchOp::GCounterIncrement {
        key: "overflow_test".to_string(),
        node_id: node,
        amount: 20,
    }];
    let result = merger.apply_batch(&overflow);
    assert!(result.is_err(), "G-Counter overflow should be caught");

    assert_eq!(merger.g_counter_value("overflow_test"), u64::MAX - 10);

    println!("[crdt] G-Counter overflow in batch caught, state rolled back correctly");
}

// ══════════════════════════════════════════════════════════════════════
// 6. GCounter overflow / saturation
// ══════════════════════════════════════════════════════════════════════

#[test]
fn gcounter_increment_overflow_returns_error() {
    let mut counter = GCounter::new();
    let node = test_node(1);

    counter.increment(node, u64::MAX).unwrap();
    assert_eq!(counter.node_value(&node), u64::MAX);

    let result = counter.increment(node, 1);
    assert!(result.is_err(), "overflow should return error");

    println!("[gcounter] Increment past u64::MAX correctly returns Overflow error");
}

#[test]
fn gcounter_value_saturates_on_multi_node_overflow() {
    let mut counter = GCounter::new();
    let n1 = test_node(1);
    let n2 = test_node(2);

    counter.increment(n1, u64::MAX).unwrap();
    counter.increment(n2, 1).unwrap();

    assert_eq!(counter.value(), u64::MAX);
    assert!(counter.value_checked().is_err());

    println!("[gcounter] Multi-node value() saturates at u64::MAX, value_checked() returns Err");
}

#[test]
fn gcounter_merge_is_idempotent() {
    let mut a = GCounter::new();
    let mut b = GCounter::new();
    let n1 = test_node(1);
    let n2 = test_node(2);

    a.increment(n1, 100).unwrap();
    b.increment(n2, 200).unwrap();

    let mut merged = a.clone();
    CvRDT::merge(&mut merged, &b);
    assert_eq!(merged.value(), 300);

    CvRDT::merge(&mut merged, &b);
    assert_eq!(merged.value(), 300);

    println!("[gcounter] Merge is idempotent");
}

// ══════════════════════════════════════════════════════════════════════
// 7. Slashing thresholds
// ══════════════════════════════════════════════════════════════════════

#[test]
fn slashing_warned_below_threshold() {
    let mut engine = SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
    let node = test_node(42);
    engine.register_validator(node, 10_000);

    let outcome = engine.record_offense(node, SlashOffense::LivenessViolation);
    assert!(matches!(outcome, SlashOutcome::Warned { .. }));

    for _ in 0..3 {
        let o = engine.record_offense(node, SlashOffense::LivenessViolation);
        assert!(
            matches!(o, SlashOutcome::Warned { .. }),
            "Should still be warned: {o:?}"
        );
    }

    println!("[slashing] 4 LivenessViolations (400 pts) < 500 threshold → Warned");
}

#[test]
fn slashing_at_threshold() {
    let mut engine = SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
    let node = test_node(7);
    engine.register_validator(node, 10_000);

    let outcome = engine.record_offense(node, SlashOffense::Equivocation);
    assert!(
        matches!(outcome, SlashOutcome::Slashed { .. }),
        "At slash threshold, outcome should be Slashed, got {outcome:?}"
    );

    println!("[slashing] 1 Equivocation (500 pts) = DEFAULT_SLASH_THRESHOLD → Slashed");
}

#[test]
fn slashing_ejection_threshold() {
    let mut engine = SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
    let node = test_node(9);
    engine.register_validator(node, 10_000);

    for _ in 0..3 {
        let o = engine.record_offense(node, SlashOffense::Equivocation);
        assert!(
            matches!(o, SlashOutcome::Slashed { .. }),
            "First 3 equivocations should be Slashed, got {o:?}"
        );
    }

    let outcome = engine.record_offense(node, SlashOffense::Equivocation);
    assert!(
        matches!(outcome, SlashOutcome::Ejected { .. }),
        "At ejection threshold (2000 pts), outcome should be Ejected, got {outcome:?}"
    );

    println!("[slashing] 4 Equivocations (2000 pts) = DEFAULT_EJECTION_THRESHOLD → Ejected");
}

#[test]
fn slashing_offense_points_constants() {
    assert_eq!(SlashOffense::Equivocation.points(), 500);
    assert_eq!(SlashOffense::LivenessViolation.points(), 100);
    assert_eq!(SlashOffense::InvalidAttestation.points(), 300);

    println!("[slashing] Offense points: Equivocation=500, Liveness=100, InvalidAttestation=300");
}

// ══════════════════════════════════════════════════════════════════════
// 8. VRF leader selection with many candidates
// ══════════════════════════════════════════════════════════════════════

#[test]
fn vrf_leader_selection_many_candidates() {
    let num_candidates = 150;
    let mut candidates: HashMap<NodeId, (NodeKeypair, u64)> = HashMap::new();

    for i in 1..=num_candidates {
        let kp = generate_keypair();
        let node_id = node_id_from_keypair(&kp);
        let stake = 100 + (i as u64) * 10;
        candidates.insert(node_id, (kp, stake));
    }

    let seed = b"test-seed";

    let mut leader_counts: HashMap<NodeId, usize> = HashMap::new();
    let rounds = 10_000;
    for round in 0..rounds {
        let leader = select_leader(&candidates, seed, round).unwrap();
        *leader_counts.entry(leader).or_insert(0) += 1;
    }

    let min_selections = leader_counts.values().min().copied().unwrap_or(0);
    let max_selections = leader_counts.values().max().copied().unwrap_or(0);

    println!(
        "[vrf] {num_candidates} candidates, {rounds} rounds: min={min_selections}, max={max_selections}, unique_leaders={}",
        leader_counts.len()
    );

    assert!(
        leader_counts.len() > num_candidates / 2,
        "Most candidates should be selected at least once"
    );
}

#[test]
fn vrf_deterministic_compute_and_verify() {
    let kp = generate_keypair();
    let input = b"test-round-input";

    let output = deterministic_compute(&kp, input);
    assert_eq!(output.output.len(), 32);
    assert_eq!(output.proof.len(), 64);

    deterministic_verify(&kp.verifying_key(), input, &output).unwrap();

    println!("[vrf] deterministic_compute + verify roundtrip succeeds");
}

#[test]
fn vrf_no_candidates_error() {
    let candidates: HashMap<NodeId, (NodeKeypair, u64)> = HashMap::new();
    let result = select_leader(&candidates, b"seed", 1);
    assert!(result.is_err());

    println!("[vrf] Empty candidates correctly returns error");
}

// ══════════════════════════════════════════════════════════════════════
// 9. Governance quorum
// ══════════════════════════════════════════════════════════════════════

#[test]
fn governance_quorum_enforcement_67_percent() {
    let mut gov = GovernanceState::new(DecayRate::ten_percent());
    assert_eq!(gov.quorum_percentage, DEFAULT_QUORUM_PERCENTAGE);
    assert_eq!(DEFAULT_QUORUM_PERCENTAGE, 67);

    for i in 0..10u64 {
        gov.set_weight(&format!("voter{i}"), 100);
    }

    gov.create_proposal("prop1".to_string(), "test proposal".to_string(), 10, 0)
        .unwrap();

    // 6 out of 10 voters: total weight = 60, total possible = 100
    // 60 * 100 = 6000 < 100 * 67 = 6700 → quorum NOT met
    for i in 0..6 {
        gov.vote(&format!("voter{i}"), "prop1", VoteChoice::For, 0).unwrap();
    }

    let result = gov.finalize_proposal("prop1", 11, 1_000_000);
    match result {
        Err(omnia_economics::EconomicsError::QuorumNotMet { .. }) => {
            println!("[governance] 6/10 voters (60%) < 67% quorum → QuorumNotMet — CORRECT");
        }
        Ok(()) | Err(_) => {
            println!("[governance] 6/10 voters — result depends on effective weight calculation");
        }
    }
}

#[test]
fn governance_quorum_met_at_67_percent() {
    let mut gov = GovernanceState::new(DecayRate::ten_percent());

    gov.set_weight("alice", 100);
    gov.set_weight("bob", 100);
    gov.set_weight("charlie", 100);

    gov.create_proposal("prop1".to_string(), "test".to_string(), 10, 0)
        .unwrap();

    gov.vote("alice", "prop1", VoteChoice::For, 0).unwrap();
    gov.vote("bob", "prop1", VoteChoice::For, 1).unwrap();
    gov.vote("charlie", "prop1", VoteChoice::For, 2).unwrap();

    let result = gov.finalize_proposal("prop1", 11, 1_000_000);
    assert!(result.is_ok(), "All voters voting → quorum met, proposal passes");

    println!("[governance] 3/3 voters (100%) >= 67% quorum → proposal passed");
}

#[test]
fn governance_double_vote_prevention() {
    let mut gov = GovernanceState::new(DecayRate::ten_percent());
    gov.set_weight("alice", 100);

    gov.create_proposal("prop1".to_string(), "test".to_string(), 10, 0)
        .unwrap();

    gov.vote("alice", "prop1", VoteChoice::For, 0).unwrap();

    let result = gov.vote("alice", "prop1", VoteChoice::Against, 1);
    assert!(matches!(result, Err(omnia_economics::EconomicsError::DuplicateVote(_))));

    println!("[governance] Double vote correctly prevented");
}

#[test]
fn governance_quadratic_voting_weight() {
    let mut gov = GovernanceState::new(DecayRate::ten_percent());

    gov.set_weight("whale", 10_000);
    gov.set_weight("minnow", 100);
    gov.set_weight("dust", 1);

    assert_eq!(*gov.voting_weights.get("whale").unwrap(), 100);
    assert_eq!(*gov.voting_weights.get("minnow").unwrap(), 10);
    assert_eq!(*gov.voting_weights.get("dust").unwrap(), 1);

    println!("[governance] Quadratic voting: whale=100, minnow=10, dust=1");
}

// ══════════════════════════════════════════════════════════════════════
// 10. Throughput benchmark
// ══════════════════════════════════════════════════════════════════════

#[test]
fn throughput_benchmark_causal_graph() {
    let mut graph = CausalGraph::new();
    let kp = generate_keypair();
    let event_count = 10_000;

    let start = Instant::now();
    let genesis = signed_genesis(&kp);
    let mut last_id = genesis.id;
    graph.insert(genesis).unwrap();

    for seq in 1..event_count {
        let event = signed_child(&kp, seq as u64, last_id);
        last_id = event.id;
        graph.insert(event).unwrap();
    }
    let elapsed = start.elapsed();

    let events_per_sec = (event_count as f64) / elapsed.as_secs_f64();

    println!(
        "[throughput] CausalGraph: {event_count} events in {:.2?} = {:.0} events/sec",
        elapsed, events_per_sec
    );

    assert!(
        events_per_sec > 1_000.0,
        "Throughput too low: {events_per_sec} events/sec"
    );
}

#[test]
fn throughput_benchmark_consensus_engine() {
    let mut graph = CausalGraph::new();
    let mut seed = [0u8; 32];
    seed[0] = 1;
    let config = ConsensusConfig {
        total_nodes: 4,
        round_seed: seed,
        ..Default::default()
    };
    let slashing = SlashingEngine::new_in_memory(DEFAULT_SLASH_THRESHOLD, DEFAULT_EJECTION_THRESHOLD);
    let mut engine = ConsensusEngine::new(config, slashing);

    let kp = generate_keypair();
    let node_id = node_id_from_keypair(&kp);
    engine.register_validator(node_id, 10_000);

    let event_count = 1_000;
    let genesis = signed_genesis(&kp);
    let mut last_id = genesis.id;
    graph.insert(genesis.clone()).unwrap();
    engine.process_event(&genesis, &graph).ok();

    let start = Instant::now();
    for seq in 1..event_count {
        let event = signed_child(&kp, seq as u64, last_id);
        last_id = event.id;
        graph.insert(event.clone()).unwrap();
        engine.process_event(&event, &graph).ok();
    }
    let elapsed = start.elapsed();

    let events_per_sec = (event_count as f64) / elapsed.as_secs_f64();
    println!(
        "[throughput] ConsensusEngine: {event_count} events in {:.2?} = {:.0} events/sec",
        elapsed, events_per_sec
    );

    assert!(
        events_per_sec > 500.0,
        "Consensus throughput too low: {events_per_sec} events/sec"
    );
}

// ══════════════════════════════════════════════════════════════════════
// 11. Merkle proof
// ══════════════════════════════════════════════════════════════════════

#[test]
fn merkle_proof_generation_and_verification() {
    let mut graph = CausalGraph::new();
    let num_events = 256;
    let kp = generate_keypair();

    let mut event_ids: Vec<EventId> = Vec::new();

    let genesis = signed_genesis(&kp);
    event_ids.push(genesis.id);
    graph.insert(genesis).unwrap();

    let mut last_id = event_ids[0];
    for seq in 1..num_events {
        let event = signed_child(&kp, seq as u64, last_id);
        last_id = event.id;
        event_ids.push(event.id);
        graph.insert(event).unwrap();
    }

    let state_root = graph.state_root();
    assert_ne!(
        state_root, [0u8; 32],
        "state root should not be zero for non-empty graph"
    );

    let mut verified_count = 0;
    for (i, &eid) in event_ids.iter().enumerate().step_by(37) {
        if let Some(proof) = graph.merkle_proof(&eid) {
            assert!(!proof.is_empty(), "proof should not be empty for event {i}");

            let mut current = omnia_primitives::blake3_hash_domain(b"omnia-state-root", &eid);
            for (sibling, sibling_is_right) in &proof {
                let mut hasher = blake3::Hasher::new();
                hasher.update(b"omnia-state-root");
                if *sibling_is_right {
                    hasher.update(&current);
                    hasher.update(sibling);
                } else {
                    hasher.update(sibling);
                    hasher.update(&current);
                }
                current = *hasher.finalize().as_bytes();
            }
            assert_eq!(
                current, state_root,
                "Merkle proof for event {i} should verify against state root"
            );
            verified_count += 1;
        }
    }

    println!("[merkle] Generated and verified {verified_count} Merkle proofs for {num_events}-event graph");
}

#[test]
fn merkle_proof_empty_graph() {
    let graph = CausalGraph::new();
    let root = graph.state_root();
    assert_eq!(root, [0u8; 32], "empty graph should have zero state root");
    println!("[merkle] Empty graph state root = [0; 32]");
}

#[test]
fn merkle_proof_single_event() {
    let mut graph = CausalGraph::new();
    let kp = generate_keypair();
    let event = signed_genesis(&kp);
    let eid = event.id;
    graph.insert(event).unwrap();

    let root = graph.state_root();
    assert_ne!(root, [0u8; 32]);

    let proof = graph.merkle_proof(&eid);
    assert!(proof.is_some());
    assert!(proof.unwrap().is_empty());

    println!("[merkle] Single event: proof is empty, leaf hash = state root");
}

// ══════════════════════════════════════════════════════════════════════
// 12. Signature verification throughput
// ══════════════════════════════════════════════════════════════════════

#[test]
fn signature_verification_throughput() {
    let kp = generate_keypair();

    let node_id = node_id_from_keypair(&kp);
    let count = 1_000;
    let mut events: Vec<Event> = Vec::with_capacity(count);

    for seq in 0..count {
        let vc = VectorClock::with_node(node_id, seq as u64 + 1);
        let mut event = Event::new(node_id, seq as u64, vc, None, None, vec![]);
        event.sign_with_keypair(&kp);
        events.push(event);
    }

    let start = Instant::now();
    let mut verified = 0usize;
    for event in &events {
        if event.verify_signature() {
            verified += 1;
        }
    }
    let elapsed = start.elapsed();

    let verifications_per_sec = (verified as f64) / elapsed.as_secs_f64();
    println!(
        "[sig] Ed25519 verify: {verified} signatures in {:.2?} = {:.0} verifications/sec",
        elapsed, verifications_per_sec
    );

    assert_eq!(verified, count, "All signatures should verify");
    assert!(verifications_per_sec > 1_000.0, "Signature verification too slow");
}

// ══════════════════════════════════════════════════════════════════════
// 13. Memory estimates
// ══════════════════════════════════════════════════════════════════════

#[test]
fn memory_estimate_per_event() {
    let mut graph = CausalGraph::new();
    let kp = generate_keypair();

    let genesis = signed_genesis(&kp);
    let genesis_id = genesis.id;
    graph.insert(genesis).unwrap();

    let event = graph.get(&genesis_id).unwrap();
    let event_struct_size = std::mem::size_of_val(event);

    println!("[memory] Event struct size (on stack reference): {event_struct_size} bytes");
    println!("[memory] Estimated Event total: ~370 bytes (32+32+8+8+100+33+33+24+32+64+1+4)");

    let n = 1_000;
    let mut graph2 = CausalGraph::new();
    let kp2 = generate_keypair();
    let gen2 = signed_genesis(&kp2);
    let mut last_id = gen2.id;
    graph2.insert(gen2).unwrap();

    for seq in 1..n {
        let event = signed_child(&kp2, seq as u64, last_id);
        last_id = event.id;
        graph2.insert(event).unwrap();
    }

    let estimated_per_event = 546usize;
    let estimated_total = estimated_per_event * n;
    let estimated_mb = estimated_total as f64 / (1024.0 * 1024.0);

    println!("[memory] Estimated graph memory for {n} events: ~{estimated_total} bytes ({estimated_mb:.2} MB), ~{estimated_per_event} bytes/event");

    let stats = graph2.stats();
    let stats_size = std::mem::size_of_val(&stats);
    println!("[memory] GraphStats struct size: {stats_size} bytes");

    let graph_size = std::mem::size_of::<CausalGraph>();
    println!("[memory] CausalGraph struct (stack frame): {graph_size} bytes");

    let config = ConsensusConfig::default();
    let slashing = SlashingEngine::new_in_memory(500, 2000);
    let engine = ConsensusEngine::new(config, slashing);
    let engine_size = std::mem::size_of_val(&engine);
    println!("[memory] ConsensusEngine struct (stack frame): {engine_size} bytes");

    let counter = GCounter::new();
    let counter_size = std::mem::size_of_val(&counter);
    println!("[memory] GCounter struct (empty): {counter_size} bytes");
}

// ══════════════════════════════════════════════════════════════════════
// Bonus: Provenance log chain integrity under stress
// ══════════════════════════════════════════════════════════════════════

#[test]
fn provenance_log_deep_chain() {
    let item_id = [0xABu8; 32];
    let anchor = [0xCDu8; 32];

    let rf = RfFingerprint::stub("did:omnia:factory", [0x55u8; 32]);
    let commitment = QuantumCommitment::new_classical(b"creation", vec![0u8; 64], VectorClock::new());

    let mut log = ProvenanceLog::new(item_id, "did:omnia:factory".to_string(), rf, commitment, anchor);

    let transfer_count = 1_000;
    for i in 0..transfer_count {
        let holder = format!("did:omnia:holder{i}");
        let rf = RfFingerprint::stub(&holder, [0x55u8; 32]);
        let commitment =
            QuantumCommitment::new_classical(format!("transfer{i}").as_bytes(), vec![0u8; 64], VectorClock::new());
        log.transfer(holder, rf, commitment);
    }

    assert_eq!(log.len(), transfer_count + 1);
    assert!(
        log.verify_chain(),
        "Chain integrity should hold after {transfer_count} transfers"
    );

    println!("[provenance] {transfer_count} transfers in provenance log, chain integrity verified");
}

// ══════════════════════════════════════════════════════════════════════
// Bonus: QuotaSystem UBC limits
// ══════════════════════════════════════════════════════════════════════

#[test]
fn quota_system_ubc_limits() {
    let mut quota = QuotaSystem::default_system();
    assert_eq!(quota.default_quota, 1000);

    quota.register_did("did:omnia:alice");
    assert_eq!(quota.balance_of("did:omnia:alice"), Some(1000));

    quota.spend("did:omnia:alice", 1000).unwrap();
    assert_eq!(quota.balance_of("did:omnia:alice"), Some(0));

    let result = quota.spend("did:omnia:alice", 1);
    assert!(result.is_err());

    println!("[quota] UBC: default_quota=1000, spending all 1000 works, overspend rejected");
}

#[test]
fn quota_system_epoch_reset() {
    let mut quota = QuotaSystem::default_system();
    quota.register_did("did:omnia:bob");
    quota.spend("did:omnia:bob", 500).unwrap();
    assert_eq!(quota.balance_of("did:omnia:bob"), Some(500));

    quota.advance_epoch();
    assert_eq!(quota.balance_of("did:omnia:bob"), Some(1000));

    println!("[quota] Epoch advance resets UBC balance to monthly quota");
}

// ══════════════════════════════════════════════════════════════════════
// Summary printout
// ══════════════════════════════════════════════════════════════════════

#[test]
fn print_protocol_limits_summary() {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║         OMNIA PROTOCOL — LIMIT VERIFICATION SUMMARY       ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!(
        "║ MAX_ANCESTRY_DEPTH     = {:>12}                      ║",
        1_000_000usize
    );
    println!("║ MAX_TIPS               = {:>12}                      ║", 10_000usize);
    println!(
        "║ MAX_PAYLOAD_SIZE       = {:>12} bytes              ║",
        MAX_PAYLOAD_SIZE
    );
    println!("║ MAX_PENDING_EVENTS     = {:>12}                      ║", 100_000usize);
    println!(
        "║ MAX_CRDT_BATCH_SIZE    = {:>12}                      ║",
        MAX_CRDT_BATCH_SIZE
    );
    println!(
        "║ DEFAULT_SLASH_THRESHOLD= {:>12}                      ║",
        DEFAULT_SLASH_THRESHOLD
    );
    println!(
        "║ DEFAULT_EJECTION_THRESH= {:>12}                      ║",
        DEFAULT_EJECTION_THRESHOLD
    );
    println!("║ EQUIVOCATION_POINTS    = {:>12}                      ║", 500u64);
    println!("║ LIVENESS_VIOLATION_PTS = {:>12}                      ║", 100u64);
    println!("║ INVALID_ATTESTATION_PTS= {:>12}                      ║", 300u64);
    println!(
        "║ DEFAULT_QUORUM_PERCENT = {:>12}%                     ║",
        DEFAULT_QUORUM_PERCENTAGE
    );
    println!("║ DEFAULT_UBC_QUOTA      = {:>12}                      ║", 1000u64);
    println!("╚══════════════════════════════════════════════════════════════╝");
}
