//! Genesis allocation and reward schedule — Spec §5.2, §5.3
//!
//! ## Genesis Allocation (§5.2)
//!
//! | Bucket                | Share | Plancks                  |
//! |---                     |---:   |---:                       |
//! | Network incentives     | 40%   | 400,000,000,000,000,000   |
//! | Team and contributors  | 15%   | 150,000,000,000,000,000   |
//! | Early investors/seed   | 10%   | 100,000,000,000,000,000   |
//! | Ecosystem fund         | 15%   | 150,000,000,000,000,000   |
//! | Treasury reserve       | 10%   | 100,000,000,000,000,000   |
//! | Liquidity & market ops | 10%   | 100,000,000,000,000,000   |
//!
//! ## Reward Schedule (§5.3)
//!
//! | Year | OMNIA Reward |
//! |------|-------------|
//! | 1    | 80,000,000  |
//! | 2    | 60,000,000  |
//! | 3    | 45,000,000  |
//! | 4    | 34,000,000  |
//!
//! Remaining 181M needs full schedule after pilot validation.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::supply::SupplyAuthority;
use crate::treasury::{AllocationBucket, VestingSchedule};

// --- Genesis Allocation Record ---

/// A single genesis allocation entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisAllocation {
    /// Destination wallet/account.
    pub recipient: String,
    /// Which bucket this allocation comes from.
    pub bucket: AllocationBucket,
    /// Amount allocated (plancks).
    pub amount: u64,
    /// Vesting schedule (for team bucket; None for immediately available).
    pub vesting: Option<VestingSchedule>,
    /// Purpose/reference description.
    pub purpose: String,
}

/// The complete genesis allocation plan.
/// All allocations that occur at chain genesis.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GenesisPlan {
    /// Individual allocations.
    pub allocations: Vec<GenesisAllocation>,
}

impl GenesisPlan {
    /// Create the canonical genesis plan per Spec §5.2.
    /// Allocates all 1B OMNIA into the 6 buckets.
    /// Individual bucket recipients must be configured separately.
    pub fn canonical() -> Self {
        Self {
            allocations: Vec::new(),
        }
    }

    /// Add an allocation to the plan.
    pub fn allocate(
        &mut self,
        recipient: String,
        bucket: AllocationBucket,
        amount: u64,
        vesting: Option<VestingSchedule>,
        purpose: String,
    ) {
        self.allocations.push(GenesisAllocation {
            recipient,
            bucket,
            amount,
            vesting,
            purpose,
        });
    }

    /// Get total planned allocation per bucket.
    pub fn bucket_totals(&self) -> BTreeMap<AllocationBucket, u64> {
        let mut totals: BTreeMap<AllocationBucket, u64> = BTreeMap::new();
        for alloc in &self.allocations {
            *totals.entry(alloc.bucket).or_insert(0) += alloc.amount;
        }
        totals
    }

    /// Get total planned allocation across all buckets.
    pub fn total_allocation(&self) -> u64 {
        self.allocations.iter().map(|a| a.amount).sum()
    }

    /// Validate that no bucket exceeds its hard cap.
    pub fn validate_bucket_caps(&self) -> Vec<String> {
        let totals = self.bucket_totals();
        let mut violations = Vec::new();
        for (bucket, total) in &totals {
            let cap = bucket.hard_cap();
            if *total > cap {
                violations.push(format!(
                    "bucket {} allocated {} but cap is {}",
                    bucket, total, cap
                ));
            }
        }
        violations
    }
}

// --- Reward Schedule ---

/// Reward schedule per Spec §5.3.
/// Fixed annual rewards for years 1–4; remaining 181M TBD.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardSchedule {
    /// Annual rewards by year (1-indexed). Values in plancks.
    pub annual_rewards: BTreeMap<u32, u64>,
    /// Unclaimed reward treatment policy.
    pub unclaimed_policy: UnclaimedRewardPolicy,
    /// Slashed validator treatment.
    pub slashed_policy: SlashedRewardPolicy,
}

/// What happens to unclaimed rewards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnclaimedRewardPolicy {
    /// Unclaimed rewards return to the treasury.
    ReturnToTreasury,
    /// Unclaimed rewards roll forward to the next period.
    RollForward,
    /// Unclaimed rewards are permanently lost.
    PermanentlyLost,
}

/// What happens to rewards from slashed validators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlashedRewardPolicy {
    /// Slashed validator rewards are burned.
    Burned,
    /// Slashed validator rewards return to the reward pool.
    ReturnToPool,
    /// Slashed validator rewards go to treasury.
    ReturnToTreasury,
}

impl Default for RewardSchedule {
    fn default() -> Self {
        Self::canonical()
    }
}

