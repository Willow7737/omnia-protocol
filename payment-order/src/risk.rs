//! Circuit-breaker risk limits — Financial Specification §15
//!
//! Before public operation the system MUST implement configurable limits for:
//!
//! | Limit | Purpose |
//! |-------|---------|
//! | Per-order GHS limit | Limits payment and fraud exposure |
//! | Daily customer limit | Controls cumulative risk |
//! | Daily merchant limit | Controls business and settlement exposure |
//! | Treasury allocation limit | Prevents inventory drain |
//! | Provider exposure limit | Limits unreconciled payment risk |
//! | Manual-review threshold | Routes unusual orders to operations |
//! | Refund exposure limit | Prevents uncontrolled liability |
//! | Price movement tolerance | Pauses allocation when quotes become stale |
//! | On-chain pending timeout | Prevents indefinite uncertain delivery |
//! | Aggregate subsidy budget | Prevents unbounded acquisition spending |
//!
//! Circuit breakers MUST be able to pause new allocations without
//! destroying existing balances or preventing users from viewing
//! transaction history.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::PaymentError;

// --- Default limits ---

/// Default per-order GHS limit: 5,000 GHS = 500,000 pesewas.
pub const DEFAULT_PER_ORDER_GHS_LIMIT: u64 = 500_000;

/// Default daily customer GHS limit: 20,000 GHS = 2,000,000 pesewas.
pub const DEFAULT_DAILY_CUSTOMER_LIMIT: u64 = 2_000_000;

/// Default daily merchant GHS limit: 2,000,000 GHS = 200,000,000 pesewas.
pub const DEFAULT_DAILY_MERCHANT_LIMIT: u64 = 200_000_000;

/// Default treasury allocation limit (OMNIA plancks per day): 500,000 OMNIA.
pub const DEFAULT_DAILY_TREASURY_LIMIT: u64 = 500_000_000_000_000;

/// Default provider exposure limit (GHS pesewas): 10,000,000 GHS.
pub const DEFAULT_PROVIDER_EXPOSURE_LIMIT: u64 = 1_000_000_000;

/// Default manual-review threshold (GHS pesewas): 3,000 GHS.
pub const DEFAULT_MANUAL_REVIEW_THRESHOLD: u64 = 300_000;

/// Default refund exposure limit (GHS pesewas): 5,000,000 GHS.
pub const DEFAULT_REFUND_EXPOSURE_LIMIT: u64 = 500_000_000;

/// Default price movement tolerance basis points: 200 bps = 2%.
pub const DEFAULT_PRICE_MOVEMENT_TOLERANCE_BPS: u64 = 200;

/// Default on-chain pending timeout: 10 minutes in ms.
pub const DEFAULT_ON_CHAIN_TIMEOUT_MS: u64 = 10 * 60 * 1_000;

/// Default aggregate subsidy budget (OMNIA plancks): 50,000,000 OMNIA.
pub const DEFAULT_AGGREGATE_SUBSIDY_BUDGET: u64 = 50_000_000_000_000_000;

/// Default quote validity: 5 minutes in ms.
pub const DEFAULT_QUOTE_VALIDITY_MS: u64 = 5 * 60 * 1_000;

/// Default payment callback timeout: 30 minutes in ms.
pub const DEFAULT_PAYMENT_TIMEOUT_MS: u64 = 30 * 60 * 1_000;

// --- Risk limits struct ---

/// Configurable risk limits per Spec §15.
///
/// All amounts are in the smallest unit of their respective asset
/// (GHS pesewas for GHS limits, OMNIA plancks for OMNIA limits).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskLimits {
    /// Maximum GHS amount per single order.
    pub per_order_ghs_limit: u64,
    /// Maximum GHS amount per customer per day.
    pub daily_customer_limit: u64,
    /// Maximum GHS amount per merchant per day.
    pub daily_merchant_limit: u64,
    /// Maximum OMNIA (plancks) the treasury can allocate per day.
    pub daily_treasury_allocation_limit: u64,
    /// Maximum unreconciled GHS exposure per provider.
    pub provider_exposure_limit: u64,
    /// Orders above this GHS amount require manual review.
    pub manual_review_threshold: u64,
    /// Maximum total GHS in pending refunds at any time.
    pub refund_exposure_limit: u64,
    /// Price movement tolerance in basis points.
    /// If the market rate moves more than this from the quoted rate,
    /// the allocation is paused.
    pub price_movement_tolerance_bps: u64,
    /// On-chain transaction pending timeout in milliseconds.
    pub on_chain_timeout_ms: u64,
    /// Aggregate subsidy budget (OMNIA plancks) for pilot acquisition.
    pub aggregate_subsidy_budget: u64,
    /// Quote validity window in milliseconds.
    pub quote_validity_ms: u64,
    /// Payment callback timeout in milliseconds.
    pub payment_timeout_ms: u64,
}

