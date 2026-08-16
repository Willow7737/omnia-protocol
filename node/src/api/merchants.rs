//! Merchant onboarding and settlement-facing API — Financial Specification §9.
//!
//! The first pilot records merchant prices in GHS, generates a time-limited
//! OMNIA quote, and returns a QR/invoice payload. It does not promise GHS
//! redemption. Merchant confirmation is a service operation derived from an
//! authenticated delivery key, not from a customer-controlled status field.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::state::AppState;
use omnia_payment_order::auth::{Credential, ServiceRoleRegistry};
use omnia_payment_order::merchant::{
    BusinessCategory, MerchantPayment, MerchantPaymentStatus, MerchantProfile, MerchantReceipt, MerchantRiskTier,
    OnboardingGrant, SettlementPreference,
};
use omnia_payment_order::{MobileMoneyProvider, QuoteRequest};

/// Runtime merchant record. A durable deployment should replace this store
/// with an append-only database while preserving these domain boundaries.
#[derive(Debug, Default)]
pub struct MerchantRegistry {
    next_merchant_id: u64,
    next_payment_id: u64,
    profiles: HashMap<String, MerchantRecord>,
}

#[derive(Debug)]
struct MerchantRecord {
    owner_did: String,
    settlement_public_key: Option<String>,
    profile: MerchantProfile,
    payments: Vec<MerchantPayment>,
    receipts: HashMap<String, MerchantReceipt>,
}

impl MerchantRegistry {
    /// Create an empty merchant registry.
    pub fn new() -> Self {
        Self::default()
    }

    fn insert_profile(
        &mut self,
        owner_did: String,
        settlement_public_key: Option<String>,
        mut profile: MerchantProfile,
    ) -> MerchantProfile {
        self.next_merchant_id = self.next_merchant_id.saturating_add(1);
        profile.merchant_id = format!("M-{}", self.next_merchant_id);
        self.profiles.insert(
            profile.merchant_id.clone(),
            MerchantRecord {
                owner_did,
                settlement_public_key,
                profile: profile.clone(),
                payments: Vec::new(),
                receipts: HashMap::new(),
            },
        );
        profile
    }
}

/// Merchant registration request.
#[derive(Debug, Clone, Deserialize)]
pub struct MerchantRegistrationRequest {
    /// Business display name.
    pub business_name: String,
    /// Business category.
    pub category: BusinessCategory,
    /// Preferred settlement mode.
    pub settlement_preference: SettlementPreference,
    /// Merchant support contact.
    pub support_contact: String,
    /// Optional Ed25519 public key that receives OMNIA settlement.
    pub settlement_public_key: Option<String>,
}

/// Merchant payment-request body.
#[derive(Debug, Clone, Deserialize)]
pub struct MerchantPaymentRequestBody {
    /// GHS price in pesewas.
    pub ghs_price_pesewas: u64,
}

