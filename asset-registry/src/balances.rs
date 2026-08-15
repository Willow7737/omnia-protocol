//! Asset-scoped balances — Financial Specification §4.3
//!
//! Implements `balance[asset_id][account_id] = amount`.
//!
//! Every balance operation validates against the asset registry:
//! - Asset must exist and be active
//! - Transfers only allowed for transferable assets (not UBC)
//! - Frozen assets cannot be transferred
//! - Balance operations update supply compartments
//!
//! ## Separation from FinancialShard
//!
//! The existing `shards::financial::FinancialState` is single-asset.
//! This module is the multi-asset replacement that will eventually
//! replace it. During migration, both coexist.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::RegistryError;
use crate::registry::AssetRegistry;
use crate::supply::SupplyAuthority;
use crate::types::AssetId;

/// A 32-byte account identifier (Ed25519 public key).
pub type AccountId = [u8; 32];

/// Balance events for audit trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BalanceEvent {
    /// Tokens transferred between accounts.
    Transferred {
        asset_id: AssetId,
        from: AccountId,
        to: AccountId,
        amount: u64,
    },
    /// Tokens minted to an account.
    Minted {
        asset_id: AssetId,
        to: AccountId,
        amount: u64,
        authority: SupplyAuthority,
    },
    /// Tokens burned from an account.
    Burned {
        asset_id: AssetId,
        from: AccountId,
        amount: u64,
        authority: SupplyAuthority,
    },
    /// Account balance frozen (e.g., legal hold).
    Frozen { asset_id: AssetId, account: AccountId },
    /// Account balance unfrozen.
    Unfrozen { asset_id: AssetId, account: AccountId },
}

/// Per-account balance state for a single asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetBalance {
    /// Free (transferable) balance.
    pub free: u64,
    /// Locked balance (vesting, staking reserve, legal hold).
    pub locked: u64,
    /// Whether this account is frozen for this asset.
    pub frozen: bool,
}

impl AssetBalance {
    /// Create a new zero balance.
    pub fn new() -> Self {
        Self {
            free: 0,
            locked: 0,
            frozen: false,
        }
    }

    /// Total balance (free + locked).
    pub fn total(&self) -> u64 {
        self.free.saturating_add(self.locked)
    }
}

impl Default for AssetBalance {
    fn default() -> Self {
        Self::new()
    }
}

/// Asset-scoped balance ledger.
///
/// Layout: `balances[asset_id][account_id] = AssetBalance`
///
/// Per Spec §4.3: "Balances MUST be scoped by asset."
/// Per Spec §4.4: "The protocol MUST NOT use a single untyped amount field for multiple assets."
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetScopedBalances {
    /// `balance[asset_id][account_id] = AssetBalance`
    balances: BTreeMap<AssetId, BTreeMap<AccountId, AssetBalance>>,
    /// Append-only event log.
    events: Vec<BalanceEvent>,
}

impl AssetScopedBalances {
    /// Create an empty balance ledger.
    pub fn new() -> Self {
        Self {
            balances: BTreeMap::new(),
            events: Vec::new(),
        }
    }

    /// Get the free balance of an account for a specific asset.
    pub fn free_balance(&self, asset_id: AssetId, account: &AccountId) -> u64 {
        self.balances
            .get(&asset_id)
            .and_then(|accounts| accounts.get(account))
            .map(|b| b.free)
            .unwrap_or(0)
    }

    /// Get the total balance (free + locked) for an account/asset.
    pub fn total_balance(&self, asset_id: AssetId, account: &AccountId) -> u64 {
        self.balances
            .get(&asset_id)
            .and_then(|accounts| accounts.get(account))
            .map(|b| b.total())
            .unwrap_or(0)
    }

    /// Get the locked balance.
    pub fn locked_balance(&self, asset_id: AssetId, account: &AccountId) -> u64 {
        self.balances
            .get(&asset_id)
            .and_then(|accounts| accounts.get(account))
            .map(|b| b.locked)
            .unwrap_or(0)
    }

    /// Check if an account exists for an asset.
    pub fn account_exists(&self, asset_id: AssetId, account: &AccountId) -> bool {
        self.balances
            .get(&asset_id)
            .map(|accounts| accounts.contains_key(account))
            .unwrap_or(false)
    }

