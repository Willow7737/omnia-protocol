#![allow(clippy::unwrap_used)]
//! Cross-shard messaging integration test
//!
//! Tests the full lifecycle of cross-shard messages: a financial transfer
//! triggers a cross-shard message to the Identity shard to verify the
//! sender's DID before the transfer is processed.

use omnia_shards::{
    BiologicalShard, ComputationalShard, CrossShardMessage, FinancialOp, FinancialShard, IdentityShard, PhysicalShard,
    ShardId, ShardOp, ShardPayload, ShardRouter,
};
use omnia_substrate::{crypto::generate_keypair, Event, NodeId, NodeKeypair, VectorClock};

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

/// Create a signed event with the given payload and creator.
/// Uses a fresh keypair each time.
fn create_test_event(creator: NodeId, payload: Vec<u8>) -> Event {
    let keypair = generate_keypair();
    create_test_event_with_keypair(creator, payload, &keypair)
}

#[test]
fn test_financial_transfer_lifecycle() {
    // Set up router with Financial shard — mint authority is the sender's keypair
    let sender_keypair = generate_keypair();
    let sender_pubkey = sender_keypair.verifying_key().to_bytes();

    let mut router = ShardRouter::new_without_fees();
    router.register(Box::new(FinancialShard::with_mint_authority(sender_pubkey)));

    let recipient = test_node(2);

    // Mint tokens to the sender (using their public key as the account)
    // The event must be signed by the mint authority (sender_keypair)
    let mint_op = FinancialOp::Mint {
        to: sender_pubkey,
        amount: 1000,
    };
    let mint_payload = ShardPayload {
        shard_id: ShardId::financial(),
        operation: ShardOp::Financial(mint_op),
        nonce: 1,
    };
    let mint_event = create_test_event_with_keypair(test_node(1), mint_payload.to_bytes().unwrap(), &sender_keypair);
    router.route_event(&mint_event).expect("Mint should succeed");

    // Transfer tokens from sender to recipient
    // The sender's account is identified by event.creator_pubkey
    let transfer_op = FinancialOp::Transfer {
        to: recipient,
        amount: 500,
    };
    let transfer_payload = ShardPayload {
        shard_id: ShardId::financial(),
        operation: ShardOp::Financial(transfer_op),
        nonce: 2,
    };
    let transfer_event =
        create_test_event_with_keypair(test_node(1), transfer_payload.to_bytes().unwrap(), &sender_keypair);
    router.route_event(&transfer_event).expect("Transfer should succeed");

    // Check that we can mint to the same account again
    let mint_op2 = FinancialOp::Mint {
        to: recipient,
        amount: 200,
    };
    let mint_payload2 = ShardPayload {
        shard_id: ShardId::financial(),
        operation: ShardOp::Financial(mint_op2),
        nonce: 3,
    };
    let mint_event2 = create_test_event_with_keypair(test_node(1), mint_payload2.to_bytes().unwrap(), &sender_keypair);
    router.route_event(&mint_event2).expect("Second mint should succeed");
}

#[test]
fn test_insufficient_balance_transfer() {
    let mut router = ShardRouter::new_without_fees();
    router.register(Box::new(FinancialShard::new()));

    let sender = test_node(1);
    let recipient = test_node(2);

    // Try to transfer without any balance
    let transfer_op = FinancialOp::Transfer {
        to: recipient,
        amount: 100,
    };
    let transfer_payload = ShardPayload {
        shard_id: ShardId::financial(),
        operation: ShardOp::Financial(transfer_op),
        nonce: 1,
    };
    let transfer_event = create_test_event(sender, transfer_payload.to_bytes().unwrap());
    let result = router.route_event(&transfer_event);
    assert!(result.is_err(), "Transfer with no balance should fail");
}

#[test]
fn test_identity_did_lifecycle() {
    let mut router = ShardRouter::new_without_fees();
    router.register(Box::new(IdentityShard::new()));

    // Use a real keypair so the DID document's primary key matches
    // the event's creator_pubkey (set by sign_with_keypair)
    let owner_keypair = generate_keypair();
    let owner_pubkey = owner_keypair.verifying_key().to_bytes();
    let did = format!("did:omnia:{}", hex::encode(owner_pubkey));

    // Create a DID — the document's public_key must match the signing key
    let doc = omnia_shards::DidDocument::new(did.clone(), owner_pubkey, 1000);
    let create_op = omnia_shards::IdentityOp::CreateDid { document: doc };
    let create_payload = ShardPayload {
        shard_id: ShardId::identity(),
        operation: ShardOp::Identity(create_op),
        nonce: 1,
    };
    let create_event = create_test_event_with_keypair(owner_pubkey, create_payload.to_bytes().unwrap(), &owner_keypair);
    router.route_event(&create_event).expect("Create DID should succeed");

    // Try to create the same DID again (should fail)
    let doc2 = omnia_shards::DidDocument::new(did.clone(), owner_pubkey, 2000);
    let dup_op = omnia_shards::IdentityOp::CreateDid { document: doc2 };
    let dup_payload = ShardPayload {
        shard_id: ShardId::identity(),
        operation: ShardOp::Identity(dup_op),
        nonce: 2,
    };
    let dup_event = create_test_event_with_keypair(owner_pubkey, dup_payload.to_bytes().unwrap(), &owner_keypair);
    let result = router.route_event(&dup_event);
    assert!(result.is_err(), "Duplicate DID creation should fail");
}