impl RewardSchedule {
    /// Create the canonical 4-year reward schedule per Spec §5.3.
    /// Plancks: 1 OMNIA = 10^9 plancks.
    pub fn canonical() -> Self {
        let mut annual_rewards = BTreeMap::new();
        // Year 1: 80,000,000 OMNIA
        annual_rewards.insert(1, 80_000_000_000_000_000);
        // Year 2: 60,000,000 OMNIA
        annual_rewards.insert(2, 60_000_000_000_000_000);
        // Year 3: 45,000,000 OMNIA
        annual_rewards.insert(3, 45_000_000_000_000_000);
        // Year 4: 34,000,000 OMNIA
        annual_rewards.insert(4, 34_000_000_000_000_000);

        Self {
            annual_rewards,
            unclaimed_policy: UnclaimedRewardPolicy::ReturnToTreasury,
            slashed_policy: SlashedRewardPolicy::ReturnToPool,
        }
    }

    /// Get the reward for a specific year.
    /// Returns None if the year is not in the schedule.
    pub fn reward_for_year(&self, year: u32) -> Option<u64> {
        self.annual_rewards.get(&year).copied()
    }

    /// Get the total scheduled rewards across all years.
    pub fn total_scheduled(&self) -> u64 {
        self.annual_rewards.values().copied().sum()
    }

    /// Get total scheduled rewards in OMNIA (human-readable).
    pub fn total_scheduled_omnia(&self) -> u64 {
        self.total_scheduled() / 1_000_000_000
    }
}

// --- Issuance Authority (Spec §6.1) ---

/// The 5 issuance authorities per Spec §6.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IssuanceAuthority {
    /// Genesis — initial allocation only.
    Genesis,
    /// Treasury — transfers already-issued OMNIA (does NOT mint).
    Treasury,
    /// Reward — releases approved reward budget.
    Reward,
    /// Governance — bounded parameter changes after timelock.
    Governance,
    /// External adapter — NO native OMNIA minting authority.
    External,
}

impl IssuanceAuthority {
    /// Return true if this authority can mint new OMNIA.
    /// Per Spec §6.1, only Genesis can mint at chain start.
    /// Treasury and Reward TRANSFER already-issued OMNIA.
    pub fn can_mint(&self) -> bool {
        matches!(self, IssuanceAuthority::Genesis)
    }

    /// Return the label for event logging.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Genesis => "genesis",
            Self::Treasury => "treasury",
            Self::Reward => "reward",
            Self::Governance => "governance",
            Self::External => "external",
        }
    }

    /// Map to the SupplyAuthority for supply event tracking.
    pub fn to_supply_authority(&self) -> SupplyAuthority {
        match self {
            Self::Genesis => SupplyAuthority::Genesis,
            Self::Treasury => SupplyAuthority::Treasury,
            Self::Reward => SupplyAuthority::Reward,
            Self::Governance => SupplyAuthority::Governance,
            Self::External => SupplyAuthority::Protocol,
        }
    }
}

// --- Treasury Accounting Categories (Spec §6.3) ---

/// Granular treasury accounting categories per Spec §6.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TreasuryCategory {
    /// Pilot allocation inventory (GHS → OMNIA bridge).
    PilotAllocation,
    /// Liquidity and settlement facility.
    LiquiditySettlement,
    /// Ecosystem grants and partnerships.
    EcosystemGrants,
    /// Operating reserve for protocol operations.
    OperatingReserve,
    /// Locked/vested allocations (team, early investors).
    LockedVested,
    /// Provider fee subsidies.
    ProviderFeeSubsidies,
    /// Refunds and reserved for pending refunds.
    RefundsReserved,
    /// Realized conversion effects.
    RealizedConversion,
    /// Unrealized conversion effects.
    UnrealizedConversion,
    /// All external funds held.
    ExternalFunds,
}

impl TreasuryCategory {
    /// Return the category label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::PilotAllocation => "pilot_allocation",
            Self::LiquiditySettlement => "liquidity_settlement",
            Self::EcosystemGrants => "ecosystem_grants",
            Self::OperatingReserve => "operating_reserve",
            Self::LockedVested => "locked_vested",
            Self::ProviderFeeSubsidies => "provider_fee_subsidies",
            Self::RefundsReserved => "refunds_reserved",
            Self::RealizedConversion => "realized_conversion",
            Self::UnrealizedConversion => "unrealized_conversion",
            Self::ExternalFunds => "external_funds",
        }
    }
}

/// Per-category treasury balance tracking.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TreasuryAccounting {
    /// Balances by category.
    pub category_balances: BTreeMap<String, u64>,
}