    /// Credit (mint) tokens to an account.
    ///
    /// Updates both the balance ledger and the supply tracker's
    /// account_balances compartment.
    pub fn credit(
        &mut self,
        asset_id: AssetId,
        account: &AccountId,
        amount: u64,
        authority: SupplyAuthority,
        registry: &mut AssetRegistry,
    ) -> Result<BalanceEvent, RegistryError> {
        if amount == 0 {
            return Err(RegistryError::InvariantViolation("credit amount must be > 0".into()));
        }

        let def = registry
            .get(asset_id)
            .ok_or(RegistryError::AssetNotFound(asset_id.as_u32()))?
            .clone();

        // Record mint in supply tracker
        registry.supply_tracker_mut().mint(
            asset_id,
            amount,
            authority.clone(),
            format!("credit to {}", hex::encode(account)),
            None,
            &def,
        )?;

        // Update supply compartment
        if let Some(supply) = registry.supply_tracker_mut().get_mut(asset_id) {
            supply.account_balances = supply
                .account_balances
                .checked_add(amount)
                .ok_or_else(|| RegistryError::SupplyAccounting("account_balances overflow".into()))?;
        }

        // Update balance ledger
        let asset_balances = self.balances.entry(asset_id).or_default();
        let bal = asset_balances.entry(*account).or_default();
        if bal.frozen {
            return Err(RegistryError::AssetFrozen(asset_id.as_u32()));
        }
        bal.free = bal
            .free
            .checked_add(amount)
            .ok_or_else(|| RegistryError::SupplyAccounting("balance credit overflow".into()))?;

        let event = BalanceEvent::Minted {
            asset_id,
            to: *account,
            amount,
            authority,
        };
        self.events.push(event.clone());
        Ok(event)
    }

    /// Transfer tokens between accounts for a specific asset.
    ///
    /// Enforces:
    /// - Asset exists, is active, and is transferable
    /// - Sender has sufficient free balance
    /// - Sender is not frozen
    /// - No cross-asset contamination (enforced by type system)
    /// - Supply compartments updated correctly (account_balances unchanged — just moves between accounts)
    pub fn transfer(
        &mut self,
        asset_id: AssetId,
        from: &AccountId,
        to: &AccountId,
        amount: u64,
        registry: &AssetRegistry,
    ) -> Result<BalanceEvent, RegistryError> {
        if amount == 0 {
            return Err(RegistryError::InvariantViolation("transfer amount must be > 0".into()));
        }

        if from == to {
            return Err(RegistryError::InvariantViolation("cannot transfer to self".into()));
        }

        // Check asset is transferable
        registry.can_transfer(asset_id)?;

        // Get or create sender balance
        let asset_balances = self.balances.entry(asset_id).or_default();
        let sender =
            asset_balances
                .get_mut(from)
                .ok_or(RegistryError::InsufficientBalance(asset_id.as_u32(), 0, amount))?;

        if sender.frozen {
            return Err(RegistryError::AssetFrozen(asset_id.as_u32()));
        }

        if sender.free < amount {
            return Err(RegistryError::InsufficientBalance(
                asset_id.as_u32(),
                sender.free,
                amount,
            ));
        }

        // Debit sender
        sender.free -= amount;

        // Credit recipient (may create new account)
        let recipient = asset_balances.entry(*to).or_default();
        if recipient.frozen {
            // Rollback sender
            asset_balances.get_mut(from).unwrap().free += amount;
            return Err(RegistryError::AssetFrozen(asset_id.as_u32()));
        }
        let new_recipient_free = match recipient.free.checked_add(amount) {
            Some(v) => v,
            None => {
                // Rollback sender
                asset_balances.get_mut(from).unwrap().free += amount;
                return Err(RegistryError::SupplyAccounting("recipient balance overflow".into()));
            }
        };
        recipient.free = new_recipient_free;

        // Note: total supply is unchanged by transfer.
        // account_balances compartment sum is also unchanged.
        // Only individual account balances change.

        let event = BalanceEvent::Transferred {
            asset_id,
            from: *from,
            to: *to,
            amount,
        };
        self.events.push(event.clone());
        Ok(event)
    }

