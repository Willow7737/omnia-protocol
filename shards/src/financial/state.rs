//! Financial shard state
//!
//! The Financial shard maintains account balances and total supply. Unlike
//! other shards that can use CRDTs, financial operations require **strict
//! causal ordering** because transfers and burns are not commutative.
//!
//! ## AccountBalance: Intentionally different from the CRDT version
//!
//! `AccountBalance` here is **intentionally different** from
//! `omnia_consensus::crdt::AccountBalance` (which wraps a grow-only GCounter).
//! The two types serve fundamentally different purposes:
//!
//! | Feature              | Financial (`shards`)            | Consensus (`crdt`)           |
//! |----------------------|---------------------------------|------------------------------|
//! | Internal type        | `u64 balance`                   | `GCounter` (per-node counts) |
//! | Decrement support    | ✅ Required for transfers/burns | ❌ GCounter is increment-only |
//! | Causal tracking      | `VectorClock` per account       | Per-node `NodeId` in GCounter |
//! | Merge semantics      | Last-write-wins (not CRDT)      | GCounter merge (commutative)  |
//!
//! Replacing this with the CRDT version would break `Transfer` and `Burn`
//! operations that require balance reduction. The name collision is already
//! resolved at the re-export level: `shards::FinancialAccountBalance` vs
//! `consensus::crdt::AccountBalance`.

use std::collections::HashMap;

use omnia_substrate::{Event, VectorClock};
use serde::{Deserialize, Serialize};

use super::ops::{AccountId, FinancialOp};
use crate::shard::ShardError;

/// A single account's balance with causal tracking.
///
/// Uses a simple `u64` balance (not a GCounter) because financial
/// operations need decrement support and strict ordering. The
/// `last_update` vector clock allows the shard to detect and reject
/// conflicting concurrent updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountBalance {
    /// Current balance.
    pub balance: u64,
    /// Vector clock at the time of the last update.
    pub last_update: VectorClock,
}

impl AccountBalance {
    /// Create a new account with zero balance.
    pub fn new() -> Self {
        Self {
            balance: 0,
            last_update: VectorClock::new(),
        }
    }

    /// Create a new account with an initial balance.
    pub fn with_balance(balance: u64) -> Self {
        Self {
            balance,
            last_update: VectorClock::new(),
        }
    }

    /// Return the current balance.
    pub fn value(&self) -> u64 {
        self.balance
    }

    /// Increment the balance by `amount`, recording the causal context.
    pub fn increment(&mut self, amount: u64, vc: &VectorClock) {
        self.balance += amount;
        self.last_update.merge(vc);
    }

    /// Decrement the balance by `amount`, recording the causal context.
    ///
    /// Returns an error if the account has insufficient funds.
    pub fn decrement(&mut self, amount: u64, vc: &VectorClock) -> Result<(), ShardError> {
        if self.balance < amount {
            return Err(ShardError::ValidationFailed("Insufficient balance".into()));
        }
        self.balance -= amount;
        self.last_update.merge(vc);
        Ok(())
    }
}

impl Default for AccountBalance {
    fn default() -> Self {
        Self::new()
    }
}

/// The full state of the Financial shard.
///
/// Tracks per-account balances and the total supply of the native asset.
/// All mutations go through `apply()`, which enforces validation rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinancialState {
    /// Per-account balances.
    pub balances: HashMap<AccountId, AccountBalance>,
    /// Total supply across all accounts.
    pub total_supply: u64,
}

impl FinancialState {
    /// Version byte prefixed to serialized snapshots for format migration.
    const FINANCIAL_STATE_VERSION: u8 = 1;

    /// Create an empty financial state with no accounts.
    pub fn new() -> Self {
        Self {
            balances: HashMap::new(),
            total_supply: 0,
        }
    }

    /// Get the balance of an account (0 if the account doesn't exist).
    pub fn balance_of(&self, account: &AccountId) -> u64 {
        self.balances.get(account).map(|b| b.value()).unwrap_or(0)
    }

    /// Apply a financial operation, mutating state.
    ///
    /// For `Transfer`, the sender is taken from `event.creator_pubkey`.
    /// The vector clock from the event is used for causal tracking.
    pub fn apply(&mut self, op: &FinancialOp, event: &Event) -> Result<(), ShardError> {
        match op {
            FinancialOp::Transfer { to, amount } => {
                let from = event.creator_pubkey;
                let vc = &event.vector_clock;

                // Debit the sender
                let from_balance = self.balances.get_mut(&from).ok_or_else(|| {
                    ShardError::ValidationFailed("Sender account not found".into())
                })?;

                if from_balance.value() < *amount {
                    return Err(ShardError::ValidationFailed("Insufficient balance".into()));
                }
                from_balance.decrement(*amount, vc)?;

                // Credit the recipient
                let to_balance = self.balances.entry(*to).or_default();
                to_balance.increment(*amount, vc);

                Ok(())
            }
            FinancialOp::Mint { to, amount } => {
                let vc = &event.vector_clock;
                let balance = self.balances.entry(*to).or_default();
                balance.increment(*amount, vc);
                self.total_supply += amount;
                Ok(())
            }
            FinancialOp::Burn { from, amount } => {
                let vc = &event.vector_clock;
                let balance = self
                    .balances
                    .get_mut(from)
                    .ok_or_else(|| ShardError::ValidationFailed("Account not found".into()))?;

                if balance.value() < *amount {
                    return Err(ShardError::ValidationFailed("Insufficient balance".into()));
                }
                balance.decrement(*amount, vc)?;
                self.total_supply -= amount;
                Ok(())
            }
            FinancialOp::BalanceQuery { .. } => {
                // Read-only — no state mutation.
                Ok(())
            }
        }
    }

    /// Serialize the state to bytes for snapshots.
    ///
    /// The output is prefixed with a version byte to support future
    /// state-format migrations.
    pub fn to_bytes(&self) -> Result<Vec<u8>, postcard::Error> {
        let mut bytes = vec![Self::FINANCIAL_STATE_VERSION];
        bytes.extend(postcard::to_allocvec(self)?);
        Ok(bytes)
    }

    /// Deserialize state from bytes.
    ///
    /// Reads and validates the version byte before deserializing the
    /// payload. Returns an error if the version is unsupported.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, postcard::Error> {
        if bytes.is_empty() {
            return Err(postcard::Error::DeserializeUnexpectedEnd);
        }
        let version = bytes[0];
        if version != Self::FINANCIAL_STATE_VERSION {
            return Err(postcard::Error::DeserializeUnexpectedEnd);
        }
        postcard::from_bytes(&bytes[1..])
    }
}

impl Default for FinancialState {
    fn default() -> Self {
        Self::new()
    }
}
