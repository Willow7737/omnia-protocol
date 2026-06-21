#![allow(clippy::unwrap_used)]
//! End-to-end UBC lifecycle + governance test
//!
//! Tests the full lifecycle of the economics layer:
//! 1. DID registration and UBC token creation
//! 2. Spending UBC for transactions
//! 3. Epoch advancement and balance reset
//! 4. Useful-work reward earning
//! 5. Governance proposal creation and voting
//! 6. Quadratic voting weight calculation
//! 7. Reputation decay for inactive voters

use omnia_economics::{
    DecayRate, EconomicsError, EconomicsOp, EconomicsState, QuotaSystem, UbcToken, UsefulWorkProof, UsefulWorkType,
    VoteChoice, DEFAULT_UBC_QUOTA,
};

/// Helper: create a non-zero result hash.
fn nonzero_hash() -> [u8; 32] {
    let mut hash = [0u8; 32];
    hash[0] = 1;
    hash
}

#[test]
fn test_ubc_token_creation() {
    let token = UbcToken::new("did:omnia:alice".to_string(), 1000, 0);
    assert_eq!(token.owner_did, "did:omnia:alice");
    assert_eq!(token.balance, 1000);
    assert_eq!(token.monthly_quota, 1000);
    assert_eq!(token.last_reset_epoch, 0);
}

#[test]
fn test_ubc_spend_success() {
    let mut token = UbcToken::new("did:omnia:alice".to_string(), 1000, 0);
    assert!(token.spend(500).is_ok());
    assert_eq!(token.balance, 500);
}

#[test]
fn test_ubc_spend_insufficient() {
    let mut token = UbcToken::new("did:omnia:alice".to_string(), 100, 0);
    let result = token.spend(200);
    assert!(matches!(
        result,
        Err(EconomicsError::InsufficientUbc { have: 100, need: 200 })
    ));
}

#[test]
fn test_ubc_spend_zero_amount() {
    let mut token = UbcToken::new("did:omnia:alice".to_string(), 1000, 0);
    let result = token.spend(0);
    assert!(matches!(result, Err(EconomicsError::InvalidAmount(_))));
}

#[test]
fn test_ubc_mint_monthly_resets_balance() {
    let mut token = UbcToken::new("did:omnia:alice".to_string(), 1000, 0);
    token.spend(800).unwrap();
    assert_eq!(token.balance, 200);

    // Advancing the epoch resets balance to monthly quota
    token.mint_monthly(1);
    assert_eq!(token.balance, 1000);
    assert_eq!(token.last_reset_epoch, 1);
}

#[test]
fn test_ubc_mint_monthly_no_double_reset() {
    let mut token = UbcToken::new("did:omnia:alice".to_string(), 1000, 0);
    token.mint_monthly(1);
    assert_eq!(token.balance, 1000);

    // Minting again in the same epoch is a no-op
    token.spend(500).unwrap();
    token.mint_monthly(1);
    assert_eq!(token.balance, 500);
}

#[test]
fn test_ubc_reward_additive() {
    let mut token = UbcToken::new("did:omnia:alice".to_string(), 1000, 0);
    token.spend(500).unwrap();
    token.reward(300).unwrap();
    assert_eq!(token.balance, 800);
}

#[test]
fn test_quota_system_register_and_spend() {
    let mut system = QuotaSystem::default_system();
    system.register_did("did:omnia:alice");

    assert!(system.is_registered("did:omnia:alice"));
    assert_eq!(system.balance_of("did:omnia:alice"), Some(DEFAULT_UBC_QUOTA));

    assert!(system.spend("did:omnia:alice", 300).is_ok());
    assert_eq!(system.balance_of("did:omnia:alice"), Some(DEFAULT_UBC_QUOTA - 300));
}

#[test]
fn test_quota_system_spend_unregistered() {
    let mut system = QuotaSystem::default_system();
    let result = system.spend("did:omnia:unknown", 100);
    assert!(matches!(result, Err(EconomicsError::DidNotRegistered(_))));
}

#[test]
fn test_quota_system_advance_epoch() {
    let mut system = QuotaSystem::default_system();
    system.register_did("did:omnia:alice");
    system.spend("did:omnia:alice", 800).unwrap();
    assert_eq!(system.balance_of("did:omnia:alice"), Some(DEFAULT_UBC_QUOTA - 800));

    system.advance_epoch();
    assert_eq!(system.current_epoch, 1);
    assert_eq!(system.balance_of("did:omnia:alice"), Some(DEFAULT_UBC_QUOTA));
}

