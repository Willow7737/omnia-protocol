#![allow(clippy::unwrap_used)]
//! Integration tests: Authorization rejection for Physical and Financial shards
//!
//! Tests that operations requiring authorization (TransferOwnership on Physical,
//! Burn on Financial) are properly rejected when attempted by a non-owner.

use omnia_shards::{FinancialOp, FinancialState, PhysicalOp, PhysicalState, ShardError};
use omnia_substrate::crypto::generate_keypair;
use omnia_substrate::{Event, NodeId, NodeKeypair, VectorClock};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create a test `NodeId` from a single byte (for vector-clock entries).
fn test_node(id: u8) -> NodeId {
    let mut node = [0u8; 32];
    node[0] = id;
    node
}

/// Build a signed `Event` whose `creator_pubkey` matches `keypair`.
fn make_signed_event(keypair: &NodeKeypair, sequence: u64, node_id: NodeId) -> Event {
    let vc = VectorClock::with_node(node_id, sequence + 1);
    let mut event = Event::new(node_id, sequence, vc, None, None, vec![]);
    event.sign_with_keypair(keypair);
    event
}

/// Helper: extract the 32-byte public key from a keypair.
fn account_id(keypair: &NodeKeypair) -> [u8; 32] {
    keypair.verifying_key().to_bytes()
}

// ---------------------------------------------------------------------------
// Physical shard: TransferOwnership unauthorized
// ---------------------------------------------------------------------------

#[test]
fn test_transfer_ownership_unauthorized() {
    let owner_kp = generate_keypair();
    let attacker_kp = generate_keypair();
    let owner_id = account_id(&owner_kp);
    let attacker_id = account_id(&attacker_kp);
    let new_owner = [0xFF; 32];
    let node = test_node(1);

    let mut state = PhysicalState::new();
    let item_id = [0x42; 32];

    // Anchor an item owned by `owner_id`
    let anchor_op = PhysicalOp::AnchorItem {
        item_id,
        owner: owner_id,
        metadata: vec![1, 2, 3],
    };
    state.apply(&anchor_op, &VectorClock::with_node(node, 1), None).unwrap();

    // Verify the current owner
    assert_eq!(state.current_owner(&item_id), Some(owner_id));

    // Attempt TransferOwnership signed by the attacker (not the owner)
    let transfer_op = PhysicalOp::TransferOwnership { item_id, new_owner };
    let attacker_event = make_signed_event(&attacker_kp, 1, node);
    let result = state.apply(&transfer_op, &attacker_event.vector_clock, Some(attacker_id));

    assert!(result.is_err(), "TransferOwnership by non-owner should fail");
    match result.unwrap_err() {
        ShardError::ValidationFailed(msg) => {
            assert!(
                msg.contains("TransferOwnership authorization failed"),
                "expected 'TransferOwnership authorization failed', got '{msg}'"
            );
        }
        other => panic!("expected ValidationFailed, got {other:?}"),
    }

    // Owner should remain unchanged
    assert_eq!(state.current_owner(&item_id), Some(owner_id));
}

// ---------------------------------------------------------------------------
// Financial shard: Burn unauthorized
// ---------------------------------------------------------------------------

#[test]
fn test_burn_unauthorized() {
    let owner_kp = generate_keypair();
    let attacker_kp = generate_keypair();
    let owner_id = account_id(&owner_kp);
    let _attacker_id = account_id(&attacker_kp);
    let node = test_node(1);

    let mut state = FinancialState::new();

    // Mint tokens to the owner
    let mint_op = FinancialOp::Mint {
        to: owner_id,
        amount: 500,
    };
    let mint_event = make_signed_event(&owner_kp, 0, node);
    state.apply(&mint_op, &mint_event).unwrap();
    assert_eq!(state.balance_of(&owner_id), 500);

    // Attempt Burn signed by the attacker (not the account owner)
    let burn_op = FinancialOp::Burn {
        from: owner_id,
        amount: 100,
    };
    let attacker_event = make_signed_event(&attacker_kp, 1, node);
    let result = state.apply(&burn_op, &attacker_event);

    assert!(result.is_err(), "Burn by non-owner should fail");
    match result.unwrap_err() {
        ShardError::ValidationFailed(msg) => {
            assert!(
                msg.contains("Burn authorization failed"),
                "expected 'Burn authorization failed', got '{msg}'"
            );
        }
        other => panic!("expected ValidationFailed, got {other:?}"),
    }

    // Balance should remain unchanged
    assert_eq!(state.balance_of(&owner_id), 500);
}
