//! Governance proposal and voting API handlers
//!
//! Provides endpoints for creating governance proposals and casting votes:
//! - `POST /api/v1/governance/proposals` — submit a new proposal
//! - `GET /api/v1/governance/proposals` — list all proposals
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
use crate::api::economics::with_economics;
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
    with_economics(&state, |econ| {
        let current_epoch = econ.current_epoch();
        let result = econ.governance.create_proposal(
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
    })?
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

    let choice = parse_vote_choice(&body.choice)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({"error": e.to_string()}))))?;

    with_economics(&state, |econ| {
        let current_epoch = econ.current_epoch();

        // Reject votes from voters with no registered stake
        if !econ.governance.voting_weights.contains_key(&voter_did) {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "Voter has no registered stake. Register stake before voting."
                })),
            ));
        }

        let effective_weight = econ.governance.effective_weight(&voter_did, current_epoch);

        let result = econ
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
    })?
}

/// Handler for `GET /api/v1/governance/proposals`.
///
/// Returns all proposals currently tracked by the governance state.
/// Proposals are returned in insertion order (which approximates creation
/// order, since `HashMap` does not preserve it; for true chronological
/// ordering, sort by `created_at_epoch` client-side).
#[utoipa::path(
    get,
    path = "/api/v1/governance/proposals",
    responses(
        (status = 200, description = "List of proposals"),
    )
)]
pub async fn list_proposals(State(state): State<AppState>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // Build the proposal list under the shared economics lock — a derived
    // `status` field is added per proposal without mutating state.
    let proposals: Vec<Value> = with_economics(&state, |econ| {
        let current_epoch = econ.current_epoch();
        econ.governance
            .proposals
            .values()
            .map(|p| {
                let status = if p.execution_time.is_some() {
                    "passed"
                } else if p.is_expired(current_epoch) {
                    "expired"
                } else {
                    "voting"
                };
                let base = serde_json::to_value(p).unwrap_or_else(|_| json!({}));
                let mut obj = base.as_object().cloned().unwrap_or_default();
                obj.insert("status".to_string(), json!(status));
                // Compute participation totals for convenience
                let total = p
                    .votes_for
                    .saturating_add(p.votes_against)
                    .saturating_add(p.votes_abstain);
                obj.insert("total_participation".to_string(), json!(total));
                Value::Object(obj)
            })
            .collect()
    })?;
    Ok(Json(json!({
        "proposals": proposals,
        "count": proposals.len(),
    })))
}

/// Parse a vote choice string into a `VoteChoice` enum.
///
/// Accepts case-insensitive values: "for", "against", "abstain".
/// Leading/trailing whitespace is trimmed before matching, so a user
/// who accidentally includes a leading space (e.g. " for") still gets
/// their vote counted. This matters in a governance context — a silent
/// rejection due to whitespace is a correctness issue, not user error.
fn parse_vote_choice(s: &str) -> Result<VoteChoice, ApiParseError> {
    match s.trim().to_lowercase().as_str() {
        "for" => Ok(VoteChoice::For),
        "against" => Ok(VoteChoice::Against),
        "abstain" => Ok(VoteChoice::Abstain),
        other => Err(ApiParseError::InvalidVoteChoice(other.to_string())),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_vote_choice_for() {
        assert_eq!(parse_vote_choice("for").unwrap(), VoteChoice::For);
        // Case-insensitive
        assert_eq!(parse_vote_choice("FOR").unwrap(), VoteChoice::For);
        assert_eq!(parse_vote_choice("For").unwrap(), VoteChoice::For);
    }

    #[test]
    fn test_parse_vote_choice_against() {
        assert_eq!(parse_vote_choice("against").unwrap(), VoteChoice::Against);
        assert_eq!(parse_vote_choice("AGAINST").unwrap(), VoteChoice::Against);
    }

    #[test]
    fn test_parse_vote_choice_abstain() {
        assert_eq!(parse_vote_choice("abstain").unwrap(), VoteChoice::Abstain);
        assert_eq!(parse_vote_choice("ABSTAIN").unwrap(), VoteChoice::Abstain);
    }

    #[test]
    fn test_parse_vote_choice_invalid() {
        let result = parse_vote_choice("yes");
        assert!(result.is_err());
        match result.unwrap_err() {
            ApiParseError::InvalidVoteChoice(s) => assert!(s.contains("yes")),
            other => panic!("Expected InvalidVoteChoice, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_vote_choice_empty_string() {
        let result = parse_vote_choice("");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_vote_choice_with_whitespace_trimmed() {
        // Whitespace IS trimmed — " for " should parse as For.
        // This prevents silent rejection of votes with accidental leading/
        // trailing spaces, which is a correctness issue in governance.
        assert_eq!(parse_vote_choice(" for ").unwrap(), VoteChoice::For);
        assert_eq!(parse_vote_choice("\tagainst\t").unwrap(), VoteChoice::Against);
        assert_eq!(parse_vote_choice(" abstain").unwrap(), VoteChoice::Abstain);
    }

    #[test]
    fn test_api_parse_error_display() {
        let e = ApiParseError::InvalidVoteChoice("maybe".into());
        assert_eq!(
            e.to_string(),
            "invalid vote choice: 'maybe'. Must be 'for', 'against', or 'abstain'"
        );

        let e = ApiParseError::MissingParameter("did".into());
        assert_eq!(e.to_string(), "missing parameter: did");

        let e = ApiParseError::InvalidParameter("amount".into());
        assert_eq!(e.to_string(), "invalid parameter: amount");

        let e = ApiParseError::UnknownOperation("foo".into());
        assert_eq!(e.to_string(), "unknown operation: 'foo'");
    }
}
