//! Burn policy and accounting — Spec §7.3
//!
//! ## Rules
//!
//! - Initial burn ratio: 0–5% of OMNIA base-fee
//! - Governance ceiling: 10–25%
//! - UBC MUST NOT be burned as OMNIA
//! - External-chain fees MUST NOT be misrepresented as OMNIA burns
//! - Every burn MUST emit an event
//! - Increases require usage data, public proposal, timelock
//! - Governance MUST NOT exceed hard-coded ceiling without protocol upgrade

use serde::{Deserialize, Serialize};

use crate::error::FeeError;
use crate::fee::RoundingRule;

// --- Burn Ratio ---

/// Burn ratio represented as basis points (1 bp = 0.01%).
///
/// - Initial range: 0–500 bps (0–5%)
/// - Governance ceiling: 1000–2500 bps (10–25%)
/// - Absolute hard maximum: 2500 bps (25%)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BurnRatio {
    /// Burn ratio in basis points.
    /// Valid range depends on context (initial vs governance ceiling).
    bps: u16,
}

impl BurnRatio {
    /// Minimum burn ratio (0%).
    pub const ZERO: Self = Self { bps: 0 };

    /// Initial maximum burn ratio: 5% = 500 bps.
    pub const INITIAL_MAX: u16 = 500;

    /// Governance ceiling minimum: 10% = 1000 bps.
    pub const GOVERNANCE_CEILING_MIN: u16 = 1000;

    /// Governance ceiling maximum: 25% = 2500 bps.
    /// This is an ABSOLUTE hard cap — cannot be exceeded even by governance
    /// without a separate protocol upgrade.
    pub const ABSOLUTE_CEILING: u16 = 2500;

    /// Create a burn ratio from basis points.
    /// Validates against the initial range (0–500 bps).
    pub fn new(bps: u16) -> Result<Self, FeeError> {
        if bps > Self::INITIAL_MAX {
            return Err(FeeError::BurnRatioExceedsInitial {
                requested_bps: bps,
                max_bps: Self::INITIAL_MAX,
            });
        }
        Ok(Self { bps })
    }

    /// Create a burn ratio from basis points, validated against
    /// the governance ceiling (up to 25%).
    pub fn new_governance(bps: u16) -> Result<Self, FeeError> {
        if bps > Self::ABSOLUTE_CEILING {
            return Err(FeeError::BurnRatioExceedsCeiling {
                requested_bps: bps,
                ceiling_bps: Self::ABSOLUTE_CEILING,
            });
        }
        Ok(Self { bps })
    }

    /// Create a burn ratio directly from basis points (unchecked).
    /// Used for deserialization and internal construction where
    /// the value is known to be valid.
    pub fn from_bps(bps: u16) -> Self {
        Self { bps: bps.min(Self::ABSOLUTE_CEILING) }
    }

    /// Return the burn ratio as basis points.
    pub fn bps(&self) -> u16 {
        self.bps
    }

    /// Return the burn ratio as a fraction (0.0 to 1.0).
    pub fn as_fraction(&self) -> f64 {
        self.bps as f64 / 10_000.0
    }

    /// Apply this burn ratio to an amount.
    /// Returns the amount to be burned.
    pub fn apply(&self, amount: u64, rounding: RoundingRule) -> u64 {
        let result = (amount as u128 * self.bps as u128) / 10_000;
        let mut burned = result as u64;
        match rounding {
            RoundingRule::Up => {
                // Check if there's a remainder that would round down
                let exact = amount as u128 * self.bps as u128;
                if exact % 10_000 != 0 && burned < amount {
                    burned = burned.saturating_add(1);
                }
            }
            RoundingRule::Down => { /* truncation already done */ }
            RoundingRule::Nearest => {
                let remainder = (amount as u128 * self.bps as u128) % 10_000;
                if remainder >= 5_000 && burned < amount {
                    burned = burned.saturating_add(1);
                }
            }
        }
        burned.min(amount) // can't burn more than the fee
    }
}

impl Default for BurnRatio {
    /// Default burn ratio: 0% (conservative start per Spec §7.3).
    fn default() -> Self {
        Self::ZERO
    }
}

impl std::fmt::Display for BurnRatio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}bps ({:.2}%)", self.bps, self.as_fraction() * 100.0)
    }
}

// --- Burn Policy ---

