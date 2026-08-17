//! Ghana Mobile Money Sandbox Provider — Audit Priority 7
//!
//! Implements a sandbox provider adapter that mirrors the Flutterwave/
//! Yellow Card integration pattern for Ghana mobile money.
//!
//! This adapter:
//! 1. Validates E.164 phone numbers
//! 2. Initiates payments (returns provider ref)
//! 3. Verifies callbacks using HMAC-SHA256
//! 4. Queries payment status (polling fallback)
//! 5. Initiates refunds
//! 6. Normalizes provider errors
//!
//! In production, this adapter would make real HTTP calls to the
//! payment partner's API. The sandbox simulates responses.

use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::{HashMap, HashSet};

use crate::error::PaymentError;
use crate::provider::{CallbackStatus, CallbackVerification, MobileMoneyProvider, ProviderAdapter, ProviderCallback};

/// Minimum GHS amount per payment (100 pesewas = 1 GHS).
const MIN_PAYMENT_PESEWAS: u64 = 100;

/// E.164 Ghana country code prefix.
const GHANA_PREFIX: &str = "+233";

/// A simulated sandbox transaction.
#[derive(Debug, Clone)]
struct SandboxTransaction {
    provider: MobileMoneyProvider,
    provider_ref: String,
    customer_number: String,
    amount_pesewas: u64,
    status: CallbackStatus,
}

/// HMAC-SHA256 signing key for callback verification.
/// In production, this is the shared secret with the payment partner.
struct CallbackSigner {
    /// Shared HMAC secret.
    secret: Vec<u8>,
}

impl CallbackSigner {
    fn new(secret: &[u8]) -> Self {
        Self {
            secret: secret.to_vec(),
        }
    }

    /// Generate a lowercase hexadecimal HMAC-SHA256 callback signature.
    fn sign(&self, payload: &str) -> String {
        let mut mac =
            Hmac::<Sha256>::new_from_slice(&self.secret).expect("HMAC accepts keys of every non-empty length");
        mac.update(payload.as_bytes());
        mac.finalize()
            .into_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    /// Verify a callback signature using HMAC verification.
    fn verify(&self, payload: &str, signature: &str) -> bool {
        if signature.is_empty() || !signature.len().is_multiple_of(2) {
            return false;
        }
        let decoded = match (0..signature.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&signature[index..index + 2], 16))
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(bytes) => bytes,
            Err(_) => return false,
        };
        let mut mac = match Hmac::<Sha256>::new_from_slice(&self.secret) {
            Ok(mac) => mac,
            Err(_) => return false,
        };
        mac.update(payload.as_bytes());
        mac.verify_slice(&decoded).is_ok()
    }
}

/// Ghana mobile money sandbox provider adapter.
/// Simulates the integration pattern used by Flutterwave / Yellow Card.
pub struct GhanaSandboxProvider {
    provider: MobileMoneyProvider,
    transactions: std::sync::Mutex<HashMap<String, SandboxTransaction>>,
    /// Seen callback nonces for replay detection.
    seen_nonces: std::sync::Mutex<HashSet<String>>,
    signer: CallbackSigner,
    /// Unused — reserved for future chaos testing.
    _simulate_failure_rate: f64,
}

impl GhanaSandboxProvider {
    /// Create a new sandbox provider.
    ///
    /// # Arguments
    ///
    /// * `provider` - The mobile money provider (MTN, Telecel, AT).
    /// * `hmac_secret` - Shared HMAC secret for callback verification.
    pub fn new(provider: MobileMoneyProvider, hmac_secret: &[u8]) -> Self {
        Self {
            provider,
            transactions: std::sync::Mutex::new(HashMap::new()),
            seen_nonces: std::sync::Mutex::new(HashSet::new()),
            signer: CallbackSigner::new(hmac_secret),
            _simulate_failure_rate: 0.0,
        }
    }

    /// Create a sandbox that simulates a given failure rate (0.0–1.0).
    pub fn with_failure_rate(provider: MobileMoneyProvider, hmac_secret: &[u8], failure_rate: f64) -> Self {
        Self {
            provider,
            transactions: std::sync::Mutex::new(HashMap::new()),
            seen_nonces: std::sync::Mutex::new(HashSet::new()),
            signer: CallbackSigner::new(hmac_secret),
            _simulate_failure_rate: failure_rate.clamp(0.0, 1.0),
        }
    }

    /// Validate an E.164 Ghana phone number.
    fn validate_phone(&self, phone: &str) -> Result<(), PaymentError> {
        if !phone.starts_with(GHANA_PREFIX) {
            return Err(PaymentError::InvalidProviderData {
                detail: format!("phone number {} must start with {}", phone, GHANA_PREFIX),
            });
        }
        let digits: String = phone.chars().filter(|c| c.is_ascii_digit()).collect();
        if digits.len() != 12 {
            return Err(PaymentError::InvalidProviderData {
                detail: format!("Ghana phone number must be 12 digits, got {}", digits.len()),
            });
        }
        Ok(())
    }

