//! Payment provider adapter trait — Spec §8.1, §8.3, §8.5
//!
//! Defines a normalized interface for mobile-money provider integration.
//! The Omnia Protocol does NOT handle mobile-money interactions directly —
//! a payment partner (e.g., Flutterwave) handles local interaction.
//! Omnia controls order state, quotes, inventory, allocation, reconciliation.
//!
//! ## Provider Requirements (Spec §8.3)
//!
//! - The client MUST NOT declare payment success.
//! - The provider event MUST be authenticated.
//! - The backend MUST independently verify before allocation.
//!
//! ## Supported Providers (Ghana)
//!
//! - MTN Mobile Money (MoMo)
//! - Telecel Cash
//! - AT Money

use serde::{Deserialize, Serialize};

use crate::error::PaymentError;

// --- Provider Types ---

/// Supported mobile-money providers in Ghana.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MobileMoneyProvider {
    /// MTN Mobile Money (MoMo) — largest Ghana mobile-money network.
    Mtn,
    /// Telecel Cash (formerly Vodafone Cash).
    Telecel,
    /// AT Money (AirtelTigo Money).
    At,
}

impl MobileMoneyProvider {
    /// Return the provider identifier string.
    pub fn id(&self) -> &'static str {
        match self {
            Self::Mtn => "MTN",
            Self::Telecel => "TELECEL",
            Self::At => "AT",
        }
    }
}

impl std::fmt::Display for MobileMoneyProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.id())
    }
}

// --- Callback Types ---

/// A mobile-money provider callback payload.
/// This is what the provider sends to our webhook endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCallback {
    /// The provider that sent this callback.
    pub provider: MobileMoneyProvider,
    /// Provider-internal transaction reference.
    pub provider_tx_ref: String,
    /// Our order ID (echoed back from the payment request).
    pub order_id: String,
    /// The callback status from the provider.
    pub status: CallbackStatus,
    /// GHS amount received (in pesewas). May differ from quoted.
    pub amount_received_pesewas: u64,
    /// The customer's mobile-money number.
    pub customer_number: String,
    /// Timestamp from the provider (ms since epoch).
    pub provider_timestamp_ms: u64,
    /// Provider signature or HMAC for authentication.
    pub signature: String,
    /// Any additional data from the provider.
    pub extra: std::collections::BTreeMap<String, String>,
}

/// Status values from a provider callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallbackStatus {
    /// Payment was successful.
    Success,
    /// Payment failed.
    Failed,
    /// Payment was reversed/charged back.
    Reversed,
    /// Payment is pending (intermediate status).
    Pending,
    /// Payment timed out.
    Timeout,
}

/// Verification result for a provider callback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallbackVerification {
    /// Whether the callback signature is valid.
    pub signature_valid: bool,
    /// Whether the callback is not a replay (nonce/timestamp check).
    pub not_replay: bool,
    /// Whether the amount matches the expected range.
    pub amount_plausible: bool,
    /// Verification failure reason, if any.
    pub failure_reason: Option<String>,
}

impl CallbackVerification {
    /// Return true if the callback passes all verification checks.
    pub fn is_valid(&self) -> bool {
        self.signature_valid && self.not_replay && self.amount_plausible
    }
}

// --- Quote Types (Spec §8.4) ---

/// A time-limited OMNIA quote for a GHS payment.
/// Per Spec §8.4, the wallet MUST display all these fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OmniaQuote {
    /// Unique quote ID.
    pub quote_id: String,
    /// GHS amount (pesewas) the customer will pay.
    pub ghs_amount: u64,
    /// OMNIA quantity (plancks) the customer will receive.
    pub omnia_quantity: u64,
    /// Exchange rate: GHS per OMNIA (fixed-point).
    pub exchange_rate: u64,
    /// Provider fee in GHS (pesewas).
    pub provider_fee_ghs: u64,
    /// Omnia protocol fee (plancks).
    pub omnia_fee: u64,
    /// Spread or price impact if applicable (basis points).
    pub spread_bps: u64,
    /// Estimated delivery time in seconds.
    pub estimated_delivery_secs: u64,
    /// Quote creation timestamp (ms).
    pub created_at_ms: u64,
    /// Quote expiration timestamp (ms).
    pub expires_at_ms: u64,
    /// The provider used for this quote.
    pub provider: MobileMoneyProvider,
}

