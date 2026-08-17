//! HTTP server setup — health check, liveness/readiness probes, metrics, CORS, rate limiting, and top-level router
//!
//! This module creates the main `axum::Router` that mounts:
//! - `GET /healthz` — Kubernetes liveness probe (is process alive?)
//! - `GET /readyz` — Kubernetes readiness probe (can it serve traffic?)
//! - `GET /health` — backward-compatible health check (same as `/healthz`)
//! - `GET /metrics` — Prometheus exposition format
//! - `/api/v1/*` — the full API router from [`crate::api`]
//! - `/swagger-ui` — interactive API documentation
//! - `/api-docs/openapi.json` — OpenAPI specification
//!
//! # Liveness vs Readiness
//!
//! Kubernetes distinguishes between two types of health checks:
//!
//! - **Liveness** (`/healthz`): Is the process alive and the HTTP server
//!   responding? If this fails, Kubernetes restarts the container. This
//!   should always return 200 as long as the server is running.
//!
//! - **Readiness** (`/readyz`): Is the node operational and reachable?
//!   Returns 200 when the node has enough peers and is not syncing. Quiet
//!   networks that have not recently finalized Lane 1 events still report
//!   ready; finality counters are included as informational fields.
//!   If this fails, Kubernetes removes the pod from service endpoints
//!   but does NOT restart it.
//!
//! # Middleware (outermost first)
//!
//! 1. **CORS** — permissive defaults for development; tighten for production
//! 2. **Rate limiting** — per-client token-bucket rate limiter

use std::sync::atomic::Ordering;
use std::sync::Arc;

#[cfg(feature = "swagger-ui")]
use crate::api::ApiDoc;
use axum::extract::State;
use axum::http::StatusCode;
use axum::middleware;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::get;
use axum::Extension;
use axum::Router;
#[cfg(feature = "metrics")]
use prometheus::{Encoder, TextEncoder};
use serde_json::json;
#[cfg(feature = "swagger-ui")]
use utoipa::OpenApi;
#[cfg(feature = "swagger-ui")]
use utoipa_swagger_ui::SwaggerUi;

use crate::api;
use crate::api::auth::{self as api_auth, default_cors_layer, RateLimiter};
use crate::state::AppState;

/// Build the complete HTTP router with all routes, CORS, and rate limiting.
///
/// The router is organized as:
/// - `/healthz` — Kubernetes livenessProbe endpoint
/// - `/readyz` — Kubernetes readinessProbe endpoint
/// - `/health` — backward-compatible health check (aliases `/healthz`)
/// - `/metrics` — Prometheus metrics endpoint
/// - `/api/v1/*` — API endpoints (with JWT auth and authorization)
/// - `/swagger-ui` — Swagger UI for API documentation (optional, `swagger-ui` feature)
/// - `/api-docs/openapi.json` — OpenAPI JSON specification (optional, `swagger-ui` feature)
///
/// # Middleware
///
/// - **CORS** — allows all origins, standard methods and headers
/// - **Rate limiting** — per-client token-bucket (configurable via `OMNIA_RATE_LIMIT_RPS`)
///
/// # Layer ordering
///
/// Layers are applied in reverse order (last added = outermost):
///
/// ```text
/// Request → CORS → RateLimit → Extension(inject) → Handler
/// ```
///
/// The `Extension` layer must be innermost so it injects the
/// [`RateLimiter`] into request extensions *before* the rate-limit
/// middleware attempts to extract it.
pub fn build_http_router() -> Router<AppState> {
    let rate_limiter = Arc::new(RateLimiter::from_env());
    let cors = default_cors_layer();

    let router = Router::new()
        .route("/healthz", get(liveness_handler)) // Kubernetes livenessProbe
        .route("/readyz", get(readiness_handler)) // Kubernetes readinessProbe
        .route("/health", get(liveness_handler)) // backward compat
        .route("/metrics", get(metrics_handler))
        .nest("/api/v1", api::build_api_router());

    // Swagger UI is optional — embeds ~11MB of JS/CSS assets into the binary.
    // Enable with --features swagger-ui for development.
    #[cfg(feature = "swagger-ui")]
    let router = router.merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()));

    router
        // --- Middleware layers ---
        // Order: last added = outermost.
        // Request flow: CORS → Extension(inject) → RateLimit → Handler
        .layer(middleware::from_fn(api_auth::rate_limit_middleware))
        .layer(Extension(rate_limiter))
        .layer(cors)
}

