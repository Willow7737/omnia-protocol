//! Financial shard operations
//!
//! Defines the three core financial operations (Transfer, Mint, Burn) and
//! a read-only BalanceQuery. All amounts are `u64` for simplicity; future
//! iterations may support arbitrary-precision decimals.

use serde::{Deserialize, Serialize};

/// Account identifier — a 32-byte public key (same size as NodeId).
pub type AccountId = [u8; 32];

/// Operations supported by the Financial shard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FinancialOp {
    /// Transfer `amount` from the event creator to `to`.
    Transfer {
        /// Recipient account.
        to: AccountId,
        /// Amount to transfer.
        amount: u64,
    },
    /// Mint `amount` and credit it to `to`.
    ///
    /// Only authorized minter accounts should be able to invoke this.
    /// Authorization is checked in the validator.
    Mint {
        /// Recipient account.
        to: AccountId,
        /// Amount to mint.
        amount: u64,
    },
    /// Burn `amount` from the `from` account.
    Burn {
        /// Account to burn from.
        from: AccountId,
        /// Amount to burn.
        amount: u64,
    },
    /// Read-only balance query — does not mutate state.
    BalanceQuery {
        /// Account to query.
        account: AccountId,
    },
}