/// Service-only merchant confirmation body.
#[derive(Debug, Clone, Deserialize)]
pub struct ConfirmMerchantPaymentRequest {
    /// Chain transaction reference supplied by the delivery service.
    pub tx_reference: Option<String>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn api_error(status: StatusCode, detail: impl Into<String>) -> (StatusCode, Json<Value>) {
    (status, Json(json!({ "error": detail.into() })))
}

fn service_caller(
    registry: &ServiceRoleRegistry,
    headers: &HeaderMap,
    now_ms: u64,
) -> Result<omnia_payment_order::engine::Caller, (StatusCode, Json<Value>)> {
    let key_id = headers
        .get("x-omnia-service-key")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let service_name = headers
        .get("x-omnia-service-name")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    registry
        .resolve(
            &Credential::ApiKey {
                key_id: key_id.into(),
                service_name: service_name.into(),
            },
            now_ms,
        )
        .map_err(|error| api_error(StatusCode::UNAUTHORIZED, error.to_string()))
}

/// `POST /api/v1/merchants/register` — onboard a merchant.
pub async fn register_merchant(
    State(state): State<AppState>,
    axum::Extension(caller): axum::Extension<crate::api::auth::CallerIdentity>,
    Json(body): Json<MerchantRegistrationRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    if body.business_name.trim().is_empty() || body.support_contact.trim().is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "business_name and support_contact are required",
        ));
    }
    let profile = MerchantProfile {
        merchant_id: String::new(),
        business_name: body.business_name,
        category: body.category,
        settlement_preference: body.settlement_preference,
        support_contact: body.support_contact,
        risk_tier: MerchantRiskTier::Standard,
        daily_limit: 1_000_000_000_000_000,
        per_tx_limit: 100_000_000_000,
        active: true,
        onboarding_grant: Some(OnboardingGrant {
            amount: 0,
            purpose: "pilot onboarding record; no automatic OMNIA grant".into(),
            start_ms: now_ms(),
            end_ms: now_ms(),
            milestones: Vec::new(),
            grant_id: format!("GRANT-PENDING-{}", caller.caller_id),
        }),
    };
    if let Some(public_key) = &body.settlement_public_key {
        if !public_key.is_empty() && !public_key.chars().all(|character| character.is_ascii_hexdigit()) {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "settlement_public_key must be hexadecimal",
            ));
        }
    }
    let settlement_public_key = body.settlement_public_key.clone();
    let profile = state
        .merchant_registry
        .lock()
        .map_err(|error| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("merchant registry lock poisoned: {error}"),
            )
        })?
        .insert_profile(caller.caller_id, settlement_public_key, profile);
    Ok((StatusCode::CREATED, Json(json!({ "merchant": profile }))))
}

/// `POST /api/v1/merchants/:id/payment-request` — create a QR/invoice.
pub async fn create_payment_request(
    State(state): State<AppState>,
    axum::Extension(caller): axum::Extension<crate::api::auth::CallerIdentity>,
    Path(merchant_id): Path<String>,
    Json(body): Json<MerchantPaymentRequestBody>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    if body.ghs_price_pesewas == 0 {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "ghs_price_pesewas must be greater than zero",
        ));
    }
    let quote = {
        let mut quote_service = state.quote_service.lock().map_err(|error| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("quote service lock poisoned: {error}"),
            )
        })?;
        quote_service
            .generate_quote(
                &QuoteRequest {
                    customer_number: caller.caller_id.clone(),
                    ghs_amount_pesewas: body.ghs_price_pesewas,
                    provider: MobileMoneyProvider::Mtn,
                    recipient_ref: Some(merchant_id.clone()),
                },
                now_ms(),
            )
            .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?
    };
    let mut merchants = state.merchant_registry.lock().map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("merchant registry lock poisoned: {error}"),
        )
    })?;
    merchants.next_payment_id = merchants.next_payment_id.saturating_add(1);
    let payment_id = format!("MP-{}", merchants.next_payment_id);
    let record = merchants
        .profiles
        .get_mut(&merchant_id)
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "merchant not found"))?;
    if !record.profile.active || record.owner_did != caller.caller_id {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "caller is not the active merchant owner",
        ));
    }
    let payment = MerchantPayment {
        payment_id: payment_id.clone(),
        merchant_id: merchant_id.clone(),
        customer_wallet: caller.caller_id.clone(),
        ghs_price: body.ghs_price_pesewas,
        omnia_amount: quote.quote.omnia_quantity,
        exchange_rate: quote.quote.exchange_rate,
        quote_expiry_ms: quote.quote.expires_at_ms,
        protocol_fee: quote.quote.omnia_fee,
        status: MerchantPaymentStatus::Pending,
        created_at_ms: now_ms(),
    };
    record.payments.push(payment.clone());
    let qr_payload = json!({
        "protocol": "omnia",
        "version": 1,
        "merchant_id": merchant_id,
        "payment_id": payment_id,
        "ghs_price_pesewas": body.ghs_price_pesewas,
        "omnia_amount_plancks": quote.quote.omnia_quantity,
        "quote_expiry_ms": quote.quote.expires_at_ms,
        "merchant_public_key": record.settlement_public_key,
    });
    Ok((
        StatusCode::CREATED,
        Json(json!({ "payment_request": payment, "qr_payload": qr_payload, "signed_quote": quote })),
    ))
}

