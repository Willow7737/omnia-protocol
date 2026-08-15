//! OMNIA fee formula and activity types — Spec §7.1, §7.2
//!
//! ## Fee Formula (§7.2)
//!
//! ```text
//! user_fee = base_fee + priority_fee + applicable_service_fee
//! burned_amount = base_fee × burn_ratio
//! validator_amount = priority_fee + permitted_validator_share_of_base
//! protocol_amount = permitted_treasury_or_operational_share
//! ```
//!
//! ## Activity Types (§7.1)
//!
//! 8 activity types each with defined fee paths.
//! OMNIA MUST NOT be mandatory for basic participation.

use serde::{Deserialize, Serialize};

use crate::burn::BurnRatio;
use crate::error::FeeError;

// --- Activity Types ---

/// Activity types that incur fees, per Spec §7.1.
///
/// Each type has its own fee path and rules about which
/// asset (UBC or OMNIA) is accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ActivityType {
    /// Basic identity operations (DID creation, key rotation).
    /// Fee paid in UBC only. OMNIA NOT required.
    BasicIdentity,
    /// Compute operations (task submission, proof verification).
    /// Fee paid in UBC only.
    Compute,
    /// OMNIA transfer between accounts.
    /// Fee paid in OMNIA. Subject to burn ratio.
    OmniaTransfer,
    /// Priority block inclusion.
    /// Fee paid in OMNIA. Goes entirely to validators.
    PriorityInclusion,
    /// Ghana mobile-money bridge (GHS → OMNIA acquisition).
    /// Fee paid in GHS (provider fee) + OMNIA (protocol fee).
    GhanaMobileMoney,
    /// Merchant payment (OMNIA → merchant).
    /// Fee paid in OMNIA. Subject to burn ratio.
    MerchantPayment,
    /// External-chain operation (BTC, ETH settlement).
    /// Fee paid in the external chain's native asset.
    /// MUST NOT be misrepresented as OMNIA burn.
    ExternalChain,
    /// Governance proposal submission.
    /// Fee paid in OMNIA. Subject to burn ratio.
    GovernanceProposal,
}

impl ActivityType {
    /// Return true if this activity type accepts OMNIA for fees.
    pub fn accepts_omnia(&self) -> bool {
        matches!(
            self,
            ActivityType::OmniaTransfer
                | ActivityType::PriorityInclusion
                | ActivityType::GhanaMobileMoney
                | ActivityType::MerchantPayment
                | ActivityType::GovernanceProposal
        )
    }

    /// Return true if this activity type accepts UBC for fees.
    pub fn accepts_ubc(&self) -> bool {
        matches!(self, ActivityType::BasicIdentity | ActivityType::Compute)
    }

    /// Return true if fees from this activity are subject to the burn ratio.
    /// Only OMNIA base fees are burned.
    pub fn is_burnable(&self) -> bool {
        matches!(
            self,
            ActivityType::OmniaTransfer | ActivityType::MerchantPayment | ActivityType::GovernanceProposal
        )
    }

    /// Return the label for event logging.
    pub fn label(&self) -> &'static str {
        match self {
            Self::BasicIdentity => "basic_identity",
            Self::Compute => "compute",
            Self::OmniaTransfer => "omnia_transfer",
            Self::PriorityInclusion => "priority_inclusion",
            Self::GhanaMobileMoney => "ghana_mobile_money",
            Self::MerchantPayment => "merchant_payment",
            Self::ExternalChain => "external_chain",
            Self::GovernanceProposal => "governance_proposal",
        }
    }
}

// --- Fee Schedule ---

/// OMNIA fee schedule with per-activity-type base fees.
///
/// All amounts are in OMNIA plancks (10^-9 OMNIA) unless noted.
/// UBC fees are in UBC units (separate denomination).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OmniaFeeSchedule {
    /// Base fee for OMNIA transfers (plancks).
    pub omnia_transfer_base: u64,
    /// Base fee for merchant payments (plancks).
    pub merchant_payment_base: u64,
    /// Base fee for governance proposals (plancks).
    pub governance_proposal_base: u64,
    /// Service fee for Ghana mobile-money bridge (OMNIA plancks).
    pub ghana_service_fee: u64,
    /// Maximum base fee (plancks). No single base fee can exceed this.
    pub max_base_fee: u64,
    /// Minimum base fee (plancks). Floor to prevent zero-fee exploitation.
    pub min_base_fee: u64,
    /// UBC fee for basic identity operations.
    pub ubc_identity_fee: u64,
    /// UBC fee for compute operations.
    pub ubc_compute_fee: u64,
    /// Maximum priority fee (plancks).
    pub max_priority_fee: u64,
    /// Percentage of base fee that goes to validators (basis points).
    /// E.g., 8000 = 80% of base goes to validators.
    pub validator_share_bps: u16,
    /// Percentage of base fee that goes to protocol/treasury (basis points).
    pub protocol_share_bps: u16,
    /// Rounding direction for fee calculations.
    pub rounding: RoundingRule,
}

