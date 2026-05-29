#![allow(clippy::unwrap_used)]
#![allow(deprecated)]
//! Integration test: Layer 2 wired into Layer 1
//!
//! Creates a Substrate with a ShardRouter, submits a financial/identity event,
//! runs consensus, and verifies the shard state updated correctly.
//!
//! Because `Substrate::shard_processor` is `Option<Box<dyn EventProcessor>>`,
//! we cannot downcast back to `ShardRouter` to inspect shard state. Instead, we
//! use the `MutexShardRouter` wrapper backed by `Arc<Mutex<ShardRouter>>` so
//! that the test can both (a) attach the router to the substrate and (b) inspect
//! the shard state after processing.

use std::sync::{Arc, Mutex};

use omnia_shards::{
    FinancialOp, FinancialShard, FinancialState, IdentityOp, IdentityShard, IdentityState, MutexShardRouter, ShardId,
    ShardOp, ShardPayload, ShardRouter,
};
use omnia_substrate::{crypto::generate_keypair, Event, NodeId, Substrate, SubstrateConfig};

fn test_node(id: u8) -> NodeId {
    let mut node = [0u8; 32];
    node[0] = id;
    node
}

// ---------------------------------------------------------------------------
// Financial shard integration
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_financial_shard_wired_into_substrate() {
    // 1. Create substrate with 1-node consensus
    let config = SubstrateConfig::with_network_size(test_node(1), 1);
    let mut substrate = Substrate::new(config);

    // 2. Create shard router with financial shard, shared via Arc<Mutex>
    let router = Arc::new(Mutex::new(ShardRouter::new_without_fees()));
    router.lock().unwrap().register(Box::new(FinancialShard::new()));

    // 3. Attach router to substrate via MutexShardRouter
    let processor = MutexShardRouter::new(router.clone());
    substrate = substrate.with_shard_processor(Box::new(processor));

    // 4. Mint some tokens to an account
    let keypair = generate_keypair();
    let account = keypair.verifying_key().to_bytes();

    let mint_op = FinancialOp::Mint {
        to: account,
        amount: 1000,
    };
    let payload = ShardPayload {
        shard_id: ShardId::financial(),
        operation: ShardOp::Financial(mint_op),
        nonce: 1,
    };

    let mut event = Event::genesis(test_node(1), payload.to_bytes().unwrap()).expect("event creation should succeed");
    event.sign_with_keypair(&keypair);

    // 5. Submit event and run consensus
    substrate.submit_event(event).await.unwrap();
    let _ = substrate.process_consensus().await;

    // 6. Process finalized events through shard processor
    //    submit_event() processes the event through consensus directly,
    //    so process_consensus() may return empty for already-processed events.
    //    Use finalized_events() to get all committed event IDs.
    let finalized_ids = substrate.finalized_events();
    assert!(!finalized_ids.is_empty(), "Event should be finalized");
    let finalized_events: Vec<Event> = {
        let graph = substrate.graph().await;
        finalized_ids.iter().filter_map(|id| graph.get(id).cloned()).collect()
    };
    if let Some(ref mut proc) = substrate.shard_processor {
        for event in &finalized_events {
            proc.process_event(event).unwrap();
        }
    }

    // 7. Verify shard state
    let router = router.lock().unwrap();
    let financial = router.get_shard(&ShardId::financial()).unwrap();
    let snapshot = financial.state_snapshot().unwrap();
    let state = FinancialState::from_bytes(&snapshot).unwrap();
    assert_eq!(state.balance_of(&account), 1000);
    assert_eq!(state.total_supply, 1000);
}

// ---------------------------------------------------------------------------
// Identity shard integration
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_identity_shard_wired_into_substrate() {
    // 1. Create substrate with 1-node consensus
    let config = SubstrateConfig::with_network_size(test_node(1), 1);
    let mut substrate = Substrate::new(config);

    // 2. Create shard router with identity shard, shared via Arc<Mutex>
    let router = Arc::new(Mutex::new(ShardRouter::new_without_fees()));
    router.lock().unwrap().register(Box::new(IdentityShard::new()));

    // 3. Attach router to substrate via MutexShardRouter
    let processor = MutexShardRouter::new(router.clone());
    substrate = substrate.with_shard_processor(Box::new(processor));

    // 4. Create a DID
    let keypair = generate_keypair();
    let pubkey = keypair.verifying_key().to_bytes();
    let did = format!("did:omnia:{}", hex::encode(pubkey));

    let doc = omnia_shards::DidDocument::new(did.clone(), pubkey, 0);
    let create_op = IdentityOp::CreateDid { document: doc };
    let payload = ShardPayload {
        shard_id: ShardId::identity(),
        operation: ShardOp::Identity(create_op),
        nonce: 1,
    };

    let mut event = Event::genesis(test_node(1), payload.to_bytes().unwrap()).expect("event creation should succeed");
    event.sign_with_keypair(&keypair);

    // 5. Submit event and run consensus
    substrate.submit_event(event).await.unwrap();
    let _ = substrate.process_consensus().await;

    // 6. Process finalized events through shard processor
    //    submit_event() processes the event through consensus directly,
    //    so process_consensus() may return empty for already-processed events.
    //    Use finalized_events() to get all committed event IDs.
    let finalized_ids = substrate.finalized_events();
    assert!(!finalized_ids.is_empty(), "Event should be finalized");
    let finalized_events: Vec<Event> = {
        let graph = substrate.graph().await;
        finalized_ids.iter().filter_map(|id| graph.get(id).cloned()).collect()
    };
    if let Some(ref mut proc) = substrate.shard_processor {
        for event in &finalized_events {
            proc.process_event(event).unwrap();
        }
    }

    // 7. Verify shard state
    let router = router.lock().unwrap();
    let identity = router.get_shard(&ShardId::identity()).unwrap();
    let snapshot = identity.state_snapshot().unwrap();
    let state: IdentityState = postcard::from_bytes(&snapshot).unwrap();
    assert!(
        state.dids.contains_key(&did),
        "DID should be registered in identity shard"
    );
}
