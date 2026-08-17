#![allow(clippy::unwrap_used)]
//! Comprehensive E2E REST API integration tests for the Omnia node.
//!
//! This test suite covers every API endpoint across all authentication
//! states, plus rate limiting, privileged operations, CORS, and error
//! format consistency.
//!
//! # Test Categories
//!
//! 1. **Auth Test Matrix** — every endpoint × every auth state
//!    (no auth, valid JWT, expired JWT, wrong-secret JWT)
//! 2. **Rate Limiting** — token-bucket enforcement
//! 3. **Privileged Operations** — MintUbc / AdvanceEpoch authorization
//! 4. **CORS** — preflight and cross-origin requests
//! 5. **Error Format** — consistent `{"error": "…"}` JSON schema
//!
//! # Running
//!
//! Because these tests modify process-global environment variables,
//! they must be run with `-- --test-threads=1` to avoid race conditions
//! between parallel test instances.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use jsonwebtoken::{encode, EncodingKey, Header};
use omnia_economics::EconomicsState;
use omnia_node::api::auth::Claims;
use omnia_node::config::NodeConfig;
use omnia_node::http;
use omnia_node::state::{default_payment_services, AppState, NodeMetrics};
use omnia_shards::{
    BiologicalShard, ComputationalShard, EconomicsShard, FeeSchedule, FinancialShard, IdentityShard, PhysicalShard,
    ShardRouter,
};
use omnia_substrate::{Substrate, SubstrateConfig};
use serde_json::{json, Value};
use tokio::sync::{Mutex, RwLock};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// JWT secret used for all auth tests.
const JWT_SECRET: &str = "test-jwt-secret-api-integration";

/// Admin caller ID — authorised for privileged operations (mint, advance_epoch).
const ADMIN_CALLER: &str = "admin-caller";

/// Regular (non-admin) caller ID.
const REGULAR_CALLER: &str = "regular-caller";

// ---------------------------------------------------------------------------
// Global mutex for serialising environment-variable mutations
// ---------------------------------------------------------------------------

/// Integration tests that set `OMNIA_JWT_SECRET`, `OMNIA_AUTHORIZED_CALLERS`,
/// or `OMNIA_RATE_LIMIT_RPS` must hold this lock for the entire test duration
/// to prevent race conditions with parallel test execution.
static ENV_LOCK: std::sync::LazyLock<std::sync::Mutex<()>> = std::sync::LazyLock::new(|| std::sync::Mutex::new(()));

// ---------------------------------------------------------------------------
// RAII guard — removes environment variables when dropped
// ---------------------------------------------------------------------------

/// Removes the listed environment variables on drop, ensuring cleanup
/// even if a test panics.
struct EnvGuard {
    keys: Vec<&'static str>,
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for key in &self.keys {
            std::env::remove_var(key);
        }
    }
}

// ---------------------------------------------------------------------------
// Token helpers
// ---------------------------------------------------------------------------

/// Create a valid HS256 JWT for `caller_id`, valid for 1 hour.
///
/// Requires `OMNIA_JWT_SECRET` to be set in the environment.
fn make_valid_token(caller_id: &str) -> String {
    let secret = std::env::var("OMNIA_JWT_SECRET").expect("OMNIA_JWT_SECRET must be set");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let claims = Claims {
        sub: caller_id.to_string(),
        iat: now,
        exp: now + 3600,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .expect("Failed to encode valid token")
}

/// Create an expired JWT (exp = 1 → 1970-01-01 00:00:01 UTC).
///
/// Signed with `secret` so the server can decode it and discover
/// the expiration.
fn make_expired_token(secret: &str) -> String {
    let claims = Claims {
        sub: "expired-caller".to_string(),
        iat: 1,
        exp: 1,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .expect("Failed to encode expired token")
}

/// Create a JWT signed with a secret that differs from the one the server
/// uses. The server should reject it with 401 InvalidToken.
fn make_wrong_secret_token() -> String {
    let claims = Claims {
        sub: "wrong-secret-caller".to_string(),
        iat: 1,
        exp: 9999999999,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret("this-is-not-the-server-secret".as_bytes()),
    )
    .expect("Failed to encode wrong-secret token")
}

// ---------------------------------------------------------------------------
// Server helpers
// ---------------------------------------------------------------------------

/// Build the [`AppState`] for a test server on the given port.
fn build_test_app_state(port: u16) -> AppState {
    let node_id_bytes = {
        let mut id = [0u8; 32];
        id[..8].copy_from_slice(&42u64.to_le_bytes());
        id
    };

    let substrate_config =
        SubstrateConfig::try_new(node_id_bytes).unwrap_or_else(|e| panic!("Failed to create SubstrateConfig: {e:?}"));
    let substrate = Substrate::new(substrate_config);
    let slashing = substrate.slashing.clone();

    // Generate the node keypair once and reuse it for both the economics
    // admin_keys and the AppState.keypair field.
    let node_keypair = omnia_substrate::crypto::generate_keypair();
    let mut economics = EconomicsState::new();
    // P0-9 fix: when the 'production' feature is enabled (via --all-features),
    // MintUbc fails-closed on empty admin_keys. Add the node's own keypair
    // as an admin key so tests can mint UBC. In production, the operator
    // would configure admin keys via OMNIA_AUTHORIZED_CALLERS.
    economics.add_admin_key(node_keypair.verifying_key().to_bytes());

    let fee_schedule = FeeSchedule::standard();
    let quota = omnia_economics::QuotaSystem::default_system();
    let mut shard_router = ShardRouter::new(fee_schedule, quota);
    shard_router.register(Box::new(FinancialShard::new()));
    shard_router.register(Box::new(ComputationalShard::new()));
    shard_router.register(Box::new(PhysicalShard::new()));
    shard_router.register(Box::new(BiologicalShard::new()));
    shard_router.register(Box::new(IdentityShard::new()));
    // Single source of truth: the economics state (with admin key) lives in
    // the router's economics shard — the API reads/writes it via the shared
    // router lock (with_economics), no separate AppState.economics copy.
    shard_router.register(Box::new(EconomicsShard::new_with_state(economics)));

    let metrics = NodeMetrics::new().expect("Failed to create metrics");

    let config = NodeConfig {
        node_id: 42,
        listen_addr: "127.0.0.1:0".to_string(),
        bootstrap_nodes: vec![],
        http_port: port,
        data_dir: PathBuf::from("./test-data"),
        log_level: "warn".to_string(),
        max_payload_size: omnia_substrate::MAX_PAYLOAD_SIZE,
        pruning_depth: 0,
        snapshot_interval: 10_000,
        slashing_data_dir: None,
        nonce_data_dir: None,
        consensus_data_dir: None,
        protocol_version: omnia_substrate::PROTOCOL_VERSION.to_string(),
        fast_sync: false,
        enable_tcp_fallback: true,
        mint_authority: None,
        readiness_min_peers: 1,
        readiness_max_finalization_age: 600,
    };

    let (quote_service, payment_store, service_role_registry, ghana_provider) = default_payment_services();

    AppState {
        config,
        substrate: Arc::new(RwLock::new(substrate)),
        slashing: Arc::new(Mutex::new(slashing)),
        shard_router: Arc::new(std::sync::Mutex::new(shard_router)),
        event_store: Arc::new(RwLock::new(indexmap::IndexMap::new())),
        transfer_history: Arc::new(RwLock::new(Vec::new())),
        challenges: omnia_node::api::wallet_auth::new_challenge_store(),
        peers: Arc::new(RwLock::new(Vec::new())),
        metrics: Arc::new(metrics),
        started_at: Instant::now(),
        is_syncing: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        keypair: Some(node_keypair),
        settlement: Arc::new(omnia_adapters::MockSettlementAdapter::new()),
        #[cfg(feature = "zk")]
        ceremony_server: None,
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
        merchant_registry: Arc::new(std::sync::Mutex::new(
            omnia_node::api::merchants::MerchantRegistry::new(),
        )),
    }
}

/// Configure auth and rate-limit environment for a test server.
///
/// Callers must hold [`ENV_LOCK`] before invoking this so the process-global
/// environment remains stable while router state reads it.
fn configure_test_server_env(authorized_callers: Option<&str>, rate_limit_rps: Option<u64>) {
    std::env::set_var("OMNIA_JWT_SECRET", JWT_SECRET);
    std::env::set_var("OMNIA_JWT_ALLOW_LEGACY_HS256", "true");

    if let Some(callers) = authorized_callers {
        std::env::set_var("OMNIA_AUTHORIZED_CALLERS", callers);
    } else {
        std::env::remove_var("OMNIA_AUTHORIZED_CALLERS");
    }

    if let Some(rps) = rate_limit_rps {
        std::env::set_var("OMNIA_RATE_LIMIT_RPS", rps.to_string());
    } else {
        std::env::remove_var("OMNIA_RATE_LIMIT_RPS");
    }

    // Reset the JWT config cache and re-initialize from the env vars we just set.
    // Without this, validate_token() reads a stale cache from a prior test run
    // and rejects HS256 tokens.
    omnia_node::api::auth::reset_jwt_secret_for_test();
    omnia_node::api::auth::init_jwt_secret();
}

/// Like `setup_server` but allows pre-registering DIDs in the economics
/// state before the server starts. Uses the same ENV_LOCK + EnvGuard pattern
/// as `setup_server` to avoid env var races with parallel tests.
#[allow(clippy::await_holding_lock)]
async fn setup_server_with_economics<F>(pre_register: F) -> TestServer
where
    F: FnOnce(&mut EconomicsState),
{
    let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    configure_test_server_env(Some(ADMIN_CALLER), None);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind to random port");
    let port = listener.local_addr().expect("test assertion failed").port();

    let app_state = build_test_app_state(port);
    {
        // Pre-register DIDs directly in the single-source economics state
        // (the router's economics shard) that the handlers read/write.
        let mut router = app_state.shard_router.lock().unwrap_or_else(|e| e.into_inner());
        let econ = router
            .economics_mut()
            .expect("economics shard registered in test fixture");
        pre_register(econ);
    }

    let app = http::build_http_router().with_state(app_state);

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("Test server error");
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let base_url = format!("http://127.0.0.1:{port}");

    let env_guard = EnvGuard {
        keys: vec![
            "OMNIA_JWT_SECRET",
            "OMNIA_AUTHORIZED_CALLERS",
            "OMNIA_JWT_ALLOW_LEGACY_HS256",
            "OMNIA_RATE_LIMIT_RPS",
        ],
    };

    TestServer {
        base_url,
        _handle: handle,
        _env_guard: env_guard,
        _lock: lock,
    }
}

/// Like [`setup_server_with_economics`] but exposes the **financial**
/// shard state, so a test can pre-fund transferable balances.
#[allow(clippy::await_holding_lock)]
async fn setup_server_with_financial<F>(pre_fund: F) -> TestServer
where
    F: FnOnce(&mut omnia_shards::FinancialState),
{
    let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    configure_test_server_env(Some(ADMIN_CALLER), None);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind to random port");
    let port = listener.local_addr().expect("test assertion failed").port();

    let app_state = build_test_app_state(port);
    {
        let mut router = app_state.shard_router.lock().unwrap_or_else(|e| e.into_inner());
        let fin = router
            .financial_mut()
            .expect("financial shard registered in test fixture");
        pre_fund(fin);
    }

    let app = http::build_http_router().with_state(app_state);

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("Test server error");
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let env_guard = EnvGuard {
        keys: vec![
            "OMNIA_JWT_SECRET",
            "OMNIA_AUTHORIZED_CALLERS",
            "OMNIA_JWT_ALLOW_LEGACY_HS256",
            "OMNIA_RATE_LIMIT_RPS",
        ],
    };

    TestServer {
        base_url: format!("http://127.0.0.1:{port}"),
        _handle: handle,
        _env_guard: env_guard,
        _lock: lock,
    }
}

/// Start a test HTTP server on a random port.
///
/// The caller must hold [`ENV_LOCK`] for the lifetime of the returned server.
/// This helper configures the environment and refreshes the JWT cache before
/// constructing state or building the router.
async fn start_test_server(
    _lock: &std::sync::MutexGuard<'static, ()>,
    authorized_callers: Option<&str>,
    rate_limit_rps: Option<u64>,
) -> (String, tokio::task::JoinHandle<()>) {
    configure_test_server_env(authorized_callers, rate_limit_rps);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind to random port");
    let port = listener.local_addr().expect("test assertion failed").port();

    let app_state = build_test_app_state(port);
    let app = http::build_http_router().with_state(app_state);

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("Test server error");
    });

    // Give the server a moment to start accepting connections
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    (format!("http://127.0.0.1:{port}"), handle)
}

