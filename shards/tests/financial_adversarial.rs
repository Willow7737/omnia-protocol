#![allow(clippy::unwrap_used)]
//! FinancialShard adversarial test suite
//!
//! Tests that the Financial shard correctly rejects invalid operations and
//! maintains invariants under adversarial conditions: double-spends,
//! insufficient balances, zero-amount operations, replay scenarios, and
//! total-supply consistency.

use omnia_shards::{FinancialOp, FinancialState, FinancialValidator, ShardError};
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
///
/// Each call advances the sequence number and vector clock so that
/// every event has a unique causal context — important because the
/// financial shard uses vector clocks for conflict tracking.
fn make_signed_event(keypair: &NodeKeypair, sequence: u64, node_id: NodeId) -> Event {
    let vc = VectorClock::with_node(node_id, sequence + 1);
    let mut event = Event::new(node_id, sequence, vc, None, None, vec![]).expect("event creation should succeed");
    event.sign_with_keypair(keypair).expect("signing");
    event
}

/// Helper: create a FinancialState with mint authority set to the given keypair.
fn state_with_mint_authority(authority_keypair: &NodeKeypair) -> FinancialState {
    FinancialState::with_mint_authority(account_id(authority_keypair))
}

/// Helper: extract the 32-byte public key (AccountId) from a keypair.
fn account_id(keypair: &NodeKeypair) -> [u8; 32] {
    keypair.verifying_key().to_bytes()
}

// ---------------------------------------------------------------------------
// 1. Double-spend attack
// ---------------------------------------------------------------------------

#[test]
fn test_double_spend_attack() {
    // Setup: three accounts A, B, C
    let kp_a = generate_keypair();
    let kp_b = generate_keypair();
    let kp_c = generate_keypair();
    let a = account_id(&kp_a);
    let b = account_id(&kp_b);
    let c = account_id(&kp_c);
    let node = test_node(1);

    let mut state = state_with_mint_authority(&kp_a);

    // Mint 100 to A
    let mint_op = FinancialOp::Mint { to: a, amount: 100 };
    let mut event0 = make_signed_event(&kp_a, 0, node);
    event0.sign_with_keypair(&kp_a).expect("signing");
    state.apply(&mint_op, &event0).expect("mint should succeed");
    assert_eq!(state.balance_of(&a), 100);

    // Transfer 100 from A to B — should succeed
    let transfer1 = FinancialOp::Transfer { to: b, amount: 100 };
    let event1 = make_signed_event(&kp_a, 1, node);
    state.apply(&transfer1, &event1).expect("first transfer should succeed");
    assert_eq!(state.balance_of(&a), 0);
    assert_eq!(state.balance_of(&b), 100);

    // Attempt to transfer 100 from A to C — should fail (double-spend)
    let transfer2 = FinancialOp::Transfer { to: c, amount: 100 };
    let event2 = make_signed_event(&kp_a, 2, node);
    let result = state.apply(&transfer2, &event2);
    assert!(result.is_err(), "double-spend should be rejected");
    match result.unwrap_err() {
        ShardError::ValidationFailed(msg) => {
            assert!(
                msg.contains("Insufficient balance"),
                "expected 'Insufficient balance', got '{msg}'"
            );
        }
        other => panic!("expected ValidationFailed, got {other:?}"),
    }

    // Verify balances unchanged after failed transfer
    assert_eq!(state.balance_of(&a), 0);
    assert_eq!(state.balance_of(&b), 100);
    assert_eq!(state.balance_of(&c), 0);
}

// ---------------------------------------------------------------------------
// 2. Negative balance prevention
// ---------------------------------------------------------------------------