/// Rounding rule for fee calculations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoundingRule {
    /// Round up (user pays slightly more — conservative for the protocol).
    Up,
    /// Round down (user pays slightly less — user-friendly).
    Down,
    /// Round to nearest (standard).
    Nearest,
}

impl Default for OmniaFeeSchedule {
    fn default() -> Self {
        Self::standard()
    }
}

impl OmniaFeeSchedule {
    /// Standard fee schedule per Spec §7.2.
    ///
    /// | Activity | Base Fee (OMNIA) |
    /// |----------|------------------|
    /// | Transfer  | 0.001 OMNIA     |
    /// | Merchant  | 0.001 OMNIA     |
    /// | Governance| 1.0 OMNIA       |
    /// | Ghana svc| 0.0005 OMNIA    |
    pub fn standard() -> Self {
        Self {
            omnia_transfer_base: 1_000_000,          // 0.001 OMNIA
            merchant_payment_base: 1_000_000,        // 0.001 OMNIA
            governance_proposal_base: 1_000_000_000, // 1.0 OMNIA
            ghana_service_fee: 500_000,              // 0.0005 OMNIA
            max_base_fee: 10_000_000_000,            // 10 OMNIA
            min_base_fee: 100_000,                   // 0.0001 OMNIA
            ubc_identity_fee: 2,                     // 2 UBC
            ubc_compute_fee: 5,                      // 5 UBC
            max_priority_fee: 100_000_000,           // 0.1 OMNIA
            validator_share_bps: 8000,               // 80%
            protocol_share_bps: 2000,                // 20%
            rounding: RoundingRule::Up,
        }
    }

    /// Get the base fee for an activity type.
    pub fn base_fee_for(&self, activity: ActivityType) -> u64 {
        match activity {
            ActivityType::BasicIdentity => 0, // UBC only
            ActivityType::Compute => 0,       // UBC only
            ActivityType::OmniaTransfer => self.omnia_transfer_base,
            ActivityType::PriorityInclusion => 0, // priority only
            ActivityType::GhanaMobileMoney => self.ghana_service_fee,
            ActivityType::MerchantPayment => self.merchant_payment_base,
            ActivityType::ExternalChain => 0, // paid in external asset
            ActivityType::GovernanceProposal => self.governance_proposal_base,
        }
    }
}

// --- Fee Calculation ---

/// Result of a fee calculation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeResult {
    /// Total fee paid by the user (plancks for OMNIA, units for UBC).
    pub total_fee: u64,
    /// Amount to be burned (plancks). Only for burnable activities.
    pub burned_amount: u64,
    /// Amount going to validators (plancks).
    pub validator_amount: u64,
    /// Amount going to protocol/treasury (plancks).
    pub protocol_amount: u64,
    /// The activity type.
    pub activity: ActivityType,
    /// Whether this is a UBC-denominated fee.
    pub is_ubc: bool,
}

/// The OMNIA fee formula engine.
///
/// Implements Spec §7.2:
/// ```text
/// user_fee = base_fee + priority_fee + applicable_service_fee
/// burned_amount = base_fee × burn_ratio
/// validator_amount = priority_fee + validator_share_of_base
/// protocol_amount = protocol_share_of_base
/// ```
#[derive(Default)]
pub struct FeeFormula {
    /// The fee schedule.
    pub schedule: OmniaFeeSchedule,
    /// The current burn ratio.
    pub burn_ratio: BurnRatio,
}

impl FeeFormula {
    /// Create a fee formula with the standard schedule and default burn ratio.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a fee formula with a custom schedule and burn ratio.
    pub fn with_params(schedule: OmniaFeeSchedule, burn_ratio: BurnRatio) -> Self {
        Self { schedule, burn_ratio }
    }

