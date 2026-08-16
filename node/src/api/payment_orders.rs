//! Payment order API handlers — Spec §8
//!
//! Endpoints:
//! - `GET  /api/v1/payment-orders/:id`        — get order status
//! - `POST /api/v1/payment-orders/create`     — create a new payment order
//! - `POST /api/v1/payment-orders/:id/advance` — advance order state

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use serde::Deserialize;
use serde_json::{json, Value};
use utoipa::ToSchema;

use crate::api::auth::{AuthorizedCallers, CallerIdentity};
use crate::state::AppState;
use omnia_asset_registry::types::AssetId;
use omnia_payment_order::engine::Caller;
use omnia_payment_order::PaymentState;

/// Request body for creating a payment order.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateOrderRequest {
    /// Unique order identifier.
    pub order_id: String,
    /// Customer mobile-money number or wallet ID.
    pub customer_ref: String,
    /// Recipient wallet public key or account ID.
    pub recipient_ref: String,
    /// GHS amount in pesewas (smallest GHS unit).
    pub ghs_amount: u64,
    /// OMNIA quantity in plancks (10^-9 OMNIA).
    pub omnia_quantity: u64,
    /// Exchange rate applied (GHS per OMNIA in fixed-point).
    pub exchange_rate: u64,
    /// Mobile-money provider fee in GHS pesewas.
    pub provider_fee: u64,
    /// Omnia protocol fee in OMNIA plancks.
    pub omnia_fee: u64,
    /// Mobile-money provider name (e.g., "MTN", "Telecel", "AT").
    pub provider_name: String,
}

/// Request body for advancing an order's state.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AdvanceOrderRequest {
    /// Target state to advance the order to.
    pub next_state: String,
    /// The authenticated subject is used as the actor; callers cannot spoof a
    /// system service or treasury identity in this request body.
    /// Optional reason or reference for the transition.
    pub reason: Option<String>,
}

/// Handler for `GET /api/v1/payment-orders/:id`.
#[utoipa::path(
    get,
    path = "/api/v1/payment-orders/{order_id}",
    responses(
        (status = 200, description = "Payment order status"),
        (status = 404, description = "Order not found"),
    )
)]
pub async fn get_order(
    State(state): State<AppState>,
    Path(order_id): Path<String>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let engine = state.payment_engine.lock().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("payment engine lock poisoned: {e}") })),
        )
    })?;

    match engine.get_order(&order_id) {
        Ok(order) => Ok((
            StatusCode::OK,
            Json(json!({
                "order_id": order.order_id,
                "state": format!("{:?}", order.state),
                "customer_ref": order.customer_ref,
                "recipient_ref": order.recipient_ref,
                "asset_id": format!("{:?}", order.asset_id),
                "ghs_amount": order.ghs_amount,
                "omnia_quantity": order.omnia_quantity,
                "exchange_rate": order.exchange_rate,
                "provider_fee": order.provider_fee,
                "omnia_fee": order.omnia_fee,
                "provider_name": order.provider_name,
                "is_terminal": order.is_terminal(),
                "is_economically_delivered": order.is_economically_delivered(),
                "event_count": order.event_history.len(),
                "created_at_ms": order.created_at_ms,
                "quote_expiry_ms": order.quote_expiry_ms,
            })),
        )),
        Err(_) => Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": format!("order {order_id} not found") })),
        )),
    }
}

/// Handler for `POST /api/v1/payment-orders/create`.
#[utoipa::path(
    post,
    path = "/api/v1/payment-orders/create",
    request_body = CreateOrderRequest,
    responses(
        (status = 201, description = "Order created"),
        (status = 400, description = "Invalid request or limit exceeded"),
    )
)]
pub async fn create_order(
    State(state): State<AppState>,
    Json(body): Json<CreateOrderRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let mut engine = state.payment_engine.lock().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("payment engine lock poisoned: {e}") })),
        )
    })?;

    engine
        .create_order(
            body.order_id,
            body.customer_ref,
            body.recipient_ref,
            AssetId::OMNIA,
            body.ghs_amount,
            body.omnia_quantity,
            body.exchange_rate,
            body.provider_fee,
            body.omnia_fee,
            body.provider_name,
            now_ms,
        )
        .map(|order| {
            (
                StatusCode::CREATED,
                Json(json!({
                    "order_id": order.order_id,
                    "state": format!("{:?}", order.state),
                    "created_at_ms": now_ms,
                })),
            )
        })
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": format!("{e}") }))))
}

/// Handler for `POST /api/v1/payment-orders/:id/advance`.
#[utoipa::path(
    post,
    path = "/api/v1/payment-orders/{order_id}/advance",
    request_body = AdvanceOrderRequest,
    responses(
        (status = 200, description = "State advanced"),
        (status = 400, description = "Invalid transition"),
    )
)]
pub async fn advance_order(
    State(state): State<AppState>,
    Extension(identity): Extension<CallerIdentity>,
    Extension(authorized): Extension<Arc<AuthorizedCallers>>,
    Path(order_id): Path<String>,
    Json(body): Json<AdvanceOrderRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    if !authorized.is_authorized(&identity.caller_id) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(
                json!({ "error": format!("caller '{}' is not authorized for payment-order operations", identity.caller_id) }),
            ),
        ));
    }

    let next_state = parse_state(&body.next_state)?;
    let caller = Caller::ManualReview {
        reviewer: identity.caller_id.clone(),
    };

    let mut engine = state.payment_engine.lock().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("payment engine lock poisoned: {e}") })),
        )
    })?;

    engine
        .advance_state(&order_id, next_state, caller, now_ms, body.reason)
        .map(|event| {
            (
                StatusCode::OK,
                Json(json!({
                    "order_id": &order_id,
                    "from_state": format!("{:?}", event.from_state),
                    "to_state": format!("{:?}", event.to_state),
                    "actor": format!("{:?}", event.actor),
                    "authenticated_subject": identity.caller_id,
                    "sequence": event.sequence,
                    "timestamp_ms": event.timestamp_ms,
                    "reason": event.reason,
                })),
            )
        })
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": format!("{e}") }))))
}

/// Parse a state name into a PaymentState.
fn parse_state(name: &str) -> Result<PaymentState, (StatusCode, Json<Value>)> {
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
        "RISK_REJECTED" | "RiskRejected" => Ok(PaymentState::RiskRejected),
        "INVENTORY_UNAVAILABLE" | "InventoryUnavailable" => Ok(PaymentState::InventoryUnavailable),
        "ALLOCATION_FAILED" | "AllocationFailed" => Ok(PaymentState::AllocationFailed),
        "REFUND_PENDING" | "RefundPending" => Ok(PaymentState::RefundPending),
        "REFUNDED" | "Refunded" => Ok(PaymentState::Refunded),
        "CANCELLED" | "Cancelled" => Ok(PaymentState::Cancelled),
        "MANUAL_REVIEW" | "ManualReview" => Ok(PaymentState::ManualReview),
        _ => Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("unknown state: {name}") })),
        )),
    }
}
