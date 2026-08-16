//! Authenticated payment-order API — Financial Specification §8.
//!
//! The public wallet contract deliberately separates economic terms from payment
//! authorization: the quote endpoint accepts only the requested purchase, the
//! initiate endpoint accepts only a server-generated quote ID, and provider
//! callbacks must pass HMAC verification, replay protection, and service-key
//! authorization before they can move an order.

use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::auth::CallerIdentity;
use crate::state::AppState;
use omnia_payment_order::auth::{Credential, ServiceRoleRegistry};
use omnia_payment_order::engine::Caller;
use omnia_payment_order::{
    CallbackStatus, MobileMoneyProvider, PaymentState, ProviderAdapter, ProviderCallback, QuoteRequest,
};

/// Request for a server-generated OMNIA quote.
#[derive(Debug, Clone, Deserialize)]
pub struct QuoteRequestBody {
    /// Customer mobile-money number in Ghana E.164 form.
    pub customer_number: String,
    /// Requested GHS amount in pesewas.
    pub ghs_amount_pesewas: u64,
    /// Ghana mobile-money provider.
    pub provider: MobileMoneyProvider,
}

/// Request to initiate a payment from a server-generated quote.
#[derive(Debug, Clone, Deserialize)]
pub struct InitiatePaymentRequest {
    /// Previously issued quote ID.
    pub quote_id: String,
    /// Customer mobile-money number. It must match the quote context.
    pub customer_number: String,
}

/// Request for an authenticated internal state transition.
#[derive(Debug, Clone, Deserialize)]
pub struct InternalAdvanceRequest {
    /// Target state. The caller is derived from the service API key, never from this body.
    pub next_state: String,
    /// Optional operational reason.
    pub reason: Option<String>,
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

fn persist_order(state: &AppState, order_id: &str) -> Result<(), String> {
    let order = {
        let engine = state
            .payment_engine
            .lock()
            .map_err(|error| format!("payment engine lock poisoned: {error}"))?;
        engine.get_order(order_id).map_err(|error| error.to_string())?.clone()
    };
    for event in &order.event_history {
        state
            .payment_store
            .append_event(event)
            .map_err(|error| error.to_string())?;
    }
    state
        .payment_store
        .save_order_snapshot(&order)
        .map_err(|error| error.to_string())
}

fn order_json(order: &omnia_payment_order::PaymentOrder) -> Value {
    json!({
        "order_id": order.order_id,
        "state": order.state.label(),
        "customer_ref": order.customer_ref,
        "recipient_ref": order.recipient_ref,
        "asset_id": format!("{:?}", order.asset_id),
        "ghs_amount_pesewas": order.ghs_amount,
        "ghs_received_pesewas": order.ghs_received,
        "omnia_quantity_plancks": order.omnia_quantity,
        "exchange_rate": order.exchange_rate,
        "provider_fee_pesewas": order.provider_fee,
        "omnia_fee_plancks": order.omnia_fee,
        "provider_name": order.provider_name,
        "provider_ref": order.provider_ref,
        "inventory_reservation_ref": order.inventory_reservation_ref,
        "is_terminal": order.is_terminal(),
        "is_economically_delivered": order.is_economically_delivered(),
        "event_count": order.event_history.len(),
        "created_at_ms": order.created_at_ms,
        "updated_at_ms": order.updated_at_ms,
        "quote_timestamp_ms": order.quote_timestamp_ms,
        "quote_expiry_ms": order.quote_expiry_ms,
        "refund_status": format!("{:?}", order.refund_status),
    })
}

fn read_order(state: &AppState, order_id: &str) -> Result<omnia_payment_order::PaymentOrder, String> {
    let engine = state
        .payment_engine
        .lock()
        .map_err(|error| format!("payment engine lock poisoned: {error}"))?;
    engine
        .get_order(order_id)
        .map(|order| order.clone())
        .map_err(|error| error.to_string())
}

fn advance_and_persist(
    state: &AppState,
    order_id: &str,
    next_state: PaymentState,
    caller: Caller,
    reason: Option<String>,
    at_ms: u64,
) -> Result<(), String> {
    {
        let mut engine = state
            .payment_engine
            .lock()
            .map_err(|error| format!("payment engine lock poisoned: {error}"))?;
        engine
            .advance_state(order_id, next_state, caller, at_ms, reason)
            .map_err(|error| error.to_string())?;
    }
    persist_order(state, order_id)
}

/// `POST /api/v1/payment-orders/quote` — generate a signed quote.
pub async fn request_quote(
    State(state): State<AppState>,
    Extension(caller): Extension<CallerIdentity>,
    Json(body): Json<QuoteRequestBody>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    if body.customer_number.trim().is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "customer_number is required"));
    }
    let request = QuoteRequest {
        customer_number: body.customer_number,
        ghs_amount_pesewas: body.ghs_amount_pesewas,
        provider: body.provider,
        recipient_ref: Some(caller.caller_id.clone()),
    };
    let at_ms = now_ms();
    let signed_quote = {
        let mut service = state.quote_service.lock().map_err(|error| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("quote service lock poisoned: {error}"),
            )
        })?;
        service
            .generate_quote(&request, at_ms)
            .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?
    };
    let disclosures = signed_quote
        .quote
        .disclosure_fields()
        .into_iter()
        .map(|(label, value)| json!({ "label": label, "value": value }))
        .collect::<Vec<_>>();
    Ok((
        StatusCode::OK,
        Json(json!({
            "quote": signed_quote.quote,
            "signature": signed_quote.signature,
            "signer_public_key": signed_quote.signer_public_key,
            "disclosure_fields": disclosures,
            "total_ghs_cost_pesewas": signed_quote.quote.total_ghs_cost(),
            "net_omnia_plancks": signed_quote.quote.net_omnia(),
        })),
    ))
}