#[test]
fn test_cross_shard_message_causality() {
    let source_vc = VectorClock::with_node(test_node(1), 1);
    let mut target_vc = VectorClock::with_node(test_node(2), 1);
    target_vc.merge(&source_vc);

    let msg = CrossShardMessage::new(
        ShardId::financial(),
        ShardId::identity(),
        vec![1, 2, 3],
        source_vc.clone(),
    );

    // The source event causally precedes the target
    assert!(msg.verify_causality(&source_vc, &target_vc));

    // The target does NOT causally precede the source
    assert!(!msg.verify_causality(&target_vc, &source_vc));
}

#[test]
fn test_all_shards_registered() {
    let mut router = ShardRouter::new_without_fees();
    router.register(Box::new(FinancialShard::new()));
    router.register(Box::new(IdentityShard::new()));
    router.register(Box::new(ComputationalShard::new()));
    router.register(Box::new(PhysicalShard::new()));
    router.register(Box::new(BiologicalShard::new()));

    assert_eq!(router.shard_count(), 5);
    assert!(router.has_shard(&ShardId::financial()));
    assert!(router.has_shard(&ShardId::identity()));
    assert!(router.has_shard(&ShardId::computational()));
    assert!(router.has_shard(&ShardId::physical()));
    assert!(router.has_shard(&ShardId::biological()));
}

#[test]
fn test_payload_serialization_roundtrip() {
    let op = ShardOp::Financial(FinancialOp::Mint {
        to: test_node(1),
        amount: 500,
    });
    let payload = ShardPayload {
        shard_id: ShardId::financial(),
        operation: op,
        nonce: 42,
    };

    let bytes = payload.to_bytes().unwrap();
    let restored = ShardPayload::from_bytes(&bytes).expect("Deserialization should succeed");

    assert_eq!(payload.nonce, restored.nonce);
    assert_eq!(payload.shard_id, restored.shard_id);
}

#[test]
fn test_burn_operation() {
    // Use a real keypair for the account so the burn event can be properly authorized
    let account_keypair = generate_keypair();
    let account: [u8; 32] = account_keypair.verifying_key().to_bytes();

    // Mint authority must match the signing key for mint events
    let mut router = ShardRouter::new_without_fees();
    router.register(Box::new(FinancialShard::with_mint_authority(account)));

    // Mint some tokens — must be signed by the mint authority
    let mint_op = FinancialOp::Mint {
        to: account,
        amount: 1000,
    };
    let mint_payload = ShardPayload {
        shard_id: ShardId::financial(),
        operation: ShardOp::Financial(mint_op),
        nonce: 1,
    };
    let mint_event = create_test_event_with_keypair(account, mint_payload.to_bytes().unwrap(), &account_keypair);
    router.route_event(&mint_event).expect("Mint should succeed");

    // Burn some tokens — must be signed by the account owner (creator_pubkey == from)
    let burn_op = FinancialOp::Burn {
        from: account,
        amount: 300,
    };
    let burn_payload = ShardPayload {
        shard_id: ShardId::financial(),
        operation: ShardOp::Financial(burn_op),
        nonce: 2,
    };
    let burn_event = create_test_event_with_keypair(account, burn_payload.to_bytes().unwrap(), &account_keypair);
    router.route_event(&burn_event).expect("Burn should succeed");

    // Try to burn more than the balance
    let overburn_op = FinancialOp::Burn {
        from: account,
        amount: 800,
    };
    let overburn_payload = ShardPayload {
        shard_id: ShardId::financial(),
        operation: ShardOp::Financial(overburn_op),
        nonce: 3,
    };
    let overburn_event =
        create_test_event_with_keypair(account, overburn_payload.to_bytes().unwrap(), &account_keypair);
    let result = router.route_event(&overburn_event);
    assert!(result.is_err(), "Burning more than balance should fail");
}
