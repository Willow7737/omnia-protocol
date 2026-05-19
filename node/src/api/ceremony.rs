//! Ceremony API endpoints for the multi-party trusted setup.
//!
//! Provides HTTP endpoints for ceremony coordination:
//!
//! | Method | Path                                | Handler                        |
//! |--------|-------------------------------------|--------------------------------|
//! | GET    | `/api/v1/ceremony/state`            | `ceremony_state`               |
//! | POST   | `/api/v1/ceremony/contribute`       | `ceremony_contribute`          |
//! | GET    | `/api/v1/ceremony/transcript`       | `ceremony_transcript`          |
//! | POST   | `/api/v1/ceremony/finalize`         | `ceremony_finalize`            |
//!
//! # Current Status
//!
//! These are stub handlers that return placeholder data. The full
//! implementation requires the `CeremonyServer` to be integrated
//! into `AppState` for shared mutable access across handlers.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::state::AppState;

/// Request body for submitting a contribution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributeRequest {
    /// The serialized `Contribution` (includes PoK).
    pub contribution_json: String,
}

/// Response body for the ceremony state endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CeremonyStateResponse {
    /// Current ceremony phase.
    pub phase: String,
    /// Number of contributions received so far.
    pub contribution_count: usize,
    /// Minimum participants required to finalize.
    pub min_participants: usize,
    /// Maximum participants allowed.
    pub max_participants: usize,
    /// Current SRS transcript hash (hex-encoded, first 8 bytes).
    pub transcript_hash: String,
}

/// Get the current ceremony state.
///
/// Returns the ceremony phase, contribution count, and SRS transcript
/// hash. Clients use this to determine whether they can contribute.
///
/// **Stub**: Returns placeholder data until `CeremonyServer` is
/// integrated into `AppState`.
pub async fn ceremony_state(
    State(_state): State<AppState>,
) -> impl IntoResponse {
    // TODO: Get ceremony state from AppState when integrated
    // For now, return a stub response
    (
        StatusCode::OK,
        Json(CeremonyStateResponse {
            phase: "not_started".to_string(),
            contribution_count: 0,
            min_participants: 3,
            max_participants: 100,
            transcript_hash: "00000000".to_string(),
        }),
    )
}

/// Submit a contribution to the ceremony.
///
/// The request body contains a serialized `Contribution` with an
/// embedded Proof of Knowledge. The server verifies the contribution
/// before applying it to the SRS.
///
/// **Stub**: Returns a placeholder receipt.
pub async fn ceremony_contribute(
    State(_state): State<AppState>,
    Json(_body): Json<ContributeRequest>,
) -> impl IntoResponse {
    // TODO: Verify and apply contribution via CeremonyServer
    // For now, return a stub response
    (
        StatusCode::OK,
        Json(json!({
            "contribution_index": 0,
            "transcript_hash": "00000000",
            "message": "Contribution accepted (stub)"
        })),
    )
}

/// Download the full ceremony transcript.
///
/// Returns all contributions in order with the ceremony configuration
/// and final transcript hash. Used for independent verification.
///
/// **Stub**: Returns placeholder data.
pub async fn ceremony_transcript(
    State(_state): State<AppState>,
) -> impl IntoResponse {
    // TODO: Export transcript from CeremonyServer
    // For now, return a stub response
    (
        StatusCode::OK,
        Json(json!({
            "config": {
                "min_participants": 3,
                "max_participants": 100,
                "ceremony_id": 1,
                "degree": 65536
            },
            "contributions": [],
            "contribution_count": 0,
            "final_transcript_hash": "0000000000000000",
            "message": "Transcript export (stub)"
        })),
    )
}

/// Finalize the ceremony and derive circuit-specific keys.
///
/// Requires at least `min_participants` contributions. Returns the
/// proving and verifying keys for the rollup circuit.
///
/// **Stub**: Returns placeholder data.
pub async fn ceremony_finalize(
    State(_state): State<AppState>,
) -> impl IntoResponse {
    // TODO: Finalize via CeremonyServer
    // For now, return a stub response
    (
        StatusCode::OK,
        Json(json!({
            "message": "Ceremony finalized (stub)",
            "proving_key_size": 0,
            "verifying_key_size": 0,
            "tau_contributions": 0
        })),
    )
}
