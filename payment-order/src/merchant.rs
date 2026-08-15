//! Merchant payment types — Spec §9
//!
//! ## Onboarding (§9.1)
//! ## Payment Flow (§9.2)
//! ## Exit & Settlement (§9.3 — deferred by design)

use serde::{Deserialize, Serialize};

// --- Merchant Identity ---

/// Business category for a merchant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BusinessCategory {
    /// Retail goods and services.
    Retail,
    /// Food and beverage.
    FoodAndBeverage,
    /// Transportation and logistics.
    Transportation,
    /// Telecommunications and utilities.
    Telecom,
    /// Financial services.
    FinancialServices,
    /// Health and pharmacy.
    Health,
    /// Education.
    Education,
    /// Other.
    Other,
}

/// Risk tier for a merchant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MerchantRiskTier {
    /// Standard risk — normal limits.
    Standard,
    /// Elevated risk — reduced limits.
    Elevated,
    /// High risk — manual review for large transactions.
    High,
}

/// Merchant onboarding data per Spec §9.1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerchantProfile {
    /// Unique merchant ID.
    pub merchant_id: String,
    /// Display name.
    pub business_name: String,
    /// Business category.
    pub category: BusinessCategory,
    /// Settlement preference (OMNIA hold, etc.).
    pub settlement_preference: SettlementPreference,
    /// Support contact.
    pub support_contact: String,
    /// Risk tier.
    pub risk_tier: MerchantRiskTier,
    /// Daily OMNIA receipt limit (plancks).
    pub daily_limit: u64,
    /// Per-transaction OMNIA limit (plancks).
    pub per_tx_limit: u64,
    /// Whether the merchant is active.
    pub active: bool,
    /// Treasury onboarding grant (if any).
    pub onboarding_grant: Option<OnboardingGrant>,
}

/// How a merchant prefers to receive value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettlementPreference {
    /// Hold received OMNIA in wallet.
    HoldOmnia,
    /// Use in supplier network.
    SupplierNetwork,
    /// Mobile-money payout via partner (future).
    MomoOutPartner,
    /// Treasury buyback (future, requires separate approval).
    TreasuryBuyback,
    /// External liquidity provider (future).
    ExternalLiquidity,
}

/// Treasury onboarding grant per Spec §9.1.
/// Must be disclosed, capped, with purpose/amount/duration/milestones/reporting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnboardingGrant {
    /// Amount granted (OMNIA plancks).
    pub amount: u64,
    /// Purpose.
    pub purpose: String,
    /// Start date (ms).
    pub start_ms: u64,
    /// End date (ms).
    pub end_ms: u64,
    /// Milestones for release.
    pub milestones: Vec<String>,
    /// Grant ID for tracking.
    pub grant_id: String,
}

// --- Merchant Payment ---

/// A merchant payment request (customer pays merchant in OMNIA).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerchantPayment {
    /// Unique payment ID.
    pub payment_id: String,
    /// Merchant ID.
    pub merchant_id: String,
    /// Customer wallet.
    pub customer_wallet: String,
    /// GHS price displayed to customer.
    pub ghs_price: u64,
    /// Time-limited OMNIA quote.
    pub omnia_amount: u64,
    /// Quoted exchange rate.
    pub exchange_rate: u64,
    /// Quote expiration (ms).
    pub quote_expiry_ms: u64,
    /// Protocol fee (plancks).
    pub protocol_fee: u64,
    /// Payment status.
    pub status: MerchantPaymentStatus,
    /// Timestamp (ms).
    pub created_at_ms: u64,
}

/// Status of a merchant payment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MerchantPaymentStatus {
    /// QR generated, awaiting payment.
    Pending,
    /// Payment submitted, confirming.
    Confirming,
    /// Payment confirmed and credited to merchant.
    Confirmed,
    /// Payment failed.
    Failed,
    /// Payment refunded.
    Refunded,
}

// --- Merchant Receipt ---

/// A merchant payment receipt per Spec §9.2.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerchantReceipt {
    /// Payment ID.
    pub payment_id: String,
    /// Merchant ID.
    pub merchant_id: String,
    /// GHS price.
    pub ghs_price: u64,
    /// OMNIA amount.
    pub omnia_amount: u64,
    /// Exchange rate.
    pub exchange_rate: u64,
    /// Protocol fee.
    pub protocol_fee: u64,
    /// Net OMNIA received by merchant.
    pub net_omnia: u64,
    /// Confirmation timestamp (ms).
    pub confirmed_at_ms: u64,
    /// On-chain transaction reference.
    pub tx_reference: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merchant_profile_creation() {
        let profile = MerchantProfile {
            merchant_id: "m-1".into(),
            business_name: "Test Shop".into(),
            category: BusinessCategory::Retail,
            settlement_preference: SettlementPreference::HoldOmnia,
            support_contact: "support@testshop.com".into(),
            risk_tier: MerchantRiskTier::Standard,
            daily_limit: 10_000_000_000, // 10 OMNIA
            per_tx_limit: 1_000_000_000, // 1 OMNIA
            active: true,
            onboarding_grant: None,
        };
        assert!(profile.active);
        assert_eq!(profile.risk_tier, MerchantRiskTier::Standard);
    }

    #[test]
    fn merchant_receipt_net() {
        let receipt = MerchantReceipt {
            payment_id: "p-1".into(),
            merchant_id: "m-1".into(),
            ghs_price: 50_000,             // 500 GHS
            omnia_amount: 100_000_000_000, // 100 OMNIA
            exchange_rate: 500_000,
            protocol_fee: 1_000_000, // 0.001 OMNIA
            net_omnia: 99_999_000_000,
            confirmed_at_ms: 1000,
            tx_reference: Some("0xabc".into()),
        };
        assert_eq!(receipt.net_omnia, receipt.omnia_amount - receipt.protocol_fee);
    }
}