impl Default for RiskLimits {
    fn default() -> Self {
        Self {
            per_order_ghs_limit: DEFAULT_PER_ORDER_GHS_LIMIT,
            daily_customer_limit: DEFAULT_DAILY_CUSTOMER_LIMIT,
            daily_merchant_limit: DEFAULT_DAILY_MERCHANT_LIMIT,
            daily_treasury_allocation_limit: DEFAULT_DAILY_TREASURY_LIMIT,
            provider_exposure_limit: DEFAULT_PROVIDER_EXPOSURE_LIMIT,
            manual_review_threshold: DEFAULT_MANUAL_REVIEW_THRESHOLD,
            refund_exposure_limit: DEFAULT_REFUND_EXPOSURE_LIMIT,
            price_movement_tolerance_bps: DEFAULT_PRICE_MOVEMENT_TOLERANCE_BPS,
            on_chain_timeout_ms: DEFAULT_ON_CHAIN_TIMEOUT_MS,
            aggregate_subsidy_budget: DEFAULT_AGGREGATE_SUBSIDY_BUDGET,
            quote_validity_ms: DEFAULT_QUOTE_VALIDITY_MS,
            payment_timeout_ms: DEFAULT_PAYMENT_TIMEOUT_MS,
        }
    }
}

impl RiskLimits {
    /// Check if a GHS amount requires manual review.
    #[inline]
    pub fn requires_manual_review(&self, ghs_amount: u64) -> bool {
        ghs_amount > self.manual_review_threshold
    }

    /// Check if a price movement exceeds tolerance.
    /// `quoted_rate` and `current_rate` are in the same fixed-point format.
    /// Returns true if the movement exceeds the tolerance (allocation should pause).
    #[inline]
    pub fn price_movement_exceeded(
        &self,
        quoted_rate: u64,
        current_rate: u64,
    ) -> bool {
        if quoted_rate == 0 {
            return true;
        }
        // Calculate basis point difference: |quoted - current| / quoted * 10000
        let diff = if quoted_rate > current_rate {
            quoted_rate - current_rate
        } else {
            current_rate - quoted_rate
        };
        let bps = (diff as u128 * 10_000) / quoted_rate as u128;
        bps > self.price_movement_tolerance_bps as u128
    }
}

// --- Circuit breaker ---

/// Circuit breaker state for a specific limit type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BreakerState {
    /// Breaker is closed — normal operation.
    Closed,
    /// Breaker is open — new allocations paused.
    /// Existing orders and balances are unaffected.
    Open,
}

/// Circuit breaker system per Spec §15.
///
/// Tracks cumulative exposure and trips breakers when limits are exceeded.
/// Tripping a breaker pauses new allocations but does NOT:
/// - Destroy existing balances
/// - Prevent users from viewing transaction history
/// - Cancel in-flight orders
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreaker {
    /// Current risk limits.
    pub limits: RiskLimits,
    /// Per-customer daily GHS accumulation: customer_ref → pesewas.
    pub customer_daily: HashMap<String, u64>,
    /// Per-merchant daily GHS accumulation: merchant_ref → pesewas.
    pub merchant_daily: HashMap<String, u64>,
    /// Per-provider unreconciled GHS exposure: provider_id → pesewas.
    pub provider_exposure: HashMap<String, u64>,
    /// Total GHS in pending refunds.
    pub pending_refund_ghs: u64,
    /// Total OMNIA allocated today (plancks).
    pub daily_treasury_allocated: u64,
    /// Total OMNIA subsidy spent (plancks).
    pub aggregate_subsidy_spent: u64,
    /// Individual breaker states.
    pub breakers: HashMap<String, BreakerState>,
    /// Current day bucket (for daily reset).
    pub current_day: u64,
}

