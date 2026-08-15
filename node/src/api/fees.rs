//! Fee and burn policy API handlers — Spec §7
//!
//! Endpoints:
//! - `GET  /api/v1/fees/burn-policy`   — current burn ratio and policy
//! - `POST /api/v1/fees/calculate`       — calculate fee for an activity
//! - `GET  /api/v1/fees/stats`          — aggregate burn statistics

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use utoipa::ToSchema;

use crate::state::AppState;
use omnia_fee_burn::{ActivityType, FeeCalculation, FeeFormula};

/// Handler for `GET /api/v1/fees/burn-policy`.
#[utoipa::path(
    get,
    path = "/api/v1/fees/burn-policy",
    responses(
        (status = 200, description = "Current burn policy"),
    )
)]
pub async fn get_burn_policy(
    State(_state): State<AppState>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let formula = FeeFormula::new();

    Ok((
        StatusCode::OK,
        Json(json!({
            "initial_burn_ratio_bps": formula.burn_ratio.bps(),
            "governance_ceiling_min_bps": omnia_fee_burn::BurnRatio::GOVERNANCE_CEILING_MIN,
        })),
    ))
}

/// Request body for `POST /api/v1/fees/calculate`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct FeeCalculateRequest {
    /// Activity type.
    pub activity: String,
    /// Priority fee in plancks (0 for no priority).
    #[serde(default)]
    pub priority_fee: u64,
}

/// Handler for `POST /api/v1/fees/calculate`.
#[utoipa::path(
    post,
    path = "/api/v1/fees/calculate",
    request_body = FeeCalculateRequest,
    responses(
        (status = 200, description = "Fee calculation result"),
        (status = 400, description = "Unknown activity type"),
    )
)]
pub async fn calculate_fee(
    State(_state): State<AppState>,
    Json(body): Json<FeeCalculateRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let formula = FeeFormula::new();
    let activity = parse_activity(&body.activity)?;
    let result = formula
        .calculate(activity, body.priority_fee)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": format!("{e}") }))))?;

    Ok((
        StatusCode::OK,
        Json(json!({
            "activity": body.activity,
            "accepts_omnia": activity.accepts_omnia(),
            "accepts_ubc": activity.accepts_ubc(),
            "is_burnable": activity.is_burnable(),
            "total_fee": result.total_fee,
            "burned_amount": result.burned_amount,
            "validator_amount": result.validator_amount,
            "protocol_amount": result.protocol_amount,
            "is_ubc": result.is_ubc,
            "burn_ratio_bps": formula.burn_ratio.bps(),
        })),
    ))
}

/// Handler for `GET /api/v1/fees/stats`.
#[utoipa::path(
    get,
    path = "/api/v1/fees/stats",
    responses(
        (status = 200, description = "Aggregate burn statistics"),
    )
)]
pub async fn get_fee_stats(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let burn = state.burn_accounting.read().await;

    let mut activity_breakdown = serde_json::Map::new();
    for act in [
        ActivityType::BasicIdentity,
        ActivityType::Compute,
        ActivityType::OmniaTransfer,
        ActivityType::MerchantPayment,
        ActivityType::GhanaMobileMoney,
        ActivityType::GovernanceProposal,
        ActivityType::ExternalChain,
        ActivityType::PriorityInclusion,
    ] {
        activity_breakdown.insert(act.label().to_string(), json!(burn.burned_for_activity(act.label())));
    }

    Ok((
        StatusCode::OK,
        Json(json!({
            "total_burned": burn.total_burned(),
            "by_activity": activity_breakdown,
        })),
    ))
}

/// Parse an activity type string.
fn parse_activity(name: &str) -> Result<ActivityType, (StatusCode, Json<Value>)> {
    match name {
        "BasicIdentity" | "basic_identity" => Ok(ActivityType::BasicIdentity),
        "Compute" | "compute" => Ok(ActivityType::Compute),
        "OmniaTransfer" | "omnia_transfer" => Ok(ActivityType::OmniaTransfer),
        "MerchantPayment" | "merchant_payment" => Ok(ActivityType::MerchantPayment),
        "GhanaMobileMoney" | "ghana_mobile_money" => Ok(ActivityType::GhanaMobileMoney),
        "GovernanceProposal" | "governance_proposal" => Ok(ActivityType::GovernanceProposal),
        "ExternalChain" | "external_chain" => Ok(ActivityType::ExternalChain),
        "PriorityInclusion" | "priority_inclusion" => Ok(ActivityType::PriorityInclusion),
        _ => Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!("unknown activity type: {name}"),
            })),
        )),
    }
}
