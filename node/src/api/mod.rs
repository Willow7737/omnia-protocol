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

pub mod admin;
pub mod auth;
pub mod bridge;
pub mod ceremony;
pub mod economics;
pub mod errors;
pub mod events;
pub mod fees;
pub mod financial;
pub mod governance;
pub mod merchants;
pub mod node;
pub mod payment_orders;
pub mod shards;
pub mod supply;
pub mod treasury;
pub mod wallet_auth;

use std::sync::Arc;

use axum::extract::DefaultBodyLimit;
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
        crate::api::events::list_events,
        crate::api::events::get_event,
        crate::api::shards::submit_shard_operation,
        crate::api::governance::create_proposal,
        crate::api::governance::list_proposals,
        crate::api::governance::cast_vote,
        crate::api::economics::get_balance,
        crate::api::economics::transfer_ubc,
        crate::api::economics::list_transfers,
        crate::api::financial::get_balance,
        crate::api::financial::transfer,
        crate::api::node::node_info,
        crate::api::node::node_peers,
        crate::api::node::list_validators,
        crate::api::admin::submit_root,
        crate::api::errors::error_codes,
        crate::api::wallet_auth::request_challenge,
        crate::api::wallet_auth::login,
        crate::api::wallet_auth::register,
    ),
    components(schemas(
        crate::api::events::SubmitEventRequest,
        crate::api::events::StoredEvent,
        crate::api::shards::ShardOperationRequest,
        crate::api::governance::CreateProposalRequest,
        crate::api::governance::CastVoteRequest,
        crate::api::economics::TransferRequest,
        crate::api::financial::FinancialTransferRequest,
        crate::api::node::PeerInfo,
        crate::api::admin::SubmitRootRequest,
        crate::api::admin::SubmitRootResponse,
        crate::api::errors::ErrorCode,
        crate::api::errors::ErrorResponse,
        crate::api::wallet_auth::ChallengeRequest,
        crate::api::wallet_auth::ChallengeResponse,
        crate::api::wallet_auth::LoginRequest,
        crate::api::wallet_auth::LoginResponse,
    ))
)]
pub struct ApiDoc;

