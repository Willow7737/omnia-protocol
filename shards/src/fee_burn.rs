//! Fee-burn mechanism (Sprint 3 — §7.3)
//!
//! Implements a configurable burn-on-fee system where a portion of every
//! operation fee is permanently destroyed (not collected by any party).
//! This creates deflationary pressure on the UBC token supply.
//!
//! # Parameters
//!
//! | Parameter              | Range     | Default | Governance |
//! |------------------------|-----------|---------|------------|
//! | Baseline burn rate     | 0–5%      | 2%      | No         |
//! | Governance burn cap    | 10–25%    | 15%     | Yes        |
//!
//! The **baseline burn rate** is applied to every fee automatically.
//! Governance can raise the effective burn rate up to the **governance
//! burn cap** via proposal. The effective rate is always:
//!
//! ```text
//! effective_rate = max(baseline_rate, governance_rate)
//! ```
//!
//! ## Deflationary model
//!
//! When a fee of `F` UBC is collected, `F * effective_rate / 100` units
//! are destroyed and `F * (100 - effective_rate) / 100` units are
//! collected by the treasury (or simply destroyed in the current model
//! where UBC is soulbound and non-transferable). The burned amount is
//! tracked in [`FeeBurnTracker`] for on-chain transparency.
//!
//! ## Integration
//!
//! The `ShardRouter` calls [`FeeBurnTracker::calculate_burn`] after
//! deducting the fee from the caller's UBC quota. The burned amount
//! is logged and the remaining fee (if any) flows to the treasury
//! allocation.

use serde::{Deserialize, Serialize};

use crate::shard::ShardError;

/// Minimum baseline burn rate (0% — no mandatory burn).
pub const MIN_BASELINE_BURN_PCT: u8 = 0;

/// Maximum baseline burn rate (5% — hard cap, not governance-adjustable).
pub const MAX_BASELINE_BURN_PCT: u8 = 5;

/// Minimum governance burn cap (10% — lowest value governance can set).
pub const MIN_GOVERNANCE_BURN_CAP_PCT: u8 = 10;

/// Maximum governance burn cap (25% — highest value governance can set).
pub const MAX_GOVERNANCE_BURN_CAP_PCT: u8 = 25;

/// Default baseline burn rate (2%).
pub const DEFAULT_BASELINE_BURN_PCT: u8 = 2;

/// Default governance burn cap (15%).
pub const DEFAULT_GOVERNANCE_BURN_CAP_PCT: u8 = 15;

/// Basis points denominator for burn calculations (10000 = 100.00%).
///
/// Using basis points instead of percentages allows sub-percent precision
/// in burn rate calculations without floating-point arithmetic.
pub const BPS_DENOMINATOR: u64 = 10_000;

/// Errors specific to the fee-burn mechanism.
#[derive(Debug, Clone, PartialEq)]
pub enum FeeBurnError {
    /// The baseline burn rate is outside the allowed range [0, 5].
    BaselineRateOutOfRange {
        /// The requested baseline burn rate that was out of range.
        requested: u8,
        /// Minimum allowed baseline burn rate.
        min: u8,
        /// Maximum allowed baseline burn rate.
        max: u8,
    },
    /// The governance burn cap is outside the allowed range [10, 25].
    GovernanceCapOutOfRange {
        /// The requested governance burn cap that was out of range.
        requested: u8,
        /// Minimum allowed governance burn cap.
        min: u8,
        /// Maximum allowed governance burn cap.
        max: u8,
    },
    /// The governance-set burn rate exceeds the governance burn cap.
    GovernanceRateExceedsCap {
        /// The governance burn rate that was set.
        rate: u8,
        /// The governance burn cap that was exceeded.
        cap: u8,
    },
}

impl std::fmt::Display for FeeBurnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FeeBurnError::BaselineRateOutOfRange { requested, min, max } => {
                write!(f, "Baseline burn rate {requested}% is out of range [{min}, {max}]")
            }
            FeeBurnError::GovernanceCapOutOfRange { requested, min, max } => {
                write!(f, "Governance burn cap {requested}% is out of range [{min}, {max}]")
            }
            FeeBurnError::GovernanceRateExceedsCap { rate, cap } => {
                write!(f, "Governance burn rate {rate}% exceeds cap {cap}%")
            }
        }
    }
}