/// Liveness probe: is the process alive?
///
/// Returns 200 if the HTTP server is responding. This always succeeds
/// as long as the server is running — it does not check peer count,
/// sync status, or finalization.
///
/// Kubernetes restarts the container if this probe fails.
pub async fn liveness_handler(State(state): State<AppState>) -> impl IntoResponse {
    let node_id = state.config.node_id;
    let uptime = state.started_at.elapsed().as_secs();

    axum::Json(json!({
        "status": "alive",
        "node_id": node_id,
        "uptime_seconds": uptime,
    }))
}

/// Readiness probe: is the node operational and reachable?
///
/// Returns 200 if:
/// - At least `readiness_min_peers` peers are connected (default: 1)
/// - Not currently in fast-sync
///
/// Finality progress is reported for operator visibility, but is not a
/// readiness gate: a quiet network can be fully operational while Lane 1 has
/// no canonical commits and Lane 0 has no recent events to preconfirm.
///
/// Returns 503 with a JSON body containing a `reason` field if not ready.
/// Possible reasons: `"no_peers"`, `"syncing"`.
///
/// Kubernetes removes the pod from service endpoints if this probe fails,
/// but does NOT restart the container.
pub async fn readiness_handler(State(state): State<AppState>) -> Response {
    let node_id = state.config.node_id;
    let min_peers = state.config.readiness_min_peers;
    let peers = state.peers.read().await.len();
    let is_syncing = state.is_syncing.load(Ordering::Relaxed);
    // Issue #260: the in-memory event store starts empty on every boot,
    // so a restarted node would report finalized_height 0 (not_ready /
    // no_finalization) forever on a quiet network. The consensus engine's
    // committed count is restored from the persistent consensus store on
    // restart, so take the max of both signals.
    let event_store_len = state.event_store.read().await.len() as u64;
    let consensus_committed = state.substrate.read().await.committed_count();
    let finalized_height = event_store_len.max(consensus_committed);

    let (lane0_enabled, lane0_finalized_events) = {
        let substrate = state.substrate.read().await;
        match substrate.lane0_stats() {
            Some((_, _, finalized)) => (true, finalized),
            None => (false, 0),
        }
    };

    // Determine readiness: need peers and not syncing. Finalization signals
    // describe chain progress, not process reachability; quiet networks can
    // be operational while neither Lane 1 nor Lane 0 has recent work.
    let has_peers = peers >= min_peers;
    let not_syncing = !is_syncing;
    let is_ready = has_peers && not_syncing;

    if is_ready {
        axum::Json(json!({
            "status": "ready",
            "node_id": node_id,
            "peers": peers,
            "finalized_height": finalized_height,
            "lane0_enabled": lane0_enabled,
            "lane0_finalized_events": lane0_finalized_events,
        }))
        .into_response()
    } else {
        let reason = if !has_peers { "no_peers" } else { "syncing" };
        (
            StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(json!({
                "status": "not_ready",
                "reason": reason,
                "node_id": node_id,
                "peers": peers,
                "is_syncing": is_syncing,
                "finalized_height": finalized_height,
                "lane0_enabled": lane0_enabled,
                "lane0_finalized_events": lane0_finalized_events,
            })),
        )
            .into_response()
    }
}

/// Handler for `GET /metrics`.
///
/// Returns all registered Prometheus metrics in the standard
/// text exposition format. This endpoint is scraped by Prometheus
/// or compatible monitoring systems.
#[cfg(feature = "metrics")]
pub async fn metrics_handler() -> impl IntoResponse {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    if encoder.encode(&metric_families, &mut buffer).is_err() {
        tracing::error!("Failed to encode Prometheus metrics");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to encode metrics".to_string(),
        );
    }
    let body = String::from_utf8(buffer).unwrap_or_else(|_| {
        tracing::error!("Prometheus output contained invalid UTF-8");
        "# ERROR: invalid UTF-8 in metrics output\n".to_string()
    });
    (StatusCode::OK, body)
}