#[test]
fn test_negative_balance_prevention() {
    let kp_a = generate_keypair();
    let kp_b = generate_keypair();
    let a = account_id(&kp_a);
    let b = account_id(&kp_b);
    let node = test_node(1);

    let mut state = state_with_mint_authority(&kp_a);

    // Mint only 50 to A
    let mint_op = FinancialOp::Mint { to: a, amount: 50 };
    let event0 = make_signed_event(&kp_a, 0, node);
    state.apply(&mint_op, &event0).expect("mint should succeed");

    // Attempt to transfer 100 from A to B — more than A holds
    let transfer = FinancialOp::Transfer { to: b, amount: 100 };
    let event1 = make_signed_event(&kp_a, 1, node);
    let result = state.apply(&transfer, &event1);
    assert!(result.is_err(), "oversized transfer should fail");

    match result.unwrap_err() {
        ShardError::ValidationFailed(msg) => {
            assert!(
                msg.contains("Insufficient balance"),
                "expected 'Insufficient balance', got '{msg}'"
            );
        }
        other => panic!("expected ValidationFailed, got {other:?}"),
    }

    // A's balance should be untouched
    assert_eq!(state.balance_of(&a), 50);
    assert_eq!(state.balance_of(&b), 0);
}

// ---------------------------------------------------------------------------
// 3. Replay attack / nonce bypass — state-level determinism
// ---------------------------------------------------------------------------

#[test]
fn test_replay_attack_nonce_bypass() {
    // Replay protection (nonce checking) lives in the ShardRouter, not in
    // FinancialState itself. Here we verify that the *state* level is
    // deterministic: applying the same operation to the same starting state
    // always produces the same result. This ensures that if a replayed event
    // somehow reaches apply(), the outcome is predictable and auditable.

    let kp_a = generate_keypair();
    let kp_b = generate_keypair();
    let a = account_id(&kp_a);
    let b = account_id(&kp_b);
    let node = test_node(1);

    // Build two identical starting states
    let mut state1 = state_with_mint_authority(&kp_a);
    let mut state2 = state_with_mint_authority(&kp_a);

    let mint_op = FinancialOp::Mint { to: a, amount: 200 };
    let event0 = make_signed_event(&kp_a, 0, node);
    state1.apply(&mint_op, &event0).expect("mint in state1");
    state2.apply(&mint_op, &event0).expect("mint in state2");

    // Apply the same transfer to both states
    let transfer = FinancialOp::Transfer { to: b, amount: 75 };
    let event1 = make_signed_event(&kp_a, 1, node);
    state1.apply(&transfer, &event1).expect("transfer in state1");
    state2.apply(&transfer, &event1).expect("transfer in state2");

    // Both states must be identical — deterministic
    assert_eq!(state1.balance_of(&a), state2.balance_of(&a));
    assert_eq!(state1.balance_of(&b), state2.balance_of(&b));
    assert_eq!(state1.total_supply, state2.total_supply);

    // Applying the same op again to the same (now-mutated) state is NOT
    // idempotent — it will succeed if there is enough balance, which is
    // expected because state-level replay protection is the router's job.
    // But the result is still deterministic:
    let event2 = make_signed_event(&kp_a, 2, node);
    let r1 = state1.apply(&transfer, &event2);
    let r2 = state2.apply(&transfer, &event2);
    assert_eq!(r1.is_ok(), r2.is_ok(), "replay must be deterministic");
    if let (Ok(_), Ok(_)) = (&r1, &r2) {
        assert_eq!(state1.balance_of(&a), state2.balance_of(&a));
        assert_eq!(state1.balance_of(&b), state2.balance_of(&b));
    }
}

// ---------------------------------------------------------------------------
// 4. Concurrent transfers — balance consistency
// ---------------------------------------------------------------------------

