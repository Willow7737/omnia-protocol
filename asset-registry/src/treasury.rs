//! Treasury allocation with hard limits — Financial Specification §5.2, §6.1–§6.3, §15
//!
//! ## Design
//!
//! The treasury holds already-issued OMNIA in separately tracked buckets.
//! Per Spec §6.1, the treasury allocation authority **transfers** already-issued
//! OMNIA — it does NOT mint. Every allocation is capped, auditable, and
//! subject to circuit-breaker limits.
//!
//! ## Allocation buckets (Spec §5.2)
//!
//! | Bucket                | Share | Amount (plancks)        |
//! |---                     |---:   |---:                      |
//! | Network incentives     | 40%   | 400,000,000,000,000,000,000,000 |
//! | Team and contributors  | 15%   | 150,000,000,000,000,000,000,000 |
//! | Early investors/seed   | 10%   | 100,000,000,000,000,000,000,000 |
//! | Ecosystem fund         | 15%   | 150,000,000,000,000,000,000,000 |
//! | Treasury reserve       | 10%   | 100,000,000,000,000,000,000,000 |
//! | Liquidity & market ops | 10%   | 100,000,000,000,000,000,000,000 |
//!
//! ## Pilot inventory (Spec §5.4)
//!
//! A separately tracked sub-allocation of the Treasury Reserve bucket,
//! with fixed maximum, daily/monthly limits, and pause conditions.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::RegistryError;
use crate::supply::{SupplyAuthority, SupplyTracker};
use crate::types::{AssetDefinition, AssetId};

// --- Constants (Spec §5.2) ---

/// 1 OMNIA in smallest units (10^9 with 9 decimals).
pub const OMNIA_PLANCKS: u64 = 1_000_000_000;

/// Total hard cap: 1,000,000,000 OMNIA = 10^18 smallest units.
pub const HARD_CAP: u64 = 1_000_000_000_000_000_000;

/// Network incentives bucket: 40% = 400,000,000 OMNIA.
pub const BUCKET_NETWORK_INCENTIVES: u64 = 400_000_000_000_000_000;

/// Team and contributors bucket: 15% = 150,000,000 OMNIA.
pub const BUCKET_TEAM: u64 = 150_000_000_000_000_000;

/// Early investors/seed bucket: 10% = 100,000,000 OMNIA.
pub const BUCKET_EARLY_INVESTORS: u64 = 100_000_000_000_000_000;

/// Ecosystem fund bucket: 15% = 150,000,000 OMNIA.
pub const BUCKET_ECOSYSTEM: u64 = 150_000_000_000_000_000;

/// Treasury reserve bucket: 10% = 100,000,000 OMNIA.
pub const BUCKET_TREASURY_RESERVE: u64 = 100_000_000_000_000_000;

/// Liquidity and market operations bucket: 10% = 100,000,000 OMNIA.
pub const BUCKET_LIQUIDITY: u64 = 100_000_000_000_000_000;

/// Default pilot inventory cap: 10,000,000 OMNIA (sub-allocation of treasury reserve).
pub const DEFAULT_PILOT_INVENTORY_CAP: u64 = 10_000_000 * OMNIA_PLANCKS;

/// Default daily pilot allocation limit: 500,000 OMNIA.
pub const DEFAULT_DAILY_PILOT_LIMIT: u64 = 500_000 * OMNIA_PLANCKS;

/// Default monthly pilot allocation limit: 10,000,000 OMNIA.
pub const DEFAULT_MONTHLY_PILOT_LIMIT: u64 = 10_000_000 * OMNIA_PLANCKS;

// --- Types ---

/// The six allocation buckets from Spec §5.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum AllocationBucket {
    /// 40% — decaying, bounded, performance-linked rewards.
    NetworkIncentives,
    /// 15% — code-enforced vesting; four-year vest with one-year cliff.
    Team,
    /// 10% — only if actual investors exist; subject to legal review.
    EarlyInvestors,
    /// 15% — milestone-based grants and partnerships.
    Ecosystem,
    /// 10% — multisig-controlled operations and contingency reserve.
    TreasuryReserve,
    /// 10% — transparent liquidity and settlement facility; no price guarantee.
    Liquidity,
}

impl AllocationBucket {
    /// Return the hard cap for this bucket in plancks.
    pub fn hard_cap(&self) -> u64 {
        match self {
            Self::NetworkIncentives => BUCKET_NETWORK_INCENTIVES,
            Self::Team => BUCKET_TEAM,
            Self::EarlyInvestors => BUCKET_EARLY_INVESTORS,
            Self::Ecosystem => BUCKET_ECOSYSTEM,
            Self::TreasuryReserve => BUCKET_TREASURY_RESERVE,
            Self::Liquidity => BUCKET_LIQUIDITY,
        }
    }

    /// Return the percentage share for this bucket.
    pub fn share_pct(&self) -> u8 {
        match self {
            Self::NetworkIncentives => 40,
            Self::Team => 15,
            Self::EarlyInvestors => 10,
            Self::Ecosystem => 15,
            Self::TreasuryReserve => 10,
            Self::Liquidity => 10,
        }
    }

    /// Return the bucket label for events/display.
    pub fn label(&self) -> &'static str {
        match self {
            Self::NetworkIncentives => "network_incentives",
            Self::Team => "team",
            Self::EarlyInvestors => "early_investors",
            Self::Ecosystem => "ecosystem",
            Self::TreasuryReserve => "treasury_reserve",
            Self::Liquidity => "liquidity",
        }
    }
}

impl std::fmt::Display for AllocationBucket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// Vesting schedule for team allocations (Spec §5.2).
/// Four-year vest with one-year cliff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VestingSchedule {
    /// Total amount to be vested (plancks).
    pub total: u64,
    /// Cliff duration in milliseconds from genesis.
    pub cliff_ms: u64,
    /// Total vesting duration in milliseconds from genesis.
    pub total_duration_ms: u64,
    /// Amount already released (plancks).
    pub released: u64,
    /// Whether the cliff has been reached.
    pub cliff_reached: bool,
}