impl std::error::Error for FeeBurnError {}

/// Immutable fee-burn configuration.
///
/// The burn rates are stored as percentages (0–100) but internally
/// converted to basis points for precise arithmetic. This struct is
/// `Serialize`/`Deserialize` for persistence in chain state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeBurnConfig {
    /// Baseline burn rate in percent (0–5). Applied to every fee.
    /// This is NOT governance-adjustable — it's a protocol constant
    /// that can only be changed via runtime upgrade.
    pub baseline_burn_pct: u8,
    /// Maximum burn rate that governance can set, in percent (10–25).
    /// The governance-set rate must not exceed this cap.
    pub governance_burn_cap_pct: u8,
    /// Current burn rate set by governance, in percent (0–governance_burn_cap_pct).
    /// If governance has not set a rate, this equals `baseline_burn_pct`.
    pub governance_burn_pct: u8,
}

impl Default for FeeBurnConfig {
    fn default() -> Self {
        Self::standard()
    }
}

impl FeeBurnConfig {
    /// Create the standard fee-burn configuration.
    ///
    /// - Baseline burn rate: 2%
    /// - Governance burn cap: 15%
    /// - Governance burn rate: 2% (equals baseline until governance acts)
    pub fn standard() -> Self {
        Self {
            baseline_burn_pct: DEFAULT_BASELINE_BURN_PCT,
            governance_burn_cap_pct: DEFAULT_GOVERNANCE_BURN_CAP_PCT,
            governance_burn_pct: DEFAULT_BASELINE_BURN_PCT,
        }
    }

    /// Create a zero-burn configuration (all fees go to treasury).
    ///
    /// Useful for testing and for networks that want full fee
    /// redirection to the treasury.
    pub fn zero() -> Self {
        Self {
            baseline_burn_pct: 0,
            governance_burn_cap_pct: MIN_GOVERNANCE_BURN_CAP_PCT,
            governance_burn_pct: 0,
        }
    }

    /// Create a new configuration with the specified parameters.
    ///
    /// # Errors
    ///
    /// Returns `FeeBurnError::BaselineRateOutOfRange` if `baseline_pct`
    /// is not in `[MIN_BASELINE_BURN_PCT, MAX_BASELINE_BURN_PCT]`.
    ///
    /// Returns `FeeBurnError::GovernanceCapOutOfRange` if `governance_cap_pct`
    /// is not in `[MIN_GOVERNANCE_BURN_CAP_PCT, MAX_GOVERNANCE_BURN_CAP_PCT]`.
    pub fn new(baseline_pct: u8, governance_cap_pct: u8) -> Result<Self, FeeBurnError> {
        #[allow(clippy::absurd_extreme_comparisons)]
        if !(MIN_BASELINE_BURN_PCT..=MAX_BASELINE_BURN_PCT).contains(&baseline_pct) {
            return Err(FeeBurnError::BaselineRateOutOfRange {
                requested: baseline_pct,
                min: MIN_BASELINE_BURN_PCT,
                max: MAX_BASELINE_BURN_PCT,
            });
        }
        if !(MIN_GOVERNANCE_BURN_CAP_PCT..=MAX_GOVERNANCE_BURN_CAP_PCT).contains(&governance_cap_pct) {
            return Err(FeeBurnError::GovernanceCapOutOfRange {
                requested: governance_cap_pct,
                min: MIN_GOVERNANCE_BURN_CAP_PCT,
                max: MAX_GOVERNANCE_BURN_CAP_PCT,
            });
        }
        Ok(Self {
            baseline_burn_pct: baseline_pct,
            governance_burn_cap_pct: governance_cap_pct,
            governance_burn_pct: baseline_pct,
        })
    }