#[test]
fn test_quota_system_reward() {
    let mut system = QuotaSystem::default_system();
    system.register_did("did:omnia:alice");
    system.spend("did:omnia:alice", 500).unwrap();

    system.reward("did:omnia:alice", 200).unwrap();
    assert_eq!(
        system.balance_of("did:omnia:alice"),
        Some(DEFAULT_UBC_QUOTA - 500 + 200)
    );
}

#[test]
fn test_useful_work_proof_validate() {
    let proof = UsefulWorkProof::new(
        UsefulWorkType::AiTraining {
            model_hash: nonzero_hash(),
            training_data_hash: nonzero_hash(),
        },
        nonzero_hash(),
        100,
        vec![1, 2, 3, 4],
    )
    .expect("valid proof should construct");
    assert_eq!(proof.reward_amount(), 100);
}

#[test]
fn test_useful_work_proof_zero_compute_units() {
    let result = UsefulWorkProof::new(
        UsefulWorkType::DistributedStorage {
            data_hash: nonzero_hash(),
            storage_duration: 86400000,
        },
        nonzero_hash(),
        0,
        vec![1, 2, 3, 4],
    );
    assert!(matches!(result, Err(EconomicsError::WorkProofInvalid)));
}

#[test]
fn test_governance_quadratic_weight() {
    use omnia_economics::GovernanceState;

    let mut gov = GovernanceState::new(DecayRate::ten_percent());

    // Alice has 100 stake → isqrt(100) = 10 weight
    gov.set_weight("did:omnia:alice", 100, 0);
    assert_eq!(gov.voting_weights.get("did:omnia:alice"), Some(&10));

    // Bob has 10000 stake → isqrt(10000) = 100 weight
    gov.set_weight("did:omnia:bob", 10000, 0);
    assert_eq!(gov.voting_weights.get("did:omnia:bob"), Some(&100));

    // At epoch 0, both have full weight
    assert_eq!(gov.effective_weight("did:omnia:alice", 0), 10);
    assert_eq!(gov.effective_weight("did:omnia:bob", 0), 100);
}

#[test]
fn test_governance_decay() {
    use omnia_economics::GovernanceState;

    let mut gov = GovernanceState::new(DecayRate::ten_percent()); // 10% decay per epoch
    gov.set_weight("did:omnia:alice", 100, 0); // base weight = 10

    // At epoch 0, full weight
    assert_eq!(gov.effective_weight("did:omnia:alice", 0), 10);

    // After voting at epoch 0, then 1 epoch of inactivity
    gov.vote("did:omnia:alice", "prop1", VoteChoice::For, 0).ok();
    // Now last_active = 0, current_epoch = 1
    // inactive = 1, remaining_ppm = 900_000
    // effective = 10 * 900_000 / 1_000_000 = 9
    assert_eq!(gov.effective_weight("did:omnia:alice", 1), 9);

    // After more epochs of inactivity
    // inactive = 6, remaining = 900_000^6 / 1_000_000^6
    // Computed iteratively: should be approximately 5.3 → 5
    let weight_at_6 = gov.effective_weight("did:omnia:alice", 6);
    assert_eq!(weight_at_6, 5);
}

#[test]
fn test_governance_voting() {
    use omnia_economics::GovernanceState;

    let mut gov = GovernanceState::new(DecayRate::ten_percent());
    gov.set_weight("did:omnia:alice", 100, 0); // weight = 10
    gov.set_weight("did:omnia:bob", 400, 0); // weight = 20

    gov.create_proposal("prop1".into(), "Test proposal".into(), 10, 0)
        .unwrap();

    // Alice votes For (weight 10)
    gov.vote("did:omnia:alice", "prop1", VoteChoice::For, 0).unwrap();

    // Bob votes Against (weight 20)
    gov.vote("did:omnia:bob", "prop1", VoteChoice::Against, 0).unwrap();

    let proposal = gov.get_proposal("prop1").unwrap();
    assert_eq!(proposal.votes_for, 10);
    assert_eq!(proposal.votes_against, 20);
    assert!(!proposal.passes()); // 10 < 20
}

#[test]
fn test_governance_expired_proposal() {
    use omnia_economics::GovernanceState;

    let mut gov = GovernanceState::new(DecayRate::ten_percent());
    gov.set_weight("did:omnia:alice", 100, 0);

    gov.create_proposal("prop1".into(), "Expires soon".into(), 5, 0)
        .unwrap();

    // Voting after expiration fails
    let result = gov.vote("did:omnia:alice", "prop1", VoteChoice::For, 6);
    assert!(matches!(result, Err(EconomicsError::ProposalExpired(_))));
}