    /// Burn tokens from an account.
    ///
    /// Enforces:
    /// - UBC cannot be burned (Spec §7.3)
    /// - Account has sufficient free balance
    /// - Account is not frozen
    /// - Updates supply tracker (records burn, reduces account_balances)
    pub fn burn(
        &mut self,
        asset_id: AssetId,
        account: &AccountId,
        amount: u64,
        authority: SupplyAuthority,
        registry: &mut AssetRegistry,
    ) -> Result<BalanceEvent, RegistryError> {
        if amount == 0 {
            return Err(RegistryError::InvariantViolation("burn amount must be > 0".into()));
        }

        let def = registry
            .get(asset_id)
            .ok_or(RegistryError::AssetNotFound(asset_id.as_u32()))?
            .clone();

        // Record burn in supply tracker (also checks UBC burn prohibition)
        registry.supply_tracker_mut().burn(
            asset_id,
            amount,
            authority.clone(),
            format!("burn from {}", hex::encode(account)),
            None,
            &def,
        )?;

        // Update supply compartment
        if let Some(supply) = registry.supply_tracker_mut().get_mut(asset_id) {
            supply.account_balances = supply.account_balances.saturating_sub(amount);
        }

        // Debit balance ledger
        let asset_balances = self.balances.entry(asset_id).or_default();
        let bal = asset_balances
            .get_mut(account)
            .ok_or(RegistryError::InsufficientBalance(asset_id.as_u32(), 0, amount))?;

        if bal.frozen {
            return Err(RegistryError::AssetFrozen(asset_id.as_u32()));
        }

        if bal.free < amount {
            return Err(RegistryError::InsufficientBalance(asset_id.as_u32(), bal.free, amount));
        }

        bal.free -= amount;

        // If balance is now 0 and no locked, we could prune the entry.
        // Keep it for now to avoid re-creation churn.

        let event = BalanceEvent::Burned {
            asset_id,
            from: *account,
            amount,
            authority,
        };
        self.events.push(event.clone());
        Ok(event)
    }

    /// Lock tokens (for vesting, staking reserve, legal hold).
    /// Moves from free to locked within the same account.
    pub fn lock(&mut self, asset_id: AssetId, account: &AccountId, amount: u64) -> Result<(), RegistryError> {
        let asset_balances = self
            .balances
            .get_mut(&asset_id)
            .ok_or(RegistryError::AssetNotFound(asset_id.as_u32()))?;
        let bal = asset_balances
            .get_mut(account)
            .ok_or(RegistryError::InsufficientBalance(asset_id.as_u32(), 0, amount))?;

        if bal.free < amount {
            return Err(RegistryError::InsufficientBalance(asset_id.as_u32(), bal.free, amount));
        }

        bal.free -= amount;
        bal.locked = bal
            .locked
            .checked_add(amount)
            .ok_or_else(|| RegistryError::SupplyAccounting("locked overflow".into()))?;
        Ok(())
    }

    /// Unlock tokens (move from locked back to free).
    pub fn unlock(&mut self, asset_id: AssetId, account: &AccountId, amount: u64) -> Result<(), RegistryError> {
        let asset_balances = self
            .balances
            .get_mut(&asset_id)
            .ok_or(RegistryError::AssetNotFound(asset_id.as_u32()))?;
        let bal = asset_balances
            .get_mut(account)
            .ok_or(RegistryError::AssetNotFound(asset_id.as_u32()))?;

        if bal.locked < amount {
            return Err(RegistryError::InsufficientBalance(
                asset_id.as_u32(),
                bal.locked,
                amount,
            ));
        }

        bal.locked -= amount;
        bal.free = bal
            .free
            .checked_add(amount)
            .ok_or_else(|| RegistryError::SupplyAccounting("unlock overflow".into()))?;
        Ok(())
    }

    /// Freeze an account's balance for a specific asset (legal hold).
    pub fn freeze_account(&mut self, asset_id: AssetId, account: &AccountId) -> Result<BalanceEvent, RegistryError> {
        let asset_balances = self
            .balances
            .get_mut(&asset_id)
            .ok_or(RegistryError::AssetNotFound(asset_id.as_u32()))?;
        let bal = asset_balances
            .get_mut(account)
            .ok_or(RegistryError::AssetNotFound(asset_id.as_u32()))?;
        bal.frozen = true;

        let event = BalanceEvent::Frozen {
            asset_id,
            account: *account,
        };
        self.events.push(event.clone());
        Ok(event)
    }

    /// Unfreeze an account's balance.
    pub fn unfreeze_account(&mut self, asset_id: AssetId, account: &AccountId) -> Result<BalanceEvent, RegistryError> {
        let asset_balances = self
            .balances
            .get_mut(&asset_id)
            .ok_or(RegistryError::AssetNotFound(asset_id.as_u32()))?;
        let bal = asset_balances
            .get_mut(account)
            .ok_or(RegistryError::AssetNotFound(asset_id.as_u32()))?;
        bal.frozen = false;

        let event = BalanceEvent::Unfrozen {
            asset_id,
            account: *account,
        };
        self.events.push(event.clone());
        Ok(event)
    }