impl TreasuryAccounting {
    /// Create a new empty treasury accounting.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the balance for a category.
    pub fn balance(&self, category: &TreasuryCategory) -> u64 {
        self.category_balances
            .get(category.label())
            .copied()
            .unwrap_or(0)
    }

    /// Add to a category balance.
    pub fn credit(&mut self, category: TreasuryCategory, amount: u64) {
        *self
            .category_balances
            .entry(category.label().into())
            .or_insert(0) += amount;
    }

    /// Subtract from a category balance. Saturates at 0.
    pub fn debit(&mut self, category: TreasuryCategory, amount: u64) -> u64 {
        let current = self.balance(&category);
        let actual = amount.min(current);
        *self
            .category_balances
            .entry(category.label().into())
            .or_insert(0) -= actual;
        actual
    }

    /// Total across all categories.
    pub fn total(&self) -> u64 {
        self.category_balances.values().copied().sum()
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genesis_plan_bucket_totals() {
        let mut plan = GenesisPlan::canonical();
        plan.allocate(
            "wallet-1".into(),
            AllocationBucket::NetworkIncentives,
            200_000_000_000_000_000,
            None,
            "initial incentive pool".into(),
        );
        plan.allocate(
            "wallet-2".into(),
            AllocationBucket::NetworkIncentives,
            200_000_000_000_000_000,
            None,
            "reward pool".into(),
        );
        plan.allocate(
            "team-wallet".into(),
            AllocationBucket::Team,
            150_000_000_000_000_000,
            Some(VestingSchedule::standard(150_000_000_000_000_000)),
            "team allocation with 4yr vest".into(),
        );
        let totals = plan.bucket_totals();
        assert_eq!(totals.get(&AllocationBucket::NetworkIncentives), Some(&400_000_000_000_000_000));
        assert_eq!(totals.get(&AllocationBucket::Team), Some(&150_000_000_000_000_000));
        assert_eq!(plan.total_allocation(), 550_000_000_000_000_000);
    }

    #[test]
    fn genesis_plan_bucket_cap_validation() {
        let mut plan = GenesisPlan::canonical();
        plan.allocate(
            "wallet".into(),
            AllocationBucket::Team,
            200_000_000_000_000_000, // exceeds 150M cap
            None,
            "over-allocate".into(),
        );
        let violations = plan.validate_bucket_caps();
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("but cap is"));
    }

    #[test]
    fn reward_schedule_canonical() {
        let schedule = RewardSchedule::canonical();
        assert_eq!(schedule.reward_for_year(1), Some(80_000_000_000_000_000));
        assert_eq!(schedule.reward_for_year(2), Some(60_000_000_000_000_000));
        assert_eq!(schedule.reward_for_year(3), Some(45_000_000_000_000_000));
        assert_eq!(schedule.reward_for_year(4), Some(34_000_000_000_000_000));
        assert_eq!(schedule.reward_for_year(5), None);
        // Total: 219M OMNIA
        assert_eq!(schedule.total_scheduled_omnia(), 219_000_000);
    }

    #[test]
    fn issuance_authority_minting() {
        assert!(IssuanceAuthority::Genesis.can_mint());
        assert!(!IssuanceAuthority::Treasury.can_mint());
        assert!(!IssuanceAuthority::Reward.can_mint());
        assert!(!IssuanceAuthority::Governance.can_mint());
        assert!(!IssuanceAuthority::External.can_mint());
    }

    #[test]
    fn treasury_accounting_categories() {
        let mut acct = TreasuryAccounting::new();
        acct.credit(TreasuryCategory::PilotAllocation, 1_000_000);
        acct.credit(TreasuryCategory::OperatingReserve, 500_000);
        assert_eq!(acct.balance(&TreasuryCategory::PilotAllocation), 1_000_000);
        assert_eq!(acct.balance(&TreasuryCategory::OperatingReserve), 500_000);
        assert_eq!(acct.total(), 1_500_000);

        let debited = acct.debit(TreasuryCategory::PilotAllocation, 300_000);
        assert_eq!(debited, 300_000);
        assert_eq!(acct.balance(&TreasuryCategory::PilotAllocation), 700_000);
        assert_eq!(acct.total(), 1_200_000);
    }

    #[test]
    fn treasury_debit_saturates() {
        let mut acct = TreasuryAccounting::new();
        acct.credit(TreasuryCategory::RefundsReserved, 100);
        let debited = acct.debit(TreasuryCategory::RefundsReserved, 200);
        assert_eq!(debited, 100); // only 100 available
        assert_eq!(acct.balance(&TreasuryCategory::RefundsReserved), 0);
    }
}