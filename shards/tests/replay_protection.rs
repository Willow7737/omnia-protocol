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
    event.sign_with_keypair(keypair).expect("signing");
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
    let account1 = keypair1.verifying_key().to_bytes();
    let account2 = keypair2.verifying_key().to_bytes();

    // Both creators use nonce 1 — should both succeed (different pubkeys)
    // Use BalanceQuery instead of Mint since FinancialShard::new() has no mint authority
    let payload1 = ShardPayload {
        shard_id: ShardId::financial(),
        operation: ShardOp::Financial(FinancialOp::BalanceQuery { account: account1 }),
        nonce: 1,
    };
    let event1 = create_test_event_with_keypair(test_node(1), payload1.to_bytes().unwrap(), &keypair1);
    assert!(router.route_event(&event1).is_ok());

    let payload2 = ShardPayload {
        shard_id: ShardId::financial(),
        operation: ShardOp::Financial(FinancialOp::BalanceQuery { account: account2 }),
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

// ---------------------------------------------------------------------------
// AUDIT-2026-07 C8 (#346): persist-before-acknowledge ordering
// ---------------------------------------------------------------------------

/// A nonce store whose incremental save always fails — simulating a disk
/// fault at the persistence boundary.
struct FailingNonceStore;

impl NonceStore for FailingNonceStore {
    fn load(&self) -> Result<std::collections::HashMap<[u8; 32], u64>, omnia_shards::NonceStoreError> {
        Ok(std::collections::HashMap::new())
    }
    fn save(&self, _: &std::collections::HashMap<[u8; 32], u64>) -> Result<(), omnia_shards::NonceStoreError> {
        Err(omnia_shards::NonceStoreError::Redb("simulated disk fault".into()))
    }
    fn save_incremental(&self, _: &[u8; 32], _: u64) -> Result<(), omnia_shards::NonceStoreError> {
        Err(omnia_shards::NonceStoreError::Redb("simulated disk fault".into()))
    }
}

/// When nonce persistence fails, the node halts (fail-closed, NEW-M7) —
/// and the in-memory map must NOT have acknowledged the nonce first.
/// The pre-fix order inserted into memory before persisting, so the
/// process that panicked had already acknowledged a nonce that disk
/// never recorded.
#[test]
fn test_failed_nonce_persist_never_acknowledges_in_memory() {
    use omnia_shards::FeeSchedule;
    use std::panic::{catch_unwind, AssertUnwindSafe};

    let mut router = ShardRouter::with_nonce_store(
        FeeSchedule::zero(),
        omnia_economics::QuotaSystem::default_system(),
        Arc::new(FailingNonceStore),
    );
    router.register(Box::new(FinancialShard::new()));

    let keypair = generate_keypair();
    let creator = keypair.verifying_key().to_bytes();

    let payload = ShardPayload {
        shard_id: ShardId::financial(),
        operation: ShardOp::Financial(FinancialOp::BalanceQuery { account: creator }),
        nonce: 1,
    };
    let event = create_test_event_with_keypair(test_node(1), payload.to_bytes().unwrap(), &keypair);

    // The route must panic (fail-closed on persistence failure)…
    let outcome = catch_unwind(AssertUnwindSafe(|| router.route_event(&event)));
    assert!(outcome.is_err(), "persistence failure must halt (fail-closed)");

    // …and memory must NOT have acknowledged the nonce before the halt.
    assert_eq!(
        router.last_acknowledged_nonce(&creator),
        None,
        "in-memory nonce must not be acknowledged when persistence failed"
    );
}