#[test]
fn test_governance_inactive_voter() {
    use omnia_economics::GovernanceState;

    let mut gov = GovernanceState::new(DecayRate::from_percent(50)); // 50% decay per epoch
    gov.set_weight("did:omnia:alice", 100, 0); // weight = 10

    // After many inactive epochs, weight decays to 0
    let weight = gov.effective_weight("did:omnia:alice", 50);
    assert_eq!(weight, 0);

    let result = gov.vote("did:omnia:alice", "prop1", VoteChoice::For, 50);
    assert!(matches!(result, Err(EconomicsError::InactiveVoter(_))));
}

#[test]
#[cfg(not(feature = "production"))]
fn test_economics_state_full_lifecycle() {
    let mut state = EconomicsState::new();
    let epoch = state.current_epoch();

    // Step 1: Register two DIDs
    state
        .apply(
            &EconomicsOp::RegisterDid {
                did: "did:omnia:alice".into(),
            },
            epoch,
            None,
        )
        .unwrap();
    state
        .apply(
            &EconomicsOp::RegisterDid {
                did: "did:omnia:bob".into(),
            },
            epoch,
            None,
        )
        .unwrap();

    assert_eq!(state.balance_of("did:omnia:alice"), Some(DEFAULT_UBC_QUOTA));
    assert_eq!(state.balance_of("did:omnia:bob"), Some(DEFAULT_UBC_QUOTA));

    // Step 2: Alice spends UBC
    state
        .apply(
            &EconomicsOp::SpendUbc {
                did: "did:omnia:alice".into(),
                amount: 300,
            },
            epoch,
            None,
        )
        .unwrap();
    assert_eq!(state.balance_of("did:omnia:alice"), Some(DEFAULT_UBC_QUOTA - 300));

    // Step 3: Alice submits useful work for a reward.
    //
    // The verifier_signature is empty because this test exercises the
    // economics state machine's reward crediting logic, not the C-9
    // Ed25519 signature verification. In non-production mode (which
    // this test is gated on via #[cfg(not(feature = "production"))]
    // above), an empty signature is accepted with a warning — see
    // UsefulWorkProof::verify(). In production mode, a real 64-byte
    // Ed25519 signature over `result_hash || compute_units_consumed`
    // would be required.
    let proof = UsefulWorkProof::new(
        UsefulWorkType::AiTraining {
            model_hash: nonzero_hash(),
            training_data_hash: nonzero_hash(),
        },
        nonzero_hash(),
        500, // 500 compute units → 500 UBC reward
        Vec::new(), // empty signature: testing-mode accepted path
    )
    .expect("valid proof should construct");
    state
        .apply(
            &EconomicsOp::SubmitWork {
                did: "did:omnia:alice".into(),
                proof,
            },
            epoch,
            None,
        )
        .unwrap();
    assert_eq!(state.balance_of("did:omnia:alice"), Some(DEFAULT_UBC_QUOTA - 300 + 500));

    // Step 4: Advance epoch — balances reset
    state.apply(&EconomicsOp::AdvanceEpoch, 0, None).unwrap();
    assert_eq!(state.current_epoch(), 1);
    assert_eq!(state.balance_of("did:omnia:alice"), Some(DEFAULT_UBC_QUOTA));
    assert_eq!(state.balance_of("did:omnia:bob"), Some(DEFAULT_UBC_QUOTA));

    // Step 5: Create a governance proposal
    state
        .apply(
            &EconomicsOp::CreateProposal {
                id: "prop1".into(),
                description: "Increase UBC quota".into(),
                expires_at_epoch: 5,
            },
            1,
            None,
        )
        .unwrap();

    // Step 6: Set voting weights and vote
    // With fixed-point, set_weight sets last_active=0, so at epoch 1 there is
    // 1 epoch of inactivity decay (10%). Alice's effective weight = 10 * 900_000 / 1_000_000 = 9,
    // Bob's effective weight = 20 * 900_000 / 1_000_000 = 18.
    state.governance.set_weight("did:omnia:alice", 100, 0); // base weight = 10
    state.governance.set_weight("did:omnia:bob", 400, 0); // base weight = 20

    state
        .apply(
            &EconomicsOp::Vote {
                did: "did:omnia:alice".into(),
                proposal_id: "prop1".into(),
                choice: VoteChoice::For,
            },
            1,
            None,
        )
        .unwrap();

    state
        .apply(
            &EconomicsOp::Vote {
                did: "did:omnia:bob".into(),
                proposal_id: "prop1".into(),
                choice: VoteChoice::Against,
            },
            1,
            None,
        )
        .unwrap();

    let proposal = state.governance.get_proposal("prop1").unwrap();
    assert_eq!(proposal.votes_for, 9); // 10 * 900_000 / 1_000_000 = 9
    assert_eq!(proposal.votes_against, 18); // 20 * 900_000 / 1_000_000 = 18
    assert!(!proposal.passes());
}