impl OmniaQuote {
    /// Return true if the quote has not expired.
    pub fn is_valid(&self, now_ms: u64) -> bool {
        now_ms < self.expires_at_ms
    }

    /// Return the total cost to the customer in GHS pesewas.
    pub fn total_ghs_cost(&self) -> u64 {
        self.ghs_amount.saturating_add(self.provider_fee_ghs)
    }

    /// Return the net OMNIA received after protocol fee.
    pub fn net_omnia(&self) -> u64 {
        self.omnia_quantity.saturating_sub(self.omnia_fee)
    }

    /// Required disclosure fields per Spec §8.4.
    /// Returns a list of (label, value) pairs for wallet display.
    pub fn disclosure_fields(&self) -> Vec<(&'static str, String)> {
        vec![
            ("ghs_amount", format!("{}", self.ghs_amount)),
            ("omnia_quantity", format!("{}", self.omnia_quantity)),
            ("quoted_rate", format!("{}", self.exchange_rate)),
            ("quote_expiry", format!("{}", self.expires_at_ms)),
            ("provider_fee", format!("{} GHS", self.provider_fee_ghs)),
            ("omnia_fee", format!("{} OMNIA", self.omnia_fee)),
            ("spread_bps", format!("{}", self.spread_bps)),
            ("estimated_delivery", format!("{}s", self.estimated_delivery_secs)),
        ]
    }
}

// --- Subsidy Tracking (Spec §8.5) ---

/// Provider fee subsidy tracking.
/// Per Spec §8.5: "Pilot MAY subsidize from capped treasury acquisition budget."
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubsidyRecord {
    /// Order ID.
    pub order_id: String,
    /// Provider fee subsidized (GHS pesewas).
    pub subsidized_ghs: u64,
    /// OMNIA equivalent of the subsidy (plancks).
    pub subsidy_omnia_plancks: u64,
    /// Which treasury bucket funded this subsidy.
    pub funding_bucket: String,
    /// Timestamp (ms).
    pub timestamp_ms: u64,
}

/// Provider fee subsidy tracker.
/// Tracks total subsidies against the capped budget.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubsidyTracker {
    /// Total OMNIA plancks spent on subsidies.
    pub total_subsidy_spent: u64,
    /// Maximum subsidy budget (OMNIA plancks).
    pub max_budget: u64,
    /// Individual subsidy records.
    pub records: Vec<SubsidyRecord>,
    /// Whether subsidy program is active.
    pub active: bool,
    /// Sunset date for subsidy program (ms). None = no sunset.
    pub sunset_date_ms: Option<u64>,
}

impl SubsidyTracker {
    /// Create a new subsidy tracker with the given budget.
    pub fn new(max_budget: u64) -> Self {
        Self {
            max_budget,
            active: true,
            ..Self::default()
        }
    }

    /// Check if subsidies are currently available.
    pub fn is_available(&self, now_ms: u64) -> bool {
        if !self.active {
            return false;
        }
        if let Some(sunset) = self.sunset_date_ms {
            if now_ms >= sunset {
                return false;
            }
        }
        self.total_subsidy_spent < self.max_budget
    }

    /// Remaining subsidy budget.
    pub fn remaining(&self) -> u64 {
        self.max_budget.saturating_sub(self.total_subsidy_spent)
    }

    /// Record a subsidy.
    pub fn record(
        &mut self,
        order_id: String,
        subsidized_ghs: u64,
        subsidy_omnia: u64,
        funding_bucket: String,
        now_ms: u64,
    ) -> Result<(), PaymentError> {
        if !self.is_available(now_ms) {
            return Err(PaymentError::CircuitBreakerTripped(
                "subsidy budget exhausted or sunset".into(),
            ));
        }
        if subsidy_omnia > self.remaining() {
            return Err(PaymentError::RiskLimitExceeded {
                limit_type: "subsidy_budget".into(),
                requested: subsidy_omnia,
                allowed: self.remaining(),
            });
        }
        self.total_subsidy_spent = self.total_subsidy_spent.saturating_add(subsidy_omnia);
        self.records.push(SubsidyRecord {
            order_id,
            subsidized_ghs,
            subsidy_omnia_plancks: subsidy_omnia,
            funding_bucket,
            timestamp_ms: now_ms,
        });
        Ok(())
    }
}

