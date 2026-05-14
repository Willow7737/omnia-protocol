//! Replay protection integration test
//!
//! Verifies that the ShardRouter rejects events with non-monotonic
//! nonces for the same creator pubkey (replay attack prevention).

use omnia_shards::{
    BiologicalShard, ComputationalShard, EconomicsOp, EconomicsShard, FinancialOp, FinancialShard,
    IdentityShard, PhysicalShard, ShardId, ShardOp, ShardPayload, ShardRouter,
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

#[test]
fn test_replay_protection() {
    let mut router = ShardRouter::new_without_fees();
    router.register(Box::new(FinancialShard::new()));

    let keypair = generate_keypair();
    let creator = keypair.verifying_key().to_bytes();

    // First event with nonce 1 — should succeed
    let payload1 = ShardPayload {
        shard_id: ShardId::financial(),
        operation: ShardOp::Financial(FinancialOp::BalanceQuery { account: creator }),
        nonce: 1,
    };
    let event1 = create_test_event_with_keypair(test_node(1), payload1.to_bytes(), &keypair);
    assert!(router.route_event(&event1).is_ok());

    // Same nonce 1 again — should fail (replay)
    let payload2 = ShardPayload {
        shard_id: ShardId::financial(),
        operation: ShardOp::Financial(FinancialOp::BalanceQuery { account: creator }),
        nonce: 1,
    };
    let event2 = create_test_event_with_keypair(test_node(1), payload2.to_bytes(), &keypair);
    let result = router.route_event(&event2);
    assert!(result.is_err());
    assert!(
        result.unwrap_err().to_string().contains("Replay detected"),
        "Expected replay detection error"
    );

    // Nonce 2 — should succeed
    let payload3 = ShardPayload {
        shard_id: ShardId::financial(),
        operation: ShardOp::Financial(FinancialOp::BalanceQuery { account: creator }),
        nonce: 2,
    };
    let event3 = create_test_event_with_keypair(test_node(1), payload3.to_bytes(), &keypair);
    assert!(router.route_event(&event3).is_ok());
}

#[test]
fn test_replay_protection_different_creators() {
    let mut router = ShardRouter::new_without_fees();
    router.register(Box::new(FinancialShard::new()));

    let keypair1 = generate_keypair();
    let keypair2 = generate_keypair();

    // Both creators use nonce 1 — should both succeed (different pubkeys)
    let payload1 = ShardPayload {
        shard_id: ShardId::financial(),
        operation: ShardOp::Financial(FinancialOp::Mint {
            to: keypair1.verifying_key().to_bytes(),
            amount: 100,
        }),
        nonce: 1,
    };
    let event1 = create_test_event_with_keypair(test_node(1), payload1.to_bytes(), &keypair1);
    assert!(router.route_event(&event1).is_ok());

    let payload2 = ShardPayload {
        shard_id: ShardId::financial(),
        operation: ShardOp::Financial(FinancialOp::Mint {
            to: keypair2.verifying_key().to_bytes(),
            amount: 200,
        }),
        nonce: 1,
    };
    let event2 = create_test_event_with_keypair(test_node(2), payload2.to_bytes(), &keypair2);
    assert!(router.route_event(&event2).is_ok());
}

#[test]
fn test_economics_shard_wired() {
    let mut router = ShardRouter::new_without_fees();
    router.register(Box::new(EconomicsShard::new()));

    // Verify economics shard is registered
    let econ_shard = router.get_shard(&ShardId::economics());
    assert!(econ_shard.is_some());
    assert_eq!(econ_shard.unwrap().shard_id(), ShardId::economics());
}

#[test]
fn test_economics_shard_route_event() {
    let mut router = ShardRouter::new_without_fees();
    router.register(Box::new(EconomicsShard::new()));

    let keypair = generate_keypair();

    let payload = ShardPayload {
        shard_id: ShardId::economics(),
        operation: ShardOp::Economics(EconomicsOp::RegisterDid {
            did: "did:omnia:test".to_string(),
        }),
        nonce: 1,
    };
    let event = create_test_event_with_keypair(test_node(1), payload.to_bytes(), &keypair);
    assert!(router.route_event(&event).is_ok());
}

#[test]
fn test_all_six_shards_registered() {
    let mut router = ShardRouter::new_without_fees();
    router.register(Box::new(FinancialShard::new()));
    router.register(Box::new(IdentityShard::new()));
    router.register(Box::new(ComputationalShard::new()));
    router.register(Box::new(PhysicalShard::new()));
    router.register(Box::new(BiologicalShard::new()));
    router.register(Box::new(EconomicsShard::new()));

    assert_eq!(router.shard_count(), 6);
    assert!(router.has_shard(&ShardId::financial()));
    assert!(router.has_shard(&ShardId::identity()));
    assert!(router.has_shard(&ShardId::computational()));
    assert!(router.has_shard(&ShardId::physical()));
    assert!(router.has_shard(&ShardId::biological()));
    assert!(router.has_shard(&ShardId::economics()));
}