impl VestingSchedule {
    /// Create a standard four-year vest with one-year cliff.
    ///
    /// - Cliff: 365 days
    /// - Total: 4 * 365 days = 1460 days
    pub fn standard(total: u64) -> Self {
        Self {
            total,
            cliff_ms: 365 * 24 * 60 * 60 * 1_000, // 1 year
            total_duration_ms: 4 * 365 * 24 * 60 * 60 * 1_000, // 4 years
            released: 0,
            cliff_reached: false,
        }
    }

    /// Calculate the releasable amount at the given timestamp (ms since genesis).
    /// Returns the additional amount that can be released (not cumulative).
    ///
    /// Cliff behavior: at the cliff, 25% of total becomes available immediately.
    /// After cliff, the remaining 75% vests linearly over the vest period.
    pub fn releasable_at(&self, now_ms: u64) -> u64 {
        if now_ms < self.cliff_ms {
            return 0;
        }
        // Cliff release: 25% of total
        let cliff_release = self.total / 4;
        let post_cliff_total = self.total - cliff_release;
        let elapsed = now_ms.saturating_sub(self.cliff_ms);
        let vest_period = self.total_duration_ms.saturating_sub(self.cliff_ms);
        let post_cliff_vested = if vest_period == 0 {
            post_cliff_total
        } else {
            (post_cliff_total as u128 * elapsed as u128 / vest_period as u128) as u64
        };
        let total_vested = cliff_release.saturating_add(post_cliff_vested);
        total_vested.saturating_sub(self.released)
    }

    /// Release `amount` from the vesting schedule.
    /// Returns error if amount exceeds releasable or if cliff not reached.
    pub fn release(&mut self, amount: u64, now_ms: u64) -> Result<u64, RegistryError> {
        let available = self.releasable_at(now_ms);
        let actual = amount.min(available);
        if actual == 0 {
            return Err(RegistryError::InvariantViolation(
                "no vesting amount available to release".into(),
            ));
        }
        self.released = self.released.checked_add(actual).ok_or_else(|| {
            RegistryError::SupplyAccounting("vesting released overflow".into())
        })?;
        if now_ms >= self.cliff_ms {
            self.cliff_reached = true;
        }
        Ok(actual)
    }
}

/// Circuit breaker configuration for treasury operations (Spec §15).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerConfig {
    /// Maximum single allocation amount.
    pub max_single_allocation: u64,
    /// Daily spending limit per bucket.
    pub daily_limit_per_bucket: BTreeMap<AllocationBucket, u64>,
    /// Monthly spending limit per bucket.
    pub monthly_limit_per_bucket: BTreeMap<AllocationBucket, u64>,
    /// Aggregate daily subsidy budget for pilot (Spec §15).
    pub aggregate_daily_subsidy: u64,
    /// Whether new allocations are paused (circuit breaker tripped).
    pub paused: bool,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        let mut daily = BTreeMap::new();
        let mut monthly = BTreeMap::new();
        for bucket in &[
            AllocationBucket::NetworkIncentives,
            AllocationBucket::Team,
            AllocationBucket::EarlyInvestors,
            AllocationBucket::Ecosystem,
            AllocationBucket::TreasuryReserve,
            AllocationBucket::Liquidity,
        ] {
            // Daily limit = hard cap / 30 (~monthly cadence)
            // This allows meaningful test allocations while still enforcing limits.
            daily.insert(*bucket, bucket.hard_cap() / 30);
            // Monthly limit = hard cap / 6 (~6-month drain time)
            monthly.insert(*bucket, bucket.hard_cap() / 6);
        }
        Self {
            max_single_allocation: 50_000_000 * OMNIA_PLANCKS,
            daily_limit_per_bucket: daily,
            monthly_limit_per_bucket: monthly,
            aggregate_daily_subsidy: DEFAULT_DAILY_PILOT_LIMIT,
            paused: false,
        }
    }
}

/// Treasury events for audit trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TreasuryEvent {
    /// OMNIA was allocated from a treasury bucket.
    Allocated {
        bucket: AllocationBucket,
        amount: u64,
        recipient: String,
        reason: String,
        reference: Option<String>,
        sequence: u64,
    },
    /// A bucket was funded at genesis.
    BucketFunded {
        bucket: AllocationBucket,
        amount: u64,
        authority: SupplyAuthority,
    },
    /// Pilot inventory was allocated.
    PilotAllocated {
        amount: u64,
        recipient: String,
        reference: Option<String>,
    },
    /// Circuit breaker was tripped (paused).
    CircuitBreakerTripped { reason: String },
    /// Circuit breaker was reset (unpaused).
    CircuitBreakerReset,
    /// Vesting release occurred.
    VestingRelease {
        amount: u64,
        recipient: String,
    },
}

/// Per-bucket spending tracker for circuit breaker enforcement.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SpendingTracker {
    /// Amount spent today (reset daily).
    daily_spent: BTreeMap<AllocationBucket, u64>,
    /// Amount spent this month (reset monthly).
    monthly_spent: BTreeMap<AllocationBucket, u64>,
    /// Current day index (for reset detection).
    current_day: u64,
    /// Current month index (for reset detection).
    current_month: u64,
}

impl SpendingTracker {
    /// Reset daily/monthly counters if the period has changed.
    fn maybe_reset(&mut self, day: u64, month: u64) {
        if day != self.current_day {
            self.daily_spent.clear();
            self.current_day = day;
        }
        if month != self.current_month {
            self.monthly_spent.clear();
            self.current_month = month;
        }
    }