/// Holds a running test server together with the RAII guards that ensure
/// environment-variable cleanup and lock release.
struct TestServer {
    base_url: String,
    _handle: tokio::task::JoinHandle<()>,
    _env_guard: EnvGuard,
    _lock: std::sync::MutexGuard<'static, ()>,
}

/// Set up a test server with JWT authentication and optional rate limiting.
///
/// Sets the following environment variables before constructing the router:
/// - `OMNIA_JWT_SECRET` = [`JWT_SECRET`]
/// - `OMNIA_AUTHORIZED_CALLERS` = [`ADMIN_CALLER`]
/// - `OMNIA_RATE_LIMIT_RPS` = `rate_limit_rps` (if provided)
///
/// The env vars and the serialisation lock are released when the returned
/// [`TestServer`] is dropped.
#[allow(clippy::await_holding_lock)]
async fn setup_server(rate_limit_rps: Option<u64>) -> TestServer {
    let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let (base_url, handle) = start_test_server(&lock, Some(ADMIN_CALLER), rate_limit_rps).await;

    let env_guard = EnvGuard {
        keys: vec![
            "OMNIA_JWT_SECRET",
            "OMNIA_AUTHORIZED_CALLERS",
            "OMNIA_JWT_ALLOW_LEGACY_HS256",
            "OMNIA_RATE_LIMIT_RPS",
        ],
    };

    TestServer {
        base_url,
        _handle: handle,
        _env_guard: env_guard,
        _lock: lock,
    }
}

/// Like [`setup_server`] but boots with Lane 0 enabled and `validators`
/// members, each with stake 1.
///
/// The default fixture leaves Lane 0 disabled, which is exactly the case
/// where the validator-count fields report `null`. To assert they report the
/// **real** fast-path set size — the number that differs from the staking
/// registry's `count` on the live mesh — Lane 0 has to actually be on.
#[allow(clippy::await_holding_lock)]
async fn setup_server_with_lane0(validators: usize) -> TestServer {
    let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    configure_test_server_env(Some(ADMIN_CALLER), None);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind to random port");
    let port = listener.local_addr().expect("test assertion failed").port();

    let app_state = build_test_app_state(port);
    {
        let keys: Vec<_> = (0..validators)
            .map(|_| omnia_substrate::crypto::generate_keypair())
            .collect();
        let set = omnia_substrate::lane0::ValidatorSet::new(keys.iter().map(|k| (k.verifying_key().to_bytes(), 1)))
            .expect("non-empty validator set with non-zero stake");
        app_state.substrate.write().await.init_lane0(set);
    }

    let app = http::build_http_router().with_state(app_state);

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("Test server error");
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    TestServer {
        base_url: format!("http://127.0.0.1:{port}"),
        _handle: handle,
        _env_guard: EnvGuard {
            keys: vec![
                "OMNIA_JWT_SECRET",
                "OMNIA_AUTHORIZED_CALLERS",
                "OMNIA_JWT_ALLOW_LEGACY_HS256",
                "OMNIA_RATE_LIMIT_RPS",
            ],
        },
        _lock: lock,
    }
}

// ===========================================================================
//  1. AUTH TEST MATRIX — every endpoint × every auth state
// ===========================================================================

// ---- GET /api/v1/node/info ----

#[tokio::test]
async fn test_auth_node_info() {
    let server = setup_server(None).await;
    let client = reqwest::Client::new();

    // No auth → 200 (node/info is a public endpoint for dashboards)
    let resp = client
        .get(format!("{}/api/v1/node/info", server.base_url))
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 200, "Public endpoint should be accessible without auth");
    let body: Value = resp.json().await.expect("test assertion failed");
    assert!(body["node_id"].is_string(), "Response should contain node_id");
}

/// A client cannot tell "how many validators secure this network" from
/// `GET /api/v1/validators` alone: that endpoint reports the
/// `validator_candidates` staking registry, and on the live five-node mesh
/// only the genesis node is registered, so it answers `count: 1`. The
/// fast-path set size lives on `lane0` in node info, and these two tests pin
/// it in both directions — reported when Lane 0 is on, explicitly `null`
/// when it is off (never silently absent, never a misleading zero).
#[tokio::test]
async fn test_node_info_reports_lane0_validator_set_size() {
    let server = setup_server_with_lane0(5).await;
    let client = reqwest::Client::new();

    let body: Value = client
        .get(format!("{}/api/v1/node/info", server.base_url))
        .send()
        .await
        .expect("test assertion failed")
        .json()
        .await
        .expect("test assertion failed");

    assert_eq!(
        body["lane0"]["validator_count"], 5,
        "node info must report the size of the active Lane 0 set, not the staking registry"
    );
    assert_eq!(body["lane0"]["validator_stake"], 5, "five validators at stake 1 each");
}

#[tokio::test]
async fn test_validators_endpoint_reports_lane0_count_alongside_registry() {
    let server = setup_server_with_lane0(5).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/api/v1/validators", server.base_url))
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 200, "validators is a public endpoint for dashboards");
    let body: Value = resp.json().await.expect("test assertion failed");

    // `count` is the staking registry, which the fixture leaves empty. That
    // is the whole point: a caller reading only `count` would conclude the
    // network has no validators while five are finalizing Lane 0.
    assert_eq!(body["count"], 0, "no candidates registered in this fixture");
    assert_eq!(
        body["lane0_validator_count"], 5,
        "the response must carry the fast-path set size so `count` cannot be mistaken for it"
    );
}

#[tokio::test]
async fn test_lane0_validator_count_is_null_when_lane0_disabled() {
    let server = setup_server(None).await;
    let client = reqwest::Client::new();

    let body: Value = client
        .get(format!("{}/api/v1/validators", server.base_url))
        .send()
        .await
        .expect("test assertion failed")
        .json()
        .await
        .expect("test assertion failed");

    assert!(
        body.get("lane0_validator_count").is_some_and(Value::is_null),
        "with Lane 0 off the field must be present and null — a client has to be able to \
         distinguish 'not running Lane 0' from 'zero validators', got {:?}",
        body.get("lane0_validator_count")
    );
}

// ---- GET /api/v1/node/peers ----

#[tokio::test]
async fn test_auth_node_peers() {
    let server = setup_server(None).await;
    let client = reqwest::Client::new();

    // No auth → 200 (node/peers is a public endpoint for dashboards)
    let resp = client
        .get(format!("{}/api/v1/node/peers", server.base_url))
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 200, "Public endpoint should be accessible without auth");
    let body: Value = resp.json().await.expect("test assertion failed");
    assert!(body["peers"].is_array(), "Response should contain peers array");
}

// ---- POST /api/v1/events ----

#[tokio::test]
async fn test_auth_submit_event() {
    let server = setup_server(None).await;
    let client = reqwest::Client::new();
    let valid_token = make_valid_token(REGULAR_CALLER);
    let expired_token = make_expired_token(JWT_SECRET);
    let wrong_secret_token = make_wrong_secret_token();

    let event_body = json!({
        "payload": hex::encode(b"hello omnia"),
        "event_type": "test"
    });

    // No auth → 401
    let resp = client
        .post(format!("{}/api/v1/events", server.base_url))
        .json(&event_body)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 401);

    // Valid JWT → 201
    let resp = client
        .post(format!("{}/api/v1/events", server.base_url))
        .bearer_auth(&valid_token)
        .json(&event_body)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 201, "Valid JWT should yield 201 for event submission");
    let body: Value = resp.json().await.expect("test assertion failed");
    assert!(body["event_id"].is_string(), "Response should contain event_id");

    // Expired JWT → 401
    let resp = client
        .post(format!("{}/api/v1/events", server.base_url))
        .bearer_auth(&expired_token)
        .json(&event_body)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 401);

    // Wrong-secret JWT → 401
    let resp = client
        .post(format!("{}/api/v1/events", server.base_url))
        .bearer_auth(&wrong_secret_token)
        .json(&event_body)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 401);
}

// ---- GET /api/v1/events/{id} ----

#[tokio::test]
async fn test_auth_get_event() {
    let server = setup_server(None).await;
    let client = reqwest::Client::new();
    let valid_token = make_valid_token(REGULAR_CALLER);
    let expired_token = make_expired_token(JWT_SECRET);
    let wrong_secret_token = make_wrong_secret_token();

    // First, submit an event so we can retrieve it
    let event_body = json!({
        "payload": hex::encode(b"test event for retrieval"),
        "event_type": "test"
    });
    let resp = client
        .post(format!("{}/api/v1/events", server.base_url))
        .bearer_auth(&valid_token)
        .json(&event_body)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 201);
    let create_body: Value = resp.json().await.expect("test assertion failed");
    let event_id = create_body["event_id"]
        .as_str()
        .expect("test assertion failed")
        .to_string();

    // No auth → 401
    let resp = client
        .get(format!("{}/api/v1/events/{event_id}", server.base_url))
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 401);

    // Valid JWT → 200 (event found)
    let resp = client
        .get(format!("{}/api/v1/events/{event_id}", server.base_url))
        .bearer_auth(&valid_token)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.expect("test assertion failed");
    assert_eq!(body["id"].as_str().expect("test assertion failed"), event_id);

    // Valid JWT + nonexistent event → 404
    let resp = client
        .get(format!("{}/api/v1/events/nonexistent", server.base_url))
        .bearer_auth(&valid_token)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 404);

    // Expired JWT → 401
    let resp = client
        .get(format!("{}/api/v1/events/{event_id}", server.base_url))
        .bearer_auth(&expired_token)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 401);

    // Wrong-secret JWT → 401
    let resp = client
        .get(format!("{}/api/v1/events/{event_id}", server.base_url))
        .bearer_auth(&wrong_secret_token)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 401);
}

// ---- POST /api/v1/shards/{shard_id}/operations ----

#[tokio::test]
async fn test_auth_shard_operation() {
    let server = setup_server(None).await;
    let client = reqwest::Client::new();
    let valid_token = make_valid_token(REGULAR_CALLER);
    let expired_token = make_expired_token(JWT_SECRET);
    let wrong_secret_token = make_wrong_secret_token();

    // Use a non-privileged operation (register) to test auth
    let op_body = json!({
        "operation": "register",
        "params": {"did": "did:test:user1"}
    });

    // No auth → 401
    let resp = client
        .post(format!("{}/api/v1/shards/economics/operations", server.base_url))
        .json(&op_body)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 401);

    // Valid JWT → 200 (register is non-privileged, so any valid JWT works)
    let resp = client
        .post(format!("{}/api/v1/shards/economics/operations", server.base_url))
        .bearer_auth(&valid_token)
        .json(&op_body)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(
        resp.status(),
        200,
        "Valid JWT should yield 200 for non-privileged shard operation"
    );

    // Expired JWT → 401
    let resp = client
        .post(format!("{}/api/v1/shards/economics/operations", server.base_url))
        .bearer_auth(&expired_token)
        .json(&op_body)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 401);

    // Wrong-secret JWT → 401
    let resp = client
        .post(format!("{}/api/v1/shards/economics/operations", server.base_url))
        .bearer_auth(&wrong_secret_token)
        .json(&op_body)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 401);
}