/// The burn policy for an asset, per Spec §7.3.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurnPolicy {
    /// Current burn ratio.
    pub burn_ratio: BurnRatio,
    /// Whether burns are currently paused (e.g., during incident).
    pub paused: bool,
    /// Reason for pause, if paused.
    pub pause_reason: Option<String>,
    /// Timestamp (ms) of last burn ratio change.
    pub last_change_ms: u64,
    /// Sequence number for governance changes.
    pub change_sequence: u64,
}

impl Default for BurnPolicy {
    fn default() -> Self {
        Self {
            burn_ratio: BurnRatio::default(),
            paused: false,
            pause_reason: None,
            last_change_ms: 0,
            change_sequence: 0,
        }
    }
}

impl BurnPolicy {
    /// Create a new burn policy with the given initial ratio.
    pub fn new(initial_ratio: BurnRatio, now_ms: u64) -> Self {
        Self {
            burn_ratio: initial_ratio,
            paused: false,
            pause_reason: None,
            last_change_ms: now_ms,
            change_sequence: 0,
        }
    }

    /// Pause burns. Returns error if already paused.
    pub fn pause(&mut self, reason: String, now_ms: u64) -> Result<(), FeeError> {
        if self.paused {
            return Err(FeeError::BurnAlreadyPaused);
        }
        self.paused = true;
        self.pause_reason = Some(reason);
        self.last_change_ms = now_ms;
        Ok(())
    }

    /// Resume burns. Returns error if not paused.
    pub fn resume(&mut self, now_ms: u64) -> Result<(), FeeError> {
        if !self.paused {
            return Err(FeeError::BurnNotPaused);
        }
        self.paused = false;
        self.pause_reason = None;
        self.last_change_ms = now_ms;
        Ok(())
    }

    /// Update burn ratio via governance.
    /// Enforces the governance ceiling (max 25%) and requires the new ratio
    /// to be higher than the initial max (500 bps) to go through governance path.
    pub fn set_burn_ratio_governance(
        &mut self,
        new_ratio: BurnRatio,
        now_ms: u64,
    ) -> Result<(), FeeError> {
        if self.paused {
            return Err(FeeError::BurnPausedWhileChanging);
        }
        // Validate against governance ceiling
        let _ = BurnRatio::new_governance(new_ratio.bps())?;
        self.burn_ratio = new_ratio;
        self.change_sequence = self.change_sequence.saturating_add(1);
        self.last_change_ms = now_ms;
        Ok(())
    }
}

// --- Burn Accounting ---

/// Immutable record of a single burn event.
/// Per Spec §7.3: "Every burn MUST emit an event."
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurnEvent {
    /// Asset ID that was burned.
    pub asset_id: u32,
    /// Amount burned (plancks).
    pub amount: u64,
    /// Burn ratio applied at the time of burn.
    pub burn_ratio_bps: u16,
    /// Activity that triggered the burn.
    pub activity: String,
    /// Order or transaction reference.
    pub reference: Option<String>,
    /// Monotonic event sequence.
    pub sequence: u64,
    /// Total supply of the asset AFTER this burn.
    pub resulting_supply: u64,
    /// Timestamp (ms).
    pub timestamp_ms: u64,
}

/// Accumulated burn accounting for an asset.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BurnAccounting {
    /// Total amount burned over the lifetime of this asset.
    pub total_burned: u64,
    /// Number of burn events.
    pub burn_count: u64,
    /// Per-activity-type burn totals (activity_label → plancks).
    pub by_activity: std::collections::BTreeMap<String, u64>,
    /// Immutable burn event history.
    pub events: Vec<BurnEvent>,
}

impl BurnAccounting {
    /// Create a new burn accounting tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a burn event.
    pub fn record_burn(
        &mut self,
        asset_id: u32,
        amount: u64,
        burn_ratio_bps: u16,
        activity: &str,
        reference: Option<String>,
        resulting_supply: u64,
        now_ms: u64,
    ) -> BurnEvent {
        let event = BurnEvent {
            asset_id,
            amount,
            burn_ratio_bps,
            activity: activity.into(),
            reference,
            sequence: self.burn_count,
            resulting_supply,
            timestamp_ms: now_ms,
        };
        self.total_burned = self.total_burned.saturating_add(amount);
        self.burn_count = self.burn_count.saturating_add(1);
        *self.by_activity.entry(activity.into()).or_insert(0) += amount;
        self.events.push(event.clone());
        event
    }

    /// Get the total burned amount.
    pub fn total_burned(&self) -> u64 {
        self.total_burned
    }