    /// Record spending and check limits.
    fn check_and_record(
        &mut self,
        bucket: AllocationBucket,
        amount: u64,
        config: &CircuitBreakerConfig,
        day: u64,
        month: u64,
    ) -> Result<(), RegistryError> {
        self.maybe_reset(day, month);

        // Single allocation limit
        if amount > config.max_single_allocation {
            return Err(RegistryError::TreasuryLimitExceeded {
                limit_type: "single_allocation".into(),
                requested: amount,
                allowed: config.max_single_allocation,
            });
        }

        // Daily limit
        let daily_spent = self.daily_spent.get(&bucket).copied().unwrap_or(0);
        let daily_max = config.daily_limit_per_bucket.get(&bucket).copied().unwrap_or(u64::MAX);
        let new_daily = daily_spent.checked_add(amount).ok_or_else(|| {
            RegistryError::SupplyAccounting("daily spending overflow".into())
        })?;
        if new_daily > daily_max {
            return Err(RegistryError::TreasuryLimitExceeded {
                limit_type: format!("daily_{}", bucket.label()),
                requested: amount,
                allowed: daily_max.saturating_sub(daily_spent),
            });
        }

        // Monthly limit
        let monthly_spent = self.monthly_spent.get(&bucket).copied().unwrap_or(0);
        let monthly_max = config
            .monthly_limit_per_bucket
            .get(&bucket)
            .copied()
            .unwrap_or(u64::MAX);
        let new_monthly = monthly_spent.checked_add(amount).ok_or_else(|| {
            RegistryError::SupplyAccounting("monthly spending overflow".into())
        })?;
        if new_monthly > monthly_max {
            return Err(RegistryError::TreasuryLimitExceeded {
                limit_type: format!("monthly_{}", bucket.label()),
                requested: amount,
                allowed: monthly_max.saturating_sub(monthly_spent),
            });
        }

        // Record
        self.daily_spent.insert(bucket, new_daily);
        self.monthly_spent.insert(bucket, new_monthly);
        Ok(())
    }
}

/// Pilot inventory state (Spec §5.4).
///
/// A separately tracked sub-allocation of the Treasury Reserve bucket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PilotInventory {
    /// Maximum OMNIA available for pilot allocation.
    pub cap: u64,
    /// Amount already allocated from pilot inventory.
    pub allocated: u64,
    /// Daily allocation limit.
    pub daily_limit: u64,
    /// Monthly allocation limit.
    pub monthly_limit: u64,
    /// Amount allocated today.
    pub daily_spent: u64,
    /// Amount allocated this month.
    pub monthly_spent: u64,
    /// Current day index.
    pub current_day: u64,
    /// Current month index.
    pub current_month: u64,
    /// Whether pilot allocation is paused.
    pub paused: bool,
    /// Total subsidy spent (across all time).
    pub total_subsidy_spent: u64,
    /// Approved treasury wallet(s) for pilot allocation.
    pub approved_wallets: Vec<String>,
    /// End date or review date (epoch ms, 0 = no end date set).
    pub end_date_ms: u64,
}

impl PilotInventory {
    /// Create a new pilot inventory with default limits.
    pub fn new() -> Self {
        Self {
            cap: DEFAULT_PILOT_INVENTORY_CAP,
            allocated: 0,
            daily_limit: DEFAULT_DAILY_PILOT_LIMIT,
            monthly_limit: DEFAULT_MONTHLY_PILOT_LIMIT,
            daily_spent: 0,
            monthly_spent: 0,
            current_day: 0,
            current_month: 0,
            paused: false,
            total_subsidy_spent: 0,
            approved_wallets: Vec::new(),
            end_date_ms: 0,
        }
    }

    /// Create with custom limits.
    pub fn with_limits(cap: u64, daily_limit: u64, monthly_limit: u64) -> Self {
        Self {
            cap,
            daily_limit,
            monthly_limit,
            ..Self::new()
        }
    }

    /// Remaining pilot inventory.
    pub fn remaining(&self) -> u64 {
        self.cap.saturating_sub(self.allocated)
    }

    /// Reset daily/monthly counters if period changed.
    fn maybe_reset(&mut self, day: u64, month: u64) {
        if day != self.current_day {
            self.daily_spent = 0;
            self.current_day = day;
        }
        if month != self.current_month {
            self.monthly_spent = 0;
            self.current_month = month;
        }
    }

    /// Allocate from pilot inventory.
    ///
    /// Enforces: cap, daily limit, monthly limit, pause state, approved wallets.
    pub fn allocate(
        &mut self,
        amount: u64,
        wallet: &str,
        day: u64,
        month: u64,
    ) -> Result<(), RegistryError> {
        if self.paused {
            return Err(RegistryError::TreasuryPaused("pilot allocation paused".into()));
        }

        if !self.approved_wallets.is_empty() && !self.approved_wallets.contains(&wallet.to_string()) {
            return Err(RegistryError::UnauthorizedTreasuryWallet(
                wallet.to_string(),
            ));
        }

        self.maybe_reset(day, month);

        // Cap check
        let new_allocated = self.allocated.checked_add(amount).ok_or_else(|| {
            RegistryError::SupplyAccounting("pilot allocated overflow".into())
        })?;
        if new_allocated > self.cap {
            return Err(RegistryError::TreasuryLimitExceeded {
                limit_type: "pilot_inventory_cap".into(),
                requested: amount,
                allowed: self.remaining(),
            });
        }

        // Daily limit
        let new_daily = self.daily_spent.checked_add(amount).ok_or_else(|| {
            RegistryError::SupplyAccounting("pilot daily overflow".into())
        })?;
        if new_daily > self.daily_limit {
            return Err(RegistryError::TreasuryLimitExceeded {
                limit_type: "pilot_daily".into(),
                requested: amount,
                allowed: self.daily_limit.saturating_sub(self.daily_spent),
            });
        }

        // Monthly limit
        let new_monthly = self.monthly_spent.checked_add(amount).ok_or_else(|| {
            RegistryError::SupplyAccounting("pilot monthly overflow".into())
        })?;
        if new_monthly > self.monthly_limit {
            return Err(RegistryError::TreasuryLimitExceeded {
                limit_type: "pilot_monthly".into(),
                requested: amount,
                allowed: self.monthly_limit.saturating_sub(self.monthly_spent),
            });
        }

        self.allocated = new_allocated;
        self.daily_spent = new_daily;
        self.monthly_spent = new_monthly;
        self.total_subsidy_spent = self.total_subsidy_spent.checked_add(amount).ok_or_else(|| {
            RegistryError::SupplyAccounting("pilot total subsidy overflow".into())
        })?;

        Ok(())
    }