#[test]
fn test_concurrent_transfers_balance_consistency() {
    let kp_a = generate_keypair();
    let kp_b = generate_keypair();
    let kp_c = generate_keypair();
    let a = account_id(&kp_a);
    let b = account_id(&kp_b);
    let c = account_id(&kp_c);
    let node = test_node(1);

    let mut state = state_with_mint_authority(&kp_a);

    // Mint 100 to A
    let mint_op = FinancialOp::Mint { to: a, amount: 100 };
    let event0 = make_signed_event(&kp_a, 0, node);
    state.apply(&mint_op, &event0).expect("mint should succeed");

    // Transfer 60 from A to B
    let transfer1 = FinancialOp::Transfer { to: b, amount: 60 };
    let event1 = make_signed_event(&kp_a, 1, node);
    state.apply(&transfer1, &event1).expect("transfer A->B should succeed");

    // Transfer 40 from A to C
    let transfer2 = FinancialOp::Transfer { to: c, amount: 40 };
    let event2 = make_signed_event(&kp_a, 2, node);
    state.apply(&transfer2, &event2).expect("transfer A->C should succeed");

    // Verify final balances
    assert_eq!(state.balance_of(&a), 0, "A should have 0");
    assert_eq!(state.balance_of(&b), 60, "B should have 60");
    assert_eq!(state.balance_of(&c), 40, "C should have 40");

    // Another transfer from A should fail
    let transfer3 = FinancialOp::Transfer { to: b, amount: 1 };
    let event3 = make_signed_event(&kp_a, 3, node);
    let result = state.apply(&transfer3, &event3);
    assert!(result.is_err(), "transfer from empty account should fail");
}

// ---------------------------------------------------------------------------
// 5. Zero-amount transfer rejected
// ---------------------------------------------------------------------------

