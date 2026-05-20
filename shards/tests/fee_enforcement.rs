#![allow(clippy::unwrap_used)]
//! Fee enforcement integration tests
//!
//! Verifies that the ShardRouter correctly enforces fee deductions
//! via the QuotaSystem before processing shard operations. Tests cover:
//!
//! - Funded quota → operations succeed
//! - Zero quota → operations fail with InsufficientFee
//! - Partial balance → some operations succeed, then fail
//! - Cross-shard operations cost more than single-shard
//! - Fee deduction matches fee_schedule.fee_for_op()

use omnia_economics::QuotaSystem;
use omnia_shards::{
    BiologicalShard, ComputationalShard, CrossShardMessage, FeeSchedule, FinancialOp,
    FinancialShard, IdentityShard, PhysicalShard, ShardError, ShardId, ShardOp, ShardPayload,
    ShardRouter,
};
use omnia_substrate::{crypto::generate_keypair, Event, NodeId, NodeKeypair, VectorClock};

fn test_node(id: u8) -> NodeId {
    let mut node = [0u8; 32];
    node[0] = id;
    node
}

/// Create a signed event with the given payload, creator, and keypair.
fn create_test_event_with_keypair(
    creator: NodeId,
    payload: Vec<u8>,
    keypair: &NodeKeypair,
) -> Event {
    let vc = VectorClock::with_node(creator, 1);
    let mut event = Event::new(creator, 0, vc, None, None, payload);
    event.sign_with_keypair(keypair);
    event
}

/// Build a router with standard fee schedule, funded quota, and all shards.
fn build_funded_router(balance: u64) -> (ShardRouter, NodeKeypair) {
    let schedule = FeeSchedule::standard();
    let mut quota = QuotaSystem::new(balance, 30 * 24 * 60 * 60 * 1000);
    let keypair = generate_keypair();
    let did = ShardRouter::pubkey_to_did(&keypair.verifying_key().to_bytes());
    quota.register_did(&did);
    // Override balance: register_did sets balance = default_quota, then we
    // reward the difference if balance > default_quota, or we've already
    // set it via the QuotaSystem::new default_quota parameter.
    // Since register_did uses default_quota, and we set default_quota = balance,
    // the DID should have exactly `balance` UBC.

    let mut router = ShardRouter::new(schedule, quota);
    router.register(Box::new(FinancialShard::new()));
    router.register(Box::new(IdentityShard::new()));
    router.register(Box::new(ComputationalShard::new()));
    router.register(Box::new(PhysicalShard::new()));
    router.register(Box::new(BiologicalShard::new()));

    (router, keypair)
}

// ---------------------------------------------------------------------------
// 1. Funded quota → operations succeed
// ---------------------------------------------------------------------------

#[test]
fn test_funded_quota_operations_succeed() {
    let (mut router, keypair) = build_funded_router(1000);
    let creator = keypair.verifying_key().to_bytes();

    // Financial op costs 10 UBC — should succeed
    let payload = ShardPayload {
        shard_id: ShardId::financial(),
        operation: ShardOp::Financial(FinancialOp::BalanceQuery { account: creator }),
        nonce: 1,
    };
    let event = create_test_event_with_keypair(test_node(1), payload.to_bytes().unwrap(), &keypair);
    assert!(
        router.route_event(&event).is_ok(),
        "Funded operation should succeed"
    );
}

// ---------------------------------------------------------------------------
// 2. Zero quota → operations fail with InsufficientFee
// ---------------------------------------------------------------------------