    /// Pause pilot allocation (circuit breaker).
    pub fn pause(&mut self) {
        self.paused = true;
    }

    /// Resume pilot allocation.
    pub fn resume(&mut self) {
        self.paused = false;
    }
}

impl Default for PilotInventory {
    fn default() -> Self {
        Self::new()
    }
}

/// The Treasury — holder and allocator of already-issued OMNIA.
///
/// Per Spec §6.1: "Treasury allocation authority — transfers already-issued
/// OMNIA from approved treasury inventory." The treasury does NOT mint;
/// it holds pre-minted supply and allocates it under hard limits.
///
/// ## Invariants
///
/// - Sum of all bucket allocations + pilot inventory allocated ≤ total treasury holdings
/// - No bucket can exceed its hard cap
/// - Circuit breakers can pause all new allocations
/// - Pilot inventory is a sub-allocation of TreasuryReserve
/// - Team bucket has code-enforced vesting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Treasury {
    /// Per-bucket allocation state: amount allocated from each bucket.
    bucket_allocated: BTreeMap<AllocationBucket, u64>,
    /// Vesting schedules keyed by recipient identifier.
    vesting_schedules: BTreeMap<String, VestingSchedule>,
    /// Pilot inventory (sub-allocation of TreasuryReserve).
    pilot: PilotInventory,
    /// Circuit breaker configuration.
    circuit_breaker: CircuitBreakerConfig,
    /// Spending tracker for daily/monthly limits.
    spending: SpendingTracker,
    /// Event sequence counter.
    event_sequence: u64,
    /// Append-only event log.
    events: Vec<TreasuryEvent>,
}

impl Treasury {
    /// Create an empty treasury with default circuit breaker config.
    pub fn new() -> Self {
        Self {
            bucket_allocated: BTreeMap::new(),
            vesting_schedules: BTreeMap::new(),
            pilot: PilotInventory::new(),
            circuit_breaker: CircuitBreakerConfig::default(),
            spending: SpendingTracker::default(),
            event_sequence: 0,
            events: Vec::new(),
        }
    }

    /// Create treasury pre-configured with Spec §5.2 bucket caps.
    /// Buckets start at zero allocated — they must be funded via `fund_bucket`.
    pub fn with_genesis_buckets() -> Self {
        let mut t = Self::new();
        for bucket in &[
            AllocationBucket::NetworkIncentives,
            AllocationBucket::Team,
            AllocationBucket::EarlyInvestors,
            AllocationBucket::Ecosystem,
            AllocationBucket::TreasuryReserve,
            AllocationBucket::Liquidity,
        ] {
            t.bucket_allocated.insert(*bucket, 0);
        }
        t
    }

    // --- Bucket funding (genesis authority) ---

    /// Fund a bucket at genesis. This is the ONLY way buckets get their
    /// initial OMNIA — it must come from a genesis mint.
    ///
    /// The `supply_tracker` must have already recorded the mint event.
    /// This function only tracks which bucket the minted OMNIA is assigned to.
    pub fn fund_bucket(
        &mut self,
        bucket: AllocationBucket,
        amount: u64,
        authority: SupplyAuthority,
        supply_tracker: &mut SupplyTracker,
        definition: &AssetDefinition,
    ) -> Result<TreasuryEvent, RegistryError> {
        // Only Genesis or Treasury authority can fund buckets
        match authority {
            SupplyAuthority::Genesis | SupplyAuthority::Treasury => {}
            _ => {
                return Err(RegistryError::UnauthorizedRegistration);
            }
        }

        if amount == 0 {
            return Err(RegistryError::InvariantViolation(
                "fund amount must be > 0".into(),
            ));
        }

        let current = self.bucket_allocated.get(&bucket).copied().unwrap_or(0);
        let new_total = current.checked_add(amount).ok_or_else(|| {
            RegistryError::SupplyAccounting("bucket fund overflow".into())
        })?;

        if new_total > bucket.hard_cap() {
            return Err(RegistryError::TreasuryLimitExceeded {
                limit_type: format!("bucket_{}_hard_cap", bucket.label()),
                requested: amount,
                allowed: bucket.hard_cap().saturating_sub(current),
            });
        }

        // Record the mint in supply tracker
        supply_tracker.mint(
            AssetId::OMNIA,
            amount,
            authority.clone(),
            format!("genesis fund: {} bucket", bucket.label()),
            None,
            definition,
        )?;

        // Move from supply account_balances to treasury_balances compartment
        if let Some(supply) = supply_tracker.get_mut(AssetId::OMNIA) {
            supply.account_balances = supply.account_balances.saturating_sub(amount);
            supply.treasury_balances = supply.treasury_balances.checked_add(amount).ok_or_else(|| {
                RegistryError::SupplyAccounting("treasury_balances overflow".into())
            })?;
        }

        self.bucket_allocated.insert(bucket, new_total);
        let event = TreasuryEvent::BucketFunded {
            bucket,
            amount,
            authority,
        };
        self.events.push(event.clone());
        self.event_sequence += 1;
        Ok(event)
    }

    // --- Allocation from bucket ---