    /// Calculate the fee for an activity.
    pub fn calculate(&self, activity: ActivityType, priority_fee: u64) -> Result<FeeResult, FeeError> {
        // UBC-only activities
        if activity.accepts_ubc() && !activity.accepts_omnia() {
            let base = match activity {
                ActivityType::BasicIdentity => self.schedule.ubc_identity_fee,
                ActivityType::Compute => self.schedule.ubc_compute_fee,
                _ => 0,
            };
            return Ok(FeeResult {
                total_fee: base,
                burned_amount: 0,
                validator_amount: base,
                protocol_amount: 0,
                activity,
                is_ubc: true,
            });
        }

        // Priority-only activity
        if activity == ActivityType::PriorityInclusion {
            if priority_fee > self.schedule.max_priority_fee {
                return Err(FeeError::PriorityFeeExceeded {
                    requested: priority_fee,
                    maximum: self.schedule.max_priority_fee,
                });
            }
            return Ok(FeeResult {
                total_fee: priority_fee,
                burned_amount: 0,
                validator_amount: priority_fee,
                protocol_amount: 0,
                activity,
                is_ubc: false,
            });
        }

        // External chain — no OMNIA fee
        if activity == ActivityType::ExternalChain {
            return Ok(FeeResult {
                total_fee: 0,
                burned_amount: 0,
                validator_amount: 0,
                protocol_amount: 0,
                activity,
                is_ubc: false,
            });
        }

        // OMNIA-denominated activities with base + priority + service
        let base_fee = self.schedule.base_fee_for(activity);

        // Enforce min/max
        let base_fee = base_fee.max(self.schedule.min_base_fee).min(self.schedule.max_base_fee);

        // Enforce priority max
        if priority_fee > self.schedule.max_priority_fee {
            return Err(FeeError::PriorityFeeExceeded {
                requested: priority_fee,
                maximum: self.schedule.max_priority_fee,
            });
        }

        // Apply service fee for Ghana
        let service_fee = if activity == ActivityType::GhanaMobileMoney {
            self.schedule.ghana_service_fee
        } else {
            0
        };

        let total_fee = base_fee
            .checked_add(priority_fee)
            .and_then(|v| v.checked_add(service_fee))
            .ok_or(FeeError::ArithmeticOverflow("fee calculation".into()))?;

        // Burn calculation: burned = base_fee × burn_ratio
        let burned_amount = if activity.is_burnable() {
            self.burn_ratio.apply(base_fee, self.schedule.rounding)
        } else {
            0
        };

        // Distribute the NON-BURNED portion of base_fee between validators and protocol.
        // This ensures: burned + validator_from_base + protocol_amount = base_fee
        let distributable_base = base_fee.saturating_sub(burned_amount);

        // Validator share: priority_fee + distributable_base × validator_share_bps / 10000
        let validator_from_base =
            (distributable_base as u128 * self.schedule.validator_share_bps as u128 / 10_000) as u64;
        let validator_amount = priority_fee
            .checked_add(validator_from_base)
            .ok_or(FeeError::ArithmeticOverflow("validator share".into()))?;

        // Protocol share: distributable_base × protocol_share_bps / 10000
        let protocol_amount = (distributable_base as u128 * self.schedule.protocol_share_bps as u128 / 10_000) as u64;

        Ok(FeeResult {
            total_fee,
            burned_amount,
            validator_amount,
            protocol_amount,
            activity,
            is_ubc: false,
        })
    }
}

// --- Trait ---

/// Trait for types that can calculate fees.
/// Enables dependency injection for testing.
pub trait FeeCalculation {
    /// Calculate the fee for the given activity and priority.
    fn calculate_fee(&self, activity: ActivityType, priority_fee: u64) -> Result<FeeResult, FeeError>;
}