    /// Set the governance burn rate via a governance proposal.
    ///
    /// The rate must be within `[baseline_burn_pct, governance_burn_cap_pct]`.
    /// Governance can only raise the burn rate — it cannot lower it below
    /// the baseline.
    pub fn set_governance_rate(&mut self, rate: u8) -> Result<(), FeeBurnError> {
        if rate > self.governance_burn_cap_pct {
            return Err(FeeBurnError::GovernanceRateExceedsCap {
                rate,
                cap: self.governance_burn_cap_pct,
            });
        }
        self.governance_burn_pct = rate;
        Ok(())
    }

    /// Get the effective burn rate as a percentage.
    ///
    /// The effective rate is `max(baseline, governance_rate)`. This
    /// ensures the baseline is always applied even if governance sets
    /// a lower rate.
    pub fn effective_burn_pct(&self) -> u8 {
        self.baseline_burn_pct.max(self.governance_burn_pct)
    }

    /// Get the effective burn rate in basis points.
    pub fn effective_burn_bps(&self) -> u64 {
        u64::from(self.effective_burn_pct()) * 100
    }
}

/// Tracks cumulative fee-burn statistics for on-chain transparency.
///
/// This struct is persisted as part of the shard state and provides
/// an auditable record of all UBC burned through the fee mechanism.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FeeBurnTracker {
    /// Total UBC units burned through the fee-burn mechanism.
    pub total_burned: u64,
    /// Number of fee-burn events processed.
    pub burn_event_count: u64,
    /// Burn statistics broken down by shard domain.
    pub per_domain_burned: BTreeMapWrapper,
    /// The fee-burn configuration at the time of the last burn.
    pub config: FeeBurnConfig,
}

/// A thin wrapper around a `BTreeMap<String, u64>` for domain-burn tracking.
///
/// Using a wrapper avoids exposing BTreeMap directly and enables
/// deterministic serialization.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BTreeMapWrapper(pub std::collections::BTreeMap<String, u64>);

impl BTreeMapWrapper {
    /// Create an empty domain burn tracker.
    pub fn new() -> Self {
        Self(std::collections::BTreeMap::new())
    }

    /// Record a burn amount for a domain.
    pub fn record(&mut self, domain: &str, amount: u64) {
        *self.0.entry(domain.to_string()).or_insert(0) += amount;
    }

    /// Get the total burned for a specific domain.
    pub fn get(&self, domain: &str) -> u64 {
        self.0.get(domain).copied().unwrap_or(0)
    }

    /// Get an iterator over (domain, amount) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &u64)> {
        self.0.iter()
    }
}

impl FeeBurnTracker {
    /// Create a new fee-burn tracker with the given configuration.
    pub fn new(config: FeeBurnConfig) -> Self {
        Self {
            total_burned: 0,
            burn_event_count: 0,
            per_domain_burned: BTreeMapWrapper::new(),
            config,
        }
    }

    /// Calculate the burn amount for a given fee.
    ///
    /// Returns the number of UBC units to burn. The formula is:
    ///
    /// ```text
    /// burned = fee * effective_burn_bps / BPS_DENOMINATOR
    /// ```
    ///
    /// Uses integer division (rounds down). For a 2% rate on a
    /// 10-UBC fee: `10 * 200 / 10000 = 0` (rounds down to zero).
    /// This is by design — very small fees produce negligible burns.
    pub fn calculate_burn(&self, fee: u64) -> u64 {
        let bps = self.config.effective_burn_bps();
        fee * bps / BPS_DENOMINATOR
    }