// ---- POST /api/v1/governance/proposals ----

#[tokio::test]
async fn test_auth_create_proposal() {
    let server = setup_server(None).await;
    let client = reqwest::Client::new();
    let valid_token = make_valid_token(REGULAR_CALLER);
    let expired_token = make_expired_token(JWT_SECRET);
    let wrong_secret_token = make_wrong_secret_token();

    let proposal_body = json!({
        "id": "proposal-test-1",
        "description": "A test governance proposal",
        "expires_at_epoch": 100
    });

    // No auth → 401
    let resp = client
        .post(format!("{}/api/v1/governance/proposals", server.base_url))
        .json(&proposal_body)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 401);

    // Valid JWT → 201
    let resp = client
        .post(format!("{}/api/v1/governance/proposals", server.base_url))
        .bearer_auth(&valid_token)
        .json(&proposal_body)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 201, "Valid JWT should yield 201 for proposal creation");
    let body: Value = resp.json().await.expect("test assertion failed");
    assert_eq!(body["id"].as_str().expect("test assertion failed"), "proposal-test-1");

    // Expired JWT → 401
    let resp = client
        .post(format!("{}/api/v1/governance/proposals", server.base_url))
        .bearer_auth(&expired_token)
        .json(&json!({
            "id": "proposal-expired",
            "description": "Should not work",
            "expires_at_epoch": 200
        }))
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 401);

    // Wrong-secret JWT → 401
    let resp = client
        .post(format!("{}/api/v1/governance/proposals", server.base_url))
        .bearer_auth(&wrong_secret_token)
        .json(&json!({
            "id": "proposal-wrong",
            "description": "Should not work",
            "expires_at_epoch": 200
        }))
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 401);
}

// ---- POST /api/v1/governance/vote ----

#[tokio::test]
async fn test_auth_cast_vote() {
    let server = setup_server(None).await;
    let client = reqwest::Client::new();
    let valid_token = make_valid_token(REGULAR_CALLER);
    let expired_token = make_expired_token(JWT_SECRET);
    let wrong_secret_token = make_wrong_secret_token();

    // Note: the vote will fail at business-logic level (voter has no stake),
    // but auth should still pass, yielding 400 rather than 401.
    let vote_body = json!({
        "did": "did:test:voter",
        "proposal_id": "proposal-1",
        "choice": "for"
    });

    // No auth → 401
    let resp = client
        .post(format!("{}/api/v1/governance/vote", server.base_url))
        .json(&vote_body)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 401);

    // Valid JWT → 400 (auth passed, but voter has no registered stake)
    let resp = client
        .post(format!("{}/api/v1/governance/vote", server.base_url))
        .bearer_auth(&valid_token)
        .json(&vote_body)
        .send()
        .await
        .expect("test assertion failed");
    assert!(
        resp.status() == 400 || resp.status() == 200,
        "Valid JWT should yield 400 (no stake) or 200, not 401; got {}",
        resp.status()
    );

    // Expired JWT → 401
    let resp = client
        .post(format!("{}/api/v1/governance/vote", server.base_url))
        .bearer_auth(&expired_token)
        .json(&vote_body)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 401);

    // Wrong-secret JWT → 401
    let resp = client
        .post(format!("{}/api/v1/governance/vote", server.base_url))
        .bearer_auth(&wrong_secret_token)
        .json(&vote_body)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 401);
}

// ---- GET /api/v1/economics/balance/{did} ----

#[tokio::test]
async fn test_auth_get_balance() {
    let server = setup_server(None).await;
    let client = reqwest::Client::new();
    let valid_token = make_valid_token(REGULAR_CALLER);
    let expired_token = make_expired_token(JWT_SECRET);
    let wrong_secret_token = make_wrong_secret_token();

    // No auth → 401
    let resp = client
        .get(format!("{}/api/v1/economics/balance/did:test:unknown", server.base_url))
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 401);

    // Valid JWT → 404 (DID not registered — auth passed, handler returned 404)
    let resp = client
        .get(format!("{}/api/v1/economics/balance/did:test:unknown", server.base_url))
        .bearer_auth(&valid_token)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(
        resp.status(),
        404,
        "Valid JWT should yield 404 for unregistered DID, not 401"
    );

    // Expired JWT → 401
    let resp = client
        .get(format!("{}/api/v1/economics/balance/did:test:unknown", server.base_url))
        .bearer_auth(&expired_token)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 401);

    // Wrong-secret JWT → 401
    let resp = client
        .get(format!("{}/api/v1/economics/balance/did:test:unknown", server.base_url))
        .bearer_auth(&wrong_secret_token)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 401);
}

// ---- POST /api/v1/economics/transfer ----

#[tokio::test]
async fn test_auth_transfer() {
    let server = setup_server(None).await;
    let client = reqwest::Client::new();
    let valid_token = make_valid_token(REGULAR_CALLER);
    let expired_token = make_expired_token(JWT_SECRET);
    let wrong_secret_token = make_wrong_secret_token();

    let transfer_body = json!({
        "from_did": "did:test:sender",
        "to_did": "did:test:recipient",
        "amount": 100
    });

    // No auth → 401
    let resp = client
        .post(format!("{}/api/v1/economics/transfer", server.base_url))
        .json(&transfer_body)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 401);

    // Valid JWT → 404 (sender not registered — auth passed, handler returned 404)
    let resp = client
        .post(format!("{}/api/v1/economics/transfer", server.base_url))
        .bearer_auth(&valid_token)
        .json(&transfer_body)
        .send()
        .await
        .expect("test assertion failed");
    assert!(
        resp.status() == 404 || resp.status() == 400,
        "Valid JWT should yield 404/400 for unregistered DID, not 401; got {}",
        resp.status()
    );

    // Expired JWT → 401
    let resp = client
        .post(format!("{}/api/v1/economics/transfer", server.base_url))
        .bearer_auth(&expired_token)
        .json(&transfer_body)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 401);

    // Wrong-secret JWT → 401
    let resp = client
        .post(format!("{}/api/v1/economics/transfer", server.base_url))
        .bearer_auth(&wrong_secret_token)
        .json(&transfer_body)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 401);
}

// ===========================================================================
//  2. RATE LIMITING TESTS
// ===========================================================================

#[tokio::test]
async fn test_rate_limit_events_endpoint() {
    // Use a very low rate limit: 1 rps → burst capacity = 2
    let server = setup_server(Some(1)).await;
    let client = reqwest::Client::new();
    let token = make_valid_token(REGULAR_CALLER);

    let event_body = json!({
        "payload": hex::encode(b"rate limit test"),
        "event_type": "test"
    });

    let mut got_429 = false;
    for _ in 0..10 {
        let resp = client
            .post(format!("{}/api/v1/events", server.base_url))
            .bearer_auth(&token)
            .json(&event_body)
            .send()
            .await
            .expect("test assertion failed");

        if resp.status() == 429 {
            got_429 = true;
            // Verify the 429 response body
            let body: Value = resp.json().await.expect("test assertion failed");
            assert!(body["error"].is_string(), "429 response should contain 'error' field");
            let error_msg = body["error"].as_str().expect("test assertion failed");
            assert!(
                error_msg.contains("rate limit"),
                "Error message should mention rate limit, got: {error_msg}"
            );
            break;
        }
    }

    assert!(
        got_429,
        "Should have received at least one 429 Too Many Requests response"
    );
}

// ===========================================================================
//  3. PRIVILEGED OPERATION TESTS
// ===========================================================================

#[tokio::test]
async fn test_mint_ubc_non_admin_forbidden() {
    let server = setup_server(None).await;
    let client = reqwest::Client::new();
    let regular_token = make_valid_token(REGULAR_CALLER);

    let mint_body = json!({
        "operation": "mint",
        "params": {"did": "did:test:mint-recipient", "amount": 1000}
    });

    let resp = client
        .post(format!("{}/api/v1/shards/economics/operations", server.base_url))
        .bearer_auth(&regular_token)
        .json(&mint_body)
        .send()
        .await
        .expect("test assertion failed");

    assert_eq!(
        resp.status(),
        403,
        "Non-admin caller should get 403 Forbidden for MintUbc"
    );
    let body: Value = resp.json().await.expect("test assertion failed");
    assert!(body["error"].is_string(), "403 response should have 'error' field");
    let error_msg = body["error"].as_str().expect("test assertion failed");
    assert!(
        error_msg.contains("not authorized"),
        "Error message should mention authorization, got: {error_msg}"
    );
}

#[tokio::test]
async fn test_mint_ubc_admin_ok() {
    let server = setup_server(None).await;
    let client = reqwest::Client::new();
    let admin_token = make_valid_token(ADMIN_CALLER);

    let mint_body = json!({
        "operation": "mint",
        "params": {"did": "did:test:mint-recipient", "amount": 1000}
    });

    let resp = client
        .post(format!("{}/api/v1/shards/economics/operations", server.base_url))
        .bearer_auth(&admin_token)
        .json(&mint_body)
        .send()
        .await
        .expect("test assertion failed");

    assert_eq!(resp.status(), 200, "Admin caller should get 200 OK for MintUbc");
    let body: Value = resp.json().await.expect("test assertion failed");
    assert_eq!(body["status"].as_str().expect("test assertion failed"), "processed");
    assert_eq!(body["operation"].as_str().expect("test assertion failed"), "mint");
}

#[tokio::test]
async fn test_advance_epoch_non_admin_forbidden() {
    let server = setup_server(None).await;
    let client = reqwest::Client::new();
    let regular_token = make_valid_token(REGULAR_CALLER);

    let advance_body = json!({
        "operation": "advance_epoch",
        "params": {}
    });

    let resp = client
        .post(format!("{}/api/v1/shards/economics/operations", server.base_url))
        .bearer_auth(&regular_token)
        .json(&advance_body)
        .send()
        .await
        .expect("test assertion failed");

    assert_eq!(
        resp.status(),
        403,
        "Non-admin caller should get 403 Forbidden for AdvanceEpoch"
    );
    let body: Value = resp.json().await.expect("test assertion failed");
    assert!(body["error"].is_string(), "403 response should have 'error' field");
    let error_msg = body["error"].as_str().expect("test assertion failed");
    assert!(
        error_msg.contains("not authorized"),
        "Error message should mention authorization, got: {error_msg}"
    );
}

#[tokio::test]
async fn test_advance_epoch_admin_ok() {
    let server = setup_server(None).await;
    let client = reqwest::Client::new();
    let admin_token = make_valid_token(ADMIN_CALLER);

    let advance_body = json!({
        "operation": "advance_epoch",
        "params": {}
    });

    let resp = client
        .post(format!("{}/api/v1/shards/economics/operations", server.base_url))
        .bearer_auth(&admin_token)
        .json(&advance_body)
        .send()
        .await
        .expect("test assertion failed");

    assert_eq!(resp.status(), 200, "Admin caller should get 200 OK for AdvanceEpoch");
    let body: Value = resp.json().await.expect("test assertion failed");
    assert_eq!(body["status"].as_str().expect("test assertion failed"), "processed");
    assert_eq!(
        body["operation"].as_str().expect("test assertion failed"),
        "advance_epoch"
    );
}

// ===========================================================================
//  4. CORS TESTS
// ===========================================================================

