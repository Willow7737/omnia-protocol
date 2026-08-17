//! Financial shard validator
//!
//! The validator checks whether a financial operation would succeed without
//! actually mutating state. This is used for pre-flight checks and for
//! rejecting invalid operations before they enter the consensus pipeline.

use super::ops::FinancialOp;
use super::state::FinancialState;
use crate::payload::ShardOp;
use crate::shard::ShardError;

/// Validator for the Financial shard.
///
/// Holds a reference to the current state so it can check balances and
/// other invariants without modifying anything.
pub struct FinancialValidator;

impl FinancialValidator {
    /// Validate a financial operation against the given state.
    ///
    /// Returns `Ok(())` if the operation would succeed, or a `ShardError`
    /// explaining why it would fail.
    pub fn validate(state: &FinancialState, op: &FinancialOp) -> Result<(), ShardError> {
        match op {
            FinancialOp::Transfer { to, amount } => {
                if *amount == 0 {
                    return Err(ShardError::InvalidOperation(
                        "Transfer amount must be greater than zero".into(),
                    ));
                }
                // Note: the actual sender is taken from event.creator_pubkey
                // at apply time, so we can only check the recipient here.
                let _ = to; // Recipient existence is not required (will be created)
                Ok(())
            }
            FinancialOp::Mint { to, amount } => {
                if *amount == 0 {
                    return Err(ShardError::InvalidOperation(
                        "Mint amount must be greater than zero".into(),
                    ));
                }
                let _ = to;
                Ok(())
            }
            FinancialOp::Burn { from, amount } => {
                if *amount == 0 {
                    return Err(ShardError::InvalidOperation(
                        "Burn amount must be greater than zero".into(),
                    ));
                }
                let balance = state.balance_of(from);
                if balance < *amount {
                    return Err(ShardError::ValidationFailed("Insufficient balance for burn".into()));
                }
                Ok(())
            }
            FinancialOp::BalanceQuery { .. } => Ok(()),
            FinancialOp::SignedTransfer {
                from,
                to,
                amount,
                nonce,
                signature,
            } => {
                if *amount == 0 {
                    return Err(ShardError::InvalidOperation(
                        "Transfer amount must be greater than zero".into(),
                    ));
                }
                if from == to {
                    return Err(ShardError::InvalidOperation(
                        "Cannot transfer to the sending account".into(),
                    ));
                }
                if signature.len() != 64 {
                    return Err(ShardError::ValidationFailed(format!(
                        "SignedTransfer signature must be 64 bytes, got {}",
                        signature.len()
                    )));
                }
                if let Some(&last) = state.signed_transfer_nonces.get(from) {
                    if *nonce <= last {
                        return Err(ShardError::ValidationFailed(format!(
                            "SignedTransfer nonce {nonce} is not greater than the last accepted nonce {last} for this sender"
                        )));
                    }
                }
                if state.balance_of(from) < *amount {
                    return Err(ShardError::ValidationFailed("Insufficient balance".into()));
                }
                // The signature itself is deliberately NOT verified here.
                // Validation is a pre-flight check that runs against a
                // possibly stale state; `apply` verifies the signature and
                // is the authority. Duplicating verification here would
                // cost an Ed25519 check per pre-flight without adding any
                // guarantee that `apply` does not already provide.
                Ok(())
            }
        }
    }