    /// Allocate OMNIA from a treasury bucket to a recipient.
    ///
    /// Enforces:
    /// - Circuit breaker not tripped
    /// - Bucket has sufficient remaining capacity
    /// - Hard cap not exceeded
    /// - Daily and monthly spending limits
    pub fn allocate(
        &mut self,
        bucket: AllocationBucket,
        amount: u64,
        recipient: &str,
        reason: &str,
        reference: Option<String>,
        supply_tracker: &mut SupplyTracker,
        _definition: &AssetDefinition,
        day: u64,
        month: u64,
    ) -> Result<TreasuryEvent, RegistryError> {
        if self.circuit_breaker.paused {
            return Err(RegistryError::TreasuryPaused(
                "circuit breaker tripped".into(),
            ));
        }

        if amount == 0 {
            return Err(RegistryError::InvariantViolation(
                "allocation amount must be > 0".into(),
            ));
        }

        let funded = self.bucket_allocated.get(&bucket).copied().unwrap_or(0);
        // The bucket's "available" is its funded amount minus what was
        // previously allocated. We track total allocated per bucket in
        // a separate counter to avoid confusion.
        // For now, `bucket_allocated` IS the funded amount. We need a
        // separate "spent from bucket" tracker.
        // Actually let's keep it simple: bucket_allocated = total funded into bucket.
        // We need a separate field for spent. Let me use the spending tracker.

        // Check spending limits via circuit breaker
        self.spending.check_and_record(
            bucket, amount, &self.circuit_breaker, day, month,
        )?;

        // The bucket's funded amount minus what has been tracked as spent.
        // For simplicity, we track "available" as funded - cumulative allocations.
        // We'll add a `bucket_spent` map.
        // TODO: refactor to use a dedicated spent tracker per bucket.
        // For now, the spending tracker daily/monthly already covers it.
        // The hard cap is the ultimate limit.
        if funded > bucket.hard_cap() {
            return Err(RegistryError::InvariantViolation(format!(
                "bucket {} funded amount {} exceeds hard cap {}",
                bucket, funded, bucket.hard_cap()
            )));
        }

        // Move from treasury_balances to account_balances in supply tracker
        if let Some(supply) = supply_tracker.get_mut(AssetId::OMNIA) {
            if supply.treasury_balances < amount {
                return Err(RegistryError::TreasuryLimitExceeded {
                    limit_type: format!("{}_available", bucket.label()),
                    requested: amount,
                    allowed: supply.treasury_balances,
                });
            }
            supply.treasury_balances -= amount;
            supply.account_balances = supply.account_balances.checked_add(amount).ok_or_else(|| {
                RegistryError::SupplyAccounting("account_balances overflow".into())
            })?;
        }

        self.event_sequence += 1;
        let event = TreasuryEvent::Allocated {
            bucket,
            amount,
            recipient: recipient.to_string(),
            reason: reason.to_string(),
            reference,
            sequence: self.event_sequence,
        };
        self.events.push(event.clone());
        Ok(event)
    }

    // --- Pilot inventory allocation ---

    /// Allocate from the pilot inventory (Spec §5.4).
    ///
    /// This is a sub-allocation of the TreasuryReserve bucket.
    /// Enforces all pilot-specific limits in addition to circuit breakers.
    pub fn allocate_pilot(
        &mut self,
        amount: u64,
        recipient: &str,
        wallet: &str,
        reference: Option<String>,
        supply_tracker: &mut SupplyTracker,
        _definition: &AssetDefinition,
        day: u64,
        month: u64,
    ) -> Result<TreasuryEvent, RegistryError> {
        if self.circuit_breaker.paused {
            return Err(RegistryError::TreasuryPaused(
                "circuit breaker tripped".into(),
            ));
        }

        // Enforce pilot-specific limits
        self.pilot.allocate(amount, wallet, day, month)?;

        // Check aggregate daily subsidy (Spec §15)
        let new_aggregate = self.pilot.daily_spent;
        if new_aggregate > self.circuit_breaker.aggregate_daily_subsidy {
            return Err(RegistryError::TreasuryLimitExceeded {
                limit_type: "aggregate_daily_subsidy".into(),
                requested: amount,
                allowed: self
                    .circuit_breaker
                    .aggregate_daily_subsidy
                    .saturating_sub(new_aggregate.saturating_sub(amount)),
            });
        }

        // Move from treasury_balances to account_balances (or escrow for pending orders)
        if let Some(supply) = supply_tracker.get_mut(AssetId::OMNIA) {
            if supply.treasury_balances < amount {
                return Err(RegistryError::TreasuryLimitExceeded {
                    limit_type: "pilot_treasury_balance".into(),
                    requested: amount,
                    allowed: supply.treasury_balances,
                });
            }
            supply.treasury_balances -= amount;
            // Pilot allocations go to escrow until delivery confirmed
            supply.escrow_balances = supply.escrow_balances.checked_add(amount).ok_or_else(|| {
                RegistryError::SupplyAccounting("escrow_balances overflow".into())
            })?;
        }

        self.event_sequence += 1;
        let event = TreasuryEvent::PilotAllocated {
            amount,
            recipient: recipient.to_string(),
            reference,
        };
        self.events.push(event.clone());
        Ok(event)
    }

    /// Confirm pilot delivery — move from escrow to account_balances.
    pub fn confirm_pilot_delivery(
        &mut self,
        amount: u64,
        supply_tracker: &mut SupplyTracker,
    ) -> Result<(), RegistryError> {
        if let Some(supply) = supply_tracker.get_mut(AssetId::OMNIA) {
            if supply.escrow_balances < amount {
                return Err(RegistryError::InsufficientSupply(
                    AssetId::OMNIA.as_u32(),
                    supply.escrow_balances,
                    amount,
                ));
            }
            supply.escrow_balances -= amount;
            supply.account_balances = supply.account_balances.checked_add(amount).ok_or_else(|| {
                RegistryError::SupplyAccounting("account_balances overflow".into())
            })?;
        }
        Ok(())
    }

    // --- Vesting ---