#[tokio::test]
async fn test_cors_preflight() {
    let server = setup_server(None).await;
    let client = reqwest::Client::new();

    // Send an OPTIONS preflight request with CORS headers
    let resp = client
        .request(
            reqwest::Method::OPTIONS,
            format!("{}/api/v1/node/info", server.base_url),
        )
        .header("Origin", "http://example.com")
        .header("Access-Control-Request-Method", "GET")
        .header("Access-Control-Request-Headers", "Authorization, Content-Type")
        .send()
        .await
        .expect("test assertion failed");

    // CORS preflight should return 200 (handled by the CORS layer,
    // not forwarded to auth or handler)
    assert_eq!(resp.status(), 200, "OPTIONS preflight should return 200");

    // Verify CORS response headers
    let allow_origin = resp
        .headers()
        .get("access-control-allow-origin")
        .expect("Should have access-control-allow-origin header");
    assert!(
        !allow_origin.is_empty(),
        "access-control-allow-origin should not be empty"
    );

    let allow_methods = resp
        .headers()
        .get("access-control-allow-methods")
        .expect("Should have access-control-allow-methods header");
    let methods_str = allow_methods.to_str().expect("test assertion failed");
    assert!(
        methods_str.contains("GET") && methods_str.contains("POST"),
        "Allowed methods should include GET and POST, got: {methods_str}"
    );

    let allow_headers = resp
        .headers()
        .get("access-control-allow-headers")
        .expect("Should have access-control-allow-headers header");
    let headers_str = allow_headers.to_str().expect("test assertion failed");
    assert!(
        headers_str.contains("authorization") && headers_str.contains("content-type"),
        "Allowed headers should include Authorization and Content-Type, got: {headers_str}"
    );

    let max_age = resp
        .headers()
        .get("access-control-max-age")
        .expect("Should have access-control-max-age header");
    assert_eq!(
        max_age.to_str().expect("test assertion failed"),
        "3600",
        "Max-Age should be 3600"
    );
}

#[tokio::test]
async fn test_cors_cross_origin_request() {
    let server = setup_server(None).await;
    let client = reqwest::Client::new();
    let token = make_valid_token(REGULAR_CALLER);

    // Send a regular GET request with an Origin header
    let resp = client
        .get(format!("{}/api/v1/node/info", server.base_url))
        .header("Origin", "http://example.com")
        .bearer_auth(&token)
        .send()
        .await
        .expect("test assertion failed");

    assert_eq!(resp.status(), 200, "Cross-origin GET with auth should return 200");

    // Verify the response includes CORS headers
    let allow_origin = resp
        .headers()
        .get("access-control-allow-origin")
        .expect("Cross-origin response should have access-control-allow-origin");
    assert!(
        !allow_origin.is_empty(),
        "access-control-allow-origin should not be empty"
    );
}

// ===========================================================================
//  5. ERROR FORMAT TESTS
// ===========================================================================

/// Verify that 401 Unauthorized responses use the consistent
/// `{"error": "message"}` JSON format.
#[tokio::test]
async fn test_error_format_401_unauthorized() {
    let server = setup_server(None).await;
    let client = reqwest::Client::new();

    // Use an authenticated endpoint (events) — public endpoints like node/info
    // always return 200 regardless of auth state.
    let auth_url = format!("{}/api/v1/events", server.base_url);

    // --- Missing auth header ---
    let resp = client.get(&auth_url).send().await.expect("test assertion failed");
    assert_eq!(resp.status(), 401);
    let body: Value = resp.json().await.expect("test assertion failed");
    assert!(
        body["error"].is_string(),
        "401 response should have 'error' string field, got: {body:?}"
    );
    let error_msg = body["error"].as_str().expect("test assertion failed");
    assert!(!error_msg.is_empty(), "Error message should not be empty");
    assert!(
        error_msg.contains("authorization"),
        "Missing-auth error should mention 'authorization', got: {error_msg}"
    );

    // --- Expired token ---
    let expired_token = make_expired_token(JWT_SECRET);
    let resp = client
        .get(&auth_url)
        .bearer_auth(&expired_token)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 401);
    let body: Value = resp.json().await.expect("test assertion failed");
    assert!(
        body["error"].is_string(),
        "401 response should have 'error' string field, got: {body:?}"
    );
    assert!(
        body["error"]
            .as_str()
            .expect("test assertion failed")
            .contains("expired"),
        "Expired-token error should mention 'expired', got: {}",
        body["error"].as_str().expect("test assertion failed")
    );

    // --- Invalid (wrong-secret) token ---
    let wrong_token = make_wrong_secret_token();
    let resp = client
        .get(&auth_url)
        .bearer_auth(&wrong_token)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 401);
    let body: Value = resp.json().await.expect("test assertion failed");
    assert!(
        body["error"].is_string(),
        "401 response should have 'error' string field, got: {body:?}"
    );
    assert!(
        !body["error"].as_str().expect("test assertion failed").is_empty(),
        "Invalid-token error message should not be empty"
    );
}

/// Verify that 403 Forbidden responses use the consistent
/// `{"error": "message"}` JSON format.
#[tokio::test]
async fn test_error_format_403_forbidden() {
    let server = setup_server(None).await;
    let client = reqwest::Client::new();
    let regular_token = make_valid_token(REGULAR_CALLER);

    // Attempt a privileged operation (mint) as a non-admin user
    let mint_body = json!({
        "operation": "mint",
        "params": {"did": "did:test:target", "amount": 500}
    });
    let resp = client
        .post(format!("{}/api/v1/shards/economics/operations", server.base_url))
        .bearer_auth(&regular_token)
        .json(&mint_body)
        .send()
        .await
        .expect("test assertion failed");

    assert_eq!(resp.status(), 403);
    let body: Value = resp.json().await.expect("test assertion failed");
    assert!(
        body["error"].is_string(),
        "403 response should have 'error' string field, got: {body:?}"
    );
    let error_msg = body["error"].as_str().expect("test assertion failed");
    assert!(
        error_msg.contains("not authorized"),
        "403 error should mention 'not authorized', got: {error_msg}"
    );
}

/// Verify that 404 Not Found responses use the consistent
/// `{"error": "message"}` JSON format.
#[tokio::test]
async fn test_error_format_404_not_found() {
    let server = setup_server(None).await;
    let client = reqwest::Client::new();
    let token = make_valid_token(REGULAR_CALLER);

    // Request a nonexistent event
    let resp = client
        .get(format!("{}/api/v1/events/nonexistent-id", server.base_url))
        .bearer_auth(&token)
        .send()
        .await
        .expect("test assertion failed");

    assert_eq!(resp.status(), 404);
    let body: Value = resp.json().await.expect("test assertion failed");
    assert!(
        body["error"].is_string(),
        "404 response should have 'error' string field, got: {body:?}"
    );
    let error_msg = body["error"].as_str().expect("test assertion failed");
    assert!(!error_msg.is_empty(), "404 error message should not be empty");

    // Also test 404 from economics balance (unregistered DID)
    let resp = client
        .get(format!("{}/api/v1/economics/balance/did:test:noone", server.base_url))
        .bearer_auth(&token)
        .send()
        .await
        .expect("test assertion failed");

    assert_eq!(resp.status(), 404);
    let body: Value = resp.json().await.expect("test assertion failed");
    assert!(
        body["error"].is_string(),
        "Economics 404 response should have 'error' string field, got: {body:?}"
    );
}

/// Verify that 429 Too Many Requests responses use the consistent
/// `{"error": "message"}` JSON format.
#[tokio::test]
async fn test_error_format_429_rate_limited() {
    // 1 rps → burst = 2
    let server = setup_server(Some(1)).await;
    let client = reqwest::Client::new();
    let token = make_valid_token(REGULAR_CALLER);

    let mut got_429 = false;
    for _ in 0..10 {
        let resp = client
            .get(format!("{}/api/v1/node/info", server.base_url))
            .bearer_auth(&token)
            .send()
            .await
            .expect("test assertion failed");

        if resp.status() == 429 {
            let body: Value = resp.json().await.expect("test assertion failed");
            assert!(
                body["error"].is_string(),
                "429 response should have 'error' string field, got: {body:?}"
            );
            let error_msg = body["error"].as_str().expect("test assertion failed");
            assert!(
                error_msg.contains("rate limit"),
                "429 error should mention 'rate limit', got: {error_msg}"
            );
            got_429 = true;
            break;
        }
    }

    assert!(
        got_429,
        "Should have received at least one 429 response to verify its format"
    );
}

// ===========================================================================
//  6. HANDLER BUSINESS-LOGIC BRANCH TESTS
//
// The auth tests above verify 401 behavior. These tests verify the
// handler's business-logic branches (200, 400, 404, 409) that the
// auth-only tests miss.
// ===========================================================================

/// Helper: register a DID in the economics state and mint UBC to it.
/// Called BEFORE the server starts, directly on the AppState.economics
/// instance that the handlers read from. (The shard router's EconomicsShard
/// has a separate internal state that is disconnected from AppState.economics.)
fn register_and_mint(econ: &mut EconomicsState, did: &str, amount: u64) {
    econ.quota.register_did(did);
    let _ = econ.quota.reward(did, amount);
}

// ---- governance: create_proposal 409 Conflict (duplicate) ----

#[tokio::test]
async fn test_create_proposal_duplicate_returns_409() {
    let server = setup_server(None).await;
    let client = reqwest::Client::new();
    let token = make_valid_token(REGULAR_CALLER);

    let body = json!({
        "id": "proposal-dup-test",
        "description": "First creation",
        "expires_at_epoch": 100
    });

    // First creation → 201
    let resp = client
        .post(format!("{}/api/v1/governance/proposals", server.base_url))
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 201);

    // Duplicate creation → 409
    let resp = client
        .post(format!("{}/api/v1/governance/proposals", server.base_url))
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 409, "Duplicate proposal should return 409 Conflict");
    let body: Value = resp.json().await.expect("test assertion failed");
    assert!(body["error"]
        .as_str()
        .expect("test assertion failed")
        .contains("Failed to create proposal"));
}

// ---- governance: cast_vote 400 invalid choice ----

#[tokio::test]
async fn test_cast_vote_invalid_choice_returns_400() {
    let server = setup_server(None).await;
    let client = reqwest::Client::new();
    let token = make_valid_token(REGULAR_CALLER);

    let body = json!({
        "did": "did:test:voter",
        "proposal_id": "proposal-1",
        "choice": "yes"  // Invalid — must be for/against/abstain
    });

    let resp = client
        .post(format!("{}/api/v1/governance/vote", server.base_url))
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .expect("test assertion failed");

    // Should get 400 for invalid choice, NOT 401 (auth passed) and NOT
    // the "no stake" 400 (the choice parse happens before the stake check).
    assert_eq!(resp.status(), 400, "Invalid vote choice should return 400");
    let body: Value = resp.json().await.expect("test assertion failed");
    assert!(
        body["error"]
            .as_str()
            .expect("test assertion failed")
            .contains("invalid vote choice"),
        "Error should mention invalid vote choice, got: {body}"
    );
}

// ---- governance: cast_vote with whitespace-trimmed choice ----

#[tokio::test]
async fn test_cast_vote_whitespace_choice_trimmed() {
    let server = setup_server(None).await;
    let client = reqwest::Client::new();
    let token = make_valid_token(REGULAR_CALLER);

    let body = json!({
        "did": "did:test:voter",
        "proposal_id": "proposal-1",
        "choice": " for "  // Whitespace should be trimmed
    });

    let resp = client
        .post(format!("{}/api/v1/governance/vote", server.base_url))
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .expect("test assertion failed");

    // Should NOT get the "invalid vote choice" 400 — trimming worked.
    // It will still get 400 for "no stake", but the error message should
    // NOT mention "invalid vote choice".
    assert_eq!(
        resp.status(),
        400,
        "Should get 400 for no stake, not for invalid choice"
    );
    let body: Value = resp.json().await.expect("test assertion failed");
    let error = body["error"].as_str().expect("test assertion failed");
    assert!(
        !error.contains("invalid vote choice"),
        "Whitespace should be trimmed — choice should parse as 'for'. Got error: {error}"
    );
}