    /// Record a burn event, updating all tracking fields.
    ///
    /// This should be called AFTER the burn has been executed (i.e.,
    /// the UBC units have been destroyed). The `domain` parameter
    /// identifies which shard domain the fee came from (e.g.,
    /// "financial", "identity", "cross_shard").
    ///
    /// # Errors
    ///
    /// Returns an error if `burned_amount` does not match
    /// [`calculate_burn`](Self::calculate_burn) for the given fee.
    /// This catch-consistency check prevents accounting drift.
    pub fn record_burn(&mut self, fee: u64, burned_amount: u64, domain: &str) -> Result<(), ShardError> {
        let expected = self.calculate_burn(fee);
        if burned_amount != expected {
            return Err(ShardError::ValidationFailed(format!(
                "Fee-burn consistency check failed: expected {expected} burned for fee {fee}, got {burned_amount}"
            )));
        }
        self.total_burned = self
            .total_burned
            .checked_add(burned_amount)
            .ok_or_else(|| ShardError::ValidationFailed("Fee-burn total overflow".into()))?;
        self.burn_event_count = self
            .burn_event_count
            .checked_add(1)
            .ok_or_else(|| ShardError::ValidationFailed("Fee-burn event count overflow".into()))?;
        self.per_domain_burned.record(domain, burned_amount);
        Ok(())
    }

    /// Update the fee-burn configuration (e.g., after a governance vote).
    pub fn update_config(&mut self, new_config: FeeBurnConfig) {
        self.config = new_config;
    }

    /// Get the current burn configuration.
    pub fn config(&self) -> &FeeBurnConfig {
        &self.config
    }

    /// Get the burn rate statistics.
    pub fn stats(&self) -> FeeBurnStats {
        FeeBurnStats {
            total_burned: self.total_burned,
            burn_event_count: self.burn_event_count,
            effective_burn_pct: self.config.effective_burn_pct(),
            baseline_burn_pct: self.config.baseline_burn_pct,
            governance_burn_pct: self.config.governance_burn_pct,
            governance_burn_cap_pct: self.config.governance_burn_cap_pct,
        }
    }
}

