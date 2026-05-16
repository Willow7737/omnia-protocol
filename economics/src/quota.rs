//! Monthly quota tracking and epoch management
//!
//! The `QuotaSystem` manages the lifecycle of UBC tokens across all
//! registered DIDs. It handles epoch transitions, token creation on
//! DID registration, and provides balance queries. The quota system
//! is the central registry that ties identities to their compute
//! allowances.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::EconomicsError;
use crate::ubc::UbcToken;

/// Default UBC quota per month (in compute units).
pub const DEFAULT_UBC_QUOTA: u64 = 1000;

/// Default epoch duration in milliseconds (30 days).
pub const DEFAULT_EPOCH_DURATION_MS: u64 = 30 * 24 * 60 * 60 * 1000;

/// The quota system manages UBC tokens for all registered DIDs.
///
/// It tracks the current epoch, handles epoch transitions (which
/// reset all UBC balances), and provides methods for spending,
/// rewarding, and querying balances.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaSystem {
    /// Current epoch number (incremented on each `advance_epoch` call).
    pub current_epoch: u64,
    /// Duration of one epoch in milliseconds.
    pub epoch_duration_ms: u64,
    /// Default monthly UBC quota assigned to new DIDs.
    pub default_quota: u64,
    /// DID → UBC token mapping.
    pub tokens: HashMap<String, UbcToken>,
}

impl QuotaSystem {
    /// Create a new quota system with the specified default quota and epoch duration.
    pub fn new(default_quota: u64, epoch_duration_ms: u64) -> Self {
        Self {
            current_epoch: 0,
            epoch_duration_ms,
            default_quota,
            tokens: HashMap::new(),
        }
    }

    /// Create a quota system with default parameters (1000 UBC/month, 30-day epochs).
    pub fn default_system() -> Self {
        Self::new(DEFAULT_UBC_QUOTA, DEFAULT_EPOCH_DURATION_MS)
    }

    /// Register a DID in the quota system, granting it the default monthly quota.
    ///
    /// If the DID is already registered, this is a no-op — the existing
    /// token and balance are preserved.
    pub fn register_did(&mut self, did: &str) {
        self.tokens.entry(did.to_string()).or_insert_with(|| {
            UbcToken::new(did.to_string(), self.default_quota, self.current_epoch)
        });
    }

    /// Advance to the next epoch, resetting all UBC balances.
    ///
    /// Every registered DID has its balance set back to its monthly
    /// quota. Unspent balance from the previous epoch is forfeited
    /// to prevent hoarding.
    pub fn advance_epoch(&mut self) {
        self.current_epoch += 1;
        for token in self.tokens.values_mut() {
            token.mint_monthly(self.current_epoch);
        }
    }

    /// Spend UBC from a DID's balance.
    ///
    /// Returns an error if the DID is not registered or has insufficient
    /// balance.
    pub fn spend(&mut self, did: &str, amount: u64) -> Result<(), EconomicsError> {
        let token = self
            .tokens
            .get_mut(did)
            .ok_or_else(|| EconomicsError::DidNotRegistered(did.to_string()))?;
        token.spend(amount)
    }

    /// Reward a DID with additional UBC for contributing useful work.
    ///
    /// Unlike epoch minting, this is additive — it increases the
    /// current balance without resetting it.
    pub fn reward(&mut self, did: &str, amount: u64) -> Result<(), EconomicsError> {
        let token = self
            .tokens
            .get_mut(did)
            .ok_or_else(|| EconomicsError::DidNotRegistered(did.to_string()))?;
        token.reward(amount);
        Ok(())
    }

    /// Query the current UBC balance of a DID.
    ///
    /// Returns `None` if the DID is not registered.
    pub fn balance_of(&self, did: &str) -> Option<u64> {
        self.tokens.get(did).map(|t| t.balance)
    }

    /// Query the monthly quota of a DID.
    ///
    /// Returns `None` if the DID is not registered.
    pub fn quota_of(&self, did: &str) -> Option<u64> {
        self.tokens.get(did).map(|t| t.monthly_quota)
    }

    /// Check if a DID is registered in the quota system.
    pub fn is_registered(&self, did: &str) -> bool {
        self.tokens.contains_key(did)
    }

    /// Get the number of registered DIDs.
    pub fn registered_count(&self) -> usize {
        self.tokens.len()
    }

    /// Serialize the quota system state to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        postcard::to_allocvec(self).expect("QuotaSystem serialization cannot fail")
    }

    /// Deserialize quota system state from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, postcard::Error> {
        postcard::from_bytes(bytes)
    }
}

impl Default for QuotaSystem {
    fn default() -> Self {
        Self::default_system()
    }
}
