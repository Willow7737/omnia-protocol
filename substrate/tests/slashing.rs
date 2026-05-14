//! Integration tests for the slashing module.
//!
//! These tests exercise the slashing engine both in isolation and integrated
//! with the consensus engine, verifying that Byzantine behavior is detected,
//! penalized, and that slashed nodes are rejected.

use omnia_substrate::{
    generate_keypair, CausalGraph, ConsensusConfig, ConsensusEngine, ConsensusError, Event,
    SlashOffense, SlashOutcome, SlashingEngine, VectorClock,
};

/// Helper: create a `NodeId` from a single byte.
fn node(id: u8) -> [u8; 32] {
    let mut n = [0u8; 32];
    n[0] = id;
    n
}

// ── Equivocation detection ─────────────────────────────────────────

#[test]
fn test_equivocation_detection_two_events_same_creator_sequence_different_hash() {
    let n1 = node(1);
    let kp = generate_keypair();

    // Create two events with the same creator and sequence but different payloads (→ different IDs)
    let vc = VectorClock::with_node(n1, 1);
    let mut event_a = Event::new(n1, 0, vc.clone(), None, None, vec![1]);
    event_a.sign_with_keypair(&kp);

    let mut event_b = Event::new(n1, 0, vc, None, None, vec![2]); // different payload → different id
    event_b.sign_with_keypair(&kp);

    assert!(SlashingEngine::check_equivocation(&event_a, &event_b));

    // Same event → not equivocation
    assert!(!SlashingEngine::check_equivocation(&event_a, &event_a));
}

#[test]
fn test_equivocation_triggers_slash_in_consensus() {
    let n1 = node(1);
    let kp = generate_keypair();

    let mut graph = CausalGraph::new();
    let config = ConsensusConfig::default();
    let mut engine = ConsensusEngine::new(config);
    engine.register_validator(n1, 10_000);

    // Insert and process the first event (sequence 0)
    let vc = VectorClock::with_node(n1, 1);
    let mut event_a = Event::new(n1, 0, vc.clone(), None, None, vec![1]);
    event_a.sign_with_keypair(&kp);
    graph.insert(event_a.clone()).unwrap();
    engine.process_event(&event_a, &graph).unwrap();

    // Now create an equivocating event: same creator, same sequence, different ID
    let mut event_b = Event::new(n1, 0, vc, None, None, vec![2]);
    event_b.sign_with_keypair(&kp);
    graph.insert(event_b.clone()).unwrap();

    // Processing the equivocating event should record an equivocation offense.
    // Since Equivocation = 500 points and slash_threshold = 500, the node should
    // now be slashed.
    let result = engine.process_event(&event_b, &graph);
    // The event should still be processed (slash check is before equivocation check),
    // but the equivocation should have been recorded.
    assert!(result.is_ok());

    // The node should now be slashed
    assert!(engine.is_slashed(&n1));

    // A subsequent event from the same node should be rejected
    let vc2 = VectorClock::with_node(n1, 2);
    let mut event_c = Event::new(n1, 1, vc2, None, None, vec![3]);
    event_c.sign_with_keypair(&kp);
    graph.insert(event_c.clone()).unwrap();

    let result = engine.process_event(&event_c, &graph);
    assert!(matches!(result, Err(ConsensusError::NodeSlashed(_))));
}

// ── Liveness violation ─────────────────────────────────────────────

#[test]
fn test_liveness_violation_inactive_node_accumulates_slash_points() {
    let mut engine = SlashingEngine::new_in_memory(500, 2000);
    let n1 = node(1);
    engine.register_validator(n1, 10_000);

    // Node was last active at round 5; current round is 20; threshold is 10
    // 20 - 5 = 15 > 10 → violation
    let result = engine.check_liveness(n1, 5, 20, 10);
    assert!(result.is_some());
    assert_eq!(engine.slash_points_of(&n1), 100); // LivenessViolation = 100 points

    // Multiple liveness violations accumulate
    let result = engine.check_liveness(n1, 5, 25, 10);
    assert!(result.is_some());
    assert_eq!(engine.slash_points_of(&n1), 200);
}

