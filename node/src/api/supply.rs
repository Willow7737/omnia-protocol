//! Supply API handlers — Spec §4.4, §5.1
//!
//! Endpoints:
//! - `GET /api/v1/supply` — supply snapshot per asset
//! - `GET /api/v1/supply/invariants` — verify supply invariants

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

use crate::state::AppState;
use omnia_asset_registry::types::AssetId;

/// Handler for `GET /api/v1/supply`.
#[utoipa::path(
    get,
    path = "/api/v1/supply",
    responses(
        (status = 200, description = "Per-asset supply snapshot"),
    )
)]
pub async fn get_supply(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let tracker = state.supply_tracker.read().await;

    let mut assets = Vec::new();
    for asset_id in [AssetId::OMNIA, AssetId::UBC] {
        if let Some(supply) = tracker.get(asset_id) {
            let current = supply.total_minted.saturating_sub(supply.total_burned);
            let decomposition_ok = supply.account_balances
                + supply.locked_balances
                + supply.treasury_balances
                + supply.escrow_balances
                == current;
            assets.push(json!({
                "asset": format!("{:?}", asset_id),
                "total_minted": supply.total_minted,
                "total_burned": supply.total_burned,
                "current_supply": current,
                "compartments": {
                    "account_balances": supply.account_balances,
                    "locked_balances": supply.locked_balances,
                    "treasury_balances": supply.treasury_balances,
                    "escrow_balances": supply.escrow_balances,
                },
                "invariant_holds": decomposition_ok,
                "events": supply.event_sequence,
            }));
        }
    }

    let burn = state.burn_accounting.read().await;

    Ok((
        StatusCode::OK,
        Json(json!({
            "assets": assets,
            "fee_burn_total": burn.total_burned(),
        })),
    ))
}

/// Handler for `GET /api/v1/supply/invariants`.
#[utoipa::path(
    get,
    path = "/api/v1/supply/invariants",
    responses(
        (status = 200, description = "All invariants pass"),
        (status = 502, description = "One or more invariants broken"),
    )
)]
pub async fn verify_supply_invariants(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let tracker = state.supply_tracker.read().await;
    match tracker.verify_all_invariants() {
        Ok(count) => Ok((
            StatusCode::OK,
            Json(json!({
                "status": "pass",
                "invariants_checked": count,
            })),
        )),
        Err(e) => Ok((
            StatusCode::BAD_GATEWAY,
            Json(json!({
                "status": "fail",
                "error": format!("{e}"),
            })),
        )),
    }
}