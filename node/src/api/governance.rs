//! Governance proposal and voting API handlers
//!
//! Provides endpoints for creating governance proposals and casting votes:
//! - `POST /api/v1/governance/proposals` — submit a new proposal
//! - `POST /api/v1/governance/vote` — cast a vote on a proposal

use axum::extract::State;
use axum::http::StatusCode;
use axum::Extension;
use axum::Json;
use omnia_economics::governance::VoteChoice;
use serde::Deserialize;
use serde_json::{json, Value};
use thiserror::Error;
use utoipa::ToSchema;

use crate::api::auth::CallerIdentity;
use crate::state::AppState;

/// Errors that can occur when parsing API request parameters.
#[derive(Error, Debug, serde::Serialize)]
pub enum ApiParseError {
    /// An invalid vote choice was provided.
    #[error("invalid vote choice: '{0}'. Must be 'for', 'against', or 'abstain'")]
    InvalidVoteChoice(String),
    /// A required parameter is missing.
    #[error("missing parameter: {0}")]
    MissingParameter(String),
    /// A parameter has an invalid value or type.
    #[error("invalid parameter: {0}")]
    InvalidParameter(String),
    /// An unknown operation was requested.
    #[error("unknown operation: '{0}'")]
    UnknownOperation(String),
}

/// Request body for creating a new governance proposal.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CreateProposalRequest {
    /// Unique identifier for the proposal.
    pub id: String,
    /// Human-readable description of the proposal.
    pub description: String,
    /// Epoch number when the proposal expires.
    pub expires_at_epoch: u64,
}

/// Request body for casting a vote on a governance proposal.
#[derive(Debug, Clone, Deserialize, ToSchema)]
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
#[utoipa::path(
    post,
    path = "/api/v1/governance/proposals",
    request_body = CreateProposalRequest,
    responses(
        (status = 201, description = "Proposal created"),
        (status = 409, description = "Proposal already exists"),
    )
)]
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
                    "error": format!("Failed to create proposal: {e}"),
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
///
/// The voter DID is derived from the authenticated caller identity
/// (via the JWT `sub` claim) rather than from the request body,
/// preventing impersonation attacks.
#[utoipa::path(
    post,
    path = "/api/v1/governance/vote",
    request_body = CastVoteRequest,
    responses(
        (status = 200, description = "Vote recorded"),
        (status = 400, description = "Invalid vote"),
        (status = 401, description = "Not authenticated"),
    )
)]
pub async fn cast_vote(
    State(state): State<AppState>,
    caller: Extension<CallerIdentity>,
    Json(body): Json<CastVoteRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    // Derive the voter DID from the authenticated caller identity
    // instead of trusting the `did` field in the request body.
    // This prevents impersonation — a caller cannot vote as someone else.
    let voter_did = caller.caller_id.clone();

    let choice = parse_vote_choice(&body.choice).map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({"error": e}))))?;

    let mut economics = state.economics.lock().await;
    let current_epoch = economics.current_epoch();

    // Reject votes from voters with no registered stake
    if !economics.governance.voting_weights.contains_key(&voter_did) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "Voter has no registered stake. Register stake before voting."
            })),
        ));
    }

    let effective_weight = economics.governance.effective_weight(&voter_did, current_epoch);

    let result = economics
        .governance
        .vote(&voter_did, &body.proposal_id, choice, current_epoch);

    match result {
        Ok(()) => {
            tracing::info!(
                did = %voter_did,
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
                    "did": voter_did,
                    "choice": body.choice,
                    "effective_weight": effective_weight,
                    "epoch": current_epoch,
                })),
            ))
        }
        Err(e) => {
            tracing::warn!(
                did = %voter_did,
                proposal_id = %body.proposal_id,
                error = %e,
                "Failed to cast vote"
            );
            Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": format!("Failed to cast vote: {e}"),
                    "proposal_id": body.proposal_id,
                    "did": voter_did,
                })),
            ))
        }
    }
}

/// Parse a vote choice string into a `VoteChoice` enum.
///
/// Accepts case-insensitive values: "for", "against", "abstain".
fn parse_vote_choice(s: &str) -> Result<VoteChoice, ApiParseError> {
    match s.to_lowercase().as_str() {
        "for" => Ok(VoteChoice::For),
        "against" => Ok(VoteChoice::Against),
        "abstain" => Ok(VoteChoice::Abstain),
        other => Err(ApiParseError::InvalidVoteChoice(other.to_string())),
    }
}