#[test]
fn test_zero_amount_transfer_rejected() {
    let kp_b = generate_keypair();
    let b = account_id(&kp_b);

    let state = FinancialState::new();
    let op = FinancialOp::Transfer { to: b, amount: 0 };

    let result = FinancialValidator::validate(&state, &op);
    assert!(result.is_err(), "zero-amount transfer should be rejected");
    match result.unwrap_err() {
        ShardError::InvalidOperation(msg) => {
            assert!(
                msg.contains("greater than zero"),
                "expected 'greater than zero', got '{msg}'"
            );
        }
        other => panic!("expected InvalidOperation, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 6. Zero-amount mint rejected
// ---------------------------------------------------------------------------

#[test]
fn test_zero_amount_mint_rejected() {
    let kp_a = generate_keypair();
    let a = account_id(&kp_a);

    let state = FinancialState::new();
    let op = FinancialOp::Mint { to: a, amount: 0 };

    let result = FinancialValidator::validate(&state, &op);
    assert!(result.is_err(), "zero-amount mint should be rejected");
    match result.unwrap_err() {
        ShardError::InvalidOperation(msg) => {
            assert!(
                msg.contains("greater than zero"),
                "expected 'greater than zero', got '{msg}'"
            );
        }
        other => panic!("expected InvalidOperation, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 7. Zero-amount burn rejected
// ---------------------------------------------------------------------------

#[test]
fn test_zero_amount_burn_rejected() {
    let kp_a = generate_keypair();
    let a = account_id(&kp_a);

    let mut state = state_with_mint_authority(&kp_a);
    // Give A some balance so the only failure reason is the zero amount
    let mint_op = FinancialOp::Mint { to: a, amount: 100 };
    let event0 = make_signed_event(&kp_a, 0, test_node(1));
    state.apply(&mint_op, &event0).expect("mint should succeed");

    let op = FinancialOp::Burn { from: a, amount: 0 };
    let result = FinancialValidator::validate(&state, &op);
    assert!(result.is_err(), "zero-amount burn should be rejected");
    match result.unwrap_err() {
        ShardError::InvalidOperation(msg) => {
            assert!(
                msg.contains("greater than zero"),
                "expected 'greater than zero', got '{msg}'"
            );
        }
        other => panic!("expected InvalidOperation, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 8. Burn insufficient balance
// ---------------------------------------------------------------------------

#[test]
fn test_burn_insufficient_balance() {
    let kp_a = generate_keypair();
    let a = account_id(&kp_a);
    let node = test_node(1);

    let mut state = state_with_mint_authority(&kp_a);

    // Mint only 30 to A
    let mint_op = FinancialOp::Mint { to: a, amount: 30 };
    let event0 = make_signed_event(&kp_a, 0, node);
    state.apply(&mint_op, &event0).expect("mint should succeed");

    // Attempt to burn 50 — more than A holds
    let burn_op = FinancialOp::Burn { from: a, amount: 50 };

    // Validator should catch it
    let val_result = FinancialValidator::validate(&state, &burn_op);
    assert!(val_result.is_err(), "validator should reject oversized burn");
    match val_result.unwrap_err() {
        ShardError::ValidationFailed(msg) => {
            assert!(
                msg.contains("Insufficient balance"),
                "expected 'Insufficient balance', got '{msg}'"
            );
        }
        other => panic!("expected ValidationFailed, got {other:?}"),
    }

    // apply() should also catch it
    let event1 = make_signed_event(&kp_a, 1, node);
    let apply_result = state.apply(&burn_op, &event1);
    assert!(apply_result.is_err(), "apply should reject oversized burn");

    // Balance should be untouched
    assert_eq!(state.balance_of(&a), 30);
}

// ---------------------------------------------------------------------------
// 9. Total supply consistency
// ---------------------------------------------------------------------------

#[test]
fn test_total_supply_consistency() {
    let kp_a = generate_keypair();
    let kp_b = generate_keypair();
    let kp_c = generate_keypair();
    let a = account_id(&kp_a);
    let b = account_id(&kp_b);
    let c = account_id(&kp_c);
    let node = test_node(1);

    let mut state = state_with_mint_authority(&kp_a);
    assert_eq!(state.total_supply, 0);

    // Mint 500 to A
    let event0 = make_signed_event(&kp_a, 0, node);
    state
        .apply(&FinancialOp::Mint { to: a, amount: 500 }, &event0)
        .expect("mint A");
    assert_eq!(state.total_supply, 500, "mint should add to total_supply");

    // Mint 300 to B — mint event must be signed by the mint authority (kp_a)
    let event1 = make_signed_event(&kp_a, 1, node);
    state
        .apply(&FinancialOp::Mint { to: b, amount: 300 }, &event1)
        .expect("mint B");
    assert_eq!(state.total_supply, 800, "second mint should add to total_supply");

    // Transfer 200 from A to C — should NOT change total_supply
    let event2 = make_signed_event(&kp_a, 2, node);
    state
        .apply(&FinancialOp::Transfer { to: c, amount: 200 }, &event2)
        .expect("transfer A->C");
    assert_eq!(state.total_supply, 800, "transfer should not change total_supply");

    // Burn 100 from B — should reduce total_supply
    let event3 = make_signed_event(&kp_b, 3, node);
    state
        .apply(&FinancialOp::Burn { from: b, amount: 100 }, &event3)
        .expect("burn from B");
    assert_eq!(state.total_supply, 700, "burn should subtract from total_supply");

    // Transfer 50 from C to A — still no change to total_supply
    let event4 = make_signed_event(&kp_c, 4, node);
    state
        .apply(&FinancialOp::Transfer { to: a, amount: 50 }, &event4)
        .expect("transfer C->A");
    assert_eq!(state.total_supply, 700, "transfer should not change total_supply");

    // Verify sum of all balances equals total_supply
    let sum_balances: u64 = state.balances.values().map(|ab| ab.value()).sum();
    assert_eq!(
        sum_balances, state.total_supply,
        "sum of balances must equal total_supply"
    );

    // Verify individual balances
    assert_eq!(state.balance_of(&a), 350); // 500 - 200 + 50
    assert_eq!(state.balance_of(&b), 200); // 300 - 100
    assert_eq!(state.balance_of(&c), 150); // 200 - 50
}

// ---------------------------------------------------------------------------
// 10. Transfer to self
// ---------------------------------------------------------------------------

#[test]
fn test_transfer_to_self() {
    let kp_a = generate_keypair();
    let a = account_id(&kp_a);
    let node = test_node(1);

    let mut state = state_with_mint_authority(&kp_a);

    // Mint 100 to A
    let event0 = make_signed_event(&kp_a, 0, node);
    state
        .apply(&FinancialOp::Mint { to: a, amount: 100 }, &event0)
        .expect("mint should succeed");
    assert_eq!(state.balance_of(&a), 100);
    let supply_before = state.total_supply;

    // Transfer from A to A — should succeed with no net balance change
    let transfer = FinancialOp::Transfer { to: a, amount: 50 };
    let event1 = make_signed_event(&kp_a, 1, node);
    state.apply(&transfer, &event1).expect("self-transfer should succeed");

    // Balance should be unchanged (debit 50, credit 50)
    assert_eq!(state.balance_of(&a), 100, "self-transfer should not change balance");

    // Total supply should not change
    assert_eq!(
        state.total_supply, supply_before,
        "self-transfer should not change total_supply"
    );

    // No double-counting: the balance entry for A should exist exactly once
    assert_eq!(state.balances.len(), 1, "A should have exactly one balance entry");
}

// ---------------------------------------------------------------------------
// AUDIT-2026-07 C5 (#343): transfer atomicity on recipient overflow
// ---------------------------------------------------------------------------

/// A transfer into a recipient whose balance would overflow must fail
/// WITHOUT debiting the sender. The pre-fix code decremented the sender,
/// then propagated the recipient's overflow error — permanent fund loss
/// and a broken total-supply invariant.
#[test]
fn test_transfer_recipient_overflow_is_atomic() {
    use omnia_shards::FinancialAccountBalance as AccountBalance;

    let kp_a = generate_keypair();
    let a = account_id(&kp_a);
    let node_a = test_node(1);
    let attacker_sink = [0xEEu8; 32];

    let mut state = state_with_mint_authority(&kp_a);

    // Sender holds 100.
    let event0 = make_signed_event(&kp_a, 0, node_a);
    state
        .apply(&FinancialOp::Mint { to: a, amount: 100 }, &event0)
        .expect("mint");

    // Recipient parked at u64::MAX (staged directly: represents a balance
    // reached through any path — the atomicity of apply() is what's under
    // test, not how the balance got there).
    state
        .balances
        .insert(attacker_sink, AccountBalance::with_balance(u64::MAX));

    let supply_before = state.total_supply;
    let sender_before = state.balance_of(&a);

    let event1 = make_signed_event(&kp_a, 1, node_a);
    let result = state.apply(
        &FinancialOp::Transfer {
            to: attacker_sink,
            amount: 100,
        },
        &event1,
    );

    assert!(result.is_err(), "overflowing transfer must be rejected");
    assert_eq!(
        state.balance_of(&a),
        sender_before,
        "sender must NOT be debited when the transfer fails"
    );
    assert_eq!(
        state.balance_of(&attacker_sink),
        u64::MAX,
        "recipient balance unchanged"
    );
    assert_eq!(state.total_supply, supply_before, "supply invariant preserved");
}

/// Self-transfer must succeed as a net-zero operation, including at
/// balances near u64::MAX where a naive recipient-headroom pre-check
/// would spuriously reject it.
#[test]
fn test_self_transfer_is_net_zero_even_near_max() {
    use omnia_shards::FinancialAccountBalance as AccountBalance;

    let kp_a = generate_keypair();
    let a = account_id(&kp_a);
    let node_a = test_node(1);

    let mut state = state_with_mint_authority(&kp_a);
    state.balances.insert(a, AccountBalance::with_balance(u64::MAX - 5));

    let event0 = make_signed_event(&kp_a, 0, node_a);
    state
        .apply(&FinancialOp::Transfer { to: a, amount: 1_000 }, &event0)
        .expect("self-transfer is net zero and must succeed");
    assert_eq!(state.balance_of(&a), u64::MAX - 5, "balance unchanged by self-transfer");
}