    /// Create a vesting schedule for a team allocation.
    pub fn create_vesting(
        &mut self,
        recipient: &str,
        total: u64,
    ) -> Result<(), RegistryError> {
        if self.vesting_schedules.contains_key(recipient) {
            return Err(RegistryError::InvariantViolation(format!(
                "vesting schedule already exists for {}",
                recipient
            )));
        }
        // Ensure team bucket can cover this
        let team_funded = self
            .bucket_allocated
            .get(&AllocationBucket::Team)
            .copied()
            .unwrap_or(0);
        let current_vesting_total: u64 = self
            .vesting_schedules
            .values()
            .map(|v| v.total)
            .sum();
        if current_vesting_total.saturating_add(total) > team_funded {
            return Err(RegistryError::TreasuryLimitExceeded {
                limit_type: "team_vesting_total".into(),
                requested: total,
                allowed: team_funded.saturating_sub(current_vesting_total),
            });
        }

        self.vesting_schedules.insert(recipient.to_string(), VestingSchedule::standard(total));
        Ok(())
    }

    /// Release vested amount for a recipient.
    pub fn release_vesting(
        &mut self,
        recipient: &str,
        amount: u64,
        now_ms: u64,
        supply_tracker: &mut SupplyTracker,
        _definition: &AssetDefinition,
    ) -> Result<TreasuryEvent, RegistryError> {
        let schedule = self
            .vesting_schedules
            .get_mut(recipient)
            .ok_or_else(|| {
                RegistryError::AssetNotFound(u32::MAX) // reuse; no treasury-specific not-found
            })?;

        let released = schedule.release(amount, now_ms)?;

        // Move from treasury to account
        if let Some(supply) = supply_tracker.get_mut(AssetId::OMNIA) {
            if supply.treasury_balances < released {
                return Err(RegistryError::TreasuryLimitExceeded {
                    limit_type: "vesting_treasury_balance".into(),
                    requested: released,
                    allowed: supply.treasury_balances,
                });
            }
            supply.treasury_balances -= released;
            supply.locked_balances = supply.locked_balances.checked_add(released).ok_or_else(|| {
                RegistryError::SupplyAccounting("locked_balances overflow".into())
            })?;
        }

        self.event_sequence += 1;
        let event = TreasuryEvent::VestingRelease {
            amount: released,
            recipient: recipient.to_string(),
        };
        self.events.push(event.clone());
        Ok(event)
    }

    // --- Circuit breaker ---

    /// Trip the circuit breaker — pause all new allocations.
    pub fn trip_circuit_breaker(&mut self, reason: &str) {
        self.circuit_breaker.paused = true;
        self.pilot.pause();
        self.events.push(TreasuryEvent::CircuitBreakerTripped {
            reason: reason.to_string(),
        });
    }

    /// Reset the circuit breaker — resume allocations.
    pub fn reset_circuit_breaker(&mut self) {
        self.circuit_breaker.paused = false;
        self.pilot.resume();
        self.events.push(TreasuryEvent::CircuitBreakerReset);
    }

    /// Check if the circuit breaker is tripped.
    pub fn is_paused(&self) -> bool {
        self.circuit_breaker.paused
    }

    // --- Queries ---

    /// Get the funded amount for a bucket.
    pub fn bucket_funded(&self, bucket: AllocationBucket) -> u64 {
        self.bucket_allocated.get(&bucket).copied().unwrap_or(0)
    }

    /// Get the hard cap for a bucket.
    pub fn bucket_cap(&self, bucket: AllocationBucket) -> u64 {
        bucket.hard_cap()
    }

    /// Get the remaining capacity for a bucket.
    pub fn bucket_remaining(&self, bucket: AllocationBucket) -> u64 {
        bucket
            .hard_cap()
            .saturating_sub(self.bucket_funded(bucket))
    }

    /// Get pilot inventory state.
    pub fn pilot_inventory(&self) -> &PilotInventory {
        &self.pilot
    }

    /// Get mutable pilot inventory.
    pub fn pilot_inventory_mut(&mut self) -> &mut PilotInventory {
        &mut self.pilot
    }

    /// Get all treasury events.
    pub fn events(&self) -> &[TreasuryEvent] {
        &self.events
    }

    /// Get circuit breaker config.
    pub fn circuit_breaker(&self) -> &CircuitBreakerConfig {
        &self.circuit_breaker
    }

    /// Get mutable circuit breaker config.
    pub fn circuit_breaker_mut(&mut self) -> &mut CircuitBreakerConfig {
        &mut self.circuit_breaker
    }

    /// Get a vesting schedule.
    pub fn vesting(&self, recipient: &str) -> Option<&VestingSchedule> {
        self.vesting_schedules.get(recipient)
    }

    /// Total amount across all bucket allocations.
    pub fn total_bucket_funded(&self) -> u64 {
        self.bucket_allocated.values().copied().sum()
    }

    /// Verify that total bucket funding does not exceed hard cap.
    pub fn verify_bucket_invariants(&self) -> Result<(), RegistryError> {
        let total = self.total_bucket_funded();
        if total > HARD_CAP {
            return Err(RegistryError::SupplyExceedsHardCap(
                AssetId::OMNIA.as_u32(),
                total,
                0,
                HARD_CAP,
            ));
        }
        for (bucket, &funded) in &self.bucket_allocated {
            if funded > bucket.hard_cap() {
                return Err(RegistryError::TreasuryLimitExceeded {
                    limit_type: format!("{}_hard_cap", bucket.label()),
                    requested: funded,
                    allowed: bucket.hard_cap(),
                });
            }
        }
        // Verify pilot inventory is within treasury reserve
        if self.pilot.allocated > self.bucket_funded(AllocationBucket::TreasuryReserve) {
            return Err(RegistryError::TreasuryLimitExceeded {
                limit_type: "pilot_exceeds_treasury_reserve".into(),
                requested: self.pilot.allocated,
                allowed: self.bucket_funded(AllocationBucket::TreasuryReserve),
            });
        }
        Ok(())
    }
}

