//! Treasury API handlers — Spec §5, §6
//!
//! Endpoints:
//! - `GET /api/v1/treasury/status`     — bucket balances, caps, circuit breaker
//! - `GET /api/v1/treasury/inventory`   — pilot inventory status
//! - `GET /api/v1/treasury/accounting`  — per-category accounting

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

use crate::state::AppState;
use omnia_asset_registry::{AllocationBucket, TreasuryCategory};

const ALL_BUCKETS: [AllocationBucket; 6] = [
    AllocationBucket::NetworkIncentives,
    AllocationBucket::Team,
    AllocationBucket::EarlyInvestors,
    AllocationBucket::Ecosystem,
    AllocationBucket::TreasuryReserve,
    AllocationBucket::Liquidity,
];

const ALL_CATEGORIES: [TreasuryCategory; 10] = [
    TreasuryCategory::PilotAllocation,
    TreasuryCategory::LiquiditySettlement,
    TreasuryCategory::EcosystemGrants,
    TreasuryCategory::OperatingReserve,
    TreasuryCategory::LockedVested,
    TreasuryCategory::ProviderFeeSubsidies,
    TreasuryCategory::RefundsReserved,
    TreasuryCategory::RealizedConversion,
    TreasuryCategory::UnrealizedConversion,
    TreasuryCategory::ExternalFunds,
];

/// Handler for `GET /api/v1/treasury/status`.
#[utoipa::path(
    get,
    path = "/api/v1/treasury/status",
    responses(
        (status = 200, description = "Treasury bucket status and circuit breaker state"),
    )
)]
pub async fn get_treasury_status(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let treasury = state.treasury.read().await;
    let cb = treasury.circuit_breaker();

    let all_buckets: Vec<Value> = ALL_BUCKETS
        .iter()
        .map(|b| {
            json!({
                "bucket": format!("{:?}", b),
                "cap": treasury.bucket_cap(*b),
                "funded": treasury.bucket_funded(*b),
                "spent": treasury.bucket_spent(*b),
                "available": treasury.bucket_available(*b),
            })
        })
        .collect();

    Ok((
        StatusCode::OK,
        Json(json!({
            "hard_cap": omnia_asset_registry::HARD_CAP,
            "total_funded": treasury.total_bucket_funded(),
            "paused": treasury.is_paused(),
            "circuit_breaker_paused": cb.paused,
            "buckets": all_buckets,
        })),
    ))
}

/// Handler for `GET /api/v1/treasury/inventory`.
#[utoipa::path(
    get,
    path = "/api/v1/treasury/inventory",
    responses(
        (status = 200, description = "Pilot inventory status"),
    )
)]
pub async fn get_pilot_inventory(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let treasury = state.treasury.read().await;
    let inv = treasury.pilot_inventory();

    Ok((
        StatusCode::OK,
        Json(json!({
            "cap": inv.cap,
            "allocated": inv.allocated,
            "reserved": treasury.total_reserved(),
            "remaining": inv.remaining(),
        })),
    ))
}

/// Handler for `GET /api/v1/treasury/accounting`.
#[utoipa::path(
    get,
    path = "/api/v1/treasury/accounting",
    responses(
        (status = 200, description = "Treasury accounting by category"),
    )
)]
pub async fn get_treasury_accounting(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let treasury = state.treasury.read().await;
    let acct = treasury.accounting();

    let categories: Vec<Value> = ALL_CATEGORIES
        .iter()
        .map(|cat| {
            json!({
                "category": cat.label(),
                "balance": acct.balance(cat),
            })
        })
        .collect();

    Ok((StatusCode::OK, Json(json!({ "accounting": categories }))))
}