// ---- economics: get_balance 200 (registered DID) ----

#[tokio::test]
async fn test_get_balance_registered_did_returns_200() {
    let server = setup_server_with_economics(|econ| {
        register_and_mint(econ, "did:test:balance-check", 5000);
    })
    .await;

    let client = reqwest::Client::new();
    let token = make_valid_token(REGULAR_CALLER);
    let resp = client
        .get(format!(
            "{}/api/v1/economics/balance/did:test:balance-check",
            server.base_url
        ))
        .bearer_auth(&token)
        .send()
        .await
        .expect("test assertion failed");

    assert_eq!(resp.status(), 200, "Registered DID should return 200");
    let body: Value = resp.json().await.expect("test assertion failed");
    assert_eq!(
        body["did"].as_str().expect("test assertion failed"),
        "did:test:balance-check"
    );
    assert!(
        body["balance"].as_u64().expect("test assertion failed") >= 5000,
        "Balance should reflect minted amount"
    );
    assert!(
        body["is_registered"].as_bool().expect("test assertion failed"),
        "Should be registered"
    );
}

// ---- economics: transfer 400 zero amount ----

#[tokio::test]
async fn test_transfer_zero_amount_returns_400() {
    let server = setup_server(None).await;
    let client = reqwest::Client::new();
    let token = make_valid_token(REGULAR_CALLER);

    let body = json!({
        "from_did": "did:test:sender",
        "to_did": "did:test:recipient",
        "amount": 0
    });

    let resp = client
        .post(format!("{}/api/v1/economics/transfer", server.base_url))
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .expect("test assertion failed");

    assert_eq!(resp.status(), 400, "Zero amount should return 400");
    let body: Value = resp.json().await.expect("test assertion failed");
    assert!(
        body["error"]
            .as_str()
            .expect("test assertion failed")
            .contains("must be greater than zero"),
        "Error should mention zero amount"
    );
}

// ---- economics: transfer 200 success ----

#[tokio::test]
async fn test_transfer_success_returns_200() {
    let server = setup_server_with_economics(|econ| {
        register_and_mint(econ, REGULAR_CALLER, 10_000);
        register_and_mint(econ, "did:test:recipient", 100);
    })
    .await;

    let client = reqwest::Client::new();
    let token = make_valid_token(REGULAR_CALLER);

    let body = json!({
        "from_did": REGULAR_CALLER,
        "to_did": "did:test:recipient",
        "amount": 500
    });

    let resp = client
        .post(format!("{}/api/v1/economics/transfer", server.base_url))
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .expect("test assertion failed");

    assert_eq!(resp.status(), 200, "Valid transfer should return 200");
    let body: Value = resp.json().await.expect("test assertion failed");
    assert_eq!(body["status"].as_str().expect("test assertion failed"), "completed");
    assert_eq!(body["amount"].as_u64().expect("test assertion failed"), 500);
    assert!(
        body["new_balance"].as_u64().expect("test assertion failed") >= 9_500,
        "New balance should reflect the spent amount"
    );
}

// ---- economics: transfer emits an on-chain provenance event ----

#[tokio::test]
async fn test_transfer_emits_provenance_event() {
    let server = setup_server_with_economics(|econ| {
        register_and_mint(econ, REGULAR_CALLER, 10_000);
        register_and_mint(econ, "did:test:recipient", 100);
    })
    .await;

    let client = reqwest::Client::new();
    let token = make_valid_token(REGULAR_CALLER);
    let body = json!({
        "from_did": REGULAR_CALLER,
        "to_did": "did:test:recipient",
        "amount": 250,
    });

    let resp = client
        .post(format!("{}/api/v1/economics/transfer", server.base_url))
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.expect("test assertion failed");

    // Every transfer is now recorded as a signed causal-graph event.
    let event_id = body["event_id"].as_str();
    assert!(
        event_id.is_some() && event_id.expect("test assertion failed").len() == 64,
        "transfer response must carry a 32-byte hex provenance event_id, got {:?}",
        body["event_id"]
    );

    // The transfer listing carries the same event_id.
    let list: Value = client
        .get(format!("{}/api/v1/economics/transfers", server.base_url))
        .bearer_auth(&token)
        .send()
        .await
        .expect("test assertion failed")
        .json()
        .await
        .expect("test assertion failed");
    let recorded = list["transfers"][0]["event_id"].as_str();
    assert_eq!(
        recorded, event_id,
        "listed transfer must link the same provenance event"
    );
}

// ---- economics: wallet-signed (self-sovereign) transfer, Step 2 ----

#[tokio::test]
async fn test_transfer_wallet_signed_end_to_end_with_replay_rejection() {
    use omnia_substrate::crypto::{Signer, SigningKey};
    use rand::RngCore;

    // The wallet's own keypair; its DID is derived from the public key.
    let mut seed = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut seed);
    let sk = SigningKey::from_bytes(&seed);
    let public_key_hex = hex::encode(sk.verifying_key().to_bytes());
    let wallet_did = omnia_node::api::wallet_auth::did_from_public_key(&sk.verifying_key().to_bytes());

    let wallet_did_for_setup = wallet_did.clone();
    let server = setup_server_with_economics(move |econ| {
        register_and_mint(econ, &wallet_did_for_setup, 10_000);
        register_and_mint(econ, "did:test:recipient", 100);
    })
    .await;

    let client = reqwest::Client::new();
    let token = make_valid_token(&wallet_did);

    // 1. Obtain a single-use nonce bound to the wallet's public key.
    let challenge: Value = client
        .post(format!("{}/api/v1/auth/challenge", server.base_url))
        .json(&json!({ "public_key": public_key_hex }))
        .send()
        .await
        .expect("test assertion failed")
        .json()
        .await
        .expect("test assertion failed");
    let nonce = challenge["nonce"].as_str().expect("test assertion failed").to_string();
    assert_eq!(challenge["did"].as_str().expect("test assertion failed"), wallet_did);

    // 2. Sign the canonical transfer message with the wallet key.
    let to_did = "did:test:recipient";
    let amount = 500u64;
    let message = omnia_node::api::wallet_auth::transfer_message(&nonce, &wallet_did, to_did, amount);
    let signature_hex = hex::encode(sk.sign(message.as_bytes()).to_bytes());

    // 3. Submit the wallet-signed transfer.
    let body = json!({
        "from_did": wallet_did,
        "to_did": to_did,
        "amount": amount,
        "authorization": {
            "public_key": public_key_hex,
            "nonce": nonce,
            "signature": signature_hex,
        }
    });
    let resp = client
        .post(format!("{}/api/v1/economics/transfer", server.base_url))
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 200, "wallet-signed transfer should succeed");
    let resp_body: Value = resp.json().await.expect("test assertion failed");
    assert_eq!(
        resp_body["provenance"].as_str().expect("test assertion failed"),
        "wallet_signed"
    );
    // Registration grants a base quota on top of the minted 10_000, so
    // assert the spend relative to that floor (as the v1 test does).
    let balance_after_spend = resp_body["new_balance"].as_u64().expect("test assertion failed");
    assert!(
        balance_after_spend >= 9_500,
        "new balance should reflect the 500 spend, got {balance_after_spend}"
    );

    // 4. Replay the SAME authorization — the nonce is consumed, so the
    //    spend must be rejected and the balance untouched.
    let replay = client
        .post(format!("{}/api/v1/economics/transfer", server.base_url))
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(replay.status(), 401, "replayed authorization must be rejected");

    // 5. The listing shows the provenance.
    let list: Value = client
        .get(format!("{}/api/v1/economics/transfers", server.base_url))
        .bearer_auth(&token)
        .send()
        .await
        .expect("test assertion failed")
        .json()
        .await
        .expect("test assertion failed");
    assert_eq!(
        list["transfers"][0]["provenance"]
            .as_str()
            .expect("test assertion failed"),
        "wallet_signed"
    );
}

#[tokio::test]
async fn test_transfer_wallet_signed_rejects_wrong_key_and_bad_signature() {
    use omnia_substrate::crypto::{Signer, SigningKey};
    use rand::RngCore;

    let mut seed = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut seed);
    let sk = SigningKey::from_bytes(&seed);
    let public_key_hex = hex::encode(sk.verifying_key().to_bytes());
    let wallet_did = omnia_node::api::wallet_auth::did_from_public_key(&sk.verifying_key().to_bytes());

    let wallet_did_for_setup = wallet_did.clone();
    let server = setup_server_with_economics(move |econ| {
        register_and_mint(econ, &wallet_did_for_setup, 10_000);
        register_and_mint(econ, "did:test:recipient", 100);
    })
    .await;
    let client = reqwest::Client::new();

    // Balance before any attack attempts — must be identical afterwards.
    let balance_before: Value = client
        .get(format!("{}/api/v1/economics/balance/{wallet_did}", server.base_url))
        .bearer_auth(make_valid_token(&wallet_did))
        .send()
        .await
        .expect("test assertion failed")
        .json()
        .await
        .expect("test assertion failed");
    let balance_before = balance_before["balance"].as_u64().expect("test assertion failed");

    // (a) A JWT for a DIFFERENT identity than the signing key → 403,
    //     even with a perfectly valid signature and fresh nonce.
    let challenge: Value = client
        .post(format!("{}/api/v1/auth/challenge", server.base_url))
        .json(&json!({ "public_key": public_key_hex }))
        .send()
        .await
        .expect("test assertion failed")
        .json()
        .await
        .expect("test assertion failed");
    let nonce = challenge["nonce"].as_str().expect("test assertion failed").to_string();
    // The message signs the ATTACKER's spend from their own DID — but the
    // JWT belongs to REGULAR_CALLER, whose funds would be spent.
    let message = omnia_node::api::wallet_auth::transfer_message(&nonce, &wallet_did, "did:test:recipient", 500);
    let signature_hex = hex::encode(sk.sign(message.as_bytes()).to_bytes());
    let mismatched = client
        .post(format!("{}/api/v1/economics/transfer", server.base_url))
        .bearer_auth(make_valid_token(REGULAR_CALLER))
        .json(&json!({
            "from_did": REGULAR_CALLER,
            "to_did": "did:test:recipient",
            "amount": 500,
            "authorization": {
                "public_key": public_key_hex,
                "nonce": nonce,
                "signature": signature_hex,
            }
        }))
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(
        mismatched.status(),
        403,
        "authorization key not matching the JWT identity must be rejected"
    );

    // (b) A corrupted signature with a fresh nonce → 401, no spend.
    let challenge: Value = client
        .post(format!("{}/api/v1/auth/challenge", server.base_url))
        .json(&json!({ "public_key": public_key_hex }))
        .send()
        .await
        .expect("test assertion failed")
        .json()
        .await
        .expect("test assertion failed");
    let nonce = challenge["nonce"].as_str().expect("test assertion failed").to_string();
    let message = omnia_node::api::wallet_auth::transfer_message(&nonce, &wallet_did, "did:test:recipient", 500);
    let mut sig = sk.sign(message.as_bytes()).to_bytes();
    sig[0] ^= 0xFF;
    let bad_sig = client
        .post(format!("{}/api/v1/economics/transfer", server.base_url))
        .bearer_auth(make_valid_token(&wallet_did))
        .json(&json!({
            "from_did": wallet_did,
            "to_did": "did:test:recipient",
            "amount": 500,
            "authorization": {
                "public_key": public_key_hex,
                "nonce": nonce,
                "signature": hex::encode(sig),
            }
        }))
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(bad_sig.status(), 401, "corrupted signature must be rejected");

    // Balance is untouched by all rejected attempts.
    let balance_after: Value = client
        .get(format!("{}/api/v1/economics/balance/{wallet_did}", server.base_url))
        .bearer_auth(make_valid_token(&wallet_did))
        .send()
        .await
        .expect("test assertion failed")
        .json()
        .await
        .expect("test assertion failed");
    assert_eq!(
        balance_after["balance"].as_u64().expect("test assertion failed"),
        balance_before,
        "rejected authorization attempts must never move funds"
    );
}

