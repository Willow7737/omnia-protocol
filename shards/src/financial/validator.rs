//! Financial shard validator
//!
//! The validator checks whether a financial operation would succeed without
//! actually mutating state. This is used for pre-flight checks and for
//! rejecting invalid operations before they enter the consensus pipeline.

use crate::payload::ShardOp;
use crate::shard::ShardError;
use super::ops::FinancialOp;
use super::state::FinancialState;

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