    /// Validate a `ShardOp::Financial` variant.
    ///
    /// Convenience wrapper that extracts the inner `FinancialOp` and
    /// delegates to `validate()`.
    pub fn validate_shard_op(state: &FinancialState, op: &ShardOp) -> Result<(), ShardError> {
        match op {
            ShardOp::Financial(fin_op) => Self::validate(state, fin_op),
            _ => Err(ShardError::InvalidOperation("Not a Financial operation".into())),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::financial::ops::AccountId;
    use crate::financial::state::AccountBalance;
    use omnia_substrate::crypto::generate_keypair;
    use omnia_substrate::{Event, VectorClock};

    /// Helper: create a test account ID from a simple byte pattern.
    fn test_account(id: u8) -> AccountId {
        let mut account = [0u8; 32];
        account[0] = id;
        account
    }

    /// Helper: create a signed event from the given keypair with the given payload.
    fn make_signed_event(keypair: &omnia_substrate::crypto::NodeKeypair, payload: Vec<u8>) -> Event {
        let creator = keypair.verifying_key().to_bytes();
        let vc = VectorClock::with_node(creator, 1);
        let mut event = Event::new(creator, 0, vc, None, None, payload).expect("event creation should succeed");
        event.sign_with_keypair(keypair).expect("signing");
        event
    }

    /// Helper: create a FinancialState with a single account having the given balance.
    fn state_with_balance(account: AccountId, balance: u64) -> FinancialState {
        let mut state = FinancialState::with_mint_authority(account);
        state.balances.insert(account, AccountBalance::with_balance(balance));
        state.total_supply = balance;
        state
    }

    // ── Task 4.2: FinancialShard Adversarial Test Suite ───────────────

    /// **Double-spend test**: Same funds spent twice in different events.
    ///
    /// Create a FinancialState with an account that has 100 units.
    /// Create two Transfer ops for 75 units each from the same account.
    /// First should succeed, second should fail with insufficient balance.
    #[test]
    fn test_double_spend_same_sender() {
        let sender_keypair = generate_keypair();
        let sender_pubkey: AccountId = sender_keypair.verifying_key().to_bytes();
        let recipient = test_account(0xAA);

        let mut state = state_with_balance(sender_pubkey, 100);

        // First transfer of 75 — should succeed
        let transfer1 = FinancialOp::Transfer {
            to: recipient,
            amount: 75,
        };
        let event1 = make_signed_event(&sender_keypair, vec![1]);
        assert!(
            state.apply(&transfer1, &event1).is_ok(),
            "First transfer of 75 from 100 should succeed"
        );
        assert_eq!(state.balance_of(&sender_pubkey), 25);

        // Second transfer of 75 — should fail (only 25 remaining)
        let transfer2 = FinancialOp::Transfer {
            to: recipient,
            amount: 75,
        };
        let event2 = make_signed_event(&sender_keypair, vec![2]);
        let result = state.apply(&transfer2, &event2);
        assert!(
            result.is_err(),
            "Second transfer of 75 from 25-balance account should fail"
        );
        // Verify it's a ValidationFailed error
        match result {
            Err(ShardError::ValidationFailed(msg)) => {
                assert!(
                    msg.to_lowercase().contains("insufficient"),
                    "Error should mention insufficient balance, got: {msg}"
                );
            }
            Err(other) => panic!("Expected ValidationFailed, got: {other:?}"),
            Ok(()) => panic!("Second transfer should have failed"),
        }

        // Balance should remain unchanged after failed transfer
        assert_eq!(state.balance_of(&sender_pubkey), 25);
    }

    /// **Negative balance test**: Attempt to spend more than balance.
    ///
    /// Create a FinancialState with an account that has 50 units.
    /// Create a Transfer op for 100 units — should fail with ValidationFailed.
    #[test]
    fn test_transfer_exceeds_balance() {
        let sender_keypair = generate_keypair();
        let sender_pubkey: AccountId = sender_keypair.verifying_key().to_bytes();
        let recipient = test_account(0xBB);

        let mut state = state_with_balance(sender_pubkey, 50);

        let transfer = FinancialOp::Transfer {
            to: recipient,
            amount: 100,
        };
        let event = make_signed_event(&sender_keypair, vec![1]);

        let result = state.apply(&transfer, &event);
        assert!(result.is_err(), "Transfer of 100 from 50-balance account should fail");
        match result {
            Err(ShardError::ValidationFailed(msg)) => {
                assert!(
                    msg.to_lowercase().contains("insufficient"),
                    "Error should mention insufficient balance, got: {msg}"
                );
            }
            Err(other) => panic!("Expected ValidationFailed, got: {other:?}"),
            Ok(()) => panic!("Transfer should have failed"),
        }

        // Balance should remain unchanged
        assert_eq!(state.balance_of(&sender_pubkey), 50);
    }

    /// **Replay attack test**: Same Mint event applied twice.
    ///
    /// The second application should succeed (idempotent state — balance
    /// doubles) OR fail gracefully. Since Mint doesn't have replay
    /// protection in the state layer, the same Mint applied twice will
    /// simply credit the account again. This test verifies that the
    /// system handles this gracefully without panicking or corrupting state.
    #[test]
    fn test_replay_mint_same_event() {
        let target = test_account(0xCC);
        let keypair = generate_keypair();
        let mut state = FinancialState::with_mint_authority(keypair.verifying_key().to_bytes());

        let mint_op = FinancialOp::Mint { to: target, amount: 50 };

        // Create two identical events (same keypair, same sequence — simulating replay)
        let event1 = make_signed_event(&keypair, vec![1]);

        // First Mint — should succeed
        assert!(state.apply(&mint_op, &event1).is_ok(), "First Mint should succeed");
        assert_eq!(state.balance_of(&target), 50);
        assert_eq!(state.total_supply, 50);

        // Second Mint with the same op and event — should succeed without error
        // (idempotent behavior at the state layer; replay protection is the
        // router's responsibility via nonce tracking)
        let result = state.apply(&mint_op, &event1);
        assert!(
            result.is_ok(),
            "Replayed Mint should be handled gracefully (idempotent or rejected), got: {result:?}"
        );

        // Balance should be 100 (credited twice) — the state layer doesn't
        // deduplicate; the ShardRouter nonce check prevents double-application
        // at the protocol level.
        assert_eq!(state.balance_of(&target), 100);
        assert_eq!(state.total_supply, 100);
    }

    /// **Zero-amount transfer test**: Transfer of 0 units should be rejected.
    #[test]
    fn test_zero_amount_transfer_rejected() {
        let state = FinancialState::new();
        let recipient = test_account(0xDD);

        let transfer = FinancialOp::Transfer {
            to: recipient,
            amount: 0,
        };

        // Validator should reject it
        let result = FinancialValidator::validate(&state, &transfer);
        assert!(result.is_err(), "Zero-amount transfer should be rejected by validator");
        match result {
            Err(ShardError::InvalidOperation(msg)) => {
                assert!(
                    msg.to_lowercase().contains("zero") || msg.to_lowercase().contains("greater than zero"),
                    "Error should mention zero amount, got: {msg}"
                );
            }
            Err(other) => panic!("Expected InvalidOperation, got: {other:?}"),
            Ok(()) => panic!("Zero-amount transfer should have been rejected"),
        }
    }

    /// **Zero-amount transfer also fails at apply time** — defense in depth.
    #[test]
    fn test_zero_amount_transfer_apply_rejected() {
        let sender_keypair = generate_keypair();
        let sender_pubkey: AccountId = sender_keypair.verifying_key().to_bytes();
        let recipient = test_account(0xDD);

        let mut state = state_with_balance(sender_pubkey, 100);
        let transfer = FinancialOp::Transfer {
            to: recipient,
            amount: 0,
        };
        let event = make_signed_event(&sender_keypair, vec![1]);

        // Defense in depth: apply() also rejects zero-amount transfers,
        // even if the validator were bypassed.
        let result = state.apply(&transfer, &event);
        assert!(result.is_err(), "Zero-amount transfer should be rejected by apply");
        match result {
            Err(ShardError::InvalidOperation(msg)) => {
                assert!(
                    msg.to_lowercase().contains("greater than zero"),
                    "Error should mention zero amount, got: {msg}"
                );
            }
            Err(other) => panic!("Expected InvalidOperation, got: {other:?}"),
            Ok(()) => panic!("Zero-amount transfer should have been rejected by apply"),
        }
        assert_eq!(state.balance_of(&sender_pubkey), 100);
    }

    /// **Burn more than balance test**: Burn exceeding account balance
    /// should fail with ValidationFailed.
    #[test]
    fn test_burn_exceeds_balance() {
        let account = test_account(0xEE);
        let mut state = state_with_balance(account, 30);

        let burn = FinancialOp::Burn {
            from: account,
            amount: 50,
        };

        // Validator should catch it
        let val_result = FinancialValidator::validate(&state, &burn);
        assert!(
            val_result.is_err(),
            "Burn exceeding balance should be rejected by validator"
        );
        match &val_result {
            Err(ShardError::ValidationFailed(msg)) => {
                assert!(
                    msg.to_lowercase().contains("insufficient"),
                    "Error should mention insufficient balance, got: {msg}"
                );
            }
            Err(other) => panic!("Expected ValidationFailed, got: {other:?}"),
            Ok(()) => panic!("Burn should have been rejected"),
        }

        // Apply should also fail
        let keypair = generate_keypair();
        let event = make_signed_event(&keypair, vec![1]);
        let apply_result = state.apply(&burn, &event);
        assert!(
            apply_result.is_err(),
            "Burn exceeding balance should fail at apply time too"
        );

        // Balance should remain unchanged
        assert_eq!(state.balance_of(&account), 30);
    }

    /// **Transfer to self test**: Transfer from an account to itself.
    ///
    /// The debit and credit happen to the same account. After decrement
    /// and increment, the balance should be unchanged. The operation
    /// should succeed (it is valid, just a no-op economically).
    #[test]
    fn test_transfer_to_self() {
        let sender_keypair = generate_keypair();
        let sender_pubkey: AccountId = sender_keypair.verifying_key().to_bytes();

        let mut state = state_with_balance(sender_pubkey, 100);

        // Transfer from sender to sender (self-transfer)
        let transfer = FinancialOp::Transfer {
            to: sender_pubkey,
            amount: 50,
        };
        let event = make_signed_event(&sender_keypair, vec![1]);

        let result = state.apply(&transfer, &event);
        assert!(
            result.is_ok(),
            "Self-transfer should succeed (debit + credit = net zero)"
        );

        // Balance should be unchanged: 100 - 50 + 50 = 100
        assert_eq!(
            state.balance_of(&sender_pubkey),
            100,
            "Self-transfer should leave balance unchanged"
        );
    }

    /// **Mint to new account test**: Mint to an account that doesn't
    /// exist yet should create the account and credit the balance.
    #[test]
    fn test_mint_to_new_account() {
        let new_account = test_account(0xFF);
        let keypair = generate_keypair();
        let mut state = FinancialState::with_mint_authority(keypair.verifying_key().to_bytes());

        // Verify the account doesn't exist yet
        assert_eq!(state.balance_of(&new_account), 0);
        assert!(!state.balances.contains_key(&new_account));

        let mint = FinancialOp::Mint {
            to: new_account,
            amount: 200,
        };
        let event = make_signed_event(&keypair, vec![1]);

        let result = state.apply(&mint, &event);
        assert!(result.is_ok(), "Mint to new account should succeed");

        // Account should now exist with the minted balance
        assert_eq!(state.balance_of(&new_account), 200);
        assert!(state.balances.contains_key(&new_account));

        // Total supply should be updated
        assert_eq!(state.total_supply, 200);
    }

    /// **Zero-amount mint rejected by validator** — additional edge case.
    #[test]
    fn test_zero_amount_mint_rejected() {
        let state = FinancialState::new();
        let target = test_account(0x11);

        let mint = FinancialOp::Mint { to: target, amount: 0 };

        let result = FinancialValidator::validate(&state, &mint);
        assert!(result.is_err(), "Zero-amount mint should be rejected");
        match result {
            Err(ShardError::InvalidOperation(msg)) => {
                assert!(
                    msg.to_lowercase().contains("greater than zero"),
                    "Error should mention zero amount, got: {msg}"
                );
            }
            Err(other) => panic!("Expected InvalidOperation, got: {other:?}"),
            Ok(()) => panic!("Zero-amount mint should have been rejected"),
        }
    }

    /// **Zero-amount burn rejected by validator** — additional edge case.
    #[test]
    fn test_zero_amount_burn_rejected() {
        let account = test_account(0x22);
        let state = state_with_balance(account, 100);

        let burn = FinancialOp::Burn {
            from: account,
            amount: 0,
        };

        let result = FinancialValidator::validate(&state, &burn);
        assert!(result.is_err(), "Zero-amount burn should be rejected");
        match result {
            Err(ShardError::InvalidOperation(msg)) => {
                assert!(
                    msg.to_lowercase().contains("greater than zero"),
                    "Error should mention zero amount, got: {msg}"
                );
            }
            Err(other) => panic!("Expected InvalidOperation, got: {other:?}"),
            Ok(()) => panic!("Zero-amount burn should have been rejected"),
        }
    }

    /// **Transfer from non-existent account**: Should fail gracefully.
    #[test]
    fn test_transfer_from_nonexistent_account() {
        let sender_keypair = generate_keypair();
        let _sender_pubkey: AccountId = sender_keypair.verifying_key().to_bytes();
        let recipient = test_account(0x33);

        // State with no accounts
        let mut state = FinancialState::new();

        let transfer = FinancialOp::Transfer {
            to: recipient,
            amount: 10,
        };
        let event = make_signed_event(&sender_keypair, vec![1]);

        let result = state.apply(&transfer, &event);
        assert!(result.is_err(), "Transfer from non-existent account should fail");
        match result {
            Err(ShardError::ValidationFailed(msg)) => {
                assert!(
                    msg.to_lowercase().contains("not found"),
                    "Error should mention account not found, got: {msg}"
                );
            }
            Err(other) => panic!("Expected ValidationFailed, got: {other:?}"),
            Ok(()) => panic!("Transfer from non-existent account should fail"),
        }
    }

    /// **Burn from non-existent account**: Should fail gracefully.
    #[test]
    fn test_burn_from_nonexistent_account() {
        let account = test_account(0x44);
        let state = FinancialState::new();

        let burn = FinancialOp::Burn {
            from: account,
            amount: 10,
        };

        // Validator checks balance_of (returns 0 for non-existent), so should fail
        let result = FinancialValidator::validate(&state, &burn);
        assert!(
            result.is_err(),
            "Burn from non-existent account should be rejected by validator"
        );
    }

    /// **Validate_shard_op rejects non-Financial ops**.
    #[test]
    fn test_validate_shard_op_rejects_non_financial() {
        let state = FinancialState::new();
        let non_financial = ShardOp::Economics(crate::economics_shard::EconomicsOp::MintUbc {
            did: "test".into(),
            amount: 100,
        });

        let result = FinancialValidator::validate_shard_op(&state, &non_financial);
        assert!(result.is_err(), "Non-Financial ShardOp should be rejected");
        match result {
            Err(ShardError::InvalidOperation(msg)) => {
                assert!(msg.contains("Financial"), "Error should mention Financial, got: {msg}");
            }
            Err(other) => panic!("Expected InvalidOperation, got: {other:?}"),
            Ok(()) => panic!("Non-Financial ShardOp should have been rejected"),
        }
    }

    /// **BalanceQuery always validates** — it's read-only.
    #[test]
    fn test_balance_query_always_validates() {
        let state = FinancialState::new();
        let account = test_account(0x55);

        let query = FinancialOp::BalanceQuery { account };
        assert!(
            FinancialValidator::validate(&state, &query).is_ok(),
            "BalanceQuery should always validate successfully"
        );
    }

    /// **Double-spend via Burn then Transfer**: After burning most of the
    /// balance, a subsequent transfer should fail if it exceeds remaining funds.
    #[test]
    fn test_double_spend_burn_then_transfer() {
        let sender_keypair = generate_keypair();
        let sender_pubkey: AccountId = sender_keypair.verifying_key().to_bytes();
        let recipient = test_account(0x66);

        let mut state = state_with_balance(sender_pubkey, 100);

        // Burn 80 units
        let burn = FinancialOp::Burn {
            from: sender_pubkey,
            amount: 80,
        };
        // Burn requires the event to be signed by the account owner (creator_pubkey == from)
        let burn_event = make_signed_event(&sender_keypair, vec![1]);
        assert!(state.apply(&burn, &burn_event).is_ok(), "Burn should succeed");
        assert_eq!(state.balance_of(&sender_pubkey), 20);

        // Now try to transfer 50 — should fail (only 20 left)
        let transfer = FinancialOp::Transfer {
            to: recipient,
            amount: 50,
        };
        let transfer_event = make_signed_event(&sender_keypair, vec![1]);
        let result = state.apply(&transfer, &transfer_event);
        assert!(
            result.is_err(),
            "Transfer after burn should fail due to insufficient balance"
        );

        // Balance should still be 20
        assert_eq!(state.balance_of(&sender_pubkey), 20);
    }

    /// **State serialization roundtrip** after adversarial operations.
    #[test]
    fn test_state_serialization_after_operations() {
        let account_a = test_account(0x77);
        let account_b = test_account(0x88);
        let keypair = generate_keypair();
        let mut state = FinancialState::with_mint_authority(keypair.verifying_key().to_bytes());

        // Mint to account A
        let mint = FinancialOp::Mint {
            to: account_a,
            amount: 500,
        };
        let event = make_signed_event(&keypair, vec![1]);
        state.apply(&mint, &event).expect("test assertion failed");

        // Serialize and deserialize
        let bytes = state.to_bytes().expect("test assertion failed");
        let restored = FinancialState::from_bytes(&bytes).expect("test assertion failed");

        assert_eq!(restored.balance_of(&account_a), 500);
        assert_eq!(restored.balance_of(&account_b), 0);
        assert_eq!(restored.total_supply, 500);
    }

    /// **Unauthorized mint test**: When mint_authority is set, only the
    /// authority's public key can mint new tokens.
    #[test]
    fn test_unauthorized_mint_rejected() {
        let authority_keypair = generate_keypair();
        let authority_pubkey: AccountId = authority_keypair.verifying_key().to_bytes();

        let mut state = FinancialState::with_mint_authority(authority_pubkey);

        let target = test_account(0xAA);
        let mint = FinancialOp::Mint {
            to: target,
            amount: 100,
        };

        // Mint from non-authority keypair should fail
        let unauthorized_keypair = generate_keypair();
        let event = make_signed_event(&unauthorized_keypair, vec![1]);
        let result = state.apply(&mint, &event);
        assert!(result.is_err(), "Mint from non-authority should be rejected");
        match result {
            Err(ShardError::ValidationFailed(msg)) => {
                assert!(
                    msg.to_lowercase().contains("mint authority"),
                    "Error should mention mint authority, got: {msg}"
                );
            }
            Err(other) => panic!("Expected ValidationFailed, got: {other:?}"),
            Ok(()) => panic!("Unauthorized mint should have been rejected"),
        }

        // Balance should remain 0
        assert_eq!(state.balance_of(&target), 0);
        assert_eq!(state.total_supply, 0);

        // Mint from authority keypair should succeed
        let auth_event = make_signed_event(&authority_keypair, vec![1]);
        let result = state.apply(&mint, &auth_event);
        assert!(result.is_ok(), "Mint from authority should succeed");
        assert_eq!(state.balance_of(&target), 100);
        assert_eq!(state.total_supply, 100);
    }

    // ── SignedTransfer: delegated, self-sovereign transfers ───────────
    //
    // These cover the property the whole variant exists for: the relaying
    // node's key is irrelevant, and only a signature from `from` can move
    // `from`'s balance.

    use crate::financial::ops::signed_transfer_message;
    use ed25519_dalek::{Signer, SigningKey};

    /// Helper: build a `SignedTransfer` authorized by `signer`.
    fn signed_transfer(signer: &SigningKey, to: AccountId, amount: u64, nonce: u64) -> FinancialOp {
        let from: AccountId = signer.verifying_key().to_bytes();
        let msg = signed_transfer_message(&from, &to, amount, nonce);
        FinancialOp::SignedTransfer {
            from,
            to,
            amount,
            nonce,
            signature: signer.sign(&msg).to_bytes().to_vec(),
        }
    }

    /// A transfer signed by the sender succeeds even though a *different*
    /// key signed the carrying event. This is the delegation property:
    /// the wallet authorizes, the node merely relays.
    #[test]
    fn signed_transfer_succeeds_when_relayed_by_another_key() {
        let sender = SigningKey::from_bytes(&[7u8; 32]);
        let sender_pubkey: AccountId = sender.verifying_key().to_bytes();
        let recipient = test_account(0xAA);
        let mut state = state_with_balance(sender_pubkey, 100);

        // The event is created and signed by the NODE, not the sender.
        let relaying_node = generate_keypair();
        let event = make_signed_event(&relaying_node, vec![1]);
        assert_ne!(
            event.creator_pubkey, sender_pubkey,
            "test is meaningless unless the relayer differs from the sender"
        );

        let op = signed_transfer(&sender, recipient, 40, 1);
        state
            .apply(&op, &event)
            .expect("relayed signed transfer should succeed");

        assert_eq!(state.balance_of(&sender_pubkey), 60);
        assert_eq!(state.balance_of(&recipient), 40, "recipient must actually be credited");
    }

    /// A malicious node cannot move someone else's funds by signing the
    /// event itself — the embedded signature is what is checked.
    #[test]
    fn signed_transfer_rejects_forged_authorization() {
        let victim = SigningKey::from_bytes(&[9u8; 32]);
        let victim_pubkey: AccountId = victim.verifying_key().to_bytes();
        let attacker = SigningKey::from_bytes(&[11u8; 32]);
        let attacker_account = test_account(0xEE);
        let mut state = state_with_balance(victim_pubkey, 100);

        // Attacker signs a transfer draining the victim, but claims
        // `from` is the victim's account.
        let msg = signed_transfer_message(&victim_pubkey, &attacker_account, 100, 1);
        let op = FinancialOp::SignedTransfer {
            from: victim_pubkey,
            to: attacker_account,
            amount: 100,
            nonce: 1,
            signature: attacker.sign(&msg).to_bytes().to_vec(),
        };

        // Even signing the carrying event as the victim's key would not
        // help; here the attacker relays it themselves.
        let event = make_signed_event(&generate_keypair(), vec![1]);
        let result = state.apply(&op, &event);

        assert!(result.is_err(), "forged authorization must be rejected");
        assert_eq!(state.balance_of(&victim_pubkey), 100, "victim must not be debited");
        assert_eq!(state.balance_of(&attacker_account), 0);
    }

    /// Replaying an observed authorization must fail: the nonce is
    /// consumed by the first application.
    #[test]
    fn signed_transfer_rejects_replay() {
        let sender = SigningKey::from_bytes(&[13u8; 32]);
        let sender_pubkey: AccountId = sender.verifying_key().to_bytes();
        let recipient = test_account(0xBB);
        let mut state = state_with_balance(sender_pubkey, 100);

        let op = signed_transfer(&sender, recipient, 30, 1);
        let event = make_signed_event(&generate_keypair(), vec![1]);

        state.apply(&op, &event).expect("first application should succeed");
        assert_eq!(state.balance_of(&sender_pubkey), 70);

        // Byte-identical resubmission — the classic replay.
        let replay = state.apply(&op, &event);
        assert!(replay.is_err(), "replayed authorization must be rejected");
        assert_eq!(
            state.balance_of(&sender_pubkey),
            70,
            "replay must not debit the sender a second time"
        );
        assert_eq!(state.balance_of(&recipient), 30);
    }

    /// Tampering with any signed field invalidates the signature.
    #[test]
    fn signed_transfer_rejects_tampered_fields() {
        let sender = SigningKey::from_bytes(&[17u8; 32]);
        let sender_pubkey: AccountId = sender.verifying_key().to_bytes();
        let recipient = test_account(0xCC);
        let attacker_account = test_account(0xDD);
        let event = make_signed_event(&generate_keypair(), vec![1]);

        // Amount tampered upward after signing.
        let mut state = state_with_balance(sender_pubkey, 100);
        let op = signed_transfer(&sender, recipient, 10, 1);
        let FinancialOp::SignedTransfer {
            from,
            to,
            nonce,
            signature,
            ..
        } = op.clone()
        else {
            panic!("constructed op must be a SignedTransfer");
        };
        let inflated = FinancialOp::SignedTransfer {
            from,
            to,
            amount: 99,
            nonce,
            signature: signature.clone(),
        };
        assert!(
            state.apply(&inflated, &event).is_err(),
            "amount tampering must be rejected"
        );
        assert_eq!(state.balance_of(&sender_pubkey), 100);

        // Recipient redirected after signing.
        let redirected = FinancialOp::SignedTransfer {
            from,
            to: attacker_account,
            amount: 10,
            nonce,
            signature,
        };
        assert!(
            state.apply(&redirected, &event).is_err(),
            "recipient redirection must be rejected"
        );
        assert_eq!(state.balance_of(&attacker_account), 0);
    }

    /// A rejected transfer must not advance the sender's nonce, or a
    /// third party could burn a sender's nonce space with junk.
    #[test]
    fn signed_transfer_failure_does_not_consume_nonce() {
        let sender = SigningKey::from_bytes(&[19u8; 32]);
        let sender_pubkey: AccountId = sender.verifying_key().to_bytes();
        let recipient = test_account(0xAB);
        let mut state = state_with_balance(sender_pubkey, 50);
        let event = make_signed_event(&generate_keypair(), vec![1]);

        // Overspend at nonce 1 — correctly signed, but unaffordable.
        let overspend = signed_transfer(&sender, recipient, 500, 1);
        assert!(state.apply(&overspend, &event).is_err());

        // Nonce 1 must still be usable for a legitimate transfer.
        let ok = signed_transfer(&sender, recipient, 20, 1);
        state
            .apply(&ok, &event)
            .expect("nonce 1 must survive a failed transfer");
        assert_eq!(state.balance_of(&sender_pubkey), 30);
    }

    /// Self-transfers are rejected rather than silently no-oping.
    #[test]
    fn signed_transfer_rejects_self_transfer() {
        let sender = SigningKey::from_bytes(&[23u8; 32]);
        let sender_pubkey: AccountId = sender.verifying_key().to_bytes();
        let mut state = state_with_balance(sender_pubkey, 100);
        let event = make_signed_event(&generate_keypair(), vec![1]);

        let op = signed_transfer(&sender, sender_pubkey, 10, 1);
        assert!(state.apply(&op, &event).is_err(), "self-transfer must be rejected");
        assert_eq!(state.balance_of(&sender_pubkey), 100);
    }

    /// A signature over a different domain must not verify as a transfer.
    /// Guards the domain-separation tag.
    #[test]
    fn signed_transfer_rejects_wrong_domain_signature() {
        let sender = SigningKey::from_bytes(&[29u8; 32]);
        let sender_pubkey: AccountId = sender.verifying_key().to_bytes();
        let recipient = test_account(0xAC);
        let mut state = state_with_balance(sender_pubkey, 100);
        let event = make_signed_event(&generate_keypair(), vec![1]);

        // Same fields, different domain tag.
        let mut foreign = b"omnia-auth:".to_vec();
        foreign.extend_from_slice(&sender_pubkey);
        foreign.extend_from_slice(&recipient);
        foreign.extend_from_slice(&25u64.to_le_bytes());
        foreign.extend_from_slice(&1u64.to_le_bytes());

        let op = FinancialOp::SignedTransfer {
            from: sender_pubkey,
            to: recipient,
            amount: 25,
            nonce: 1,
            signature: sender.sign(&foreign).to_bytes().to_vec(),
        };
        assert!(
            state.apply(&op, &event).is_err(),
            "a signature from another domain must not authorize a transfer"
        );
        assert_eq!(state.balance_of(&sender_pubkey), 100);
    }

    /// Total supply is conserved by a transfer — it moves value, it does
    /// not create or destroy it.
    #[test]
    fn signed_transfer_conserves_total_supply() {
        let sender = SigningKey::from_bytes(&[31u8; 32]);
        let sender_pubkey: AccountId = sender.verifying_key().to_bytes();
        let recipient = test_account(0xAD);
        let mut state = state_with_balance(sender_pubkey, 100);
        let supply_before = state.total_supply;
        let event = make_signed_event(&generate_keypair(), vec![1]);

        state
            .apply(&signed_transfer(&sender, recipient, 60, 1), &event)
            .expect("transfer should succeed");

        assert_eq!(state.total_supply, supply_before, "transfer must not change supply");
        assert_eq!(
            state.balance_of(&sender_pubkey) + state.balance_of(&recipient),
            supply_before,
            "value must be conserved across the two accounts"
        );
    }

    /// The validator agrees with `apply` on the cheap rejections, so
    /// pre-flight checks do not admit operations that would then fail.
    #[test]
    fn validator_rejects_what_apply_rejects() {
        let sender = SigningKey::from_bytes(&[37u8; 32]);
        let sender_pubkey: AccountId = sender.verifying_key().to_bytes();
        let recipient = test_account(0xAE);
        let state = state_with_balance(sender_pubkey, 50);

        // Overspend.
        let overspend = signed_transfer(&sender, recipient, 500, 1);
        assert!(FinancialValidator::validate(&state, &overspend).is_err());

        // Zero amount.
        let zero = signed_transfer(&sender, recipient, 0, 1);
        assert!(FinancialValidator::validate(&state, &zero).is_err());

        // Self-transfer.
        let looped = signed_transfer(&sender, sender_pubkey, 10, 1);
        assert!(FinancialValidator::validate(&state, &looped).is_err());

        // Malformed signature length.
        let stunted = FinancialOp::SignedTransfer {
            from: sender_pubkey,
            to: recipient,
            amount: 10,
            nonce: 1,
            signature: vec![0u8; 8],
        };
        assert!(FinancialValidator::validate(&state, &stunted).is_err());

        // A well-formed, affordable transfer passes pre-flight.
        let good = signed_transfer(&sender, recipient, 10, 1);
        assert!(FinancialValidator::validate(&state, &good).is_ok());
    }
}