// ---- economics: transfer 400 self-transfer ----

#[tokio::test]
async fn test_transfer_to_self_returns_400() {
    let server = setup_server_with_economics(|econ| {
        register_and_mint(econ, REGULAR_CALLER, 10_000);
    })
    .await;

    let client = reqwest::Client::new();
    let token = make_valid_token(REGULAR_CALLER);

    let body = json!({
        "from_did": REGULAR_CALLER,
        "to_did": REGULAR_CALLER,
        "amount": 500
    });

    let resp = client
        .post(format!("{}/api/v1/economics/transfer", server.base_url))
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .expect("test assertion failed");

    assert_eq!(resp.status(), 400, "Self-transfer should return 400");
    let body: Value = resp.json().await.expect("test assertion failed");
    assert!(
        body["error"]
            .as_str()
            .expect("test assertion failed")
            .contains("soulbound"),
        "Error should explain that UBC is soulbound"
    );

    // The balance must be untouched — nothing was burned.
    let resp = client
        .get(format!(
            "{}/api/v1/economics/balance/{}",
            server.base_url, REGULAR_CALLER
        ))
        .bearer_auth(&token)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.expect("test assertion failed");
    assert!(
        body["balance"].as_u64().expect("test assertion failed") >= 10_000,
        "Rejected self-transfer must not burn any UBC"
    );
}

// ---- economics: transfer 400 insufficient balance ----

#[tokio::test]
async fn test_transfer_insufficient_balance_returns_400() {
    let server = setup_server_with_economics(|econ| {
        register_and_mint(econ, REGULAR_CALLER, 100);
        register_and_mint(econ, "did:test:recipient", 100);
    })
    .await;

    let client = reqwest::Client::new();
    let token = make_valid_token(REGULAR_CALLER);

    let body = json!({
        "from_did": REGULAR_CALLER,
        "to_did": "did:test:recipient",
        "amount": 10_000  // Much more than the 100 balance
    });

    let resp = client
        .post(format!("{}/api/v1/economics/transfer", server.base_url))
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .expect("test assertion failed");

    assert_eq!(resp.status(), 400, "Insufficient balance should return 400");
    let body: Value = resp.json().await.expect("test assertion failed");
    assert!(
        body["error"]
            .as_str()
            .expect("test assertion failed")
            .contains("Transfer failed"),
        "Error should mention transfer failure"
    );
}

// ---- Regression: repeated submissions must chain, not equivocate ----

/// The submit handler used to create every API event via `Event::genesis()`
/// — creator + sequence 0 each time — so the second submission was
/// indistinguishable from a Byzantine fork (same creator + sequence,
/// different event ID) and the consensus layer slashed the node's own
/// validator. All subsequent submissions then failed with `NodeSlashed`.
///
/// After the chaining fix each submission extends the previous event
/// (sequence + 1, self-parent link), so any number of submissions succeeds.
#[tokio::test]
async fn test_repeated_submissions_chain_and_do_not_self_slash() {
    let server = setup_server(None).await;
    let client = reqwest::Client::new();
    let token = make_valid_token(REGULAR_CALLER);

    for i in 0..5 {
        let body = json!({
            "payload": hex::encode(format!("event {i}").as_bytes()),
            "event_type": "test"
        });
        let resp = client
            .post(format!("{}/api/v1/events", server.base_url))
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .expect("test assertion failed");
        assert_eq!(
            resp.status(),
            201,
            "submission {i} must succeed — before the chaining fix the second \
             submission self-slashed the validator and every later one failed"
        );
    }
}

// ===========================================================================
//  Wallet challenge/signature authentication
// ===========================================================================

/// End-to-end: an on-device Ed25519 keypair completes challenge → sign →
/// login, and the issued JWT works against an authenticated economics
/// endpoint for the DID derived from the key.
#[tokio::test]
async fn test_wallet_challenge_login_flow() {
    use omnia_node::api::wallet_auth::did_from_public_key;
    use omnia_substrate::crypto::{Signer, SigningKey};

    let server = setup_server(None).await;
    let client = reqwest::Client::new();

    // Generate a fresh on-device keypair.
    let signing_key = SigningKey::generate(&mut rand::rngs::OsRng);
    let pubkey_bytes = signing_key.verifying_key().to_bytes();
    let pubkey_hex = hex::encode(pubkey_bytes);
    let expected_did = did_from_public_key(&pubkey_bytes);

    // Step 1: request a challenge.
    let chal: Value = client
        .post(format!("{}/api/v1/auth/challenge", server.base_url))
        .json(&json!({ "public_key": pubkey_hex }))
        .send()
        .await
        .expect("test assertion failed")
        .json()
        .await
        .expect("test assertion failed");

    let nonce = chal["nonce"].as_str().expect("test assertion failed").to_string();
    assert_eq!(
        chal["did"].as_str().expect("test assertion failed"),
        expected_did,
        "server DID must match client-derived DID"
    );
    assert_eq!(
        chal["message"].as_str().expect("test assertion failed"),
        format!("omnia-auth:{nonce}")
    );

    // Step 2: sign the challenge message and log in.
    let message = format!("omnia-auth:{nonce}");
    let signature = signing_key.sign(message.as_bytes());
    let sig_hex = hex::encode(signature.to_bytes());

    let login_resp = client
        .post(format!("{}/api/v1/auth/login", server.base_url))
        .json(&json!({ "public_key": pubkey_hex, "signature": sig_hex, "nonce": nonce }))
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(login_resp.status(), 200, "valid login should return 200");
    let login: Value = login_resp.json().await.expect("test assertion failed");
    let token = login["token"].as_str().expect("test assertion failed").to_string();
    assert_eq!(login["did"].as_str().expect("test assertion failed"), expected_did);

    // Step 3: use the JWT against an authenticated endpoint for this DID.
    let bal_resp = client
        .get(format!("{}/api/v1/economics/balance/{}", server.base_url, expected_did))
        .bearer_auth(&token)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(bal_resp.status(), 200, "balance lookup with wallet JWT should succeed");
    let bal: Value = bal_resp.json().await.expect("test assertion failed");
    assert_eq!(bal["did"].as_str().expect("test assertion failed"), expected_did);
    assert!(
        bal["is_registered"].as_bool().expect("test assertion failed"),
        "login should have registered the DID"
    );
}

/// `POST /auth/register` registers the authenticated caller's DID —
/// the path used by wallets whose JWT was minted outside the node
/// (e.g. Supabase-account sign-in), where no challenge/login ever ran.
#[tokio::test]
async fn test_auth_register_registers_external_did() {
    let server = setup_server(None).await;
    let client = reqwest::Client::new();

    // A DID the node has never seen, with a JWT minted directly (as the
    // Supabase edge function does — same secret, sub = did).
    let did = "did:omnia:9f1e2d3c";
    let token = make_valid_token(did);

    // Before registration, balance is a 404.
    let before = client
        .get(format!("{}/api/v1/economics/balance/{}", server.base_url, did))
        .bearer_auth(&token)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(before.status(), 404, "unregistered DID should 404");

    // Register.
    let reg = client
        .post(format!("{}/api/v1/auth/register", server.base_url))
        .bearer_auth(&token)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(reg.status(), 200);
    let reg_body: Value = reg.json().await.expect("test assertion failed");
    assert_eq!(reg_body["did"].as_str().expect("test assertion failed"), did);
    assert!(reg_body["newly_registered"].as_bool().expect("test assertion failed"));
    assert!(reg_body["is_registered"].as_bool().expect("test assertion failed"));

    // Idempotent: second call succeeds and reports it already existed.
    let again = client
        .post(format!("{}/api/v1/auth/register", server.base_url))
        .bearer_auth(&token)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(again.status(), 200);
    let again_body: Value = again.json().await.expect("test assertion failed");
    assert!(!again_body["newly_registered"].as_bool().expect("test assertion failed"));

    // Balance now resolves.
    let after = client
        .get(format!("{}/api/v1/economics/balance/{}", server.base_url, did))
        .bearer_auth(&token)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(after.status(), 200, "registered DID should have a balance");

    // No JWT -> 401.
    let anon = client
        .post(format!("{}/api/v1/auth/register", server.base_url))
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(anon.status(), 401, "register requires a JWT");
}

/// A reused nonce must be rejected (single-use replay protection).
#[tokio::test]
async fn test_wallet_login_nonce_is_single_use() {
    use omnia_substrate::crypto::{Signer, SigningKey};

    let server = setup_server(None).await;
    let client = reqwest::Client::new();

    let signing_key = SigningKey::generate(&mut rand::rngs::OsRng);
    let pubkey_hex = hex::encode(signing_key.verifying_key().to_bytes());

    let chal: Value = client
        .post(format!("{}/api/v1/auth/challenge", server.base_url))
        .json(&json!({ "public_key": pubkey_hex }))
        .send()
        .await
        .expect("test assertion failed")
        .json()
        .await
        .expect("test assertion failed");
    let nonce = chal["nonce"].as_str().expect("test assertion failed").to_string();
    let sig_hex = hex::encode(signing_key.sign(format!("omnia-auth:{nonce}").as_bytes()).to_bytes());
    let payload = json!({ "public_key": pubkey_hex, "signature": sig_hex, "nonce": nonce });

    // First login consumes the nonce.
    let first = client
        .post(format!("{}/api/v1/auth/login", server.base_url))
        .json(&payload)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(first.status(), 200);

    // Replay with the same nonce must fail.
    let second = client
        .post(format!("{}/api/v1/auth/login", server.base_url))
        .json(&payload)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(second.status(), 401, "reused nonce must be rejected");
}

/// A signature that does not match the challenge must be rejected.
#[tokio::test]
async fn test_wallet_login_bad_signature_rejected() {
    use omnia_substrate::crypto::{Signer, SigningKey};

    let server = setup_server(None).await;
    let client = reqwest::Client::new();

    let signing_key = SigningKey::generate(&mut rand::rngs::OsRng);
    let pubkey_hex = hex::encode(signing_key.verifying_key().to_bytes());

    let chal: Value = client
        .post(format!("{}/api/v1/auth/challenge", server.base_url))
        .json(&json!({ "public_key": pubkey_hex }))
        .send()
        .await
        .expect("test assertion failed")
        .json()
        .await
        .expect("test assertion failed");
    let nonce = chal["nonce"].as_str().expect("test assertion failed").to_string();

    // Sign the WRONG message.
    let sig_hex = hex::encode(signing_key.sign(b"omnia-auth:not-the-nonce").to_bytes());

    let resp = client
        .post(format!("{}/api/v1/auth/login", server.base_url))
        .json(&json!({ "public_key": pubkey_hex, "signature": sig_hex, "nonce": nonce }))
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 401, "signature over wrong message must be rejected");
}

// ---------------------------------------------------------------------------
// Financial shard — transferable-asset transfers
// ---------------------------------------------------------------------------
//
// These are the counterpart to the UBC tests above. UBC is soulbound: a
// "transfer" spends the sender's quota and credits nobody. The financial
// endpoints move value between two accounts and conserve total supply,
// so the assertions here always check BOTH sides of the transfer.