impl CircuitBreaker {
    /// Create a new circuit breaker with default limits.
    pub fn new(now_ms: u64) -> Self {
        Self::with_limits(RiskLimits::default(), now_ms)
    }

    /// Create a circuit breaker with custom limits.
    pub fn with_limits(limits: RiskLimits, now_ms: u64) -> Self {
        Self {
            limits,
            customer_daily: HashMap::new(),
            merchant_daily: HashMap::new(),
            provider_exposure: HashMap::new(),
            pending_refund_ghs: 0,
            daily_treasury_allocated: 0,
            aggregate_subsidy_spent: 0,
            breakers: HashMap::new(),
            current_day: now_ms / (24 * 60 * 60 * 1_000),
        }
    }

    /// Reset daily accumulations if the day has rolled over.
    pub fn maybe_reset_daily(&mut self, now_ms: u64) {
        let today = now_ms / (24 * 60 * 60 * 1_000);
        if today != self.current_day {
            self.customer_daily.clear();
            self.merchant_daily.clear();
            self.daily_treasury_allocated = 0;
            self.current_day = today;
        }
    }

    /// Check if any circuit breaker is open.
    #[inline]
    pub fn is_any_open(&self) -> bool {
        self.breakers
            .values()
            .any(|s| *s == BreakerState::Open)
    }

    /// Check if a specific breaker is open.
    pub fn is_breaker_open(&self, name: &str) -> bool {
        self.breakers
            .get(name)
            .map(|s| *s == BreakerState::Open)
            .unwrap_or(false)
    }

    /// Trip a specific circuit breaker.
    pub fn trip(&mut self, name: &str) {
        self.breakers.insert(name.into(), BreakerState::Open);
    }

    /// Reset a specific circuit breaker (e.g., after manual review).
    pub fn reset_breaker(&mut self, name: &str) {
        self.breakers.insert(name.into(), BreakerState::Closed);
    }

    /// Pre-flight check: validate all risk limits for a new order.
    /// Returns Ok(()) if the order can proceed, or the first limit violation.
    pub fn check_order(
        &self,
        ghs_amount: u64,
        customer_ref: &str,
        merchant_ref: &str,
        provider_name: &str,
    ) -> Result<(), PaymentError> {
        // Per-order limit
        if ghs_amount > self.limits.per_order_ghs_limit {
            return Err(PaymentError::PerOrderLimitExceeded {
                amount: ghs_amount,
                limit: self.limits.per_order_ghs_limit,
            });
        }

        // Daily customer limit
        let customer_total = self.customer_daily.get(customer_ref).copied().unwrap_or(0);
        let new_customer_total = customer_total
            .checked_add(ghs_amount)
            .ok_or_else(|| PaymentError::ArithmeticOverflow("customer daily".into()))?;
        if new_customer_total > self.limits.daily_customer_limit {
            return Err(PaymentError::DailyCustomerLimitExceeded {
                customer: customer_ref.into(),
                amount: new_customer_total,
                limit: self.limits.daily_customer_limit,
            });
        }

        // Daily merchant limit
        let merchant_total = self.merchant_daily.get(merchant_ref).copied().unwrap_or(0);
        let new_merchant_total = merchant_total
            .checked_add(ghs_amount)
            .ok_or_else(|| PaymentError::ArithmeticOverflow("merchant daily".into()))?;
        if new_merchant_total > self.limits.daily_merchant_limit {
            return Err(PaymentError::RiskLimitExceeded {
                limit_type: "daily_merchant".into(),
                requested: new_merchant_total,
                allowed: self.limits.daily_merchant_limit,
            });
        }

        // Provider exposure
        let provider_total = self
            .provider_exposure
            .get(provider_name)
            .copied()
            .unwrap_or(0);
        let new_provider_total = provider_total
            .checked_add(ghs_amount)
            .ok_or_else(|| PaymentError::ArithmeticOverflow("provider exposure".into()))?;
        if new_provider_total > self.limits.provider_exposure_limit {
            return Err(PaymentError::RiskLimitExceeded {
                limit_type: "provider_exposure".into(),
                requested: new_provider_total,
                allowed: self.limits.provider_exposure_limit,
            });
        }

        // Refund exposure
        if self.pending_refund_ghs > self.limits.refund_exposure_limit {
            return Err(PaymentError::RiskLimitExceeded {
                limit_type: "refund_exposure".into(),
                requested: self.pending_refund_ghs,
                allowed: self.limits.refund_exposure_limit,
            });
        }

        // Global breakers
        if self.is_breaker_open("treasury_allocation") {
            return Err(PaymentError::CircuitBreakerTripped(
                "treasury allocation limit reached".into(),
            ));
        }
        if self.is_breaker_open("price_movement") {
            return Err(PaymentError::CircuitBreakerTripped(
                "price movement exceeded tolerance".into(),
            ));
        }
        if self.is_breaker_open("aggregate_subsidy") {
            return Err(PaymentError::CircuitBreakerTripped(
                "aggregate subsidy budget exhausted".into(),
            ));
        }

        Ok(())
    }

