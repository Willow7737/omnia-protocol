#![allow(clippy::unwrap_used)]
//! Replay protection integration test
//!
//! Verifies that the ShardRouter rejects events with non-monotonic
//! nonces for the same creator pubkey (replay attack prevention).

use omnia_shards::{
    BiologicalShard, ComputationalShard, EconomicsOp, EconomicsShard, FinancialOp, FinancialShard, IdentityShard,
    NonceStore, PhysicalShard, RedbNonceStore, ShardId, ShardOp, ShardPayload, ShardRouter,
};
use omnia_substrate::{crypto::generate_keypair, Event, NodeId, NodeKeypair, VectorClock};
use std::sync::Arc;

fn test_node(id: u8) -> NodeId {
    let mut node = [0u8; 32];
    node[0] = id;
    node
}

/// Create a signed event with the given payload, creator, and keypair.
fn create_test_event_with_keypair(creator: NodeId, payload: Vec<u8>, keypair: &NodeKeypair) -> Event {
    let vc = VectorClock::with_node(creator, 1);
    let mut event = Event::new(creator, 0, vc, None, None, payload).expect("event creation should succeed");
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
    let event1 = create_test_event_with_keypair(test_node(1), payload1.to_bytes().unwrap(), &keypair);
    assert!(router.route_event(&event1).is_ok());

    // Same nonce 1 again — should fail (replay)
    let payload2 = ShardPayload {
        shard_id: ShardId::financial(),
        operation: ShardOp::Financial(FinancialOp::BalanceQuery { account: creator }),
        nonce: 1,
    };
    let event2 = create_test_event_with_keypair(test_node(1), payload2.to_bytes().unwrap(), &keypair);
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
    let event3 = create_test_event_with_keypair(test_node(1), payload3.to_bytes().unwrap(), &keypair);
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
    let event1 = create_test_event_with_keypair(test_node(1), payload1.to_bytes().unwrap(), &keypair1);
    assert!(router.route_event(&event1).is_ok());

    let payload2 = ShardPayload {
        shard_id: ShardId::financial(),
        operation: ShardOp::Financial(FinancialOp::Mint {
            to: keypair2.verifying_key().to_bytes(),
            amount: 200,
        }),
        nonce: 1,
    };
    let event2 = create_test_event_with_keypair(test_node(2), payload2.to_bytes().unwrap(), &keypair2);
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
    let event = create_test_event_with_keypair(test_node(1), payload.to_bytes().unwrap(), &keypair);
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

/// Test that nonce persistence survives a simulated restart with redb backend.
///
/// This test creates a ShardRouter with a RedbNonceStore, routes an event
/// with nonce 1, then drops the router and creates a new one with the same
/// store. The replayed nonce 1 should be rejected.
#[test]
fn test_nonce_persistence_across_router_restart() {
    let tmp_dir = tempfile::tempdir().expect("tempdir should succeed");
    let db_path = tmp_dir.path().join("nonces.redb");
    let store: Arc<dyn NonceStore> = Arc::new(RedbNonceStore::open(&db_path).expect("nonce store open should succeed"));

    let keypair = generate_keypair();
    let creator = keypair.verifying_key().to_bytes();

    // Router 1: route event with nonce 1
    {
        let mut router1 = ShardRouter::with_nonce_store(
            omnia_shards::FeeSchedule::zero(),
            omnia_economics::QuotaSystem::default_system(),
            store.clone(),
        );
        router1.register(Box::new(FinancialShard::new()));

        let payload = ShardPayload {
            shard_id: ShardId::financial(),
            operation: ShardOp::Financial(FinancialOp::BalanceQuery { account: creator }),
            nonce: 1,
        };
        let event = create_test_event_with_keypair(test_node(1), payload.to_bytes().unwrap(), &keypair);
        assert!(router1.route_event(&event).is_ok(), "First event should succeed");
    }

    // Router 2: create with same store — replay nonce 1 should be rejected
    {
        let mut router2 = ShardRouter::with_nonce_store(
            omnia_shards::FeeSchedule::zero(),
            omnia_economics::QuotaSystem::default_system(),
            store,
        );
        router2.register(Box::new(FinancialShard::new()));

        // Replay nonce 1 → should be rejected
        let replay_payload = ShardPayload {
            shard_id: ShardId::financial(),
            operation: ShardOp::Financial(FinancialOp::BalanceQuery { account: creator }),
            nonce: 1,
        };
        let replay_event = create_test_event_with_keypair(test_node(1), replay_payload.to_bytes().unwrap(), &keypair);
        let result = router2.route_event(&replay_event);
        assert!(result.is_err(), "Replayed nonce should be rejected after restart");
        assert!(
            result.unwrap_err().to_string().contains("Replay detected"),
            "Expected replay detection error after restart"
        );

        // New nonce 2 → should succeed
        let new_payload = ShardPayload {
            shard_id: ShardId::financial(),
            operation: ShardOp::Financial(FinancialOp::BalanceQuery { account: creator }),
            nonce: 2,
        };
        let new_event = create_test_event_with_keypair(test_node(1), new_payload.to_bytes().unwrap(), &keypair);
        assert!(
            router2.route_event(&new_event).is_ok(),
            "New nonce should succeed after restart"
        );
    }
}
