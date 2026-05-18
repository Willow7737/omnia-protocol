//! API router composition
//!
//! This module assembles all API route handlers into a single
//! `axum::Router` mounted under `/api/v1`. Each sub-module
//! handles a specific domain (events, shards, governance, economics,
//! node information).
//!
//! # Middleware
//!
//! The API router applies JWT authentication as a middleware layer.
//! CORS and rate limiting are applied at the top-level HTTP router
//! (see [`crate::http`]).

pub mod auth;
pub mod economics;
pub mod errors;
pub mod events;
pub mod governance;
pub mod node;
pub mod shards;

use std::sync::Arc;

use axum::middleware;
use axum::routing::{get, post};
use axum::Extension;
use axum::Router;
use utoipa::OpenApi;

use crate::api::auth::{self as api_auth, AuthorizedCallers};
use crate::state::AppState;

/// OpenAPI specification for the Omnia node REST API.
///
/// Auto-generates the OpenAPI 3.0 spec from the utoipa path and
/// schema annotations on each handler and request/response type.
#[derive(OpenApi)]
#[openapi(
    paths(
        crate::api::events::submit_event,
        crate::api::events::get_event,
        crate::api::shards::submit_shard_operation,
        crate::api::governance::create_proposal,
        crate::api::governance::cast_vote,
        crate::api::economics::get_balance,
        crate::api::economics::transfer_ubc,
        crate::api::node::node_info,
        crate::api::node::node_peers,
        crate::api::errors::error_codes,
    ),
    components(schemas(
        crate::api::events::SubmitEventRequest,
        crate::api::events::StoredEvent,
        crate::api::shards::ShardOperationRequest,
        crate::api::governance::CreateProposalRequest,
        crate::api::governance::CastVoteRequest,
        crate::api::economics::TransferRequest,
        crate::api::node::PeerInfo,
        crate::api::errors::ErrorCode,
        crate::api::errors::ErrorResponse,
    ))
)]
pub struct ApiDoc;

/// Build the API v1 router with all route handlers and auth middleware.
///
/// Mounts the following routes:
///
/// | Method | Path                                      | Handler                |
/// |--------|-------------------------------------------|------------------------|
/// | GET    | `/api/v1/node/info`                       | `node::node_info`      |
/// | GET    | `/api/v1/node/peers`                      | `node::node_peers`     |
/// | POST   | `/api/v1/events`                          | `events::submit_event` |
/// | GET    | `/api/v1/events/:id`                      | `events::get_event`    |
/// | POST   | `/api/v1/shards/:shard_id/operations`     | `shards::submit_shard_operation` |
/// | POST   | `/api/v1/governance/proposals`            | `governance::create_proposal` |
/// | POST   | `/api/v1/governance/vote`                 | `governance::cast_vote` |
/// | GET    | `/api/v1/economics/balance/:did`          | `economics::get_balance` |
/// | POST   | `/api/v1/economics/transfer`              | `economics::transfer_ubc` |
/// | GET    | `/api/v1/errors`                          | `errors::error_codes`     |
///
/// Reads `OMNIA_AUTHORIZED_CALLERS` from the environment to configure
/// the authorization registry for privileged shard operations.
pub fn build_api_router() -> Router<AppState> {
    let authorized = Arc::new(AuthorizedCallers::from_env());
    build_api_router_with(authorized)
}

/// Build the API v1 router with an injected [`AuthorizedCallers`] registry.
///
/// This is the core constructor used by both production code and tests.
/// It applies JWT authentication middleware on top of all routes and
/// makes the [`AuthorizedCallers`] available via `Extension` so that
/// handlers can enforce privileged-operation authorization.
///
/// # Arguments
///
/// * `authorized` — shared [`AuthorizedCallers`] registry (wrapped in `Arc`)
pub fn build_api_router_with(authorized: Arc<AuthorizedCallers>) -> Router<AppState> {
    Router::new()
        // Node information
        .route("/node/info", get(node::node_info))
        .route("/node/peers", get(node::node_peers))
        // Events
        .route("/events", post(events::submit_event))
        .route("/events/{id}", get(events::get_event))
        // Shard operations
        .route(
            "/shards/{shard_id}/operations",
            post(shards::submit_shard_operation),
        )
        // Governance
        .route("/governance/proposals", post(governance::create_proposal))
        .route("/governance/vote", post(governance::cast_vote))
        // Economics
        .route("/economics/balance/{did}", get(economics::get_balance))
        .route("/economics/transfer", post(economics::transfer_ubc))
        // Error documentation
        .route("/errors", get(errors::error_codes))
        // --- Middleware layers (outermost = last added) ---
        // Provide AuthorizedCallers via Extension for handler-level checks
        .layer(Extension(authorized))
        // JWT authentication — validates Bearer token and inserts CallerIdentity
        .layer(middleware::from_fn(api_auth::require_auth))
}