    /// Get all balance events.
    pub fn events(&self) -> &[BalanceEvent] {
        &self.events
    }

    /// Get all accounts holding a specific asset.
    pub fn accounts_for_asset(&self, asset_id: AssetId) -> Vec<&AccountId> {
        self.balances
            .get(&asset_id)
            .map(|accounts| accounts.keys().collect())
            .unwrap_or_default()
    }

    /// Get total number of accounts across all assets.
    pub fn total_accounts(&self) -> usize {
        self.balances.values().map(|accounts| accounts.len()).sum()
    }
}

impl Default for AssetScopedBalances {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alice() -> AccountId {
        [0xAAu8; 32]
    }
    fn bob() -> AccountId {
        [0xBBu8; 32]
    }

    #[test]
    fn credit_and_query() {
        let mut balances = AssetScopedBalances::new();
        let mut reg = AssetRegistry::with_genesis_assets();

        balances
            .credit(AssetId::OMNIA, &alice(), 1_000_000, SupplyAuthority::Genesis, &mut reg)
            .unwrap();

        assert_eq!(balances.free_balance(AssetId::OMNIA, &alice()), 1_000_000);
        assert_eq!(balances.total_balance(AssetId::OMNIA, &alice()), 1_000_000);
        assert_eq!(balances.free_balance(AssetId::OMNIA, &bob()), 0);
    }

    #[test]
    fn transfer_between_accounts() {
        let mut balances = AssetScopedBalances::new();
        let mut reg = AssetRegistry::with_genesis_assets();

        balances
            .credit(AssetId::OMNIA, &alice(), 1_000_000, SupplyAuthority::Genesis, &mut reg)
            .unwrap();

        balances
            .transfer(AssetId::OMNIA, &alice(), &bob(), 300_000, &reg)
            .unwrap();

        assert_eq!(balances.free_balance(AssetId::OMNIA, &alice()), 700_000);
        assert_eq!(balances.free_balance(AssetId::OMNIA, &bob()), 300_000);
        // Total supply unchanged by transfer
        assert_eq!(reg.total_supply(AssetId::OMNIA), 1_000_000);
    }

    #[test]
    fn ubc_transfer_rejected() {
        let mut balances = AssetScopedBalances::new();
        let mut reg = AssetRegistry::with_genesis_assets();

        // Credit some UBC
        balances
            .credit(AssetId::UBC, &alice(), 100, SupplyAuthority::EpochEligibility, &mut reg)
            .unwrap();

        // UBC is non-transferable
        let result = balances.transfer(AssetId::UBC, &alice(), &bob(), 10, &reg);
        assert!(result.is_err());
    }

    #[test]
    fn burn_reduces_supply() {
        let mut balances = AssetScopedBalances::new();
        let mut reg = AssetRegistry::with_genesis_assets();

        balances
            .credit(AssetId::OMNIA, &alice(), 1_000_000, SupplyAuthority::Genesis, &mut reg)
            .unwrap();

        assert_eq!(reg.total_supply(AssetId::OMNIA), 1_000_000);

        balances
            .burn(AssetId::OMNIA, &alice(), 100_000, SupplyAuthority::Protocol, &mut reg)
            .unwrap();

        assert_eq!(balances.free_balance(AssetId::OMNIA, &alice()), 900_000);
        assert_eq!(reg.total_supply(AssetId::OMNIA), 900_000);
    }

    #[test]
    fn ubc_burn_prohibited() {
        let mut balances = AssetScopedBalances::new();
        let mut reg = AssetRegistry::with_genesis_assets();

        balances
            .credit(AssetId::UBC, &alice(), 100, SupplyAuthority::EpochEligibility, &mut reg)
            .unwrap();

        let result = balances.burn(AssetId::UBC, &alice(), 10, SupplyAuthority::AccountOwner, &mut reg);
        assert!(matches!(result, Err(RegistryError::UbcBurnProhibited)));
    }