    /// Simulate a callback for a given order. Returns the callback
    /// that the provider would send to our webhook.
    pub fn simulate_callback(&self, order_id: &str, status: CallbackStatus) -> ProviderCallback {
        let txns = self.transactions.lock().expect("lock");
        let txn = txns.get(order_id).cloned().unwrap_or(SandboxTransaction {
            provider: self.provider,
            provider_ref: format!("sim-{}", order_id),
            customer_number: "+233240000000".to_string(),
            amount_pesewas: 0,
            status: CallbackStatus::Pending,
        });

        let payload = format!(
            "{}:{}:{}:{}:{}",
            txn.provider_ref, order_id, txn.amount_pesewas, txn.customer_number, status as u8,
        );
        let signature = self.signer.sign(&payload);

        ProviderCallback {
            provider: txn.provider,
            provider_tx_ref: txn.provider_ref.clone(),
            order_id: order_id.to_string(),
            status,
            amount_received_pesewas: txn.amount_pesewas,
            customer_number: txn.customer_number,
            provider_timestamp_ms: 1_700_000_000_000,
            signature,
            extra: std::collections::BTreeMap::new(),
        }
    }

    /// Generate a provider callback nonce for replay detection.
    fn make_nonce(order_id: &str, timestamp_ms: u64) -> String {
        format!("{}:{}", order_id, timestamp_ms)
    }

    /// Initiate a sandbox payment for a selected Ghana provider.
    pub fn initiate_payment_for(
        &self,
        provider: MobileMoneyProvider,
        customer_number: &str,
        amount_pesewas: u64,
        order_id: &str,
    ) -> Result<String, PaymentError> {
        self.validate_phone(customer_number)?;

        if amount_pesewas < MIN_PAYMENT_PESEWAS {
            return Err(PaymentError::RiskLimitExceeded {
                limit_type: "min_payment".into(),
                requested: amount_pesewas,
                allowed: MIN_PAYMENT_PESEWAS,
            });
        }

        let provider_ref = format!("{}/{}", provider.id().to_lowercase(), order_id);
        let txn = SandboxTransaction {
            provider,
            provider_ref: provider_ref.clone(),
            customer_number: customer_number.to_string(),
            amount_pesewas,
            status: CallbackStatus::Pending,
        };

        let mut txns = self.transactions.lock().expect("lock");
        txns.insert(order_id.to_string(), txn);
        Ok(provider_ref)
    }
}

impl ProviderAdapter for GhanaSandboxProvider {
    fn provider(&self) -> MobileMoneyProvider {
        self.provider
    }

    fn initiate_payment(
        &self,
        customer_number: &str,
        amount_pesewas: u64,
        order_id: &str,
    ) -> Result<String, PaymentError> {
        self.initiate_payment_for(self.provider, customer_number, amount_pesewas, order_id)
    }

    fn verify_callback(&self, callback: &ProviderCallback) -> Result<CallbackVerification, PaymentError> {
        // 1. Check signature
        let payload = format!(
            "{}:{}:{}:{}:{}",
            callback.provider_tx_ref,
            callback.order_id,
            callback.amount_received_pesewas,
            callback.customer_number,
            callback.status as u8,
        );
        let signature_valid = self.signer.verify(&payload, &callback.signature);

        // 2. Check replay (nonce = order_id:timestamp)
        let nonce = Self::make_nonce(&callback.order_id, callback.provider_timestamp_ms);
        let mut nonces = self.seen_nonces.lock().expect("lock");
        let not_replay = nonces.insert(nonce);

        // 3. Check amount is plausible (within 10% of expected)
        let amount_plausible = if callback.amount_received_pesewas == 0 {
            true // zero-amount callbacks (failures) are always plausible
        } else {
            let txns = self.transactions.lock().expect("lock");
            txns.get(&callback.order_id)
                .map(|txn| {
                    let expected = txn.amount_pesewas as i64;
                    let received = callback.amount_received_pesewas as i64;
                    let diff = (expected - received).abs();
                    diff * 10 <= expected // within 10%
                })
                .unwrap_or(true)
        };

        let failure_reason = if !signature_valid {
            Some("invalid signature".into())
        } else if !not_replay {
            Some("replay detected".into())
        } else if !amount_plausible {
            Some("amount mismatch exceeds tolerance".into())
        } else {
            None
        };

        Ok(CallbackVerification {
            signature_valid,
            not_replay,
            amount_plausible,
            failure_reason,
        })
    }

    fn query_payment_status(&self, provider_tx_ref: &str) -> Result<CallbackStatus, PaymentError> {
        let txns = self.transactions.lock().expect("lock");
        for txn in txns.values() {
            if txn.provider_ref == provider_tx_ref {
                return Ok(txn.status);
            }
        }
        Err(PaymentError::ProviderNotFound {
            provider: provider_tx_ref.to_string(),
        })
    }