#[test]
fn test_economics_serialization_roundtrip() {
    let mut state = EconomicsState::new();
    state
        .apply(
            &EconomicsOp::RegisterDid {
                did: "did:omnia:alice".into(),
            },
            0,
            None,
        )
        .unwrap();
    state
        .apply(
            &EconomicsOp::SpendUbc {
                did: "did:omnia:alice".into(),
                amount: 100,
            },
            0,
            None,
        )
        .unwrap();

    let bytes = state.to_bytes().unwrap();
    let restored = EconomicsState::from_bytes(&bytes).unwrap();

    assert_eq!(restored.balance_of("did:omnia:alice"), Some(DEFAULT_UBC_QUOTA - 100));
}

#[test]
fn test_quota_system_double_register_no_op() {
    let mut system = QuotaSystem::default_system();
    system.register_did("did:omnia:alice");
    system.spend("did:omnia:alice", 500).unwrap();

    // Re-registering should not reset the balance
    system.register_did("did:omnia:alice");
    assert_eq!(system.balance_of("did:omnia:alice"), Some(DEFAULT_UBC_QUOTA - 500));
}

#[test]
fn test_governance_duplicate_proposal() {
    use omnia_economics::GovernanceState;

    let mut gov = GovernanceState::new(DecayRate::ten_percent());
    gov.create_proposal("prop1".into(), "First".into(), 10, 0).unwrap();

    let result = gov.create_proposal("prop1".into(), "Duplicate".into(), 10, 0);
    assert!(matches!(result, Err(EconomicsError::DuplicateProposal(_))));
}

#[test]
fn test_governance_determinism_10k_calls() {
    use omnia_economics::GovernanceState;

    let mut gov = GovernanceState::new(DecayRate::ten_percent());
    gov.set_weight("did:omnia:alice", 100, 0);

    // Call effective_weight 10,000 times — all results must be identical
    let first = gov.effective_weight("did:omnia:alice", 5);
    for _ in 0..10_000 {
        assert_eq!(
            gov.effective_weight("did:omnia:alice", 5),
            first,
            "effective_weight is not deterministic"
        );
    }
}

#[test]
fn test_governance_edge_cases() {
    use omnia_economics::fixed_point::BASIS_PPM;
    use omnia_economics::GovernanceState;

    // Zero base weight
    let gov = GovernanceState::new(DecayRate::ten_percent());
    assert_eq!(gov.effective_weight("unknown", 0), 0);

    // Zero inactive epochs
    let mut gov = GovernanceState::new(DecayRate::ten_percent());
    gov.set_weight("alice", 100, 0);
    assert_eq!(gov.effective_weight("alice", 0), 10);

    // Zero decay rate
    let mut gov = GovernanceState::new(DecayRate::new(0));
    gov.set_weight("alice", 100, 0);
    assert_eq!(gov.effective_weight("alice", 100), 10);

    // Full decay rate (100%)
    let mut gov = GovernanceState::new(DecayRate::new(BASIS_PPM));
    gov.set_weight("alice", 100, 0);
    assert_eq!(gov.effective_weight("alice", 1), 0);
}

#[test]
fn test_governance_quadratic_voting_isqrt() {
    use omnia_economics::GovernanceState;

    let mut gov = GovernanceState::new(DecayRate::ten_percent());

    // 100 stake → isqrt(100) = 10
    gov.set_weight("alice", 100, 0);
    assert_eq!(gov.voting_weights.get("alice"), Some(&10));

    // 0 stake → minimum weight of 1
    gov.set_weight("zero", 0, 0);
    assert_eq!(gov.voting_weights.get("zero"), Some(&1));

    // u64::MAX stake → isqrt(u64::MAX) = 4294967295
    gov.set_weight("whale", u64::MAX, 0);
    assert_eq!(gov.voting_weights.get("whale"), Some(&4294967295));
}