/// `GET /api/v1/merchants/:id/payments` — list merchant payments.
pub async fn list_payments(
    State(state): State<AppState>,
    axum::Extension(caller): axum::Extension<crate::api::auth::CallerIdentity>,
    Path(merchant_id): Path<String>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let merchants = state.merchant_registry.lock().map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("merchant registry lock poisoned: {error}"),
        )
    })?;
    let record = merchants
        .profiles
        .get(&merchant_id)
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "merchant not found"))?;
    if record.owner_did != caller.caller_id {
        return Err(api_error(StatusCode::FORBIDDEN, "caller is not the merchant owner"));
    }
    Ok((
        StatusCode::OK,
        Json(json!({ "merchant_id": merchant_id, "payments": record.payments })),
    ))
}

/// `POST /api/v1/merchants/:id/payments/:payment_id/confirm` — delivery-service confirmation.
pub async fn confirm_payment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((merchant_id, payment_id)): Path<(String, String)>,
    Json(body): Json<ConfirmMerchantPaymentRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let caller = service_caller(&state.service_role_registry, &headers, now_ms())?;
    if !matches!(caller, omnia_payment_order::engine::Caller::System { ref service } if service == "delivery-service") {
        return Err(api_error(StatusCode::FORBIDDEN, "delivery-service role required"));
    }
    let mut merchants = state.merchant_registry.lock().map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("merchant registry lock poisoned: {error}"),
        )
    })?;
    let record = merchants
        .profiles
        .get_mut(&merchant_id)
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "merchant not found"))?;
    let payment = record
        .payments
        .iter_mut()
        .find(|payment| payment.payment_id == payment_id)
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "payment not found"))?;
    if payment.status == MerchantPaymentStatus::Confirmed {
        let receipt = record
            .receipts
            .get(&payment_id)
            .cloned()
            .ok_or_else(|| api_error(StatusCode::CONFLICT, "confirmed payment has no receipt"))?;
        return Ok((StatusCode::OK, Json(json!({ "idempotent": true, "receipt": receipt }))));
    }
    payment.status = MerchantPaymentStatus::Confirmed;
    let receipt = MerchantReceipt {
        payment_id: payment.payment_id.clone(),
        merchant_id: payment.merchant_id.clone(),
        ghs_price: payment.ghs_price,
        omnia_amount: payment.omnia_amount,
        exchange_rate: payment.exchange_rate,
        protocol_fee: payment.protocol_fee,
        net_omnia: payment.omnia_amount.saturating_sub(payment.protocol_fee),
        confirmed_at_ms: now_ms(),
        tx_reference: body
            .tx_reference
            .or_else(|| Some(format!("merchant-confirm:{}", payment.payment_id))),
    };
    record.receipts.insert(payment_id, receipt.clone());
    Ok((StatusCode::OK, Json(json!({ "receipt": receipt }))))
}

/// `GET /api/v1/merchants/:id/receipt/:payment_id` — return a confirmed receipt.
pub async fn get_receipt(
    State(state): State<AppState>,
    axum::Extension(caller): axum::Extension<crate::api::auth::CallerIdentity>,
    Path((merchant_id, payment_id)): Path<(String, String)>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let merchants = state.merchant_registry.lock().map_err(|error| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("merchant registry lock poisoned: {error}"),
        )
    })?;
    let record = merchants
        .profiles
        .get(&merchant_id)
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "merchant not found"))?;
    if record.owner_did != caller.caller_id {
        return Err(api_error(StatusCode::FORBIDDEN, "caller is not the merchant owner"));
    }
    let receipt = record.receipts.get(&payment_id).ok_or_else(|| {
        api_error(
            StatusCode::NOT_FOUND,
            "receipt not available until payment confirmation",
        )
    })?;
    Ok((StatusCode::OK, Json(json!({ "receipt": receipt }))))
}