/// `POST /api/v1/payment-orders/initiate` — create and start an order from a quote.
pub async fn initiate_payment(
    State(state): State<AppState>,
    Extension(caller): Extension<CallerIdentity>,
    Json(body): Json<InitiatePaymentRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let at_ms = now_ms();
    let (signed_quote, bound_customer, bound_recipient, quote_used) = {
        let service = state.quote_service.lock().map_err(|error| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("quote service lock poisoned: {error}"),
            )
        })?;
        let quote = service
            .get_quote(&body.quote_id)
            .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "quote not found"))?;
        let (customer, recipient) = service
            .get_quote_context(&body.quote_id)
            .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "quote context not found"))?;
        (quote, customer, recipient, service.is_quote_used(&body.quote_id))
    };

    if !signed_quote.verify(at_ms) {
        return Err(api_error(StatusCode::CONFLICT, "quote is invalid or expired"));
    }
    if body.customer_number != bound_customer {
        return Err(api_error(StatusCode::FORBIDDEN, "customer number does not match quote"));
    }
    if bound_recipient.as_deref() != Some(caller.caller_id.as_str()) {
        return Err(api_error(StatusCode::FORBIDDEN, "quote belongs to a different wallet"));
    }
    if quote_used {
        let order_id = format!("PO-{}", body.quote_id);
        let order = read_order(&state, &order_id).map_err(|error| api_error(StatusCode::CONFLICT, error))?;
        return Ok((
            StatusCode::OK,
            Json(json!({ "idempotent": true, "order": order_json(&order) })),
        ));
    }

    let recipient_ref = bound_recipient.unwrap_or_else(|| caller.caller_id.clone());
    let order_id = format!("PO-{}", body.quote_id);
    {
        let mut engine = state.payment_engine.lock().map_err(|error| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("payment engine lock poisoned: {error}"),
            )
        })?;
        engine
            .create_order_from_quote(
                order_id.clone(),
                body.customer_number.clone(),
                recipient_ref,
                &signed_quote.quote,
                at_ms,
            )
            .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    }
    persist_order(&state, &order_id).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error))?;

    {
        let mut service = state.quote_service.lock().map_err(|error| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("quote service lock poisoned: {error}"),
            )
        })?;
        service
            .mark_quote_used(&body.quote_id)
            .map_err(|error| api_error(StatusCode::CONFLICT, error.to_string()))?;
    }

    advance_and_persist(
        &state,
        &order_id,
        PaymentState::Quoted,
        Caller::System {
            service: "quote-service".into(),
        },
        Some(body.quote_id.clone()),
        at_ms,
    )
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error))?;
    advance_and_persist(
        &state,
        &order_id,
        PaymentState::PaymentPending,
        Caller::System {
            service: "payment-service".into(),
        },
        None,
        at_ms,
    )
    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error))?;

    let provider_ref = match state.ghana_provider.initiate_payment_for(
        signed_quote.quote.provider,
        &body.customer_number,
        signed_quote.quote.total_ghs_cost(),
        &order_id,
    ) {
        Ok(provider_ref) => provider_ref,
        Err(error) => {
            let _ = advance_and_persist(
                &state,
                &order_id,
                PaymentState::PaymentFailed,
                Caller::Provider {
                    provider_id: signed_quote.quote.provider.id().into(),
                    authenticated: true,
                },
                Some(error.to_string()),
                at_ms,
            );
            return Err(api_error(StatusCode::BAD_GATEWAY, error.to_string()));
        }
    };
    {
        let mut engine = state.payment_engine.lock().map_err(|error| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("payment engine lock poisoned: {error}"),
            )
        })?;
        engine
            .set_provider_ref(&order_id, provider_ref.clone())
            .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    }
    persist_order(&state, &order_id).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error))?;
    let order = read_order(&state, &order_id).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error))?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "order": order_json(&order),
            "provider_ref": provider_ref,
            "next_step": "authorize_mobile_money_payment",
        })),
    ))
}