/// Build the API v1 router with all route handlers and auth middleware.
///
/// Mounts the following routes:
///
/// | Method | Path                                      | Handler                | Auth   |
/// |--------|-------------------------------------------|------------------------|--------|
/// | GET    | `/api/v1/node/info`                       | `node::node_info`      | Public |
/// | GET    | `/api/v1/node/peers`                      | `node::node_peers`     | Public |
/// | GET    | `/api/v1/validators`                      | `node::list_validators`| Public |
/// | POST   | `/api/v1/events`                          | `events::submit_event` | JWT    |
/// | GET    | `/api/v1/events`                          | `events::list_events`  | JWT    |
/// | GET    | `/api/v1/events/:id`                      | `events::get_event`    | JWT    |
/// | POST   | `/api/v1/shards/:shard_id/operations`     | `shards::submit_shard_operation` | JWT |
/// | POST   | `/api/v1/governance/proposals`            | `governance::create_proposal` | JWT |
/// | GET    | `/api/v1/governance/proposals`            | `governance::list_proposals`  | JWT  |
/// | POST   | `/api/v1/governance/vote`                 | `governance::cast_vote` | JWT   |
/// | GET    | `/api/v1/economics/balance/:did`          | `economics::get_balance` | JWT  |
/// | POST   | `/api/v1/economics/transfer`              | `economics::transfer_ubc` | JWT |
/// | GET    | `/api/v1/economics/transfers`             | `economics::list_transfers` | JWT |
/// | GET    | `/api/v1/financial/balance/:pubkey`       | `financial::get_balance` | JWT |
/// | POST   | `/api/v1/financial/transfer`              | `financial::transfer`  | JWT   |
/// | POST   | `/api/v1/admin/settlement/submit-root`    | `admin::submit_root`   | JWT+Priv |
/// | GET    | `/api/v1/errors`                          | `errors::error_codes`  | Public |
/// | POST   | `/api/v1/auth/challenge`                  | `wallet_auth::request_challenge` | Public |
/// | POST   | `/api/v1/auth/login`                      | `wallet_auth::login`   | Public |
/// | POST   | `/api/v1/auth/register`                   | `wallet_auth::register` | JWT   |
/// | GET    | `/api/v1/ceremony/state`                  | `ceremony::ceremony_state` | Public |
/// | POST   | `/api/v1/ceremony/contribute`             | `ceremony::ceremony_contribute` | JWT |
/// | GET    | `/api/v1/ceremony/transcript`             | `ceremony::ceremony_transcript` | Public |
/// | POST   | `/api/v1/ceremony/finalize`               | `ceremony::ceremony_finalize` | JWT |
///
/// Node info, peers, validators, errors, and ceremony read endpoints are public
/// (no JWT required) so that dashboards and monitoring tools can display live
/// network status without requiring authentication tokens. All write endpoints
/// and sensitive reads (events list, governance list, economics list) require JWT.
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
/// It separates public read-only routes from authenticated write routes:
///
/// - **Public routes** (no JWT): node info, peers, errors, ceremony state/transcript
/// - **Authenticated routes** (JWT required): events, shards, governance, economics, ceremony write ops
///
/// # Arguments
///
/// * `authorized` — shared [`AuthorizedCallers`] registry (wrapped in `Arc`)
pub fn build_api_router_with(authorized: Arc<AuthorizedCallers>) -> Router<AppState> {
    // Public routes — no JWT required (read-only status endpoints)
    let public_routes = Router::new()
        .route("/node/info", get(node::node_info))
        .route("/node/peers", get(node::node_peers))
        .route("/validators", get(node::list_validators))
        .route("/errors", get(errors::error_codes))
        .route("/ceremony/state", get(ceremony::ceremony_state))
        .route("/ceremony/transcript", get(ceremony::ceremony_transcript))
        // Protocol-wide monitoring reads (§4.4, §5, §6, §7, §8, §15).
        //
        // These are aggregates over the whole protocol — supply totals, treasury
        // buckets, fee policy, provider health — with no per-account data and no
        // caller-derived response. They are what the public dashboard and status
        // pages render, so requiring a JWT would leave those surfaces blank for
        // anyone not signed in. Every handler here takes only `State`; none reads
        // `CallerIdentity`, which is what makes serving them outside the auth
        // layer well-defined rather than merely convenient.
        //
        // Per-account reads (`/economics/balance/:did`, `/financial/balance/:pubkey`),
        // fee *calculation*, and every mutation stay authenticated below.
        .route("/supply", get(supply::get_supply))
        .route("/supply/invariants", get(supply::verify_supply_invariants))
        .route("/treasury/status", get(treasury::get_treasury_status))
        .route("/treasury/inventory", get(treasury::get_pilot_inventory))
        .route("/treasury/accounting", get(treasury::get_treasury_accounting))
        .route("/fees/burn-policy", get(fees::get_burn_policy))
        .route("/fees/stats", get(fees::get_fee_stats))
        .route("/economics/fee-burn", get(economics::get_fee_burn_stats))
        .route("/bridge/providers", get(bridge::list_providers))
        .route("/bridge/health", get(bridge::bridge_health))
        // Wallet challenge/signature login — issues JWTs, so must be public.
        .route("/auth/challenge", post(wallet_auth::request_challenge))
        .route("/auth/login", post(wallet_auth::login))
        // Provider callbacks authenticate with HMAC plus a registered service key,
        // not with a wallet JWT.
        .route("/payment-orders/callback", post(payment_orders::provider_callback));

    // Authenticated routes — JWT required (write endpoints + sensitive reads)
    let authenticated_routes = Router::new()
        // Admin — settlement submission (JWT + AuthorizedCallers required)
        .route("/admin/settlement/submit-root", post(admin::submit_root))
        // Register the caller's DID (for externally-minted JWTs, e.g. the
        // wallet's Supabase sign-in; challenge/signature login self-registers)
        .route("/auth/register", post(wallet_auth::register))
        // Events
        .route("/events", post(events::submit_event).get(events::list_events))
        .route("/events/:id", get(events::get_event))
        // Shard operations
        .route("/shards/:shard_id/operations", post(shards::submit_shard_operation))
        // Governance
        .route(
            "/governance/proposals",
            post(governance::create_proposal).get(governance::list_proposals),
        )
        .route("/governance/vote", post(governance::cast_vote))
        // Economics
        .route("/economics/balance/:did", get(economics::get_balance))
        .route("/economics/transfer", post(economics::transfer_ubc))
        .route("/economics/transfers", get(economics::list_transfers))
        // Financial — the transferable-asset ledger, distinct from UBC:
        // these transfers credit the recipient and conserve total supply.
        .route("/financial/balance/:pubkey", get(financial::get_balance))
        .route("/financial/transfer", post(financial::transfer))
        // Ceremony write operations
        .route("/ceremony/contribute", post(ceremony::ceremony_contribute))
        .route("/ceremony/finalize", post(ceremony::ceremony_finalize))
        // Fee calculation is a POST that prices a hypothetical operation, not a
        // monitoring read, so it stays behind the JWT with the other non-reads.
        // The monitoring aggregates it sits beside are served publicly above.
        .route("/fees/calculate", post(fees::calculate_fee))
        // Payment orders (§8): economic terms come only from signed quotes.
        .route("/payment-orders/quote", post(payment_orders::request_quote))
        .route("/payment-orders/initiate", post(payment_orders::initiate_payment))
        .route("/payment-orders/:id", get(payment_orders::get_order))
        .route("/payment-orders/:id/advance", post(payment_orders::advance_order))
        // Merchant onboarding and settlement (§9).
        .route("/merchants/register", post(merchants::register_merchant))
        .route(
            "/merchants/:id/payment-request",
            post(merchants::create_payment_request),
        )
        .route("/merchants/:id/payments", get(merchants::list_payments))
        .route("/merchants/:id/receipt/:payment_id", get(merchants::get_receipt))
        .route(
            "/merchants/:id/payments/:payment_id/confirm",
            post(merchants::confirm_payment),
        )
        // --- Middleware layers (outermost = last added) ---
        // Provide AuthorizedCallers via Extension for handler-level checks
        .layer(Extension(Arc::clone(&authorized)))
        // JWT authentication — validates Bearer token and inserts CallerIdentity
        .layer(middleware::from_fn(api_auth::require_auth))
        // Body size limit — reject oversized request bodies (10 MiB)
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024));

    Router::new().merge(public_routes).merge(authenticated_routes)
}