#[test]
fn test_liveness_no_violation_within_threshold() {
    let mut engine = SlashingEngine::new_in_memory(500, 2000);
    let n1 = node(1);
    engine.register_validator(n1, 10_000);

    // Node was last active at round 5; current round is 10; threshold is 10
    // 10 - 5 = 5 ≤ 10 → no violation
    let result = engine.check_liveness(n1, 5, 10, 10);
    assert!(result.is_none());
    assert_eq!(engine.slash_points_of(&n1), 0);
}

// ── Accumulated slash points exceed threshold → Slashed ────────────

#[test]
fn test_accumulated_points_exceed_slash_threshold() {
    let mut engine = SlashingEngine::new_in_memory(500, 2000);
    let n1 = node(1);
    engine.register_validator(n1, 10_000);

    // 5 × LivenessViolation = 500 points → slash threshold
    for _ in 0..4 {
        let outcome = engine.record_offense(n1, SlashOffense::LivenessViolation);
        // After 4 offenses: 400 points, still warned
        assert!(matches!(outcome, SlashOutcome::Warned { .. }));
    }

    // 5th offense: 500 points ≥ 500 threshold → Slashed
    let last_outcome = engine.record_offense(n1, SlashOffense::LivenessViolation);
    assert!(matches!(last_outcome, SlashOutcome::Slashed { .. }));
    assert!(engine.is_slashed(&n1));
}

// ── Slashed node's events are rejected ─────────────────────────────

#[test]
fn test_slashed_node_events_rejected() {
    let n1 = node(1);
    let kp = generate_keypair();

    let mut graph = CausalGraph::new();
    let config = ConsensusConfig::default();
    let mut engine = ConsensusEngine::new(config);
    engine.register_validator(n1, 10_000);

    // First, process a valid event
    let vc = VectorClock::with_node(n1, 1);
    let mut event_a = Event::new(n1, 0, vc.clone(), None, None, vec![1]);
    event_a.sign_with_keypair(&kp);
    graph.insert(event_a.clone()).unwrap();
    engine.process_event(&event_a, &graph).unwrap();

    // Now trigger slashing via equivocation
    let mut event_b = Event::new(n1, 0, vc, None, None, vec![2]);
    event_b.sign_with_keypair(&kp);
    graph.insert(event_b.clone()).unwrap();
    engine.process_event(&event_b, &graph).unwrap();

    // Node should be slashed now
    assert!(engine.is_slashed(&n1));

    // Try to submit a new event from the slashed node
    let vc2 = VectorClock::with_node(n1, 2);
    let mut event_c = Event::new(n1, 1, vc2, None, None, vec![3]);
    event_c.sign_with_keypair(&kp);
    graph.insert(event_c.clone()).unwrap();

    let result = engine.process_event(&event_c, &graph);
    assert!(matches!(result, Err(ConsensusError::NodeSlashed(_))));
}

// ── Honest node with no offenses → never slashed ───────────────────

#[test]
fn test_honest_node_never_slashed() {
    let mut engine = SlashingEngine::new_in_memory(500, 2000);
    let n1 = node(1);
    engine.register_validator(n1, 10_000);

    assert!(!engine.is_slashed(&n1));
    assert!(!engine.is_ejected(&n1));
    assert_eq!(engine.slash_points_of(&n1), 0);
    assert_eq!(engine.stake_of(&n1), 10_000);

    // Check liveness — node is active
    let result = engine.check_liveness(n1, 10, 12, 10);
    assert!(result.is_none());

    // Still not slashed
    assert!(!engine.is_slashed(&n1));
}

// ── register_validator → stake is tracked correctly ────────────────

#[test]
fn test_register_validator_stake_tracked() {
    let mut engine = SlashingEngine::new_in_memory(500, 2000);
    let n1 = node(1);
    let n2 = node(2);

    // Before registration
    assert_eq!(engine.stake_of(&n1), 0);
    assert_eq!(engine.stake_of(&n2), 0);

    // Register validators
    engine.register_validator(n1, 10_000);
    engine.register_validator(n2, 25_000);

    assert_eq!(engine.stake_of(&n1), 10_000);
    assert_eq!(engine.stake_of(&n2), 25_000);

    // Slash points should still be 0
    assert_eq!(engine.slash_points_of(&n1), 0);
    assert_eq!(engine.slash_points_of(&n2), 0);

    // Re-registering should update stake
    engine.register_validator(n1, 15_000);
    assert_eq!(engine.stake_of(&n1), 15_000);
}