/// `POST /api/v1/payment-orders/callback` — authenticated Ghana provider webhook.
pub async fn provider_callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(callback): Json<ProviderCallback>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let provider_key = headers
        .get("x-omnia-provider-key")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !state
        .service_role_registry
        .is_authorized_provider_callback(callback.provider.id(), provider_key)
    {
        return Err(api_error(
            StatusCode::UNAUTHORIZED,
            "provider service key is not authorized",
        ));
    }

    let verification = state
        .ghana_provider
        .verify_callback(&callback)
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error.to_string()))?;
    if !verification.is_valid() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            verification
                .failure_reason
                .unwrap_or_else(|| "callback rejected".into()),
        ));
    }

    let at_ms = now_ms();
    let current = read_order(&state, &callback.order_id).map_err(|error| api_error(StatusCode::NOT_FOUND, error))?;
    if current.provider_name != callback.provider.id() || current.customer_ref != callback.customer_number {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "callback provider or customer does not match order",
        ));
    }
    if let Some(provider_ref) = &current.provider_ref {
        if provider_ref != &callback.provider_tx_ref {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "provider transaction reference does not match order",
            ));
        }
    }

    match callback.status {
        CallbackStatus::Pending => {}
        CallbackStatus::Failed => {
            if current.state == PaymentState::PaymentPending {
                advance_and_persist(
                    &state,
                    &callback.order_id,
                    PaymentState::PaymentFailed,
                    Caller::Provider {
                        provider_id: callback.provider.id().into(),
                        authenticated: true,
                    },
                    Some("provider reported failure".into()),
                    at_ms,
                )
                .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error))?;
            }
        }
        CallbackStatus::Timeout => {
            if current.state == PaymentState::PaymentPending {
                advance_and_persist(
                    &state,
                    &callback.order_id,
                    PaymentState::PaymentTimeout,
                    Caller::System {
                        service: "timeout-monitor".into(),
                    },
                    Some("provider timeout".into()),
                    at_ms,
                )
                .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error))?;
            }
        }
        CallbackStatus::Reversed => {
            if current.state == PaymentState::PaymentVerified {
                advance_and_persist(
                    &state,
                    &callback.order_id,
                    PaymentState::PaymentReversed,
                    Caller::Provider {
                        provider_id: callback.provider.id().into(),
                        authenticated: true,
                    },
                    Some("provider reversal".into()),
                    at_ms,
                )
                .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error))?;
            }
        }
        CallbackStatus::Success => {
            let expected = current.ghs_amount.saturating_add(current.provider_fee);
            {
                let mut engine = state.payment_engine.lock().map_err(|error| {
                    api_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("payment engine lock poisoned: {error}"),
                    )
                })?;
                engine
                    .set_ghs_received(&callback.order_id, callback.amount_received_pesewas)
                    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
            }
            persist_order(&state, &callback.order_id)
                .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error))?;
            if callback.amount_received_pesewas < expected {
                advance_and_persist(
                    &state,
                    &callback.order_id,
                    PaymentState::PaymentUnderpaid,
                    Caller::System {
                        service: "verification-service".into(),
                    },
                    Some(format!(
                        "expected {expected}, received {}",
                        callback.amount_received_pesewas
                    )),
                    at_ms,
                )
                .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error))?;
            } else if callback.amount_received_pesewas > expected {
                advance_and_persist(
                    &state,
                    &callback.order_id,
                    PaymentState::PaymentOverpaid,
                    Caller::System {
                        service: "verification-service".into(),
                    },
                    Some(format!(
                        "expected {expected}, received {}",
                        callback.amount_received_pesewas
                    )),
                    at_ms,
                )
                .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error))?;
            } else if current.state == PaymentState::PaymentPending {
                advance_and_persist(
                    &state,
                    &callback.order_id,
                    PaymentState::PaymentVerified,
                    Caller::Provider {
                        provider_id: callback.provider.id().into(),
                        authenticated: true,
                    },
                    Some(callback.provider_tx_ref.clone()),
                    at_ms,
                )
                .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error))?;
                advance_and_persist(
                    &state,
                    &callback.order_id,
                    PaymentState::RiskReview,
                    Caller::System {
                        service: "risk-engine".into(),
                    },
                    None,
                    at_ms,
                )
                .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error))?;
                advance_and_persist(
                    &state,
                    &callback.order_id,
                    PaymentState::RiskApproved,
                    Caller::System {
                        service: "risk-engine".into(),
                    },
                    None,
                    at_ms,
                )
                .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error))?;

                let approved = read_order(&state, &callback.order_id)
                    .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error))?;
                let reservation = state.treasury.write().await.reserve_inventory(
                    approved.omnia_quantity,
                    &approved.recipient_ref,
                    &approved.provider_name,
                    &approved.order_id,
                    at_ms,
                );
                match reservation {
                    Ok(_) => {
                        {
                            let mut engine = state.payment_engine.lock().map_err(|error| {
                                api_error(
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                    format!("payment engine lock poisoned: {error}"),
                                )
                            })?;
                            engine
                                .set_inventory_reservation_ref(&callback.order_id, callback.order_id.clone())
                                .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
                        }
                        persist_order(&state, &callback.order_id)
                            .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error))?;
                        advance_and_persist(
                            &state,
                            &callback.order_id,
                            PaymentState::InventoryReserved,
                            Caller::Treasury,
                            Some("pilot inventory reserved".into()),
                            at_ms,
                        )
                        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error))?;
                    }
                    Err(error) => {
                        advance_and_persist(
                            &state,
                            &callback.order_id,
                            PaymentState::InventoryUnavailable,
                            Caller::Treasury,
                            Some(error.to_string()),
                            at_ms,
                        )
                        .map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error))?;
                    }
                }
            }
        }
    }

    let order =
        read_order(&state, &callback.order_id).map_err(|error| api_error(StatusCode::INTERNAL_SERVER_ERROR, error))?;
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "order": order_json(&order), "verified": true })),
    ))
}