/// Build the wallet-side authorization for a financial transfer.
///
/// Mirrors what a wallet does on-device: sign the canonical message with
/// the account's own key. Kept as an independent implementation of the
/// encoding rather than calling the shard helper, so a change to the wire
/// format has to be made deliberately in both places instead of silently
/// agreeing with itself.
fn sign_financial_transfer(
    signing_key: &omnia_substrate::crypto::SigningKey,
    to: &[u8; 32],
    amount: u64,
    nonce: u64,
) -> String {
    use omnia_substrate::crypto::Signer;
    let from: [u8; 32] = signing_key.verifying_key().to_bytes();
    let mut msg = b"omnia-financial-transfer:v1".to_vec();
    msg.extend_from_slice(&from);
    msg.extend_from_slice(to);
    msg.extend_from_slice(&amount.to_le_bytes());
    msg.extend_from_slice(&nonce.to_le_bytes());
    hex::encode(signing_key.sign(&msg).to_bytes())
}

/// Derive the DID the node will expect for a given account key.
fn did_for(pubkey: &[u8; 32]) -> String {
    omnia_node::api::wallet_auth::did_from_public_key(pubkey)
}

/// The full loop: fund an account, sign a transfer on the "wallet" side,
/// submit it, and confirm the recipient was actually credited.
///
/// This is the behaviour UBC structurally cannot provide, and the reason
/// the financial endpoints exist.
#[tokio::test]
async fn financial_transfer_credits_the_recipient() {
    let alice = omnia_substrate::crypto::SigningKey::from_bytes(&[3u8; 32]);
    let alice_pk: [u8; 32] = alice.verifying_key().to_bytes();
    let bob = omnia_substrate::crypto::SigningKey::from_bytes(&[5u8; 32]);
    let bob_pk: [u8; 32] = bob.verifying_key().to_bytes();

    let server = setup_server_with_financial(|fin| {
        fin.balances
            .insert(alice_pk, omnia_shards::FinancialAccountBalance::with_balance(1_000));
        fin.total_supply = 1_000;
    })
    .await;

    let client = reqwest::Client::new();
    let token = make_valid_token(&did_for(&alice_pk));

    // The wallet asks which nonce to use.
    let balance: Value = client
        .get(format!(
            "{}/api/v1/financial/balance/{}",
            server.base_url,
            hex::encode(alice_pk)
        ))
        .bearer_auth(&token)
        .send()
        .await
        .expect("test assertion failed")
        .json()
        .await
        .expect("test assertion failed");
    assert_eq!(balance["balance"], 1_000);
    assert_eq!(balance["next_nonce"], 1, "an account that never sent starts at nonce 1");

    let resp = client
        .post(format!("{}/api/v1/financial/transfer", server.base_url))
        .bearer_auth(&token)
        .json(&json!({
            "from": hex::encode(alice_pk),
            "to": hex::encode(bob_pk),
            "amount": 250,
            "nonce": 1,
            "signature": sign_financial_transfer(&alice, &bob_pk, 250, 1),
        }))
        .send()
        .await
        .expect("test assertion failed");

    assert_eq!(resp.status(), 200, "a correctly signed transfer should be applied");
    let body: Value = resp.json().await.expect("test assertion failed");
    assert_eq!(body["sender_balance"], 750);
    assert_eq!(
        body["recipient_balance"], 250,
        "the recipient must be credited — this is the whole point"
    );
    assert!(
        body["event_id"].as_str().is_some_and(|s| !s.is_empty()),
        "the transfer must be recorded on the causal graph"
    );

    // Confirm through a fresh read, not just the write's response body.
    let bob_balance: Value = client
        .get(format!(
            "{}/api/v1/financial/balance/{}",
            server.base_url,
            hex::encode(bob_pk)
        ))
        .bearer_auth(&token)
        .send()
        .await
        .expect("test assertion failed")
        .json()
        .await
        .expect("test assertion failed");
    assert_eq!(bob_balance["balance"], 250);
    assert_eq!(
        bob_balance["total_supply"], 1_000,
        "a transfer moves value; it must not mint or burn"
    );
}

/// A caller cannot spend an account they do not hold the key for, even
/// with a perfectly valid JWT for their own identity.
#[tokio::test]
async fn financial_transfer_rejects_spending_another_account() {
    let victim = omnia_substrate::crypto::SigningKey::from_bytes(&[7u8; 32]);
    let victim_pk: [u8; 32] = victim.verifying_key().to_bytes();
    let attacker = omnia_substrate::crypto::SigningKey::from_bytes(&[11u8; 32]);
    let attacker_pk: [u8; 32] = attacker.verifying_key().to_bytes();

    let server = setup_server_with_financial(|fin| {
        fin.balances
            .insert(victim_pk, omnia_shards::FinancialAccountBalance::with_balance(1_000));
        fin.total_supply = 1_000;
    })
    .await;

    let client = reqwest::Client::new();
    // Attacker authenticates as themselves — a legitimate session.
    let token = make_valid_token(&did_for(&attacker_pk));

    let resp = client
        .post(format!("{}/api/v1/financial/transfer", server.base_url))
        .bearer_auth(&token)
        .json(&json!({
            "from": hex::encode(victim_pk),
            "to": hex::encode(attacker_pk),
            "amount": 1_000,
            // Signed by the attacker's key, claiming the victim as sender.
            "signature": sign_financial_transfer(&attacker, &attacker_pk, 1_000, 1),
            "nonce": 1,
        }))
        .send()
        .await
        .expect("test assertion failed");

    assert_eq!(
        resp.status(),
        403,
        "a caller must not be able to move an account they do not control"
    );

    let balance: Value = client
        .get(format!(
            "{}/api/v1/financial/balance/{}",
            server.base_url,
            hex::encode(victim_pk)
        ))
        .bearer_auth(&token)
        .send()
        .await
        .expect("test assertion failed")
        .json()
        .await
        .expect("test assertion failed");
    assert_eq!(balance["balance"], 1_000, "the victim must be untouched");
}

/// Replaying a previously accepted transfer must not move value twice.
#[tokio::test]
async fn financial_transfer_rejects_replay() {
    let alice = omnia_substrate::crypto::SigningKey::from_bytes(&[13u8; 32]);
    let alice_pk: [u8; 32] = alice.verifying_key().to_bytes();
    let bob = omnia_substrate::crypto::SigningKey::from_bytes(&[17u8; 32]);
    let bob_pk: [u8; 32] = bob.verifying_key().to_bytes();

    let server = setup_server_with_financial(|fin| {
        fin.balances
            .insert(alice_pk, omnia_shards::FinancialAccountBalance::with_balance(500));
        fin.total_supply = 500;
    })
    .await;

    let client = reqwest::Client::new();
    let token = make_valid_token(&did_for(&alice_pk));
    let body = json!({
        "from": hex::encode(alice_pk),
        "to": hex::encode(bob_pk),
        "amount": 100,
        "nonce": 1,
        "signature": sign_financial_transfer(&alice, &bob_pk, 100, 1),
    });

    let first = client
        .post(format!("{}/api/v1/financial/transfer", server.base_url))
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(first.status(), 200);

    // Byte-identical resubmission.
    let replay = client
        .post(format!("{}/api/v1/financial/transfer", server.base_url))
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(replay.status(), 400, "a replayed transfer must be rejected");

    let balance: Value = client
        .get(format!(
            "{}/api/v1/financial/balance/{}",
            server.base_url,
            hex::encode(bob_pk)
        ))
        .bearer_auth(&token)
        .send()
        .await
        .expect("test assertion failed")
        .json()
        .await
        .expect("test assertion failed");
    assert_eq!(balance["balance"], 100, "the recipient must be credited exactly once");
}

/// A transfer larger than the balance is refused and changes nothing.
#[tokio::test]
async fn financial_transfer_rejects_overspend() {
    let alice = omnia_substrate::crypto::SigningKey::from_bytes(&[19u8; 32]);
    let alice_pk: [u8; 32] = alice.verifying_key().to_bytes();
    let bob = omnia_substrate::crypto::SigningKey::from_bytes(&[23u8; 32]);
    let bob_pk: [u8; 32] = bob.verifying_key().to_bytes();

    let server = setup_server_with_financial(|fin| {
        fin.balances
            .insert(alice_pk, omnia_shards::FinancialAccountBalance::with_balance(50));
        fin.total_supply = 50;
    })
    .await;

    let client = reqwest::Client::new();
    let token = make_valid_token(&did_for(&alice_pk));

    let resp = client
        .post(format!("{}/api/v1/financial/transfer", server.base_url))
        .bearer_auth(&token)
        .json(&json!({
            "from": hex::encode(alice_pk),
            "to": hex::encode(bob_pk),
            "amount": 5_000,
            "nonce": 1,
            "signature": sign_financial_transfer(&alice, &bob_pk, 5_000, 1),
        }))
        .send()
        .await
        .expect("test assertion failed");
    assert_eq!(resp.status(), 400, "an unaffordable transfer must be rejected");

    let balance: Value = client
        .get(format!(
            "{}/api/v1/financial/balance/{}",
            server.base_url,
            hex::encode(alice_pk)
        ))
        .bearer_auth(&token)
        .send()
        .await
        .expect("test assertion failed")
        .json()
        .await
        .expect("test assertion failed");
    assert_eq!(balance["balance"], 50, "a rejected transfer must not debit the sender");
    assert_eq!(
        balance["next_nonce"], 1,
        "a rejected transfer must not consume the nonce"
    );
}

/// Tampering with the amount after signing invalidates the authorization.
#[tokio::test]
async fn financial_transfer_rejects_amount_tampering() {
    let alice = omnia_substrate::crypto::SigningKey::from_bytes(&[29u8; 32]);
    let alice_pk: [u8; 32] = alice.verifying_key().to_bytes();
    let bob = omnia_substrate::crypto::SigningKey::from_bytes(&[31u8; 32]);
    let bob_pk: [u8; 32] = bob.verifying_key().to_bytes();

    let server = setup_server_with_financial(|fin| {
        fin.balances
            .insert(alice_pk, omnia_shards::FinancialAccountBalance::with_balance(1_000));
        fin.total_supply = 1_000;
    })
    .await;

    let client = reqwest::Client::new();
    let token = make_valid_token(&did_for(&alice_pk));

    let resp = client
        .post(format!("{}/api/v1/financial/transfer", server.base_url))
        .bearer_auth(&token)
        .json(&json!({
            "from": hex::encode(alice_pk),
            "to": hex::encode(bob_pk),
            // Signed for 10, submitted as 900.
            "amount": 900,
            "nonce": 1,
            "signature": sign_financial_transfer(&alice, &bob_pk, 10, 1),
        }))
        .send()
        .await
        .expect("test assertion failed");

    assert_eq!(
        resp.status(),
        401,
        "an amount not covered by the signature must be rejected"
    );

    let balance: Value = client
        .get(format!(
            "{}/api/v1/financial/balance/{}",
            server.base_url,
            hex::encode(alice_pk)
        ))
        .bearer_auth(&token)
        .send()
        .await
        .expect("test assertion failed")
        .json()
        .await
        .expect("test assertion failed");
    assert_eq!(balance["balance"], 1_000);
}