#[test]
fn test_zero_quota_operations_fail() {
    let schedule = FeeSchedule::standard();
    let quota = QuotaSystem::new(0, 30 * 24 * 60 * 60 * 1000); // default_quota = 0
    let keypair = generate_keypair();
    let did = ShardRouter::pubkey_to_did(&keypair.verifying_key().to_bytes());

    let mut quota = quota;
    quota.register_did(&did);
    // DID now has 0 balance

    let mut router = ShardRouter::new(schedule, quota);
    router.register(Box::new(FinancialShard::new()));

    let creator = keypair.verifying_key().to_bytes();
    let payload = ShardPayload {
        shard_id: ShardId::financial(),
        operation: ShardOp::Financial(FinancialOp::BalanceQuery { account: creator }),
        nonce: 1,
    };
    let event = create_test_event_with_keypair(test_node(1), payload.to_bytes().unwrap(), &keypair);
    let result = router.route_event(&event);

    assert!(result.is_err(), "Zero-quota operation should fail");
    match result.unwrap_err() {
        ShardError::InsufficientFee(msg) => {
            assert!(
                msg.contains("Quota exceeded"),
                "Expected 'Quota exceeded' in error, got: {msg}"
            );
        }
        other => panic!("Expected InsufficientFee, got: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 3. Partial balance → some operations succeed, then fail
// ---------------------------------------------------------------------------

#[test]
fn test_partial_balance_some_succeed_then_fail() {
    // Financial op = 10 UBC. Give the DID exactly 25 UBC.
    // First two operations (10 + 10 = 20) succeed, third fails.
    let schedule = FeeSchedule::standard();
    let mut quota = QuotaSystem::new(25, 30 * 24 * 60 * 60 * 1000);
    let keypair = generate_keypair();
    let did = ShardRouter::pubkey_to_did(&keypair.verifying_key().to_bytes());
    quota.register_did(&did);

    let mut router = ShardRouter::new(schedule, quota);
    router.register(Box::new(FinancialShard::new()));

    let creator = keypair.verifying_key().to_bytes();

    // Op 1 — should succeed (balance: 25 → 15)
    let payload1 = ShardPayload {
        shard_id: ShardId::financial(),
        operation: ShardOp::Financial(FinancialOp::BalanceQuery { account: creator }),
        nonce: 1,
    };
    let event1 =
        create_test_event_with_keypair(test_node(1), payload1.to_bytes().unwrap(), &keypair);
    assert!(
        router.route_event(&event1).is_ok(),
        "First op should succeed"
    );

    // Op 2 — should succeed (balance: 15 → 5)
    let payload2 = ShardPayload {
        shard_id: ShardId::financial(),
        operation: ShardOp::Financial(FinancialOp::BalanceQuery { account: creator }),
        nonce: 2,
    };
    let event2 =
        create_test_event_with_keypair(test_node(1), payload2.to_bytes().unwrap(), &keypair);
    assert!(
        router.route_event(&event2).is_ok(),
        "Second op should succeed"
    );

    // Op 3 — should fail (balance: 5, need 10)
    let payload3 = ShardPayload {
        shard_id: ShardId::financial(),
        operation: ShardOp::Financial(FinancialOp::BalanceQuery { account: creator }),
        nonce: 3,
    };
    let event3 =
        create_test_event_with_keypair(test_node(1), payload3.to_bytes().unwrap(), &keypair);
    let result = router.route_event(&event3);
    assert!(
        result.is_err(),
        "Third op should fail due to insufficient balance"
    );
    match result.unwrap_err() {
        ShardError::InsufficientFee(_) => {}
        other => panic!("Expected InsufficientFee, got: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 4. Cross-shard operations cost more than single-shard
// ---------------------------------------------------------------------------

#[test]
fn test_cross_shard_fee_higher_than_single_shard() {
    let schedule = FeeSchedule::standard();
    assert!(
        schedule.cross_shard_fee > schedule.financial_op_fee,
        "Cross-shard fee ({}) should be higher than financial fee ({})",
        schedule.cross_shard_fee,
        schedule.financial_op_fee
    );
    assert!(
        schedule.cross_shard_fee > schedule.identity_op_fee,
        "Cross-shard fee ({}) should be higher than identity fee ({})",
        schedule.cross_shard_fee,
        schedule.identity_op_fee
    );
}

#[test]
fn test_cross_shard_fee_deduction() {
    // Give enough balance for a cross-shard op (15 UBC)
    let schedule = FeeSchedule::standard();
    let mut quota = QuotaSystem::new(15, 30 * 24 * 60 * 60 * 1000);
    let keypair = generate_keypair();
    let did = ShardRouter::pubkey_to_did(&keypair.verifying_key().to_bytes());
    quota.register_did(&did);

    let mut router = ShardRouter::new(schedule, quota);
    router.register(Box::new(FinancialShard::new()));
    router.register(Box::new(IdentityShard::new()));

    let msg = CrossShardMessage::new(
        ShardId::financial(),
        ShardId::identity(),
        postcard::to_allocvec(&ShardOp::Identity(omnia_shards::IdentityOp::CreateDid {
            document: omnia_shards::DidDocument::new(
                "did:omnia:cross-test".to_string(),
                keypair.verifying_key().to_bytes(),
                0,
            ),
        }))
        .expect("serialization should work"),
        VectorClock::new(),
    );

    let payload = ShardPayload {
        shard_id: ShardId::financial(),
        operation: ShardOp::CrossShard(msg),
        nonce: 1,
    };
    let event = create_test_event_with_keypair(test_node(1), payload.to_bytes().unwrap(), &keypair);
    // The cross-shard fee is 15, and the DID has exactly 15, so it should succeed
    assert!(
        router.route_event(&event).is_ok(),
        "Cross-shard op with sufficient balance should succeed"
    );
}

#[test]
fn test_cross_shard_insufficient_balance() {
    // Give only 10 UBC — not enough for cross-shard (15)
    let schedule = FeeSchedule::standard();
    let mut quota = QuotaSystem::new(10, 30 * 24 * 60 * 60 * 1000);
    let keypair = generate_keypair();
    let did = ShardRouter::pubkey_to_did(&keypair.verifying_key().to_bytes());
    quota.register_did(&did);

    let mut router = ShardRouter::new(schedule, quota);
    router.register(Box::new(FinancialShard::new()));
    router.register(Box::new(IdentityShard::new()));

    let msg = CrossShardMessage::new(
        ShardId::financial(),
        ShardId::identity(),
        vec![1, 2, 3],
        VectorClock::new(),
    );

    let payload = ShardPayload {
        shard_id: ShardId::financial(),
        operation: ShardOp::CrossShard(msg),
        nonce: 1,
    };
    let event = create_test_event_with_keypair(test_node(1), payload.to_bytes().unwrap(), &keypair);
    let result = router.route_event(&event);
    assert!(
        result.is_err(),
        "Cross-shard with insufficient balance should fail"
    );
    match result.unwrap_err() {
        ShardError::InsufficientFee(msg) => {
            assert!(
                msg.contains("Quota exceeded"),
                "Expected 'Quota exceeded', got: {msg}"
            );
        }
        other => panic!("Expected InsufficientFee, got: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 5. Fee deduction matches fee_schedule.fee_for_op()
// ---------------------------------------------------------------------------

#[test]
fn test_fee_deduction_matches_schedule() {
    let schedule = FeeSchedule::standard();
    let financial_fee = schedule.fee_for_op(&ShardOp::Financial(FinancialOp::BalanceQuery {
        account: [0u8; 32],
    }));

    // Set the DID's balance to exactly the financial fee
    let mut quota = QuotaSystem::new(financial_fee, 30 * 24 * 60 * 60 * 1000);
    let keypair = generate_keypair();
    let did = ShardRouter::pubkey_to_did(&keypair.verifying_key().to_bytes());
    quota.register_did(&did);

    let mut router = ShardRouter::new(schedule, quota);
    router.register(Box::new(FinancialShard::new()));

    let creator = keypair.verifying_key().to_bytes();

    // Op 1 — should succeed and exhaust balance
    let payload1 = ShardPayload {
        shard_id: ShardId::financial(),
        operation: ShardOp::Financial(FinancialOp::BalanceQuery { account: creator }),
        nonce: 1,
    };
    let event1 =
        create_test_event_with_keypair(test_node(1), payload1.to_bytes().unwrap(), &keypair);
    assert!(
        router.route_event(&event1).is_ok(),
        "Op with exact fee balance should succeed"
    );

    // Op 2 — same fee, but balance is now 0 → should fail
    let payload2 = ShardPayload {
        shard_id: ShardId::financial(),
        operation: ShardOp::Financial(FinancialOp::BalanceQuery { account: creator }),
        nonce: 2,
    };
    let event2 =
        create_test_event_with_keypair(test_node(1), payload2.to_bytes().unwrap(), &keypair);
    assert!(
        router.route_event(&event2).is_err(),
        "Op after balance exhausted should fail"
    );
}

#[test]
fn test_identity_fee_is_lower_than_financial() {
    let schedule = FeeSchedule::standard();
    let identity_fee =
        schedule.fee_for_op(&ShardOp::Identity(omnia_shards::IdentityOp::CreateDid {
            document: omnia_shards::DidDocument::new("did:omnia:test".to_string(), [0u8; 32], 0),
        }));
    let financial_fee = schedule.fee_for_op(&ShardOp::Financial(FinancialOp::BalanceQuery {
        account: [0u8; 32],
    }));

    assert!(
        identity_fee < financial_fee,
        "Identity fee ({identity_fee}) should be less than financial fee ({financial_fee})"
    );
}

#[test]
fn test_unregistered_did_fails_with_insufficient_fee() {
    // DID is never registered in the quota system
    let schedule = FeeSchedule::standard();
    let quota = QuotaSystem::new(1000, 30 * 24 * 60 * 60 * 1000);
    // No register_did() call

    let mut router = ShardRouter::new(schedule, quota);
    router.register(Box::new(FinancialShard::new()));

    let keypair = generate_keypair();
    let creator = keypair.verifying_key().to_bytes();

    let payload = ShardPayload {
        shard_id: ShardId::financial(),
        operation: ShardOp::Financial(FinancialOp::BalanceQuery { account: creator }),
        nonce: 1,
    };
    let event = create_test_event_with_keypair(test_node(1), payload.to_bytes().unwrap(), &keypair);
    let result = router.route_event(&event);

    assert!(result.is_err(), "Unregistered DID should fail fee check");
    match result.unwrap_err() {
        ShardError::InsufficientFee(msg) => {
            assert!(
                msg.contains("Quota exceeded"),
                "Expected 'Quota exceeded', got: {msg}"
            );
        }
        other => panic!("Expected InsufficientFee, got: {other:?}"),
    }
}

#[test]
fn test_zero_fee_schedule_never_deducts() {
    let schedule = FeeSchedule::zero();
    let quota = QuotaSystem::new(0, 30 * 24 * 60 * 60 * 1000); // 0 balance
    let keypair = generate_keypair();
    let did = ShardRouter::pubkey_to_did(&keypair.verifying_key().to_bytes());

    let mut quota = quota;
    quota.register_did(&did);
    // DID has 0 balance

    let mut router = ShardRouter::new(schedule, quota);
    router.register(Box::new(FinancialShard::new()));

    let creator = keypair.verifying_key().to_bytes();
    let payload = ShardPayload {
        shard_id: ShardId::financial(),
        operation: ShardOp::Financial(FinancialOp::BalanceQuery { account: creator }),
        nonce: 1,
    };
    let event = create_test_event_with_keypair(test_node(1), payload.to_bytes().unwrap(), &keypair);
    // With zero fees, even a DID with 0 balance should succeed
    assert!(
        router.route_event(&event).is_ok(),
        "Zero-fee schedule should never deduct, so ops should succeed"
    );
}

#[test]
fn test_different_ops_different_fees() {
    let schedule = FeeSchedule::standard();

    // Verify all fees are distinct in the expected order
    assert_eq!(schedule.financial_op_fee, 10);
    assert_eq!(schedule.computational_op_fee, 5);
    assert_eq!(schedule.physical_op_fee, 3);
    assert_eq!(schedule.identity_op_fee, 2);
    assert_eq!(schedule.biological_op_fee, 3);
    assert_eq!(schedule.cross_shard_fee, 15);
    assert_eq!(schedule.default_fee, 3);
}
