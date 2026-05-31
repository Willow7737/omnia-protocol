//! Universal Basic Compute (UBC) Token
//!
//! The UBC token is the soulbound monthly allowance that guarantees every
//! DID a free quota of transactions and compute. It is **non-transferable**
//! — tied to the DID, not a wallet. Each month (epoch boundary), the
//! balance resets to the monthly quota. Spending consumes the balance
//! without creating a transferable asset.

use serde::{Deserialize, Serialize};

use crate::error::EconomicsError;

/// A soulbound UBC token owned by a single DID.
///
/// Unlike regular tokens, UBC cannot be transferred between identities.
/// It exists to ensure that participation in the network is a right,
/// not a privilege — every identity receives compute and transaction
/// capacity regardless of economic status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UbcToken {
    /// DID that owns this UBC (soulbound — cannot transfer).
    pub owner_did: String,
    /// Current balance of compute units available for spending.
    pub balance: u64,
    /// Monthly quota (reset every epoch boundary).
    pub monthly_quota: u64,
    /// Epoch number when the balance was last reset.
    pub last_reset_epoch: u64,
}

impl UbcToken {
    /// Create a new UBC token for the given DID with the specified monthly quota.
    pub fn new(owner_did: String, monthly_quota: u64, current_epoch: u64) -> Self {
        Self {
            owner_did,
            balance: monthly_quota,
            monthly_quota,
            last_reset_epoch: current_epoch,
        }
    }

    /// Mint the monthly quota at an epoch boundary.
    ///
    /// If the epoch has advanced since the last reset, the balance is
    /// set back to the full monthly quota. This is not additive — any
    /// unspent balance from the previous epoch is forfeited, preventing
    /// hoarding of compute rights.
    pub fn mint_monthly(&mut self, epoch: u64) {
        if epoch > self.last_reset_epoch {
            self.balance = self.monthly_quota;
            self.last_reset_epoch = epoch;
        }
    }

    /// Spend UBC for a transaction or compute operation.
    ///
    /// Returns an error if the balance is insufficient. This is a
    /// pure consumption operation — the spent units are destroyed,
    /// not transferred to another identity.
    pub fn spend(&mut self, amount: u64) -> Result<(), EconomicsError> {
        if amount == 0 {
            return Err(EconomicsError::InvalidAmount(
                "Spend amount must be greater than zero".into(),
            ));
        }
        if self.balance < amount {
            return Err(EconomicsError::InsufficientUbc {
                have: self.balance,
                need: amount,
            });
        }
        self.balance -= amount;
        Ok(())
    }

    /// Add compute units to the balance as a reward for useful work.
    ///
    /// Unlike `mint_monthly`, this is additive — it increases the
    /// current balance without resetting it. This allows identities
    /// to earn extra compute capacity by contributing to the network.
    ///
    /// # Errors
    ///
    /// Returns [`EconomicsError::BalanceOverflow`] if the addition would
    /// overflow `u64`. This should never happen in practice but is
    /// checked to prevent silent balance truncation via `saturating_add`.
    pub fn reward(&mut self, amount: u64) -> Result<(), EconomicsError> {
        self.balance = self
            .balance
            .checked_add(amount)
            .ok_or_else(|| EconomicsError::BalanceOverflow)?;
        Ok(())
    }
}