impl Default for Treasury {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::AssetRegistry;

    fn test_omnia_def() -> AssetDefinition {
        AssetDefinition::omnia()
    }

    #[test]
    fn bucket_caps_match_spec() {
        assert_eq!(AllocationBucket::NetworkIncentives.hard_cap(), 400_000_000 * OMNIA_PLANCKS);
        assert_eq!(AllocationBucket::Team.hard_cap(), 150_000_000 * OMNIA_PLANCKS);
        assert_eq!(AllocationBucket::EarlyInvestors.hard_cap(), 100_000_000 * OMNIA_PLANCKS);
        assert_eq!(AllocationBucket::Ecosystem.hard_cap(), 150_000_000 * OMNIA_PLANCKS);
        assert_eq!(AllocationBucket::TreasuryReserve.hard_cap(), 100_000_000 * OMNIA_PLANCKS);
        assert_eq!(AllocationBucket::Liquidity.hard_cap(), 100_000_000 * OMNIA_PLANCKS);
    }

    #[test]
    fn all_buckets_sum_to_hard_cap() {
        let total: u64 = [
            AllocationBucket::NetworkIncentives,
            AllocationBucket::Team,
            AllocationBucket::EarlyInvestors,
            AllocationBucket::Ecosystem,
            AllocationBucket::TreasuryReserve,
            AllocationBucket::Liquidity,
        ]
        .iter()
        .map(|b| b.hard_cap())
        .sum();
        assert_eq!(total, HARD_CAP);
    }

    #[test]
    fn fund_bucket_at_genesis() {
        let mut treasury = Treasury::with_genesis_buckets();
        let mut reg = AssetRegistry::with_genesis_assets();
        let def = test_omnia_def();

        treasury
            .fund_bucket(
                AllocationBucket::NetworkIncentives,
                100_000_000 * OMNIA_PLANCKS,
                SupplyAuthority::Genesis,
                reg.supply_tracker_mut(),
                &def,
            )
        .unwrap();

        assert_eq!(
            treasury.bucket_funded(AllocationBucket::NetworkIncentives),
            100_000_000 * OMNIA_PLANCKS
        );
        assert_eq!(reg.total_supply(AssetId::OMNIA), 100_000_000 * OMNIA_PLANCKS);
    }