/// Handler for `GET /metrics` when metrics feature is disabled.
#[cfg(not(feature = "metrics"))]
pub async fn metrics_handler() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        "Metrics endpoint disabled (compile WITH --features metrics to enable)\n",
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::api::events::StoredEvent;
    use crate::api::node::PeerInfo;
    use crate::config::NodeConfig;
    #[cfg(feature = "metrics")]
    use crate::state::NodeMetrics;
    use axum::body::Body;
    use axum::http::{Request, StatusCode as HttpStatus};
    use omnia_shards::ShardRouter;
    use omnia_substrate::Substrate;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;
    use std::time::Instant;
    use tokio::sync::{Mutex, RwLock};
    use tower::ServiceExt;

    /// Create a test `AppState` with configurable peers and syncing state.
    fn make_test_state(peers: Vec<PeerInfo>, is_syncing: bool, event_count: usize) -> AppState {
        let config = NodeConfig {
            node_id: 1,
            listen_addr: "0.0.0.0:4001".to_string(),
            bootstrap_nodes: vec![],
            http_port: 8080,
            data_dir: PathBuf::from("./data"),
            log_level: "info".to_string(),
            max_payload_size: 1024 * 1024,
            pruning_depth: 0,
            snapshot_interval: 10_000,
            slashing_data_dir: None,
            nonce_data_dir: None,
            consensus_data_dir: None,
            protocol_version: "4.0.0".to_string(),
            fast_sync: false,
            enable_tcp_fallback: true,
            mint_authority: None,
            readiness_min_peers: 1,
            readiness_max_finalization_age: 600,
        };

        let substrate_config = omnia_substrate::SubstrateConfig::try_new(config.node_id_bytes())
            .expect("valid substrate config in test fixture");
        let substrate = Substrate::new(substrate_config);
        let slashing_engine = substrate.slashing.clone();

        let fee_schedule = omnia_shards::FeeSchedule::standard();
        let quota = omnia_economics::QuotaSystem::default_system();
        let mut shard_router = ShardRouter::new(fee_schedule, quota);
        // Register an economics shard so economics-backed handlers have a
        // single-source state to read/write (see `with_economics`).
        shard_router.register(Box::new(omnia_shards::EconomicsShard::new()));

        let event_store: indexmap::IndexMap<String, StoredEvent> = (0..event_count)
            .map(|i| {
                let key = format!("event_{i}");
                let event = StoredEvent {
                    id: key.clone(),
                    creator: "test".to_string(),
                    sequence: i as u64,
                    timestamp: 0,
                    payload: String::new(),
                    event_type: "generic".to_string(),
                    status: "finalized".to_string(),
                };
                (key, event)
            })
            .collect();

        #[cfg(feature = "metrics")]
        let metrics = NodeMetrics::new().expect("Failed to create metrics");

        let (quote_service, payment_store, service_role_registry, ghana_provider) =
            crate::state::default_payment_services();

        AppState {
            config,
            substrate: Arc::new(RwLock::new(substrate)),
            slashing: Arc::new(Mutex::new(slashing_engine)),
            shard_router: Arc::new(std::sync::Mutex::new(shard_router)),
            event_store: Arc::new(RwLock::new(event_store)),
            transfer_history: Arc::new(RwLock::new(Vec::new())),
            challenges: crate::api::wallet_auth::new_challenge_store(),
            peers: Arc::new(RwLock::new(peers)),
            #[cfg(feature = "metrics")]
            metrics: Arc::new(metrics),
            started_at: Instant::now(),
            is_syncing: Arc::new(AtomicBool::new(is_syncing)),
            keypair: Some(omnia_substrate::crypto::generate_keypair()),
            settlement: Arc::new(omnia_adapters::MockSettlementAdapter::new()),
            #[cfg(feature = "zk")]
            ceremony_server: None,
            // Sprint 3–5 financial pallet state
            asset_registry: Arc::new(RwLock::new(omnia_asset_registry::AssetRegistry::new())),
            supply_tracker: Arc::new(RwLock::new(omnia_asset_registry::SupplyTracker::new())),
            treasury: Arc::new(RwLock::new(omnia_asset_registry::Treasury::new())),
            fee_schedule: Arc::new(RwLock::new(omnia_fee_burn::OmniaFeeSchedule::default())),
            burn_accounting: Arc::new(RwLock::new(omnia_fee_burn::BurnAccounting::new())),
            payment_engine: Arc::new(std::sync::Mutex::new(omnia_payment_order::PaymentEngine::new(0))),
            quote_service,
            payment_store,
            service_role_registry,
            ghana_provider,
            merchant_registry: Arc::new(std::sync::Mutex::new(crate::api::merchants::MerchantRegistry::new())),
        }
    }

    fn make_peer(id: &str) -> PeerInfo {
        PeerInfo {
            peer_id: id.to_string(),
            address: format!("/ip4/127.0.0.1/tcp/4001/p2p/{id}"),
            connected_at: 0,
        }
    }

    #[tokio::test]
    async fn test_liveness_always_returns_200() {
        // Even with 0 peers and syncing, liveness should return 200
        let state = make_test_state(vec![], true, 0);
        let app = build_http_router().with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .expect("test assertion failed"),
            )
            .await
            .expect("test assertion failed");

        assert_eq!(response.status(), HttpStatus::OK);
    }

    #[tokio::test]
    async fn test_readiness_returns_503_when_no_peers() {
        // No peers → 503 with reason "no_peers"
        let state = make_test_state(vec![], false, 1);
        let app = build_http_router().with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/readyz")
                    .body(Body::empty())
                    .expect("test assertion failed"),
            )
            .await
            .expect("test assertion failed");

        assert_eq!(response.status(), HttpStatus::SERVICE_UNAVAILABLE);

        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .expect("test assertion failed");
        let body: serde_json::Value = serde_json::from_slice(&body).expect("test assertion failed");
        assert_eq!(body["status"], "not_ready");
        assert_eq!(body["reason"], "no_peers");
    }

    #[tokio::test]
    async fn test_readiness_returns_503_when_syncing() {
        // Fast-sync active → 503 with reason "syncing"
        let state = make_test_state(vec![make_peer("peer1")], true, 1);
        let app = build_http_router().with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/readyz")
                    .body(Body::empty())
                    .expect("test assertion failed"),
            )
            .await
            .expect("test assertion failed");

        assert_eq!(response.status(), HttpStatus::SERVICE_UNAVAILABLE);

        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .expect("test assertion failed");
        let body: serde_json::Value = serde_json::from_slice(&body).expect("test assertion failed");
        assert_eq!(body["status"], "not_ready");
        assert_eq!(body["reason"], "syncing");
    }

    #[tokio::test]
    async fn test_readiness_returns_200_when_operational_but_idle() {
        // Peers + not syncing + no Lane 1 or Lane 0 finality is still ready:
        // readiness tracks process reachability, not recent traffic.
        let state = make_test_state(vec![make_peer("peer1")], false, 0);
        let app = build_http_router().with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/readyz")
                    .body(Body::empty())
                    .expect("test assertion failed"),
            )
            .await
            .expect("test assertion failed");

        assert_eq!(response.status(), HttpStatus::OK);

        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .expect("test assertion failed");
        let body: serde_json::Value = serde_json::from_slice(&body).expect("test assertion failed");
        assert_eq!(body["status"], "ready");
        assert_eq!(body["finalized_height"], 0);
        assert_eq!(body["lane0_enabled"], false);
        assert_eq!(body["lane0_finalized_events"], 0);
    }

    #[tokio::test]
    async fn test_readiness_returns_200_with_lane0_finality_but_no_lane1_finalization() {
        // Lane 0 preconfirmation demonstrates useful finality progress even
        // when Lane 1 canonical commits remain at 0; either way, readiness is
        // governed by peers + sync state.
        let state = make_test_state(vec![make_peer("peer1")], false, 0);
        {
            let mut substrate = state.substrate.write().await;
            let keypair = state.keypair.as_ref().expect("test assertion failed");
            let validators = omnia_substrate::lane0::ValidatorSet::new([(keypair.verifying_key().to_bytes(), 1)])
                .expect("test assertion failed");
            substrate.init_lane0(validators);
            substrate.add_validator(state.config.node_id_bytes(), keypair.clone(), 1);
            let mut event = omnia_substrate::Event::genesis(state.config.node_id_bytes(), vec![1, 2, 3])
                .expect("valid genesis event");
            event.sign_with_keypair(keypair).expect("signing");
            substrate.submit_event(event).await.expect("test assertion failed");
        }
        let app = build_http_router().with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/readyz")
                    .body(Body::empty())
                    .expect("test assertion failed"),
            )
            .await
            .expect("test assertion failed");

        assert_eq!(response.status(), HttpStatus::OK);

        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .expect("test assertion failed");
        let body: serde_json::Value = serde_json::from_slice(&body).expect("test assertion failed");
        assert_eq!(body["status"], "ready");
        assert_eq!(body["finalized_height"], 0);
        assert_eq!(body["lane0_enabled"], true);
        assert_eq!(body["lane0_finalized_events"], 1);
    }

    #[tokio::test]
    async fn test_readiness_returns_200_when_healthy() {
        // Peers + not syncing + finalization → 200
        let state = make_test_state(vec![make_peer("peer1")], false, 1);
        let app = build_http_router().with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/readyz")
                    .body(Body::empty())
                    .expect("test assertion failed"),
            )
            .await
            .expect("test assertion failed");

        assert_eq!(response.status(), HttpStatus::OK);

        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .expect("test assertion failed");
        let body: serde_json::Value = serde_json::from_slice(&body).expect("test assertion failed");
        assert_eq!(body["status"], "ready");
        assert_eq!(body["peers"], 1);
        assert_eq!(body["finalized_height"], 1);
    }

    /// Issue #260 regression: a restarted node has an empty in-memory
    /// event store, but its persistent consensus store still knows how
    /// many events were committed. `/readyz` must report the restored
    /// height instead of `no_finalization` forever.
    #[tokio::test]
    async fn test_readiness_survives_restart_via_consensus_store() {
        use omnia_substrate::{ConsensusStore, PersistedConsensusState, RedbConsensusStore};

        let db_path = std::env::temp_dir().join(format!("omnia-readyz-260-{}.redb", std::process::id()));
        let _ = std::fs::remove_file(&db_path);

        // "Previous run": persist a consensus state with 4 committed events.
        {
            let store = RedbConsensusStore::open(&db_path).expect("test assertion failed");
            store
                .save_state(&PersistedConsensusState {
                    current_round: 6,
                    round_seed: [0u8; 32],
                    committed_events: 4,
                    last_finalized_round: 4,
                    active_validators: vec![],
                    equivocation_tracking: std::collections::HashMap::new(),
                    first_event_for_sequence: std::collections::HashMap::new(),
                    version: 2,
                })
                .expect("test assertion failed");
        }

        // "Restart": fresh AppState with an EMPTY HTTP event store, whose
        // substrate restores the committed count from the redb store.
        let mut state = make_test_state(vec![make_peer("peer1")], false, 0);
        let mut substrate_config =
            omnia_substrate::SubstrateConfig::try_new(state.config.node_id_bytes()).expect("valid substrate config");
        substrate_config.consensus_data_dir = Some(db_path.clone());
        state.substrate = Arc::new(RwLock::new(Substrate::new(substrate_config)));

        let app = build_http_router().with_state(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/readyz")
                    .body(Body::empty())
                    .expect("test assertion failed"),
            )
            .await
            .expect("test assertion failed");

        let _ = std::fs::remove_file(&db_path);

        assert_eq!(response.status(), HttpStatus::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .expect("test assertion failed");
        let body: serde_json::Value = serde_json::from_slice(&body).expect("test assertion failed");
        assert_eq!(body["status"], "ready");
        assert_eq!(
            body["finalized_height"], 4,
            "finalized_height must come from the restored consensus state"
        );
    }

    #[tokio::test]
    async fn test_health_backward_compat() {
        // /health still works and returns liveness (200) response
        let state = make_test_state(vec![], true, 0);
        let app = build_http_router().with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .expect("test assertion failed"),
            )
            .await
            .expect("test assertion failed");

        assert_eq!(response.status(), HttpStatus::OK);

        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .expect("test assertion failed");
        let body: serde_json::Value = serde_json::from_slice(&body).expect("test assertion failed");
        assert_eq!(body["status"], "alive");
    }
}