    /// Record that an order has been created (accumulate daily limits).
    pub fn record_order(
        &mut self,
        ghs_amount: u64,
        customer_ref: &str,
        merchant_ref: &str,
        provider_name: &str,
    ) {
        *self
            .customer_daily
            .entry(customer_ref.into())
            .or_insert(0) += ghs_amount;
        *self
            .merchant_daily
            .entry(merchant_ref.into())
            .or_insert(0) += ghs_amount;
        *self
            .provider_exposure
            .entry(provider_name.into())
            .or_insert(0) += ghs_amount;
    }

    /// Record a treasury allocation of `omnia_amount` plancks.
    /// Trips the treasury breaker if daily limit exceeded.
    pub fn record_treasury_allocation(&mut self, omnia_amount: u64) {
        self.daily_treasury_allocated = self
            .daily_treasury_allocated
            .saturating_add(omnia_amount);
        if self.daily_treasury_allocated > self.limits.daily_treasury_allocation_limit {
            self.trip("treasury_allocation");
        }
    }

    /// Record a subsidy spend.
    /// Trips the subsidy breaker if budget exceeded.
    pub fn record_subsidy_spend(&mut self, omnia_amount: u64) {
        self.aggregate_subsidy_spent = self
            .aggregate_subsidy_spent
            .saturating_add(omnia_amount);
        if self.aggregate_subsidy_spent > self.limits.aggregate_subsidy_budget {
            self.trip("aggregate_subsidy");
        }
    }

    /// Record provider exposure decrease (after reconciliation).
    pub fn reconcile_provider(&mut self, provider_name: &str, ghs_amount: u64) {
        let current = self
            .provider_exposure
            .get(provider_name)
            .copied()
            .unwrap_or(0);
        self.provider_exposure
            .insert(provider_name.into(), current.saturating_sub(ghs_amount));
    }

    /// Record a pending refund.
    pub fn record_refund_pending(&mut self, ghs_amount: u64) {
        self.pending_refund_ghs = self.pending_refund_ghs.saturating_add(ghs_amount);
        if self.pending_refund_ghs > self.limits.refund_exposure_limit {
            self.trip("refund_exposure");
        }
    }