/// The wallet's exact wire payload, accepted by the live HTTP endpoint.
///
/// The two implementations of the transfer authorization — Dart in the
/// wallet, Rust in the shard — agree only if their bytes agree. The
/// wallet's `test/financial_transfer_test.dart` asserts that the Dart
/// signer produces exactly the hex below for this key, recipient, amount
/// and nonce. This test feeds those same literals through the real HTTP
/// handler, so together the two pin the whole path: what the wallet signs
/// is what the node accepts.
///
/// If either side's encoding drifts, one of the two tests fails here
/// rather than in production as an unexplained rejected payment.
#[tokio::test]
async fn financial_transfer_accepts_the_wallets_exact_payload() {
    // Seed [3u8; 32] — the wallet derives this same public key from it.
    const WALLET_PUBKEY: &str = "ed4928c628d1c2c6eae90338905995612959273a5c63f93636c14614ac8737d1";
    // Recipient [5u8; 32].
    const RECIPIENT: &str = "0505050505050505050505050505050505050505050505050505050505050505";
    // Produced by the Dart wallet for (WALLET_PUBKEY -> RECIPIENT, 250, 7).
    const WALLET_SIGNATURE: &str = concat!(
        "52fa8d6cb50440b776dbf6d65a6ed1fb589ae07505804248e437f814290812b8",
        "6b8f9203c047c004e291dd27a5669a863ed4e51e6399bf03a5e8c79efd76cd05",
    );

    let wallet_pk: [u8; 32] = hex::decode(WALLET_PUBKEY)
        .expect("valid hex")
        .try_into()
        .expect("32 bytes");

    let server = setup_server_with_financial(|fin| {
        fin.balances
            .insert(wallet_pk, omnia_shards::FinancialAccountBalance::with_balance(1_000));
        fin.total_supply = 1_000;
    })
    .await;

    let client = reqwest::Client::new();
    let token = make_valid_token(&did_for(&wallet_pk));

    let resp = client
        .post(format!("{}/api/v1/financial/transfer", server.base_url))
        .bearer_auth(&token)
        .json(&json!({
            "from": WALLET_PUBKEY,
            "to": RECIPIENT,
            "amount": 250,
            "nonce": 7,
            "signature": WALLET_SIGNATURE,
        }))
        .send()
        .await
        .expect("test assertion failed");

    assert_eq!(
        resp.status(),
        200,
        "the node must accept the exact payload the wallet produces"
    );
    let body: Value = resp.json().await.expect("test assertion failed");
    assert_eq!(body["sender_balance"], 750);
    assert_eq!(body["recipient_balance"], 250);
    assert_eq!(body["authorization"], "wallet_signed");
}

// ===========================================================================
//  6. SETTLEMENT SUBMISSION — submit-root auth + logic tests
// ===========================================================================

/// A settlement adapter that reports `is_live() == true` so the
/// submit-root handler proceeds past the adapter-liveness gate.
struct LiveTestSettlementAdapter;

#[async_trait::async_trait]
impl omnia_adapters::settlement::SettlementAdapter for LiveTestSettlementAdapter {
    async fn submit_root(
        &self,
        root: [u8; 32],
    ) -> Result<omnia_adapters::settlement::TxHash, omnia_adapters::settlement::SettlementError> {
        // Echo the root as the tx hash so tests can verify which root
        // was actually passed to the adapter.
        Ok(omnia_adapters::settlement::TxHash(root))
    }
    async fn fetch_finality(
        &self,
        tx: omnia_adapters::settlement::TxHash,
    ) -> Result<omnia_adapters::settlement::FinalityProof, omnia_adapters::settlement::SettlementError> {
        Ok(omnia_adapters::settlement::FinalityProof {
            tx_hash: tx,
            block_number: 0,
            confirmation_count: 1,
            proof_hash: [0u8; 32],
        })
    }
    async fn verify_inclusion(
        &self,
        _leaf: &[u8; 32],
        _proof: &omnia_adapters::merkle::MerkleProof,
    ) -> Result<bool, omnia_adapters::settlement::SettlementError> {
        Ok(true)
    }
    fn is_live(&self) -> bool {
        true
    }
}

/// Build [`AppState`] with a custom settlement adapter (instead of the
/// default mock which always reports `is_live() == false`).
fn build_test_app_state_with_settlement(
    port: u16,
    settlement: Arc<dyn omnia_adapters::settlement::SettlementAdapter>,
) -> AppState {
    let mut state = build_test_app_state(port);
    state.settlement = settlement;
    state
}

/// Spawn a test server that uses [`LiveTestSettlementAdapter`] so the
/// submit-root handler can reach the root-determination logic.
#[allow(clippy::await_holding_lock)]
async fn setup_live_settlement_server() -> TestServer {
    let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    configure_test_server_env(Some(ADMIN_CALLER), None);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind to random port");
    let port = listener.local_addr().expect("test assertion failed").port();

    let app_state = build_test_app_state_with_settlement(port, Arc::new(LiveTestSettlementAdapter));
    let app = http::build_http_router().with_state(app_state);

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("Test server error");
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let env_guard = EnvGuard {
        keys: vec![
            "OMNIA_JWT_SECRET",
            "OMNIA_AUTHORIZED_CALLERS",
            "OMNIA_JWT_ALLOW_LEGACY_HS256",
            "OMNIA_RATE_LIMIT_RPS",
        ],
    };

    TestServer {
        base_url: format!("http://127.0.0.1:{port}"),
        _handle: handle,
        _env_guard: env_guard,
        _lock: lock,
    }
}

/// Spawn a test server with a live settlement adapter and a known Lane 0
/// root injected into the Substrate.  Returns the server and the 32-byte
/// root that was injected (hex-encoded with `0x` prefix).
#[allow(clippy::await_holding_lock)]
async fn setup_live_settlement_server_with_root() -> (TestServer, String) {
    let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    configure_test_server_env(Some(ADMIN_CALLER), None);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind to random port");
    let port = listener.local_addr().expect("test assertion failed").port();

    let known_root = [0xAB_u8; 32];
    let root_hex = format!("0x{}", hex::encode(known_root));

    let app_state = build_test_app_state_with_settlement(port, Arc::new(LiveTestSettlementAdapter));
    app_state.substrate.write().await.test_inject_lane0_root(known_root);

    let app = http::build_http_router().with_state(app_state);

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("Test server error");
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let env_guard = EnvGuard {
        keys: vec![
            "OMNIA_JWT_SECRET",
            "OMNIA_AUTHORIZED_CALLERS",
            "OMNIA_JWT_ALLOW_LEGACY_HS256",
            "OMNIA_RATE_LIMIT_RPS",
            "OMNIA_SETTLEMENT_ALLOW_CUSTOM_ROOT",
        ],
    };

    let server = TestServer {
        base_url: format!("http://127.0.0.1:{port}"),
        _handle: handle,
        _env_guard: env_guard,
        _lock: lock,
    };

    (server, root_hex)
}

// ---- No JWT → 401 (middleware rejects before handler) ----

#[tokio::test]
async fn test_submit_root_no_jwt_rejected() {
    let server = setup_server(None).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/api/v1/admin/settlement/submit-root", server.base_url))
        .send()
        .await
        .expect("test assertion failed");

    assert_eq!(resp.status(), 401, "submit-root must reject requests without a JWT");
    let body: Value = resp.json().await.expect("test assertion failed");
    assert!(body["error"].is_string(), "401 response should have an 'error' field");
}

// ---- Valid JWT but not in AuthorizedCallers → 403 ----

#[tokio::test]
async fn test_submit_root_non_admin_forbidden() {
    let server = setup_server(None).await;
    let client = reqwest::Client::new();
    let regular_token = make_valid_token(REGULAR_CALLER);

    let resp = client
        .post(format!("{}/api/v1/admin/settlement/submit-root", server.base_url))
        .bearer_auth(&regular_token)
        .send()
        .await
        .expect("test assertion failed");

    assert_eq!(resp.status(), 403, "non-admin caller must get 403 for submit-root");
    let body: Value = resp.json().await.expect("test assertion failed");
    assert!(body["error"].is_string());
    let error_msg = body["error"].as_str().expect("test assertion failed");
    assert!(
        error_msg.contains("not authorized"),
        "error should mention authorization, got: {error_msg}"
    );
}

// ---- Adapter not live → 503 (MockSettlementAdapter.is_live() == false) ----

#[tokio::test]
async fn test_submit_root_adapter_not_live() {
    let server = setup_server(None).await;
    let client = reqwest::Client::new();
    let admin_token = make_valid_token(ADMIN_CALLER);

    let resp = client
        .post(format!("{}/api/v1/admin/settlement/submit-root", server.base_url))
        .bearer_auth(&admin_token)
        .send()
        .await
        .expect("test assertion failed");

    assert_eq!(resp.status(), 503, "mock adapter must cause 503 Service Unavailable");
    let body: Value = resp.json().await.expect("test assertion failed");
    assert!(body["error"].is_string());
    assert!(
        body["error"]
            .as_str()
            .expect("test assertion failed")
            .contains("not live"),
        "error should mention adapter not live"
    );
}

// ---- No Lane 0 root → 404 (live adapter but nothing finalized yet) ----

#[tokio::test]
async fn test_submit_root_no_lane0_root() {
    let server = setup_live_settlement_server().await;
    let client = reqwest::Client::new();
    let admin_token = make_valid_token(ADMIN_CALLER);

    let resp = client
        .post(format!("{}/api/v1/admin/settlement/submit-root", server.base_url))
        .bearer_auth(&admin_token)
        .send()
        .await
        .expect("test assertion failed");

    assert_eq!(resp.status(), 404, "must return 404 when Lane 0 has no leading root");
    let body: Value = resp.json().await.expect("test assertion failed");
    assert!(body["error"].is_string());
    assert!(
        body["error"]
            .as_str()
            .expect("test assertion failed")
            .contains("no Lane 0 leading root"),
        "error should mention no root available"
    );
}

// ---- Caller-supplied root silently ignored; response echoes state root ----

#[tokio::test]
async fn test_submit_root_custom_root_ignored_when_flag_unset() {
    let (server, state_root_hex) = setup_live_settlement_server_with_root().await;
    let client = reqwest::Client::new();
    let admin_token = make_valid_token(ADMIN_CALLER);

    // Ensure the debug flag is NOT set (default behaviour).
    std::env::remove_var("OMNIA_SETTLEMENT_ALLOW_CUSTOM_ROOT");

    // Send a body with a DIFFERENT root than what consensus state holds.
    let attacker_root = "0x".to_string() + &"de".repeat(32);
    let resp = client
        .post(format!("{}/api/v1/admin/settlement/submit-root", server.base_url))
        .bearer_auth(&admin_token)
        .json(&json!({ "root": attacker_root }))
        .send()
        .await
        .expect("test assertion failed");

    assert_eq!(resp.status(), 200, "should succeed with state-sourced root");
    let body: Value = resp.json().await.expect("test assertion failed");

    // The response must echo the STATE root, not the caller's root.
    assert_eq!(
        body["root"].as_str().expect("test assertion failed"),
        &state_root_hex,
        "response root must match the consensus state root, not the caller-supplied value"
    );
    // The tx hash should also be based on the state root since our
    // LiveTestSettlementAdapter echoes root bytes as the tx hash.
    assert_eq!(
        body["tx_hash"].as_str().expect("test assertion failed"),
        &state_root_hex,
        "tx hash must correspond to the state root"
    );
}

// ---- Debug flag set → caller-supplied root is honored ----

#[tokio::test]
async fn test_submit_root_custom_root_honored_when_flag_set() {
    let (server, _state_root_hex) = setup_live_settlement_server_with_root().await;
    let client = reqwest::Client::new();
    let admin_token = make_valid_token(ADMIN_CALLER);

    // Enable the debug override.
    std::env::set_var("OMNIA_SETTLEMENT_ALLOW_CUSTOM_ROOT", "true");

    let custom_root = "0x".to_string() + &"cc".repeat(32);
    let resp = client
        .post(format!("{}/api/v1/admin/settlement/submit-root", server.base_url))
        .bearer_auth(&admin_token)
        .json(&json!({ "root": &custom_root }))
        .send()
        .await
        .expect("test assertion failed");

    assert_eq!(resp.status(), 200, "should succeed with custom root when flag is set");
    let body: Value = resp.json().await.expect("test assertion failed");

    assert_eq!(
        body["root"].as_str().expect("test assertion failed"),
        &custom_root,
        "response root must match the caller-supplied root when the debug flag is set"
    );
}