    #[test]
    fn insufficient_balance_rejected() {
        let mut balances = AssetScopedBalances::new();
        let mut reg = AssetRegistry::with_genesis_assets();

        balances
            .credit(AssetId::OMNIA, &alice(), 100, SupplyAuthority::Genesis, &mut reg)
            .unwrap();

        let result = balances.transfer(AssetId::OMNIA, &alice(), &bob(), 200, &reg);
        assert!(matches!(result, Err(RegistryError::InsufficientBalance(_, 100, 200))));
    }

    #[test]
    fn frozen_account_cannot_transfer() {
        let mut balances = AssetScopedBalances::new();
        let mut reg = AssetRegistry::with_genesis_assets();

        balances
            .credit(AssetId::OMNIA, &alice(), 1_000_000, SupplyAuthority::Genesis, &mut reg)
            .unwrap();

        balances.freeze_account(AssetId::OMNIA, &alice()).unwrap();

        let result = balances.transfer(AssetId::OMNIA, &alice(), &bob(), 100, &reg);
        assert!(matches!(result, Err(RegistryError::AssetFrozen(_))));

        // Unfreeze restores transfer
        balances.unfreeze_account(AssetId::OMNIA, &alice()).unwrap();
        balances.transfer(AssetId::OMNIA, &alice(), &bob(), 100, &reg).unwrap();
    }

    #[test]
    fn lock_and_unlock() {
        let mut balances = AssetScopedBalances::new();
        let mut reg = AssetRegistry::with_genesis_assets();

        balances
            .credit(AssetId::OMNIA, &alice(), 1_000_000, SupplyAuthority::Genesis, &mut reg)
            .unwrap();

        // Lock 400k
        balances.lock(AssetId::OMNIA, &alice(), 400_000).unwrap();
        assert_eq!(balances.free_balance(AssetId::OMNIA, &alice()), 600_000);
        assert_eq!(balances.locked_balance(AssetId::OMNIA, &alice()), 400_000);
        assert_eq!(balances.total_balance(AssetId::OMNIA, &alice()), 1_000_000);

        // Can only transfer from free balance
        let result = balances.transfer(AssetId::OMNIA, &alice(), &bob(), 700_000, &reg);
        assert!(matches!(
            result,
            Err(RegistryError::InsufficientBalance(_, 600_000, 700_000))
        ));

        // Unlock 200k
        balances.unlock(AssetId::OMNIA, &alice(), 200_000).unwrap();
        assert_eq!(balances.free_balance(AssetId::OMNIA, &alice()), 800_000);
        assert_eq!(balances.locked_balance(AssetId::OMNIA, &alice()), 200_000);
    }

    #[test]
    fn self_transfer_rejected() {
        let mut balances = AssetScopedBalances::new();
        let mut reg = AssetRegistry::with_genesis_assets();

        balances
            .credit(AssetId::OMNIA, &alice(), 1_000, SupplyAuthority::Genesis, &mut reg)
            .unwrap();

        let result = balances.transfer(AssetId::OMNIA, &alice(), &alice(), 100, &reg);
        assert!(matches!(result, Err(RegistryError::InvariantViolation(_))));
    }

    #[test]
    fn separate_assets_independent() {
        let mut balances = AssetScopedBalances::new();
        let mut reg = AssetRegistry::with_genesis_assets();

        // Credit OMNIA and UBC to same account
        balances
            .credit(AssetId::OMNIA, &alice(), 500, SupplyAuthority::Genesis, &mut reg)
            .unwrap();
        balances
            .credit(AssetId::UBC, &alice(), 300, SupplyAuthority::EpochEligibility, &mut reg)
            .unwrap();

        // Burning OMNIA does not affect UBC
        balances
            .burn(AssetId::OMNIA, &alice(), 200, SupplyAuthority::Protocol, &mut reg)
            .unwrap();

        assert_eq!(balances.free_balance(AssetId::OMNIA, &alice()), 300);
        assert_eq!(balances.free_balance(AssetId::UBC, &alice()), 300);
        assert_eq!(reg.total_supply(AssetId::OMNIA), 300);
        assert_eq!(reg.total_supply(AssetId::UBC), 300);
    }

    #[test]
    fn events_logged() {
        let mut balances = AssetScopedBalances::new();
        let mut reg = AssetRegistry::with_genesis_assets();

        balances
            .credit(AssetId::OMNIA, &alice(), 1_000, SupplyAuthority::Genesis, &mut reg)
            .unwrap();
        balances.transfer(AssetId::OMNIA, &alice(), &bob(), 300, &reg).unwrap();

        assert_eq!(balances.events().len(), 2);
    }
}