impl FeeCalculation for FeeFormula {
    fn calculate_fee(&self, activity: ActivityType, priority_fee: u64) -> Result<FeeResult, FeeError> {
        self.calculate(activity, priority_fee)
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use crate::burn::BurnRatio;

    #[test]
    fn ubc_identity_fee_is_ubc() {
        let formula = FeeFormula::new();
        let result = formula
            .calculate(ActivityType::BasicIdentity, 0)
            .expect("test assertion failed");
        assert!(result.is_ubc);
        assert_eq!(result.total_fee, 2); // standard UBC identity fee
        assert_eq!(result.burned_amount, 0); // UBC not burned
    }

    #[test]
    fn ubc_compute_fee() {
        let formula = FeeFormula::new();
        let result = formula
            .calculate(ActivityType::Compute, 0)
            .expect("test assertion failed");
        assert!(result.is_ubc);
        assert_eq!(result.total_fee, 5);
        assert_eq!(result.burned_amount, 0);
    }

    #[test]
    fn omnia_transfer_with_burn() {
        let formula = FeeFormula::with_params(
            OmniaFeeSchedule::standard(),
            BurnRatio::from_bps(300), // 3%
        );
        let result = formula
            .calculate(ActivityType::OmniaTransfer, 0)
            .expect("test assertion failed");
        assert!(!result.is_ubc);
        assert_eq!(result.total_fee, 1_000_000); // 0.001 OMNIA base
                                                 // burned = 1_000_000 * 300 / 10000 = 30,000
        assert_eq!(result.burned_amount, 30_000);
        // distributable = 1_000_000 - 30,000 = 970,000
        // validator = 970,000 * 8000 / 10000 = 776,000
        assert_eq!(result.validator_amount, 776_000);
        // protocol = 970,000 * 2000 / 10000 = 194,000
        assert_eq!(result.protocol_amount, 194_000);
        // invariant: burned + validator + protocol = 30k + 776k + 194k = 1,000,000 = base
    }

    #[test]
    fn omnia_transfer_with_priority() {
        let formula = FeeFormula::new();
        let result = formula
            .calculate(ActivityType::OmniaTransfer, 50_000_000)
            .expect("test assertion failed");
        assert_eq!(result.total_fee, 51_000_000); // base + priority
                                                  // validator gets priority + share of base
        assert!(result.validator_amount > 50_000_000);
    }

    #[test]
    fn priority_fee_exceeds_max() {
        let formula = FeeFormula::new();
        let err = formula
            .calculate(ActivityType::OmniaTransfer, 200_000_000)
            .expect_err("test assertion failed");
        assert!(matches!(err, FeeError::PriorityFeeExceeded { .. }));
    }

    #[test]
    fn external_chain_no_omnia_fee() {
        let formula = FeeFormula::new();
        let result = formula
            .calculate(ActivityType::ExternalChain, 0)
            .expect("test assertion failed");
        assert_eq!(result.total_fee, 0);
        assert_eq!(result.burned_amount, 0);
    }

    #[test]
    fn ghana_mobile_money_has_service_fee() {
        let formula = FeeFormula::new();
        let result = formula
            .calculate(ActivityType::GhanaMobileMoney, 0)
            .expect("test assertion failed");
        // base (500k) + service (500k) = 1,000,000
        assert_eq!(result.total_fee, 1_000_000);
        // Ghana is not burnable
        assert_eq!(result.burned_amount, 0);
    }

    #[test]
    fn merchant_payment_is_burnable() {
        let formula = FeeFormula::with_params(
            OmniaFeeSchedule::standard(),
            BurnRatio::from_bps(500), // 5%
        );
        let result = formula
            .calculate(ActivityType::MerchantPayment, 0)
            .expect("test assertion failed");
        assert!(result.burned_amount > 0);
        // burned = 1_000_000 * 500 / 10000 = 50,000
        assert_eq!(result.burned_amount, 50_000);
    }

    #[test]
    fn governance_proposal_high_base_fee() {
        let formula = FeeFormula::new();
        let result = formula
            .calculate(ActivityType::GovernanceProposal, 0)
            .expect("test assertion failed");
        assert_eq!(result.total_fee, 1_000_000_000); // 1 OMNIA
    }

    #[test]
    fn fee_separation_enforced() {
        // UBC activities must NOT produce OMNIA fees
        let formula = FeeFormula::new();
        let ubc_result = formula
            .calculate(ActivityType::BasicIdentity, 0)
            .expect("test assertion failed");
        assert!(ubc_result.is_ubc);
        // OMNIA activities must NOT be UBC
        let omnia_result = formula
            .calculate(ActivityType::OmniaTransfer, 0)
            .expect("test assertion failed");
        assert!(!omnia_result.is_ubc);
    }

    #[test]
    fn zero_burn_ratio() {
        let formula = FeeFormula::with_params(OmniaFeeSchedule::standard(), BurnRatio::from_bps(0));
        let result = formula
            .calculate(ActivityType::OmniaTransfer, 0)
            .expect("test assertion failed");
        assert_eq!(result.burned_amount, 0);
        assert_eq!(result.total_fee, 1_000_000);
    }

    #[test]
    fn min_base_fee_enforced() {
        let formula = FeeFormula::with_params(
            OmniaFeeSchedule {
                omnia_transfer_base: 10, // below min
                min_base_fee: 100_000,
                ..OmniaFeeSchedule::standard()
            },
            BurnRatio::default(),
        );
        let result = formula
            .calculate(ActivityType::OmniaTransfer, 0)
            .expect("test assertion failed");
        assert_eq!(result.total_fee, 100_000); // min enforced
    }

    #[test]
    fn activity_type_acceptance() {
        assert!(ActivityType::BasicIdentity.accepts_ubc());
        assert!(!ActivityType::BasicIdentity.accepts_omnia());
        assert!(ActivityType::OmniaTransfer.accepts_omnia());
        assert!(!ActivityType::OmniaTransfer.accepts_ubc());
        assert!(ActivityType::Compute.accepts_ubc());
        assert!(!ActivityType::Compute.accepts_omnia());
        assert!(ActivityType::OmniaTransfer.is_burnable());
        assert!(!ActivityType::GhanaMobileMoney.is_burnable());
        assert!(ActivityType::MerchantPayment.is_burnable());
        assert!(ActivityType::GovernanceProposal.is_burnable());
        assert!(!ActivityType::PriorityInclusion.is_burnable());
    }
}
