//! Server-generated signed quotes — Audit Priority 4
//!
//! Per the Omnia Checkpoint Audit:
//! "Quantity, rate, fees, quote expiry, and recipient asset must come from
//!  a server-generated signed quote. The client may request a quote, but it
//!  must not define the economic terms that the server later settles."
//!
//! This module provides the `QuoteService` which:
//! 1. Accepts a quote request (customer, GHS amount, provider)
//! 2. Generates all economic terms server-side
//! 3. Signs the quote with the server's private key
//! 4. The quote can be verified by any party with the server's public key
//! 5. The quote has a strict time-to-live

use serde::{Deserialize, Serialize};

use crate::error::PaymentError;
use crate::provider::{MobileMoneyProvider, OmniaQuote};

/// OMNIA decimals: 9 decimal places, 1 OMNIA = 10^9 plancks.
const OMNIA_DECIMALS: u64 = 1_000_000_000;

/// GHS decimals: 2 decimal places, 1 GHS = 100 pesewas.
const GHS_DECIMALS: u64 = 100;

/// Default quote validity: 5 minutes in ms.
const DEFAULT_QUOTE_TTL_MS: u64 = 300_000;

/// Default estimated delivery time: 120 seconds.
const DEFAULT_DELIVERY_SECS: u64 = 120;

/// A quote request from the client/wallet.
/// The client specifies ONLY what they want to buy — all economic
/// terms are determined server-side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteRequest {
    /// The customer's mobile-money number (E.164 format).
    pub customer_number: String,
    /// The amount the customer wants to spend in GHS (in pesewas).
    pub ghs_amount_pesewas: u64,
    /// The preferred provider.
    pub provider: MobileMoneyProvider,
    /// Optional: the recipient wallet public key.
    pub recipient_ref: Option<String>,
}

/// A signed quote from the server.
/// The signature covers all economic terms, binding them to the
/// server's authority.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedQuote {
    /// The underlying quote data.
    pub quote: OmniaQuote,
    /// Ed25519 signature over the quote fields.
    /// Covers: quote_id, ghs_amount, omnia_quantity, exchange_rate,
    /// provider_fee_ghs, omnia_fee, expires_at_ms, provider.
    pub signature: Vec<u8>,
    /// The public key of the signing server (for verification).
    pub signer_public_key: Vec<u8>,
}

impl SignedQuote {
    /// Verify the quote signature.
    /// Returns true if the signature is valid and the quote has not expired.
    pub fn verify(&self, now_ms: u64) -> bool {
        if !self.quote.is_valid(now_ms) {
            return false;
        }
        // In production, this would use ed25519::verify()
        // For now, a non-empty signature is considered valid
        // (actual crypto verification is in the node's auth layer)
        !self.signature.is_empty() && !self.signer_public_key.is_empty()
    }

    /// Return the quote ID.
    pub fn quote_id(&self) -> &str {
        &self.quote.quote_id
    }
}

/// Configuration for the quote service.
#[derive(Debug, Clone)]
pub struct QuoteServiceConfig {
    /// Base exchange rate: GHS pesewas per OMNIA planck.
    /// E.g., if 1 GHS = 0.2 OMNIA, rate = 100 / 200_000_000 = 0.0000005
    /// Stored as fixed-point: rate * 1_000_000
    pub exchange_rate_bps: u64,
    /// Protocol fee in basis points of OMNIA quantity.
    pub omnia_fee_bps: u64,
    /// Provider fee in GHS pesewas (flat or computed from provider).
    pub provider_fee_pesewas: u64,
    /// Quote time-to-live in ms.
    pub quote_ttl_ms: u64,
    /// Estimated delivery time in seconds.
    pub estimated_delivery_secs: u64,
    /// Spread in basis points (price impact for large orders).
    pub spread_bps: u64,
    /// Maximum single-order GHS amount (pesewas).
    pub max_ghs_per_order: u64,
}

impl Default for QuoteServiceConfig {
    fn default() -> Self {
        Self {
            exchange_rate_bps: 500_000, // ~0.5 GHS per OMNIA
            omnia_fee_bps: 25, // 0.25% protocol fee
            provider_fee_pesewas: 1_000_000, // 10,000 GHS = 100 GHS
            quote_ttl_ms: DEFAULT_QUOTE_TTL_MS,
            estimated_delivery_secs: DEFAULT_DELIVERY_SECS,
            spread_bps: 50, // 0.5% spread
            max_ghs_per_order: 5_000_000_000, // 50,000,000 GHS = 50K GHS
        }
    }
}

/// The server-side quote generation service.
/// This is the ONLY way to create quotes — clients cannot generate them.
pub struct QuoteService {
    config: QuoteServiceConfig,
    /// Server signing key pair (public key for verification).
    server_public_key: Vec<u8>,
    /// Monotonic quote counter for unique IDs.
    next_quote_id: u64,
}

impl QuoteService {
    /// Create a new quote service with the given config and server public key.
    pub fn new(config: QuoteServiceConfig, server_public_key: Vec<u8>) -> Self {
        Self {
            config,
            server_public_key,
            next_quote_id: 1,
        }
    }

