//! Governance proposal and voting API handlers
//!
//! Provides endpoints for creating governance proposals and casting votes:
//! - `POST /api/v1/governance/proposals` — submit a new proposal
//! - `POST /api/v1/governance/vote` — cast a vote on a proposal

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use omnia_economics::governance::VoteChoice;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;

/// Request body for creating a new governance proposal.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateProposalRequest {
    /// Unique identifier for the proposal.
    pub id: String,
    /// Human-readable description of the proposal.
    pub description: String,
    /// Epoch number when the proposal expires.
    pub expires_at_epoch: u64,
}

/// Request body for casting a vote on a governance proposal.
#[derive(Debug, Clone, Deserialize)]
pub struct CastVoteRequest {
    /// DID of the voter.
    pub did: String,
    /// ID of the proposal to vote on.
    pub proposal_id: String,
    /// Vote choice: "for", "against", or "abstain".
    pub choice: String,
}

/// Handler for `POST /api/v1/governance/proposals`.
///
/// Creates a new governance proposal with quadratic voting and
/// reputation decay. Returns 201 on success.
pub async fn create_proposal(
    State(state): State<AppState>,
    Json(body): Json<CreateProposalRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let mut economics = state.economics.lock().await;
    let current_epoch = economics.current_epoch();

    let result = economics.governance.create_proposal(
        body.id.clone(),
        body.description.clone(),
        body.expires_at_epoch,
        current_epoch,
    );

    match result {
        Ok(()) => {
            tracing::info!(
                proposal_id = %body.id,
                expires_at_epoch = body.expires_at_epoch,
                "Governance proposal created"
            );
            Ok((
                StatusCode::CREATED,
                Json(json!({
                    "id": body.id,
                    "status": "created",
                    "created_at_epoch": current_epoch,
                    "expires_at_epoch": body.expires_at_epoch,
                })),
            ))
        }
        Err(e) => {
            tracing::warn!(
                proposal_id = %body.id,
                error = %e,
                "Failed to create governance proposal"
            );
            Err((
                StatusCode::CONFLICT,
                Json(json!({
                    "error": format!("Failed to create proposal: {}", e),
                    "proposal_id": body.id,
                })),
            ))
        }
    }
}

/// Handler for `POST /api/v1/governance/vote`.
///
/// Casts a quadratic-weighted vote on a governance proposal.
/// The voter's effective weight is calculated based on their stake
/// (via `isqrt`) and reputation decay for inactive epochs.
pub async fn cast_vote(
    State(state): State<AppState>,
    Json(body): Json<CastVoteRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let choice = parse_vote_choice(&body.choice)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({"error": e}))))?;

    let mut economics = state.economics.lock().await;
    let current_epoch = economics.current_epoch();

    // Ensure the voter has a voting weight set (default if not)
    if !economics.governance.voting_weights.contains_key(&body.did) {
        economics.governance.set_weight(&body.did, 100);
    }

    let effective_weight = economics
        .governance
        .effective_weight(&body.did, current_epoch);

    let result = economics
        .governance
        .vote(&body.did, &body.proposal_id, choice, current_epoch);

    match result {
        Ok(()) => {
            tracing::info!(
                did = %body.did,
                proposal_id = %body.proposal_id,
                choice = %body.choice,
                weight = effective_weight,
                "Vote cast on governance proposal"
            );
            Ok((
                StatusCode::OK,
                Json(json!({
                    "status": "recorded",
                    "proposal_id": body.proposal_id,
                    "did": body.did,
                    "choice": body.choice,
                    "effective_weight": effective_weight,
                    "epoch": current_epoch,
                })),
            ))
        }
        Err(e) => {
            tracing::warn!(
                did = %body.did,
                proposal_id = %body.proposal_id,
                error = %e,
                "Failed to cast vote"
            );
            Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": format!("Failed to cast vote: {}", e),
                    "proposal_id": body.proposal_id,
                    "did": body.did,
                })),
            ))
        }
    }
}

/// Parse a vote choice string into a `VoteChoice` enum.
///
/// Accepts case-insensitive values: "for", "against", "abstain".
fn parse_vote_choice(s: &str) -> Result<VoteChoice, String> {
    match s.to_lowercase().as_str() {
        "for" => Ok(VoteChoice::For),
        "against" => Ok(VoteChoice::Against),
        "abstain" => Ok(VoteChoice::Abstain),
        other => Err(format!(
            "Invalid vote choice: '{}'. Must be 'for', 'against', or 'abstain'",
            other
        )),
    }
}