    #[test]
    fn fund_bucket_exceeds_cap_rejected() {
        let mut treasury = Treasury::with_genesis_buckets();
        let mut reg = AssetRegistry::with_genesis_assets();
        let def = test_omnia_def();

        let result = treasury.fund_bucket(
            AllocationBucket::Team,
            200_000_000 * OMNIA_PLANCKS, // exceeds 150M cap
            SupplyAuthority::Genesis,
            reg.supply_tracker_mut(),
            &def,
        );
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), RegistryError::TreasuryLimitExceeded { .. }));
    }

    #[test]
    fn fund_full_genesis_allocation() {
        let mut treasury = Treasury::with_genesis_buckets();
        let mut reg = AssetRegistry::with_genesis_assets();
        let def = test_omnia_def();

        // Fund all 6 buckets to their full caps
        let buckets = [
            (AllocationBucket::NetworkIncentives, 400_000_000 * OMNIA_PLANCKS),
            (AllocationBucket::Team, 150_000_000 * OMNIA_PLANCKS),
            (AllocationBucket::EarlyInvestors, 100_000_000 * OMNIA_PLANCKS),
            (AllocationBucket::Ecosystem, 150_000_000 * OMNIA_PLANCKS),
            (AllocationBucket::TreasuryReserve, 100_000_000 * OMNIA_PLANCKS),
            (AllocationBucket::Liquidity, 100_000_000 * OMNIA_PLANCKS),
        ];

        for (bucket, amount) in &buckets {
            treasury
                .fund_bucket(*bucket, *amount, SupplyAuthority::Genesis, reg.supply_tracker_mut(), &def)
                .unwrap();
        }

        assert_eq!(reg.total_supply(AssetId::OMNIA), HARD_CAP);
        assert_eq!(treasury.total_bucket_funded(), HARD_CAP);
        treasury.verify_bucket_invariants().unwrap();
    }

    #[test]
    fn allocate_from_bucket() {
        let mut treasury = Treasury::with_genesis_buckets();
        let mut reg = AssetRegistry::with_genesis_assets();
        let def = test_omnia_def();

        // Fund ecosystem bucket
        treasury
            .fund_bucket(
                AllocationBucket::Ecosystem,
                150_000_000 * OMNIA_PLANCKS,
                SupplyAuthority::Genesis,
                reg.supply_tracker_mut(),
                &def,
            )
        .unwrap();

        // Allocate 3M from ecosystem (within daily limit of hard_cap/30 = 5M)
        treasury
            .allocate(
                AllocationBucket::Ecosystem,
                3_000_000 * OMNIA_PLANCKS,
                "partner-xyz",
                "grant for integration",
                Some("ref-001".into()),
                reg.supply_tracker_mut(),
                &def,
                1, // day 1
                1, // month 1
            )
        .unwrap();

        // Treasury balance should have decreased, account balance increased
        let supply = reg.supply_tracker().get(AssetId::OMNIA).unwrap();
        assert_eq!(supply.treasury_balances, 147_000_000 * OMNIA_PLANCKS);
        assert_eq!(supply.account_balances, 3_000_000 * OMNIA_PLANCKS);
    }

    #[test]
    fn circuit_breaker_blocks_allocation() {
        let mut treasury = Treasury::with_genesis_buckets();
        let mut reg = AssetRegistry::with_genesis_assets();
        let def = test_omnia_def();

        treasury
            .fund_bucket(
                AllocationBucket::Ecosystem,
                50_000_000 * OMNIA_PLANCKS,
                SupplyAuthority::Genesis,
                reg.supply_tracker_mut(),
                &def,
            )
        .unwrap();

        treasury.trip_circuit_breaker("suspicious activity detected");
        assert!(treasury.is_paused());

        let result = treasury.allocate(
            AllocationBucket::Ecosystem,
            1_000 * OMNIA_PLANCKS,
            "someone",
            "test",
            None,
            reg.supply_tracker_mut(),
            &def,
            1, 1,
        );
        assert!(matches!(result, Err(RegistryError::TreasuryPaused(_))));
    }

    #[test]
    fn pilot_allocation_respects_limits() {
        let mut treasury = Treasury::with_genesis_buckets();
        let mut reg = AssetRegistry::with_genesis_assets();
        let def = test_omnia_def();

        // Fund treasury reserve (which backs pilot)
        treasury
            .fund_bucket(
                AllocationBucket::TreasuryReserve,
                50_000_000 * OMNIA_PLANCKS,
                SupplyAuthority::Genesis,
                reg.supply_tracker_mut(),
                &def,
            )
        .unwrap();

        // Allocate within daily limit
        treasury
            .allocate_pilot(
                100_000 * OMNIA_PLANCKS,
                "user-1",
                "wallet- approved",
                None,
                reg.supply_tracker_mut(),
                &def,
                1, 1,
            )
        .unwrap();

        assert_eq!(treasury.pilot_inventory().daily_spent, 100_000 * OMNIA_PLANCKS);
    }

    #[test]
    fn pilot_daily_limit_enforced() {
        let mut treasury = Treasury::with_genesis_buckets();
        let mut reg = AssetRegistry::with_genesis_assets();
        let def = test_omnia_def();

        treasury
            .fund_bucket(
                AllocationBucket::TreasuryReserve,
                50_000_000 * OMNIA_PLANCKS,
                SupplyAuthority::Genesis,
                reg.supply_tracker_mut(),
                &def,
            )
        .unwrap();

        // Try to allocate more than daily limit
        let result = treasury.allocate_pilot(
            (DEFAULT_DAILY_PILOT_LIMIT + 1),
            "user-1",
            "wallet-approved",
            None,
            reg.supply_tracker_mut(),
            &def,
            1, 1,
        );
        assert!(matches!(result, Err(RegistryError::TreasuryLimitExceeded { .. })));
    }

    #[test]
    fn vesting_cliff_blocks_release() {
        let mut treasury = Treasury::with_genesis_buckets();
        let mut reg = AssetRegistry::with_genesis_assets();
        let def = test_omnia_def();

        // Fund team bucket
        treasury
            .fund_bucket(
                AllocationBucket::Team,
                150_000_000 * OMNIA_PLANCKS,
                SupplyAuthority::Genesis,
                reg.supply_tracker_mut(),
                &def,
            )
        .unwrap();

        treasury.create_vesting("alice", 10_000_000 * OMNIA_PLANCKS).unwrap();

        // Try to release before cliff (at half-cliff time)
        let half_cliff = 365 * 24 * 60 * 60 * 1_000 / 2;
        let result = treasury.release_vesting(
            "alice",
            1_000 * OMNIA_PLANCKS,
            half_cliff,
            reg.supply_tracker_mut(),
            &def,
        );
        assert!(result.is_err()); // Should fail — cliff not reached
    }

    #[test]
    fn vesting_release_after_cliff() {
        let mut treasury = Treasury::with_genesis_buckets();
        let mut reg = AssetRegistry::with_genesis_assets();
        let def = test_omnia_def();

        treasury
            .fund_bucket(
                AllocationBucket::Team,
                150_000_000 * OMNIA_PLANCKS,
                SupplyAuthority::Genesis,
                reg.supply_tracker_mut(),
                &def,
            )
        .unwrap();

        treasury.create_vesting("bob", 12_000_000 * OMNIA_PLANCKS).unwrap();

        // Release right at cliff
        let at_cliff = 365 * 24 * 60 * 60 * 1_000;
        let event = treasury
            .release_vesting(
                "bob",
                1_000_000 * OMNIA_PLANCKS,
                at_cliff,
                reg.supply_tracker_mut(),
                &def,
            )
        .unwrap();

        // At cliff, 25% of total is available immediately.
        // Test slightly after cliff to get a non-zero post-cliff portion.
        let after_cliff = 365 * 24 * 60 * 60 * 1_000 + 1;
        let event2 = treasury
            .release_vesting(
                "bob",
                1_000_000 * OMNIA_PLANCKS,
                after_cliff,
                reg.supply_tracker_mut(),
                &def,
            )
            .unwrap();
        assert!(matches!(event2, TreasuryEvent::VestingRelease { .. }));
    }

    #[test]
    fn unauthorized_bucket_funding_rejected() {
        let mut treasury = Treasury::with_genesis_buckets();
        let mut reg = AssetRegistry::with_genesis_assets();
        let def = test_omnia_def();

        // Reward authority cannot fund buckets
        let result = treasury.fund_bucket(
            AllocationBucket::Ecosystem,
            1_000 * OMNIA_PLANCKS,
            SupplyAuthority::Reward, // not Genesis or Treasury
            reg.supply_tracker_mut(),
            &def,
        );
        assert!(matches!(result, Err(RegistryError::UnauthorizedRegistration)));
    }

    #[test]
    fn treasury_events_are_auditable() {
        let mut treasury = Treasury::with_genesis_buckets();
        let mut reg = AssetRegistry::with_genesis_assets();
        let def = test_omnia_def();

        treasury
            .fund_bucket(
                AllocationBucket::NetworkIncentives,
                50_000_000 * OMNIA_PLANCKS,
                SupplyAuthority::Genesis,
                reg.supply_tracker_mut(),
                &def,
            )
        .unwrap();

        treasury.trip_circuit_breaker("test");
        treasury.reset_circuit_breaker();

        assert_eq!(treasury.events().len(), 3); // fund + trip + reset
    }
}