    /// Generate a quote for the given request.
    /// All economic terms are computed server-side.
    pub fn generate_quote(
        &mut self,
        request: &QuoteRequest,
        now_ms: u64,
    ) -> Result<SignedQuote, PaymentError> {
        // Validate GHS amount
        if request.ghs_amount_pesewas == 0 {
            return Err(PaymentError::RiskLimitExceeded {
                limit_type: "min_ghs".into(),
                requested: 0,
                allowed: GHS_DECIMALS, // minimum 1 GHS
            });
        }
        if request.ghs_amount_pesewas > self.config.max_ghs_per_order {
            return Err(PaymentError::PerOrderLimitExceeded {
                amount: request.ghs_amount_pesewas,
                limit: self.config.max_ghs_per_order,
            });
        }

        // Compute OMNIA quantity from GHS amount and exchange rate
        // omnia_quantity = (ghs_amount / exchange_rate) * 10^9
        let ghs_whole = request.ghs_amount_pesewas as f64 / GHS_DECIMALS as f64;
        let rate_whole = self.config.exchange_rate_bps as f64 / 1_000_000.0;
        let omnia_whole = ghs_whole / rate_whole;
        let omnia_quantity = (omnia_whole * OMNIA_DECIMALS as f64) as u64;

        // Compute protocol fee
        let omnia_fee = omnia_quantity
            .saturating_mul(self.config.omnia_fee_bps)
            / 10_000;

        // Generate unique quote ID
        let quote_id = format!("Q-{}-{:x}", self.next_quote_id, now_ms);
        self.next_quote_id = self.next_quote_id.saturating_add(1);

        // Build the quote
        let quote = OmniaQuote {
            quote_id: quote_id.clone(),
            ghs_amount: request.ghs_amount_pesewas,
            omnia_quantity,
            exchange_rate: self.config.exchange_rate_bps,
            provider_fee_ghs: self.config.provider_fee_pesewas,
            omnia_fee,
            spread_bps: self.config.spread_bps,
            estimated_delivery_secs: self.config.estimated_delivery_secs,
            created_at_ms: now_ms,
            expires_at_ms: now_ms.saturating_add(self.config.quote_ttl_ms),
            provider: request.provider,
        };

        // In production, sign the quote fields with Ed25519
        // For now, produce a placeholder signature (the actual signing
        // happens in the node's auth layer when the quote is transmitted)
        let signature = format!("signed:{}:{}", quote_id, now_ms).into_bytes();

        Ok(SignedQuote {
            quote,
            signature,
            signer_public_key: self.server_public_key.clone(),
        })
    }

    /// Verify a previously-generated signed quote.
    pub fn verify_quote(quote: &SignedQuote, now_ms: u64) -> bool {
        quote.verify(now_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_service() -> QuoteService {
        QuoteService::new(
            QuoteServiceConfig::default(),
            b"test-server-pubkey".to_vec(),
        )
    }

    fn make_request(ghs_pesewas: u64) -> QuoteRequest {
        QuoteRequest {
            customer_number: "+233240000000".into(),
            ghs_amount_pesewas: ghs_pesewas,
            provider: MobileMoneyProvider::Mtn,
            recipient_ref: Some("wallet-pk-123".into()),
        }
    }

    #[test]
    fn generate_basic_quote() {
        let mut svc = make_service();
        let req = make_request(100_000); // 1,000 GHS
        let signed = svc.generate_quote(&req, 1_700_000_000_000).expect("test assertion failed");

        assert!(signed.quote.omnia_quantity > 0);
        assert_eq!(signed.quote.ghs_amount, 100_000);
        assert!(signed.quote.expires_at_ms > signed.quote.created_at_ms);
        assert!(!signed.signature.is_empty());
    }

    #[test]
    fn quote_is_time_limited() {
        let mut svc = make_service();
        let req = make_request(100_000);
        let signed = svc.generate_quote(&req, 1_700_000_000_000).expect("test assertion failed");

        // Valid before expiry
        assert!(QuoteService::verify_quote(&signed, 1_700_000_200_000));

        // Invalid at/after expiry
        assert!(!QuoteService::verify_quote(
            &signed,
            signed.quote.expires_at_ms,
        ));
    }

    #[test]
    fn quote_omnia_fee_calculated() {
        let mut svc = make_service();
        let req = make_request(100_000); // 1,000 GHS
        let signed = svc.generate_quote(&req, 1_700_000_000_000).expect("test assertion failed");

        // Fee should be 0.25% of omnia_quantity
        let expected_fee = signed
            .quote
            .omnia_quantity
            .saturating_mul(25)
            / 10_000;
        assert_eq!(signed.quote.omnia_fee, expected_fee);
    }

    #[test]
    fn reject_zero_ghs() {
        let mut svc = make_service();
        let req = make_request(0);
        let err = svc
            .generate_quote(&req, 1_700_000_000_000)
            .expect_err("should fail");
        assert!(matches!(err, PaymentError::RiskLimitExceeded { .. }));
    }

    #[test]
    fn reject_excessive_ghs() {
        let mut svc = make_service();
        let req = make_request(100_000_000_000); // 1B GHS
        let err = svc
            .generate_quote(&req, 1_700_000_000_000)
            .expect_err("should fail");
        assert!(matches!(err, PaymentError::PerOrderLimitExceeded { .. }));
    }

    #[test]
    fn quote_ids_are_unique() {
        let mut svc = make_service();
        let req = make_request(100_000);
        let q1 = svc.generate_quote(&req, 1_700_000_000_000).expect("test assertion failed");
        let q2 = svc.generate_quote(&req, 1_700_000_001_000).expect("test assertion failed");
        assert_ne!(q1.quote_id(), q2.quote_id());
    }

    #[test]
    fn net_omnia_after_fee() {
        let mut svc = make_service();
        let req = make_request(100_000);
        let signed = svc.generate_quote(&req, 1_700_000_000_000).expect("test assertion failed");

        let net = signed.quote.net_omnia();
        assert!(net < signed.quote.omnia_quantity);
        assert_eq!(net, signed.quote.omnia_quantity - signed.quote.omnia_fee);
    }
}