// --- Provider Adapter Trait ---

/// Trait for mobile-money provider integration.
/// Implementors handle communication with specific providers.
pub trait ProviderAdapter {
    /// The provider this adapter handles.
    fn provider(&self) -> MobileMoneyProvider;

    /// Initiate a payment request to the provider.
    /// Returns a provider reference for tracking.
    fn initiate_payment(
        &self,
        customer_number: &str,
        amount_pesewas: u64,
        order_id: &str,
    ) -> Result<String, PaymentError>;

    /// Verify an incoming callback from the provider.
    /// Must check: signature authenticity, not a replay, amount plausible.
    fn verify_callback(&self, callback: &ProviderCallback) -> Result<CallbackVerification, PaymentError>;

    /// Query the status of a payment from the provider.
    fn query_payment_status(&self, provider_tx_ref: &str) -> Result<CallbackStatus, PaymentError>;

    /// Initiate a refund to the customer's mobile-money account.
    fn initiate_refund(
        &self,
        customer_number: &str,
        amount_pesewas: u64,
        provider_tx_ref: &str,
        reason: &str,
    ) -> Result<String, PaymentError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_validity() {
        let quote = OmniaQuote {
            quote_id: "q-1".into(),
            ghs_amount: 50_000_000,
            omnia_quantity: 100_000_000_000,
            exchange_rate: 500_000,
            provider_fee_ghs: 1_000_000,
            omnia_fee: 500_000_000,
            spread_bps: 50,
            estimated_delivery_secs: 120,
            created_at_ms: 1000,
            expires_at_ms: 1000 + 300_000, // 5 min
            provider: MobileMoneyProvider::Mtn,
        };
        assert!(quote.is_valid(2000));
        assert!(!quote.is_valid(400_000));
        assert_eq!(quote.total_ghs_cost(), 51_000_000);
        assert_eq!(quote.net_omnia(), 99_500_000_000);
    }

    #[test]
    fn quote_disclosure_fields() {
        let quote = OmniaQuote {
            quote_id: "q-1".into(),
            ghs_amount: 50_000_000,
            omnia_quantity: 100_000_000_000,
            exchange_rate: 500_000,
            provider_fee_ghs: 1_000_000,
            omnia_fee: 500_000_000,
            spread_bps: 50,
            estimated_delivery_secs: 120,
            created_at_ms: 1000,
            expires_at_ms: 4000,
            provider: MobileMoneyProvider::Telecel,
        };
        let fields = quote.disclosure_fields();
        assert_eq!(fields.len(), 8);
        assert_eq!(fields[0].0, "ghs_amount");
    }

    #[test]
    fn subsidy_tracker() {
        let mut tracker = SubsidyTracker::new(1_000_000);
        assert!(tracker.is_available(0));
        assert_eq!(tracker.remaining(), 1_000_000);

        tracker
            .record("order-1".into(), 100, 400_000, "treasury".into(), 0)
            .unwrap();
        assert_eq!(tracker.remaining(), 600_000);

        // Over budget
        assert!(tracker
            .record("order-2".into(), 100, 700_000, "treasury".into(), 0)
            .is_err());
    }

    #[test]
    fn subsidy_sunset() {
        let mut tracker = SubsidyTracker::new(1_000_000);
        tracker.sunset_date_ms = Some(5000);
        assert!(tracker.is_available(4000));
        assert!(!tracker.is_available(6000));
    }

    #[test]
    fn callback_verification_valid() {
        let v = CallbackVerification {
            signature_valid: true,
            not_replay: true,
            amount_plausible: true,
            failure_reason: None,
        };
        assert!(v.is_valid());
    }

    #[test]
    fn callback_verification_invalid() {
        let v = CallbackVerification {
            signature_valid: false,
            not_replay: true,
            amount_plausible: true,
            failure_reason: Some("bad signature".into()),
        };
        assert!(!v.is_valid());
    }
}
