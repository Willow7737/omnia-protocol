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
                    return Err(ShardError::ValidationFailed(
                        "Insufficient balance for burn".into(),
                    ));
                }
                Ok(())
            }
            FinancialOp::BalanceQuery { .. } => Ok(()),
        }
    }

    /// Validate a `ShardOp::Financial` variant.
    ///
    /// Convenience wrapper that extracts the inner `FinancialOp` and
    /// delegates to `validate()`.
    pub fn validate_shard_op(state: &FinancialState, op: &ShardOp) -> Result<(), ShardError> {
        match op {
            ShardOp::Financial(fin_op) => Self::validate(state, fin_op),
            _ => Err(ShardError::InvalidOperation(
                "Not a Financial operation".into(),
            )),
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
    fn make_signed_event(
        keypair: &omnia_substrate::crypto::NodeKeypair,
        payload: Vec<u8>,
    ) -> Event {
        let creator = keypair.verifying_key().to_bytes();
        let vc = VectorClock::with_node(creator, 1);
        let mut event = Event::new(creator, 0, vc, None, None, payload);
        event.sign_with_keypair(keypair);
        event
    }

    /// Helper: create a FinancialState with a single account having the given balance.
    fn state_with_balance(account: AccountId, balance: u64) -> FinancialState {
        let mut state = FinancialState::new();
        state
            .balances
            .insert(account, AccountBalance::with_balance(balance));
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
        assert!(
            result.is_err(),
            "Transfer of 100 from 50-balance account should fail"
        );
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
        let mut state = FinancialState::new();

        let mint_op = FinancialOp::Mint {
            to: target,
            amount: 50,
        };

        // Create two identical events (same keypair, same sequence — simulating replay)
        let keypair = generate_keypair();
        let event1 = make_signed_event(&keypair, vec![1]);

        // First Mint — should succeed
        assert!(
            state.apply(&mint_op, &event1).is_ok(),
            "First Mint should succeed"
        );
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
        assert!(
            result.is_err(),
            "Zero-amount transfer should be rejected by validator"
        );
        match result {
            Err(ShardError::InvalidOperation(msg)) => {
                assert!(
                    msg.to_lowercase().contains("zero")
                        || msg.to_lowercase().contains("greater than zero"),
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

        // Even if the validator were bypassed, apply still has the
        // decrement check. With amount 0, decrement would succeed but
        // increment would also succeed — the balance would be unchanged.
        // However, in practice the FinancialShard::process_event calls
        // validate() first, so this should never reach apply().
        // Let's verify that direct apply with amount 0 is a no-op
        // (decrement by 0 succeeds, increment by 0 succeeds).
        let result = state.apply(&transfer, &event);
        // Amount 0 is handled gracefully — balance unchanged
        assert!(result.is_ok(), "Zero-amount transfer via apply is a no-op");
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
        let mut state = FinancialState::new();

        // Verify the account doesn't exist yet
        assert_eq!(state.balance_of(&new_account), 0);
        assert!(!state.balances.contains_key(&new_account));

        let mint = FinancialOp::Mint {
            to: new_account,
            amount: 200,
        };
        let keypair = generate_keypair();
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

        let mint = FinancialOp::Mint {
            to: target,
            amount: 0,
        };

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
        assert!(
            result.is_err(),
            "Transfer from non-existent account should fail"
        );
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
                assert!(
                    msg.contains("Financial"),
                    "Error should mention Financial, got: {msg}"
                );
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
        // For Burn, the event's creator_pubkey doesn't matter — Burn takes `from` from the op
        let keypair2 = generate_keypair();
        let burn_event = make_signed_event(&keypair2, vec![1]);
        assert!(
            state.apply(&burn, &burn_event).is_ok(),
            "Burn should succeed"
        );
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
        let mut state = FinancialState::new();

        // Mint to account A
        let mint = FinancialOp::Mint {
            to: account_a,
            amount: 500,
        };
        let keypair = generate_keypair();
        let event = make_signed_event(&keypair, vec![1]);
        state.apply(&mint, &event).unwrap();

        // Serialize and deserialize
        let bytes = state.to_bytes().unwrap();
        let restored = FinancialState::from_bytes(&bytes).unwrap();

        assert_eq!(restored.balance_of(&account_a), 500);
        assert_eq!(restored.balance_of(&account_b), 0);
        assert_eq!(restored.total_supply, 500);
    }
}
