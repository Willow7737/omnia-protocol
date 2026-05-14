//! API router composition
//!
//! This module assembles all API route handlers into a single
//! `axum::Router` mounted under `/api/v1`. Each sub-module
//! handles a specific domain (events, shards, governance, economics,
//! node information).

pub mod economics;
pub mod events;
pub mod governance;
pub mod node;
pub mod shards;

use axum::routing::{get, post};
use axum::Router;

use crate::state::AppState;

/// Build the API v1 router with all route handlers.
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
pub fn build_api_router() -> Router<AppState> {
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
        .route(
            "/economics/balance/{did}",
            get(economics::get_balance),
        )
        .route("/economics/transfer", post(economics::transfer_ubc))
}