/// `GET /api/v1/payment-orders/:id` — return the authoritative order snapshot.
pub async fn get_order(
    State(state): State<AppState>,
    Extension(caller): Extension<CallerIdentity>,
    Path(order_id): Path<String>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let order = read_order(&state, &order_id).map_err(|error| api_error(StatusCode::NOT_FOUND, error))?;
    if order.customer_ref != caller.caller_id && order.recipient_ref != caller.caller_id {
        return Err(api_error(StatusCode::FORBIDDEN, "caller is not an order participant"));
    }
    Ok((StatusCode::OK, Json(order_json(&order))))
}

/// `POST /api/v1/payment-orders/:id/advance` — internal service-only transition.
pub async fn advance_order(
    State(state): State<AppState>,
    Path(order_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<InternalAdvanceRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let key_id = headers
        .get("x-omnia-service-key")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let service_name = headers
        .get("x-omnia-service-name")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let caller = state
        .service_role_registry
        .resolve(
            &Credential::ApiKey {
                key_id: key_id.into(),
                service_name: service_name.into(),
            },
            now_ms(),
        )
        .map_err(|error| api_error(StatusCode::UNAUTHORIZED, error.to_string()))?;
    let next_state = parse_state(&body.next_state).map_err(|error| api_error(StatusCode::BAD_REQUEST, error))?;
    advance_and_persist(&state, &order_id, next_state, caller, body.reason, now_ms())
        .map_err(|error| api_error(StatusCode::BAD_REQUEST, error))?;
    let order = read_order(&state, &order_id).map_err(|error| api_error(StatusCode::NOT_FOUND, error))?;
    Ok((StatusCode::OK, Json(order_json(&order))))
}

fn parse_state(name: &str) -> Result<PaymentState, String> {
    match name {
        "CREATED" | "Created" => Ok(PaymentState::Created),
        "QUOTED" | "Quoted" => Ok(PaymentState::Quoted),
        "PAYMENT_PENDING" | "PaymentPending" => Ok(PaymentState::PaymentPending),
        "PAYMENT_VERIFIED" | "PaymentVerified" => Ok(PaymentState::PaymentVerified),
        "RISK_REVIEW" | "RiskReview" => Ok(PaymentState::RiskReview),
        "RISK_APPROVED" | "RiskApproved" => Ok(PaymentState::RiskApproved),
        "INVENTORY_RESERVED" | "InventoryReserved" => Ok(PaymentState::InventoryReserved),
        "ALLOCATION_SUBMITTED" | "AllocationSubmitted" => Ok(PaymentState::AllocationSubmitted),
        "ALLOCATION_FINALIZED" | "AllocationFinalized" => Ok(PaymentState::AllocationFinalized),
        "DELIVERED" | "Delivered" => Ok(PaymentState::Delivered),
        "PAYMENT_FAILED" | "PaymentFailed" => Ok(PaymentState::PaymentFailed),
        "QUOTE_EXPIRED" | "QuoteExpired" => Ok(PaymentState::QuoteExpired),
        "PAYMENT_REVERSED" | "PaymentReversed" => Ok(PaymentState::PaymentReversed),
        "PAYMENT_UNDERPAID" | "PaymentUnderpaid" => Ok(PaymentState::PaymentUnderpaid),
        "PAYMENT_OVERPAID" | "PaymentOverpaid" => Ok(PaymentState::PaymentOverpaid),
        "PAYMENT_TIMEOUT" | "PaymentTimeout" => Ok(PaymentState::PaymentTimeout),
        "RISK_REJECTED" | "RiskRejected" => Ok(PaymentState::RiskRejected),
        "INVENTORY_UNAVAILABLE" | "InventoryUnavailable" => Ok(PaymentState::InventoryUnavailable),
        "ALLOCATION_FAILED" | "AllocationFailed" => Ok(PaymentState::AllocationFailed),
        "ON_CHAIN_TIMEOUT" | "OnChainTimeout" => Ok(PaymentState::OnChainTimeout),
        "ON_CHAIN_UNCERTAIN" | "OnChainUncertain" => Ok(PaymentState::OnChainUncertain),
        "REFUND_PENDING" | "RefundPending" => Ok(PaymentState::RefundPending),
        "REFUNDED" | "Refunded" => Ok(PaymentState::Refunded),
        "CANCELLED" | "Cancelled" => Ok(PaymentState::Cancelled),
        "MANUAL_REVIEW" | "ManualReview" => Ok(PaymentState::ManualReview),
        _ => Err(format!("unknown state: {name}")),
    }
}

/// Helper retained for tests and future callback adapters that use the registry.
pub fn registry_is_provider_key_authorized(
    registry: &ServiceRoleRegistry,
    provider: MobileMoneyProvider,
    key: &str,
) -> bool {
    registry.is_authorized_provider_callback(provider.id(), key)
}

trait _PaymentOrderProviderAdapter: ProviderAdapter {}
impl<T: ProviderAdapter> _PaymentOrderProviderAdapter for T {}

#[allow(dead_code)]
fn _provider_callback_status(_status: CallbackStatus) {}