    /// Record a completed refund.
    pub fn record_refund_completed(&mut self, ghs_amount: u64) {
        self.pending_refund_ghs = self.pending_refund_ghs.saturating_sub(ghs_amount);
        if self.pending_refund_ghs <= self.limits.refund_exposure_limit {
            self.reset_breaker("refund_exposure");
        }
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_limits_are_reasonable() {
        let limits = RiskLimits::default();
        assert_eq!(limits.per_order_ghs_limit, 500_000); // 5,000 GHS
        assert_eq!(limits.daily_customer_limit, 2_000_000); // 20,000 GHS
        assert!(limits.requires_manual_review(400_000)); // 4,000 GHS > 3,000 GHS threshold
        assert!(!limits.requires_manual_review(200_000)); // 2,000 GHS < threshold
    }

    #[test]
    fn price_movement_within_tolerance() {
        let limits = RiskLimits::default(); // 200 bps = 2%
        // 1% movement — within tolerance
        assert!(!limits.price_movement_exceeded(10_000, 10_100));
        // 3% movement — exceeds
        assert!(limits.price_movement_exceeded(10_000, 10_300));
    }

    #[test]
    fn price_movement_zero_quoted_rate() {
        let limits = RiskLimits::default();
        assert!(limits.price_movement_exceeded(0, 100)); // zero rate → always exceeded
    }

    #[test]
    fn per_order_limit_check() {
        let cb = CircuitBreaker::new(0);
        // Under limit
        assert!(cb.check_order(100_000, "cust", "merchant", "MTN").is_ok());
        // Over limit
        assert!(cb
            .check_order(600_000, "cust", "merchant", "MTN")
            .is_err());
    }

    #[test]
    fn daily_customer_limit_accumulates() {
        let mut cb = CircuitBreaker::new(0);
        // Use amounts under per-order limit (500,000 pesewas = 5,000 GHS)
        // Daily customer limit: 2,000,000 pesewas = 20,000 GHS
        cb.record_order(400_000, "cust1", "merchant", "MTN"); // 4,000 GHS
        // 4,000 + 4,000 = 8,000 < 20,000 → OK
        assert!(cb.check_order(400_000, "cust1", "merchant", "MTN").is_ok());
        // Accumulate to near limit: 4k*5 = 20k
        for _ in 0..4 {
            cb.record_order(400_000, "cust1", "merchant", "MTN");
        }
        // Total = 20,000 = exactly at limit. Adding 1 → over
        assert!(cb.check_order(1, "cust1", "merchant", "MTN").is_err());
        // Different customer → OK
        assert!(cb.check_order(400_000, "cust2", "merchant", "MTN").is_ok());
    }

    #[test]
    fn treasury_allocation_breaker_trips() {
        let mut cb = CircuitBreaker::with_limits(
            RiskLimits {
                daily_treasury_allocation_limit: 100_000,
                ..RiskLimits::default()
            },
            0,
        );
        assert!(!cb.is_breaker_open("treasury_allocation"));
        cb.record_treasury_allocation(50_000);
        assert!(!cb.is_breaker_open("treasury_allocation"));
        cb.record_treasury_allocation(60_000); // 110k > 100k limit
        assert!(cb.is_breaker_open("treasury_allocation"));
        // New orders blocked
        assert!(cb.check_order(100, "cust", "merchant", "MTN").is_err());
    }

    #[test]
    fn subsidy_breaker_trips() {
        let mut cb = CircuitBreaker::with_limits(
            RiskLimits {
                aggregate_subsidy_budget: 1_000_000,
                ..RiskLimits::default()
            },
            0,
        );
        cb.record_subsidy_spend(999_999);
        assert!(!cb.is_breaker_open("aggregate_subsidy"));
        cb.record_subsidy_spend(2);
        assert!(cb.is_breaker_open("aggregate_subsidy"));
    }

    #[test]
    fn refund_exposure_tracking() {
        let mut cb = CircuitBreaker::with_limits(
            RiskLimits {
                refund_exposure_limit: 500_000,
                ..RiskLimits::default()
            },
            0,
        );
        cb.record_refund_pending(400_000);
        assert!(!cb.is_breaker_open("refund_exposure"));
        cb.record_refund_pending(200_000);
        assert!(cb.is_breaker_open("refund_exposure"));
        // Complete a refund
        cb.record_refund_completed(300_000);
        // 300k pending < 500k limit → breaker resets
        assert!(!cb.is_breaker_open("refund_exposure"));
    }

    #[test]
    fn daily_reset() {
        let mut cb = CircuitBreaker::new(0);
        cb.record_order(1_500_000, "cust1", "merchant", "MTN");
        assert_eq!(cb.customer_daily.get("cust1"), Some(&1_500_000));
        // Simulate next day
        let next_day_ms = 25 * 60 * 60 * 1_000;
        cb.maybe_reset_daily(next_day_ms);
        assert!(!cb.customer_daily.contains_key("cust1"));
    }

    #[test]
    fn provider_reconciliation() {
        let mut cb = CircuitBreaker::new(0);
        cb.record_order(100_000, "cust", "merchant", "MTN");
        assert_eq!(cb.provider_exposure.get("MTN"), Some(&100_000));
        cb.reconcile_provider("MTN", 100_000);
        assert_eq!(cb.provider_exposure.get("MTN"), Some(&0));
    }

    #[test]
    fn reset_breaker_allows_operations() {
        let mut cb = CircuitBreaker::new(0);
        cb.trip("treasury_allocation");
        assert!(cb.is_any_open());
        assert!(cb.check_order(100, "c", "m", "MTN").is_err());
        cb.reset_breaker("treasury_allocation");
        assert!(!cb.is_any_open());
        assert!(cb.check_order(100, "c", "m", "MTN").is_ok());
    }
}