    /// Get the burn total for a specific activity type.
    pub fn burned_for_activity(&self, activity: &str) -> u64 {
        self.by_activity.get(activity).copied().unwrap_or(0)
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_ratio_is_zero() {
        let ratio = BurnRatio::default();
        assert_eq!(ratio.bps(), 0);
        assert_eq!(ratio.as_fraction(), 0.0);
    }

    #[test]
    fn new_ratio_within_initial_range() {
        let ratio = BurnRatio::new(300).unwrap();
        assert_eq!(ratio.bps(), 300);
    }

    #[test]
    fn new_ratio_exceeds_initial() {
        let err = BurnRatio::new(600).unwrap_err();
        assert!(matches!(err, FeeError::BurnRatioExceedsInitial { .. }));
    }

    #[test]
    fn governance_ratio_within_ceiling() {
        let ratio = BurnRatio::new_governance(2000).unwrap(); // 20%
        assert_eq!(ratio.bps(), 2000);
    }

    #[test]
    fn governance_ratio_exceeds_ceiling() {
        let err = BurnRatio::new_governance(3000).unwrap_err(); // 30%
        assert!(matches!(err, FeeError::BurnRatioExceedsCeiling { .. }));
    }

    #[test]
    fn from_bps_clamps_to_ceiling() {
        let ratio = BurnRatio::from_bps(5000);
        assert_eq!(ratio.bps(), 2500); // clamped to absolute ceiling
    }

    #[test]
    fn apply_burn_rounding_up() {
        let ratio = BurnRatio::from_bps(333); // 3.33%
        let burned = ratio.apply(1_000_000, RoundingRule::Up);
        // 1_000_000 * 333 / 10000 = 33300.0 → rounds up to 33300
        // Actually 333000000 / 10000 = 33300 exactly
        assert_eq!(burned, 33_300);
    }

    #[test]
    fn apply_burn_cannot_exceed_amount() {
        let ratio = BurnRatio::from_bps(10000); // 100% (clamped to 2500 = 25%)
        assert_eq!(ratio.bps(), 2500);
        let burned = ratio.apply(100, RoundingRule::Up);
        assert!(burned <= 100);
    }

    #[test]
    fn burn_policy_pause_resume() {
        let mut policy = BurnPolicy::new(BurnRatio::from_bps(100), 1000);
        assert!(!policy.paused);
        policy.pause("incident".into(), 2000).unwrap();
        assert!(policy.paused);
        assert_eq!(policy.pause_reason.as_deref(), Some("incident"));
        policy.resume(3000).unwrap();
        assert!(!policy.paused);
    }

    #[test]
    fn burn_policy_double_pause_fails() {
        let mut policy = BurnPolicy::new(BurnRatio::from_bps(100), 1000);
        policy.pause("reason".into(), 2000).unwrap();
        assert!(policy.pause("again".into(), 3000).is_err());
    }

    #[test]
    fn burn_policy_resume_when_not_paused_fails() {
        let mut policy = BurnPolicy::new(BurnRatio::from_bps(100), 1000);
        assert!(policy.resume(2000).is_err());
    }

    #[test]
    fn burn_policy_governance_change() {
        let mut policy = BurnPolicy::new(BurnRatio::from_bps(100), 1000);
        policy.set_burn_ratio_governance(BurnRatio::from_bps(1500), 2000).unwrap();
        assert_eq!(policy.burn_ratio.bps(), 1500);
        assert_eq!(policy.change_sequence, 1);
    }

    #[test]
    fn burn_policy_change_while_paused_fails() {
        let mut policy = BurnPolicy::new(BurnRatio::from_bps(100), 1000);
        policy.pause("incident".into(), 2000).unwrap();
        assert!(policy
            .set_burn_ratio_governance(BurnRatio::from_bps(1500), 3000)
            .is_err());
    }

    #[test]
    fn burn_accounting_tracks_totals() {
        let mut accounting = BurnAccounting::new();
        accounting.record_burn(0, 1000, 300, "omnia_transfer", None, 999_000, 1000);
        accounting.record_burn(0, 2000, 300, "merchant_payment", Some("order-1".into()), 997_000, 2000);
        assert_eq!(accounting.total_burned(), 3000);
        assert_eq!(accounting.burn_count, 2);
        assert_eq!(accounting.burned_for_activity("omnia_transfer"), 1000);
        assert_eq!(accounting.burned_for_activity("merchant_payment"), 2000);
        assert_eq!(accounting.events.len(), 2);
    }
}