// ── Ejection threshold → Ejected ───────────────────────────────────

#[test]
fn test_ejection_threshold_ejected() {
    let mut engine = SlashingEngine::new_in_memory(500, 2000);
    let n1 = node(1);
    engine.register_validator(n1, 10_000);

    // Accumulate points to reach ejection threshold
    // 4 × Equivocation = 2000 → ejection
    engine.record_offense(n1, SlashOffense::Equivocation); // 500
    assert!(!engine.is_ejected(&n1));

    engine.record_offense(n1, SlashOffense::Equivocation); // 1000
    assert!(!engine.is_ejected(&n1));

    engine.record_offense(n1, SlashOffense::Equivocation); // 1500
    assert!(!engine.is_ejected(&n1));

    let outcome = engine.record_offense(n1, SlashOffense::Equivocation); // 2000
    assert!(matches!(outcome, SlashOutcome::Ejected { .. }));
    assert!(engine.is_ejected(&n1));
    // Ejected implies also slashed
    assert!(engine.is_slashed(&n1));
}

#[test]
fn test_ejection_through_mixed_offenses() {
    let mut engine = SlashingEngine::new_in_memory(500, 2000);
    let n1 = node(1);
    engine.register_validator(n1, 10_000);

    // Mix offenses to reach 2000:
    // Equivocation(500) + InvalidAttestation(300) × 5 = 500 + 1500 = 2000
    engine.record_offense(n1, SlashOffense::Equivocation); // 500
    for _ in 0..4 {
        engine.record_offense(n1, SlashOffense::InvalidAttestation);
    } // 500 + 1200 = 1700
    assert!(!engine.is_ejected(&n1));

    let outcome = engine.record_offense(n1, SlashOffense::InvalidAttestation); // 2000
    assert!(matches!(outcome, SlashOutcome::Ejected { .. }));
    assert!(engine.is_ejected(&n1));
}

// ── Slashed amount reflects the node's stake ───────────────────────

#[test]
fn test_slashed_amount_reflects_stake() {
    let mut engine = SlashingEngine::new_in_memory(500, 2000);
    let n1 = node(1);
    engine.register_validator(n1, 42_000);

    let outcome = engine.record_offense(n1, SlashOffense::Equivocation); // 500 points
    assert_eq!(
        outcome,
        SlashOutcome::Slashed {
            node: n1,
            amount: 42_000
        }
    );
}

// ── Custom thresholds ──────────────────────────────────────────────

#[test]
fn test_custom_slash_thresholds() {
    let mut engine = SlashingEngine::new_in_memory(300, 1000);
    let n1 = node(1);
    engine.register_validator(n1, 5_000);

    // InvalidAttestation = 300 → exactly at slash threshold
    let outcome = engine.record_offense(n1, SlashOffense::InvalidAttestation);
    assert!(matches!(outcome, SlashOutcome::Slashed { .. }));

    // 300 + 300 + 300 + 300 = 1200 → above ejection threshold (1000)
    // Wait, we already have 300. Let's add more.
    engine.record_offense(n1, SlashOffense::InvalidAttestation); // 600
    engine.record_offense(n1, SlashOffense::InvalidAttestation); // 900
    assert!(!engine.is_ejected(&n1));

    let outcome = engine.record_offense(n1, SlashOffense::InvalidAttestation); // 1200
    assert!(matches!(outcome, SlashOutcome::Ejected { .. }));
    assert!(engine.is_ejected(&n1));
}

// ── Unregistered node offense tracking ─────────────────────────────

#[test]
fn test_offense_on_unregistered_node() {
    let mut engine = SlashingEngine::new_in_memory(500, 2000);
    let n1 = node(1);

    // Node not registered — should still track points
    let outcome = engine.record_offense(n1, SlashOffense::Equivocation);
    assert!(matches!(outcome, SlashOutcome::Slashed { amount: 0, .. }));
    assert_eq!(engine.stake_of(&n1), 0);
    assert_eq!(engine.slash_points_of(&n1), 500);
}
