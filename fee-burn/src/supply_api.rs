//! Supply query API — Spec §7.3
//!
//! "Supply API must show: total minted, total burned, current supply."
//!
//! This module provides a read-only view into the supply state.

use serde::{Deserialize, Serialize};

use crate::burn::BurnAccounting;

/// Immutable snapshot of an asset's supply state.
/// Per Spec §7.3: users and monitoring MUST be able to query
/// total minted, total burned, and current circulating supply.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplySnapshot {
    /// Asset ID.
    pub asset_id: u32,
    /// Total tokens ever minted.
    pub total_minted: u64,
    /// Total tokens ever burned.
    pub total_burned: u64,
    /// Current circulating supply = minted - burned.
    pub circulating_supply: u64,
    /// Tokens held by user accounts.
    pub account_balances: u64,
    /// Tokens locked (vesting, staking, etc.).
    pub locked_balances: u64,
    /// Tokens held in treasury buckets.
    pub treasury_balances: u64,
    /// Tokens in escrow (pending orders, etc.).
    pub escrow_balances: u64,
    /// Hard cap for this asset (if bounded).
    pub hard_cap: Option<u64>,
    /// Whether the hard cap has been reached.
    pub cap_reached: bool,
    /// Timestamp of this snapshot (ms).
    pub snapshot_timestamp_ms: u64,
}

impl SupplySnapshot {
    /// Create a supply snapshot from component values.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        asset_id: u32,
        total_minted: u64,
        burn_accounting: &BurnAccounting,
        account_balances: u64,
        locked_balances: u64,
        treasury_balances: u64,
        escrow_balances: u64,
        hard_cap: Option<u64>,
        now_ms: u64,
    ) -> Self {
        let total_burned = burn_accounting.total_burned();
        let circulating_supply = total_minted.saturating_sub(total_burned);
        let cap_reached = hard_cap.map(|cap| circulating_supply >= cap).unwrap_or(false);
        Self {
            asset_id,
            total_minted,
            total_burned,
            circulating_supply,
            account_balances,
            locked_balances,
            treasury_balances,
            escrow_balances,
            hard_cap,
            cap_reached,
            snapshot_timestamp_ms: now_ms,
        }
    }

    /// Verify the supply decomposition invariant (Spec §4.4):
    /// `circulating = account + locked + treasury + escrow`
    pub fn verify_decomposition(&self) -> bool {
        let decomposed = self
            .account_balances
            .checked_add(self.locked_balances)
            .and_then(|v| v.checked_add(self.treasury_balances))
            .and_then(|v| v.checked_add(self.escrow_balances));
        match decomposed {
            Some(total) => total == self.circulating_supply,
            None => false, // overflow = invariant broken
        }
    }

    /// Return the burn rate as a percentage (0.0 to 100.0).
    pub fn burn_rate_pct(&self) -> f64 {
        if self.total_minted == 0 {
            return 0.0;
        }
        (self.total_burned as f64 / self.total_minted as f64) * 100.0
    }

    /// Return remaining capacity before hard cap.
    pub fn remaining_capacity(&self) -> Option<u64> {
        self.hard_cap.map(|cap| cap.saturating_sub(self.circulating_supply))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::burn::BurnAccounting;

    fn make_accounting(burned: u64) -> BurnAccounting {
        let mut acc = BurnAccounting::new();
        if burned > 0 {
            acc.record_burn(0, burned, 300, "test", None, 1_000_000_000 - burned, 0);
        }
        acc
    }

    #[test]
    fn snapshot_circulating_supply() {
        let acc = make_accounting(50_000);
        let snap = SupplySnapshot::new(
            0,
            1_000_000_000,
            &acc,
            600_000,
            200_000,
            149_750_000,
            50_000,
            Some(1_000_000_000),
            0,
        );
        assert_eq!(snap.total_minted, 1_000_000_000);
        assert_eq!(snap.total_burned, 50_000);
        assert_eq!(snap.circulating_supply, 999_950_000);
        assert!(!snap.cap_reached);
    }

    #[test]
    fn snapshot_decomposition_valid() {
        let acc = make_accounting(0);
        let snap = SupplySnapshot::new(0, 1_000_000, &acc, 500_000, 200_000, 200_000, 100_000, None, 0);
        assert!(snap.verify_decomposition());
    }

    #[test]
    fn snapshot_decomposition_invalid() {
        let acc = make_accounting(0);
        let snap = SupplySnapshot::new(0, 1_000_000, &acc, 500_000, 200_000, 200_000, 999_000, None, 0);
        // 500k + 200k + 200k + 999k = 1,899k != 1,000k
        assert!(!snap.verify_decomposition());
    }

    #[test]
    fn burn_rate_calculation() {
        let acc = make_accounting(50_000_000);
        let snap = SupplySnapshot::new(0, 1_000_000_000, &acc, 0, 0, 0, 0, None, 0);
        assert!((snap.burn_rate_pct() - 5.0).abs() < 0.01);
    }

    #[test]
    fn remaining_capacity() {
        let acc = make_accounting(0);
        let snap = SupplySnapshot::new(0, 800_000_000, &acc, 0, 0, 0, 0, Some(1_000_000_000), 0);
        assert_eq!(snap.remaining_capacity(), Some(200_000_000));
    }

    #[test]
    fn cap_reached() {
        let acc = make_accounting(0);
        let snap = SupplySnapshot::new(0, 1_000_000_000, &acc, 0, 0, 0, 0, Some(1_000_000_000), 0);
        assert!(snap.cap_reached);
    }
}