/// Snapshot of fee-burn statistics, suitable for API responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeBurnStats {
    /// Total UBC units burned.
    pub total_burned: u64,
    /// Number of individual burn events.
    pub burn_event_count: u64,
    /// Current effective burn rate (percent).
    pub effective_burn_pct: u8,
    /// Baseline burn rate (percent).
    pub baseline_burn_pct: u8,
    /// Governance-set burn rate (percent).
    pub governance_burn_pct: u8,
    /// Maximum burn rate governance can set (percent).
    pub governance_burn_cap_pct: u8,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // FeeBurnConfig tests
    // ------------------------------------------------------------------

    #[test]
    fn test_standard_config_defaults() {
        let cfg = FeeBurnConfig::standard();
        assert_eq!(cfg.baseline_burn_pct, 2);
        assert_eq!(cfg.governance_burn_cap_pct, 15);
        assert_eq!(cfg.governance_burn_pct, 2);
        assert_eq!(cfg.effective_burn_pct(), 2);
    }

    #[test]
    fn test_zero_config() {
        let cfg = FeeBurnConfig::zero();
        assert_eq!(cfg.baseline_burn_pct, 0);
        assert_eq!(cfg.effective_burn_pct(), 0);
    }

    #[test]
    fn test_new_valid_config() {
        let cfg = FeeBurnConfig::new(3, 20).unwrap();
        assert_eq!(cfg.baseline_burn_pct, 3);
        assert_eq!(cfg.governance_burn_cap_pct, 20);
        assert_eq!(cfg.governance_burn_pct, 3); // defaults to baseline
    }

    #[test]
    fn test_new_baseline_out_of_range_low() {
        let result = FeeBurnConfig::new(6, 15);
        assert!(matches!(result, Err(FeeBurnError::BaselineRateOutOfRange { .. })));
    }

    #[test]
    fn test_new_governance_cap_out_of_range_low() {
        let result = FeeBurnConfig::new(2, 5);
        assert!(matches!(result, Err(FeeBurnError::GovernanceCapOutOfRange { .. })));
    }

    #[test]
    fn test_new_governance_cap_out_of_range_high() {
        let result = FeeBurnConfig::new(2, 30);
        assert!(matches!(result, Err(FeeBurnError::GovernanceCapOutOfRange { .. })));
    }

    #[test]
    fn test_set_governance_rate_valid() {
        let mut cfg = FeeBurnConfig::standard();
        cfg.set_governance_rate(10).unwrap();
        assert_eq!(cfg.governance_burn_pct, 10);
        assert_eq!(cfg.effective_burn_pct(), 10); // max(2, 10) = 10
    }

    #[test]
    fn test_set_governance_rate_exceeds_cap() {
        let mut cfg = FeeBurnConfig::standard();
        let result = cfg.set_governance_rate(20);
        assert!(matches!(result, Err(FeeBurnError::GovernanceRateExceedsCap { .. })));
    }

    #[test]
    fn test_set_governance_rate_below_baseline() {
        let mut cfg = FeeBurnConfig::new(5, 20).unwrap();
        // Setting governance to 1% when baseline is 5% should succeed
        // (governance can set lower), but effective stays at 5%
        cfg.set_governance_rate(1).unwrap();
        assert_eq!(cfg.governance_burn_pct, 1);
        assert_eq!(cfg.effective_burn_pct(), 5); // max(5, 1) = 5
    }

    #[test]
    fn test_effective_burn_pct_uses_max() {
        let mut cfg = FeeBurnConfig::new(3, 25).unwrap();
        cfg.set_governance_rate(7).unwrap();
        assert_eq!(cfg.effective_burn_pct(), 7); // max(3, 7) = 7
    }

    #[test]
    fn test_effective_burn_bps_conversion() {
        let cfg = FeeBurnConfig::new(5, 20).unwrap();
        assert_eq!(cfg.effective_burn_bps(), 500); // 5% = 500 bps
    }

    #[test]
    fn test_baseline_boundary_values() {
        // 0% baseline is valid
        assert!(FeeBurnConfig::new(0, 15).is_ok());
        // 5% baseline is valid
        assert!(FeeBurnConfig::new(5, 15).is_ok());
        // 6% baseline is invalid
        assert!(FeeBurnConfig::new(6, 15).is_err());
    }

    #[test]
    fn test_governance_cap_boundary_values() {
        // 10% cap is valid
        assert!(FeeBurnConfig::new(2, 10).is_ok());
        // 25% cap is valid
        assert!(FeeBurnConfig::new(2, 25).is_ok());
        // 9% cap is invalid
        assert!(FeeBurnConfig::new(2, 9).is_err());
        // 26% cap is invalid
        assert!(FeeBurnConfig::new(2, 26).is_err());
    }

    // ------------------------------------------------------------------
    // FeeBurnTracker tests
    // ------------------------------------------------------------------

    #[test]
    fn test_calculate_burn_2pct() {
        let tracker = FeeBurnTracker::new(FeeBurnConfig::standard());
        // 2% of 1000 = 20
        assert_eq!(tracker.calculate_burn(1000), 20);
        // 2% of 100 = 2
        assert_eq!(tracker.calculate_burn(100), 2);
        // 2% of 10 = 0 (rounds down)
        assert_eq!(tracker.calculate_burn(10), 0);
        // 2% of 0 = 0
        assert_eq!(tracker.calculate_burn(0), 0);
    }

    #[test]
    fn test_calculate_burn_5pct() {
        let tracker = FeeBurnTracker::new(FeeBurnConfig::new(5, 25).unwrap());
        // 5% of 1000 = 50
        assert_eq!(tracker.calculate_burn(1000), 50);
        // 5% of 100 = 5
        assert_eq!(tracker.calculate_burn(100), 5);
        // 5% of 10 = 0 (rounds down)
        assert_eq!(tracker.calculate_burn(10), 0);
    }

    #[test]
    fn test_calculate_burn_zero_config() {
        let tracker = FeeBurnTracker::new(FeeBurnConfig::zero());
        assert_eq!(tracker.calculate_burn(1000), 0);
    }

    #[test]
    fn test_record_burn_single() {
        let mut tracker = FeeBurnTracker::new(FeeBurnConfig::standard());
        let fee = 1000;
        let burned = tracker.calculate_burn(fee);
        tracker.record_burn(fee, burned, "financial").unwrap();
        assert_eq!(tracker.total_burned, 20);
        assert_eq!(tracker.burn_event_count, 1);
        assert_eq!(tracker.per_domain_burned.get("financial"), 20);
    }

    #[test]
    fn test_record_burn_multiple_domains() {
        let mut tracker = FeeBurnTracker::new(FeeBurnConfig::standard());
        tracker.record_burn(1000, 20, "financial").unwrap();
        tracker.record_burn(500, 10, "identity").unwrap();
        tracker.record_burn(2000, 40, "cross_shard").unwrap();
        assert_eq!(tracker.total_burned, 70);
        assert_eq!(tracker.burn_event_count, 3);
        assert_eq!(tracker.per_domain_burned.get("financial"), 20);
        assert_eq!(tracker.per_domain_burned.get("identity"), 10);
        assert_eq!(tracker.per_domain_burned.get("cross_shard"), 40);
    }

    #[test]
    fn test_record_burn_consistency_check() {
        let mut tracker = FeeBurnTracker::new(FeeBurnConfig::standard());
        let fee = 1000;
        let correct_burn = tracker.calculate_burn(fee);
        // Wrong burn amount should fail
        let result = tracker.record_burn(fee, correct_burn + 1, "financial");
        assert!(result.is_err());
        // Correct burn amount should succeed
        tracker.record_burn(fee, correct_burn, "financial").unwrap();
    }

    #[test]
    fn test_record_burn_zero_fee() {
        let mut tracker = FeeBurnTracker::new(FeeBurnConfig::standard());
        tracker.record_burn(0, 0, "financial").unwrap();
        assert_eq!(tracker.total_burned, 0);
        assert_eq!(tracker.burn_event_count, 1);
    }

    #[test]
    fn test_update_config_changes_burn_calculation() {
        let mut tracker = FeeBurnTracker::new(FeeBurnConfig::standard());
        // 2% of 1000 = 20
        assert_eq!(tracker.calculate_burn(1000), 20);

        // Raise governance rate to 10%
        let mut new_cfg = FeeBurnConfig::standard();
        new_cfg.set_governance_rate(10).unwrap();
        tracker.update_config(new_cfg);

        // 10% of 1000 = 100
        assert_eq!(tracker.calculate_burn(1000), 100);
    }

    #[test]
    fn test_stats_snapshot() {
        let mut tracker = FeeBurnTracker::new(FeeBurnConfig::standard());
        tracker.record_burn(1000, 20, "financial").unwrap();
        let stats = tracker.stats();
        assert_eq!(stats.total_burned, 20);
        assert_eq!(stats.burn_event_count, 1);
        assert_eq!(stats.effective_burn_pct, 2);
        assert_eq!(stats.baseline_burn_pct, 2);
    }

    #[test]
    fn test_serialization_roundtrip() {
        let mut tracker = FeeBurnTracker::new(FeeBurnConfig::standard());
        tracker.record_burn(1000, 20, "financial").unwrap();
        tracker.record_burn(500, 10, "identity").unwrap();
        let bytes = postcard::to_allocvec(&tracker).unwrap();
        let restored: FeeBurnTracker = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(restored.total_burned, 30);
        assert_eq!(restored.burn_event_count, 2);
        assert_eq!(restored.config.effective_burn_pct(), 2);
    }

    #[test]
    fn test_governance_rate_at_max_cap() {
        let mut cfg = FeeBurnConfig::new(2, 25).unwrap();
        cfg.set_governance_rate(25).unwrap();
        assert_eq!(cfg.effective_burn_pct(), 25);
    }

    #[test]
    fn test_governance_rate_one_above_cap() {
        let mut cfg = FeeBurnConfig::new(2, 25).unwrap();
        let result = cfg.set_governance_rate(26);
        assert!(result.is_err());
    }

    #[test]
    fn test_large_fee_burn_precision() {
        let tracker = FeeBurnTracker::new(FeeBurnConfig::new(3, 20).unwrap());
        // 3% of u64::MAX / 100 to avoid overflow
        let fee = 1_000_000_000_000_u64;
        let burned = tracker.calculate_burn(fee);
        assert_eq!(burned, 30_000_000_000); // 3% of 1T = 30B
    }
}
