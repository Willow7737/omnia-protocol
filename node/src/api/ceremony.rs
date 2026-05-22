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
//! # Integration
//!
//! These handlers use the `CeremonyServer` integrated into `AppState`.
//! The server must be initialized and started before endpoints return
//! operational data.

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
    /// The serialized `Contribution` as JSON (includes PoK).
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

/// Contribution receipt response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributeResponse {
    /// The participant's contribution index (0-based).
    pub contribution_index: usize,
    /// Hash of the SRS after this contribution.
    pub transcript_hash: String,
    /// Human-readable status message.
    pub message: String,
}

/// Get the current ceremony state.
///
/// Returns the ceremony phase, contribution count, and SRS transcript
/// hash. Clients use this to determine whether they can contribute.
///
/// Returns 503 if the ceremony server is not initialized.
pub async fn ceremony_state(State(state): State<AppState>) -> impl IntoResponse {
    #[cfg(not(feature = "zk"))]
    {
        let _ = state;
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(json!({
                "error": "ZK feature not enabled. Rebuild with --features zk."
            })),
        );
    }

    #[cfg(feature = "zk")]
    {
        let server = match &state.ceremony_server {
            Some(s) => s,
            None => {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({
                        "error": "Ceremony server not initialized"
                    })),
                );
            }
        };

        let server = match server.read() {
            Ok(s) => s,
            Err(_) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": "Ceremony server lock poisoned"})),
                );
            }
        };

        let phase = format!("{:?}", server.phase());
        let count = server.contribution_count();
        let transcript_hash = server
            .get_srs_state()
            .map(|(t, _)| hex::encode(&t[..8.min(t.len())]))
            .unwrap_or_else(|_| "00000000".to_string());

        (
            StatusCode::OK,
            Json(CeremonyStateResponse {
                phase: phase.to_lowercase(),
                contribution_count: count,
                min_participants: 3, // Will be replaced with actual config
                max_participants: 100,
                transcript_hash,
            }),
        )
    }
}

/// Submit a contribution to the ceremony.
///
/// The request body contains a serialized `Contribution` with an
/// embedded Proof of Knowledge. The server verifies the contribution
/// before applying it to the SRS.
///
/// Returns 503 if the ceremony server is not initialized.
pub async fn ceremony_contribute(
    State(state): State<AppState>,
    Json(body): Json<ContributeRequest>,
) -> impl IntoResponse {
    #[cfg(not(feature = "zk"))]
    {
        let _ = (state, body);
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(json!({
                "error": "ZK feature not enabled. Rebuild with --features zk."
            })),
        );
    }

    #[cfg(feature = "zk")]
    {
        let server = match &state.ceremony_server {
            Some(s) => s,
            None => {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({
                        "error": "Ceremony server not initialized"
                    })),
                );
            }
        };

        // Deserialize the contribution from JSON
        let contribution: omnia_adapters::setup::Contribution =
            match serde_json::from_str(&body.contribution_json) {
                Ok(c) => c,
                Err(e) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({
                            "error": format!("Invalid contribution JSON: {e}")
                        })),
                    );
                }
            };

        let result = {
            let server = match server.read() {
                Ok(s) => s,
                Err(_) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"error": "Ceremony server lock poisoned"})),
                    );
                }
            };
            server.accept_contribution(contribution)
        };

        match result {
            Ok(receipt) => (
                StatusCode::OK,
                Json(json!({
                    "contribution_index": receipt.contribution_index,
                    "transcript_hash": hex::encode(receipt.transcript_hash),
                    "message": "Contribution accepted and verified"
                })),
            ),
            Err(e) => (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": format!("Contribution rejected: {e}")
                })),
            ),
        }
    }
}

/// Download the full ceremony transcript.
///
/// Returns all contributions in order with the ceremony configuration
/// and final transcript hash. Used for independent verification.
///
/// Returns 503 if the ceremony server is not initialized.
pub async fn ceremony_transcript(State(state): State<AppState>) -> impl IntoResponse {
    #[cfg(not(feature = "zk"))]
    {
        let _ = state;
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(json!({
                "error": "ZK feature not enabled. Rebuild with --features zk."
            })),
        );
    }

    #[cfg(feature = "zk")]
    {
        let server = match &state.ceremony_server {
            Some(s) => s,
            None => {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({
                        "error": "Ceremony server not initialized"
                    })),
                );
            }
        };

        let result = {
            let server = match server.read() {
                Ok(s) => s,
                Err(_) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"error": "Ceremony server lock poisoned"})),
                    );
                }
            };
            server.export_transcript()
        };

        match result {
            Ok(transcript) => (
                StatusCode::OK,
                Json(json!({
                    "config": {
                        "min_participants": transcript.config.min_participants,
                        "max_participants": transcript.config.max_participants,
                        "ceremony_id": transcript.config.ceremony_id,
                        "degree": transcript.config.degree
                    },
                    "contribution_count": transcript.contribution_count,
                    "final_transcript_hash": hex::encode(transcript.final_transcript_hash),
                })),
            ),
            Err(e) => (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": format!("Failed to export transcript: {e}")
                })),
            ),
        }
    }
}

/// Finalize the ceremony and derive circuit-specific keys.
///
/// Requires at least `min_participants` contributions. Returns the
/// proving and verifying keys for the rollup circuit.
///
/// Returns 503 if the ceremony server is not initialized.
pub async fn ceremony_finalize(State(state): State<AppState>) -> impl IntoResponse {
    #[cfg(not(feature = "zk"))]
    {
        let _ = state;
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(json!({
                "error": "ZK feature not enabled. Rebuild with --features zk."
            })),
        );
    }

    #[cfg(feature = "zk")]
    {
        let server = match &state.ceremony_server {
            Some(s) => s,
            None => {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({
                        "error": "Ceremony server not initialized"
                    })),
                );
            }
        };

        let result = {
            let server = match server.read() {
                Ok(s) => s,
                Err(_) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"error": "Ceremony server lock poisoned"})),
                    );
                }
            };
            let circuit = omnia_adapters::circuit::RollupCircuit::empty();
            server.finalize(&circuit)
        };

        match result {
            Ok(keypair) => (
                StatusCode::OK,
                Json(json!({
                    "message": "Ceremony finalized successfully",
                    "proving_key_size": keypair.proving_key.len(),
                    "verifying_key_size": keypair.verifying_key.len(),
                    "tau_contributions": keypair.tau_contributions
                })),
            ),
            Err(e) => (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": format!("Failed to finalize ceremony: {e}")
                })),
            ),
        }
    }
}