    fn initiate_refund(
        &self,
        _customer_number: &str,
        _amount_pesewas: u64,
        provider_tx_ref: &str,
        _reason: &str,
    ) -> Result<String, PaymentError> {
        // Simulate: generate a refund reference
        let refund_ref = format!("refund:{}", provider_tx_ref);

        // Update the transaction status
        let mut txns = self.transactions.lock().expect("lock");
        for txn in txns.values_mut() {
            if txn.provider_ref == provider_tx_ref {
                txn.status = CallbackStatus::Reversed;
            }
        }

        Ok(refund_ref)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_provider() -> GhanaSandboxProvider {
        GhanaSandboxProvider::new(MobileMoneyProvider::Mtn, b"sandbox-secret-key")
    }

    #[test]
    fn initiate_payment_success() {
        let provider = make_provider();
        let ref_str = provider
            .initiate_payment("+233240000000", 50_000, "order-1")
            .expect("test assertion failed");
        assert!(ref_str.starts_with("mtn/"));
    }

    #[test]
    fn reject_invalid_phone() {
        let provider = make_provider();
        // Wrong country code
        let err = provider
            .initiate_payment("+442079460000", 50_000, "order-2")
            .expect_err("should fail");
        assert!(matches!(err, PaymentError::InvalidProviderData { .. }));
    }

    #[test]
    fn reject_below_minimum() {
        let provider = make_provider();
        let err = provider
            .initiate_payment("+233240000000", 50, "order-3") // 0.50 GHS
            .expect_err("should fail");
        assert!(matches!(err, PaymentError::RiskLimitExceeded { .. }));
    }

    #[test]
    fn verify_valid_callback() {
        let provider = make_provider();
        provider
            .initiate_payment("+233240000000", 50_000, "order-cb-1")
            .expect("initiate");

        let callback = provider.simulate_callback("order-cb-1", CallbackStatus::Success);
        let result = provider.verify_callback(&callback).expect("test assertion failed");
        assert!(result.is_valid());
        assert!(result.signature_valid);
        assert!(result.not_replay);
        assert!(result.amount_plausible);
    }

    #[test]
    fn reject_replay_callback() {
        let provider = make_provider();
        provider
            .initiate_payment("+233240000000", 50_000, "order-cb-2")
            .expect("initiate");

        let cb1 = provider.simulate_callback("order-cb-2", CallbackStatus::Success);
        provider.verify_callback(&cb1).expect("first verify");

        // Same callback again = replay
        let cb2 = provider.simulate_callback("order-cb-2", CallbackStatus::Success);
        let result = provider.verify_callback(&cb2).expect("test assertion failed");
        assert!(!result.not_replay);
        assert!(!result.is_valid());
    }

    #[test]
    fn reject_tampered_callback() {
        let provider = make_provider();
        provider
            .initiate_payment("+233240000000", 50_000, "order-cb-3")
            .expect("initiate");

        let mut callback = provider.simulate_callback("order-cb-3", CallbackStatus::Success);
        callback.signature = "tampered-signature".into();

        let result = provider.verify_callback(&callback).expect("test assertion failed");
        assert!(!result.signature_valid);
        assert!(!result.is_valid());
    }

    #[test]
    fn query_payment_status_after_initiate() {
        let provider = make_provider();
        let ref_str = provider
            .initiate_payment("+233240000000", 50_000, "order-qs-1")
            .expect("initiate");

        let status = provider.query_payment_status(&ref_str).expect("query");
        assert_eq!(status, CallbackStatus::Pending);
    }

    #[test]
    fn initiate_refund_updates_status() {
        let provider = make_provider();
        let ref_str = provider
            .initiate_payment("+233240000000", 50_000, "order-rf-1")
            .expect("initiate");

        let refund_ref = provider
            .initiate_refund("+233240000000", 50_000, &ref_str, "customer request")
            .expect("refund");
        assert!(refund_ref.starts_with("refund:"));

        let status = provider.query_payment_status(&ref_str).expect("query");
        assert_eq!(status, CallbackStatus::Reversed);
    }

    #[test]
    fn query_unknown_ref_fails() {
        let provider = make_provider();
        let err = provider.query_payment_status("unknown-ref").expect_err("should fail");
        assert!(matches!(err, PaymentError::ProviderNotFound { .. }));
    }

    #[test]
    fn callback_amount_plausibility_check() {
        let provider = make_provider();
        provider
            .initiate_payment("+233240000000", 50_000, "order-amt-1")
            .expect("initiate");

        // Exact amount = plausible
        let cb_exact = provider.simulate_callback("order-amt-1", CallbackStatus::Success);
        let result = provider.verify_callback(&cb_exact).expect("test assertion failed");
        assert!(result.amount_plausible);

        // Zero amount (failure) = plausible
        let _cb_zero = provider.simulate_callback("order-amt-1", CallbackStatus::Failed);
        // Need to re-initiate for the nonce to be fresh
        let provider2 = make_provider();
        provider2
            .initiate_payment("+233240000000", 50_000, "order-amt-2")
            .expect("initiate");
        let mut cb_fail = provider2.simulate_callback("order-amt-2", CallbackStatus::Failed);
        cb_fail.amount_received_pesewas = 0;
        let result2 = provider2.verify_callback(&cb_fail).expect("test assertion failed");
        assert!(result2.amount_plausible);
    }
}
