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
use omnia_node::api::auth::{create_token, Claims};
use omnia_node::config::NodeConfig;
use omnia_node::http;
use omnia_node::state::{AppState, NodeMetrics};
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
static ENV_LOCK: std::sync::LazyLock<tokio::sync::Mutex<()>> = std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

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

/// Create a valid JWT for `caller_id`, valid for 1 hour.
///
/// Requires `OMNIA_JWT_SECRET` to be set in the environment.
fn make_valid_token(caller_id: &str) -> String {
    create_token(caller_id, 3600).expect("Failed to create valid token")
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

    let substrate_config = SubstrateConfig::new(node_id_bytes);
    let substrate = Substrate::new(substrate_config);
    let slashing = substrate.slashing.clone();

    let fee_schedule = FeeSchedule::standard();
    let quota = omnia_economics::QuotaSystem::default_system();
    let mut shard_router = ShardRouter::new(fee_schedule, quota);
    shard_router.register(Box::new(FinancialShard::new()));
    shard_router.register(Box::new(ComputationalShard::new()));
    shard_router.register(Box::new(PhysicalShard::new()));
    shard_router.register(Box::new(BiologicalShard::new()));
    shard_router.register(Box::new(IdentityShard::new()));
    shard_router.register(Box::new(EconomicsShard::new()));

    let economics = EconomicsState::new();
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
        readiness_min_peers: 1,
        readiness_max_finalization_age: 600,
    };

    AppState {
        config,
        substrate: Arc::new(RwLock::new(substrate)),
        slashing: Arc::new(Mutex::new(slashing)),
        shard_router: Arc::new(std::sync::Mutex::new(shard_router)),
        economics: Arc::new(Mutex::new(economics)),
        event_store: Arc::new(RwLock::new(indexmap::IndexMap::new())),
        peers: Arc::new(RwLock::new(Vec::new())),
        metrics: Arc::new(metrics),
        started_at: Instant::now(),
        is_syncing: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        keypair: Some(omnia_substrate::crypto::generate_keypair()),
        settlement: Arc::new(omnia_adapters::MockSettlementAdapter::new()),
        #[cfg(feature = "zk")]
        ceremony_server: None,
    }
}

/// Start a test HTTP server on a random port, with optional pre-registration
/// of DIDs in the economics state.
///
/// `pre_register_dids` is a closure that receives the `EconomicsState` before
/// the server starts, allowing tests to register DIDs and mint UBC directly.
/// This is needed because the shard router's `EconomicsShard` has its own
/// internal state that is disconnected from `AppState.economics` — operations
/// sent through the shard API endpoint don't reach the economics handlers.
async fn start_test_server_with_economics<F>(
    pre_register: F,
) -> (String, tokio::task::JoinHandle<()>, Arc<Mutex<EconomicsState>>)
where
    F: FnOnce(&mut EconomicsState),
{
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind to random port");
    let port = listener.local_addr().unwrap().port();

    let mut app_state = build_test_app_state(port);
    {
        let mut econ = app_state.economics.lock().await;
        pre_register(&mut econ);
    }
    let economics_clone = Arc::clone(&app_state.economics);

    let app = http::build_http_router().with_state(app_state);

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("Test server error");
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    (format!("http://127.0.0.1:{port}"), handle, economics_clone)
}

/// Start a test HTTP server on a random port.
///
/// Reads `OMNIA_JWT_SECRET`, `OMNIA_AUTHORIZED_CALLERS`, and
/// `OMNIA_RATE_LIMIT_RPS` from the environment at construction time.
/// The caller must set these **before** calling this function.
async fn start_test_server() -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind to random port");
    let port = listener.local_addr().unwrap().port();

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
    _lock: tokio::sync::MutexGuard<'static, ()>,
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
async fn setup_server(rate_limit_rps: Option<u64>) -> TestServer {
    let lock = ENV_LOCK.lock().await;

    // Set auth env vars
    std::env::set_var("OMNIA_JWT_SECRET", JWT_SECRET);
    std::env::set_var("OMNIA_AUTHORIZED_CALLERS", ADMIN_CALLER);

    // Set or clear rate limit override
    if let Some(rps) = rate_limit_rps {
        std::env::set_var("OMNIA_RATE_LIMIT_RPS", rps.to_string());
    } else {
        std::env::remove_var("OMNIA_RATE_LIMIT_RPS");
    }

    let (base_url, handle) = start_test_server().await;

    let env_guard = EnvGuard {
        keys: vec!["OMNIA_JWT_SECRET", "OMNIA_AUTHORIZED_CALLERS", "OMNIA_RATE_LIMIT_RPS"],
    };

    TestServer {
        base_url,
        _handle: handle,
        _env_guard: env_guard,
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
        .unwrap();
    assert_eq!(resp.status(), 200, "Public endpoint should be accessible without auth");
    let body: Value = resp.json().await.unwrap();
    assert!(body["node_id"].is_string(), "Response should contain node_id");
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
        .unwrap();
    assert_eq!(resp.status(), 200, "Public endpoint should be accessible without auth");
    let body: Value = resp.json().await.unwrap();
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
        .unwrap();
    assert_eq!(resp.status(), 401);

    // Valid JWT → 201
    let resp = client
        .post(format!("{}/api/v1/events", server.base_url))
        .bearer_auth(&valid_token)
        .json(&event_body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "Valid JWT should yield 201 for event submission");
    let body: Value = resp.json().await.unwrap();
    assert!(body["event_id"].is_string(), "Response should contain event_id");

    // Expired JWT → 401
    let resp = client
        .post(format!("{}/api/v1/events", server.base_url))
        .bearer_auth(&expired_token)
        .json(&event_body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // Wrong-secret JWT → 401
    let resp = client
        .post(format!("{}/api/v1/events", server.base_url))
        .bearer_auth(&wrong_secret_token)
        .json(&event_body)
        .send()
        .await
        .unwrap();
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
        .unwrap();
    assert_eq!(resp.status(), 201);
    let create_body: Value = resp.json().await.unwrap();
    let event_id = create_body["event_id"].as_str().unwrap().to_string();

    // No auth → 401
    let resp = client
        .get(format!("{}/api/v1/events/{event_id}", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // Valid JWT → 200 (event found)
    let resp = client
        .get(format!("{}/api/v1/events/{event_id}", server.base_url))
        .bearer_auth(&valid_token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["id"].as_str().unwrap(), event_id);

    // Valid JWT + nonexistent event → 404
    let resp = client
        .get(format!("{}/api/v1/events/nonexistent", server.base_url))
        .bearer_auth(&valid_token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);

    // Expired JWT → 401
    let resp = client
        .get(format!("{}/api/v1/events/{event_id}", server.base_url))
        .bearer_auth(&expired_token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // Wrong-secret JWT → 401
    let resp = client
        .get(format!("{}/api/v1/events/{event_id}", server.base_url))
        .bearer_auth(&wrong_secret_token)
        .send()
        .await
        .unwrap();
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
        .unwrap();
    assert_eq!(resp.status(), 401);

    // Valid JWT → 200 (register is non-privileged, so any valid JWT works)
    let resp = client
        .post(format!("{}/api/v1/shards/economics/operations", server.base_url))
        .bearer_auth(&valid_token)
        .json(&op_body)
        .send()
        .await
        .unwrap();
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
        .unwrap();
    assert_eq!(resp.status(), 401);

    // Wrong-secret JWT → 401
    let resp = client
        .post(format!("{}/api/v1/shards/economics/operations", server.base_url))
        .bearer_auth(&wrong_secret_token)
        .json(&op_body)
        .send()
        .await
        .unwrap();
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
        .unwrap();
    assert_eq!(resp.status(), 401);

    // Valid JWT → 201
    let resp = client
        .post(format!("{}/api/v1/governance/proposals", server.base_url))
        .bearer_auth(&valid_token)
        .json(&proposal_body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "Valid JWT should yield 201 for proposal creation");
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["id"].as_str().unwrap(), "proposal-test-1");

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
        .unwrap();
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
        .unwrap();
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
        .unwrap();
    assert_eq!(resp.status(), 401);

    // Valid JWT → 400 (auth passed, but voter has no registered stake)
    let resp = client
        .post(format!("{}/api/v1/governance/vote", server.base_url))
        .bearer_auth(&valid_token)
        .json(&vote_body)
        .send()
        .await
        .unwrap();
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
        .unwrap();
    assert_eq!(resp.status(), 401);

    // Wrong-secret JWT → 401
    let resp = client
        .post(format!("{}/api/v1/governance/vote", server.base_url))
        .bearer_auth(&wrong_secret_token)
        .json(&vote_body)
        .send()
        .await
        .unwrap();
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
        .unwrap();
    assert_eq!(resp.status(), 401);

    // Valid JWT → 404 (DID not registered — auth passed, handler returned 404)
    let resp = client
        .get(format!("{}/api/v1/economics/balance/did:test:unknown", server.base_url))
        .bearer_auth(&valid_token)
        .send()
        .await
        .unwrap();
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
        .unwrap();
    assert_eq!(resp.status(), 401);

    // Wrong-secret JWT → 401
    let resp = client
        .get(format!("{}/api/v1/economics/balance/did:test:unknown", server.base_url))
        .bearer_auth(&wrong_secret_token)
        .send()
        .await
        .unwrap();
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
        .unwrap();
    assert_eq!(resp.status(), 401);

    // Valid JWT → 404 (sender not registered — auth passed, handler returned 404)
    let resp = client
        .post(format!("{}/api/v1/economics/transfer", server.base_url))
        .bearer_auth(&valid_token)
        .json(&transfer_body)
        .send()
        .await
        .unwrap();
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
        .unwrap();
    assert_eq!(resp.status(), 401);

    // Wrong-secret JWT → 401
    let resp = client
        .post(format!("{}/api/v1/economics/transfer", server.base_url))
        .bearer_auth(&wrong_secret_token)
        .json(&transfer_body)
        .send()
        .await
        .unwrap();
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
            .unwrap();

        if resp.status() == 429 {
            got_429 = true;
            // Verify the 429 response body
            let body: Value = resp.json().await.unwrap();
            assert!(body["error"].is_string(), "429 response should contain 'error' field");
            let error_msg = body["error"].as_str().unwrap();
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
        .unwrap();

    assert_eq!(
        resp.status(),
        403,
        "Non-admin caller should get 403 Forbidden for MintUbc"
    );
    let body: Value = resp.json().await.unwrap();
    assert!(body["error"].is_string(), "403 response should have 'error' field");
    let error_msg = body["error"].as_str().unwrap();
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
        .unwrap();

    assert_eq!(resp.status(), 200, "Admin caller should get 200 OK for MintUbc");
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"].as_str().unwrap(), "processed");
    assert_eq!(body["operation"].as_str().unwrap(), "mint");
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
        .unwrap();

    assert_eq!(
        resp.status(),
        403,
        "Non-admin caller should get 403 Forbidden for AdvanceEpoch"
    );
    let body: Value = resp.json().await.unwrap();
    assert!(body["error"].is_string(), "403 response should have 'error' field");
    let error_msg = body["error"].as_str().unwrap();
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
        .unwrap();

    assert_eq!(resp.status(), 200, "Admin caller should get 200 OK for AdvanceEpoch");
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"].as_str().unwrap(), "processed");
    assert_eq!(body["operation"].as_str().unwrap(), "advance_epoch");
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
        .unwrap();

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
    let methods_str = allow_methods.to_str().unwrap();
    assert!(
        methods_str.contains("GET") && methods_str.contains("POST"),
        "Allowed methods should include GET and POST, got: {methods_str}"
    );

    let allow_headers = resp
        .headers()
        .get("access-control-allow-headers")
        .expect("Should have access-control-allow-headers header");
    let headers_str = allow_headers.to_str().unwrap();
    assert!(
        headers_str.contains("authorization") && headers_str.contains("content-type"),
        "Allowed headers should include Authorization and Content-Type, got: {headers_str}"
    );

    let max_age = resp
        .headers()
        .get("access-control-max-age")
        .expect("Should have access-control-max-age header");
    assert_eq!(max_age.to_str().unwrap(), "3600", "Max-Age should be 3600");
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
        .unwrap();

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
    let resp = client.get(&auth_url).send().await.unwrap();
    assert_eq!(resp.status(), 401);
    let body: Value = resp.json().await.unwrap();
    assert!(
        body["error"].is_string(),
        "401 response should have 'error' string field, got: {body:?}"
    );
    let error_msg = body["error"].as_str().unwrap();
    assert!(!error_msg.is_empty(), "Error message should not be empty");
    assert!(
        error_msg.contains("authorization"),
        "Missing-auth error should mention 'authorization', got: {error_msg}"
    );

    // --- Expired token ---
    let expired_token = make_expired_token(JWT_SECRET);
    let resp = client.get(&auth_url).bearer_auth(&expired_token).send().await.unwrap();
    assert_eq!(resp.status(), 401);
    let body: Value = resp.json().await.unwrap();
    assert!(
        body["error"].is_string(),
        "401 response should have 'error' string field, got: {body:?}"
    );
    assert!(
        body["error"].as_str().unwrap().contains("expired"),
        "Expired-token error should mention 'expired', got: {}",
        body["error"].as_str().unwrap()
    );

    // --- Invalid (wrong-secret) token ---
    let wrong_token = make_wrong_secret_token();
    let resp = client.get(&auth_url).bearer_auth(&wrong_token).send().await.unwrap();
    assert_eq!(resp.status(), 401);
    let body: Value = resp.json().await.unwrap();
    assert!(
        body["error"].is_string(),
        "401 response should have 'error' string field, got: {body:?}"
    );
    assert!(
        !body["error"].as_str().unwrap().is_empty(),
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
        .unwrap();

    assert_eq!(resp.status(), 403);
    let body: Value = resp.json().await.unwrap();
    assert!(
        body["error"].is_string(),
        "403 response should have 'error' string field, got: {body:?}"
    );
    let error_msg = body["error"].as_str().unwrap();
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
        .unwrap();

    assert_eq!(resp.status(), 404);
    let body: Value = resp.json().await.unwrap();
    assert!(
        body["error"].is_string(),
        "404 response should have 'error' string field, got: {body:?}"
    );
    let error_msg = body["error"].as_str().unwrap();
    assert!(!error_msg.is_empty(), "404 error message should not be empty");

    // Also test 404 from economics balance (unregistered DID)
    let resp = client
        .get(format!("{}/api/v1/economics/balance/did:test:noone", server.base_url))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
    let body: Value = resp.json().await.unwrap();
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
            .unwrap();

        if resp.status() == 429 {
            let body: Value = resp.json().await.unwrap();
            assert!(
                body["error"].is_string(),
                "429 response should have 'error' string field, got: {body:?}"
            );
            let error_msg = body["error"].as_str().unwrap();
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
        .unwrap();
    assert_eq!(resp.status(), 201);

    // Duplicate creation → 409
    let resp = client
        .post(format!("{}/api/v1/governance/proposals", server.base_url))
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 409, "Duplicate proposal should return 409 Conflict");
    let body: Value = resp.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("Failed to create proposal"));
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
        .unwrap();

    // Should get 400 for invalid choice, NOT 401 (auth passed) and NOT
    // the "no stake" 400 (the choice parse happens before the stake check).
    assert_eq!(resp.status(), 400, "Invalid vote choice should return 400");
    let body: Value = resp.json().await.unwrap();
    assert!(
        body["error"].as_str().unwrap().contains("invalid vote choice"),
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
        .unwrap();

    // Should NOT get the "invalid vote choice" 400 — trimming worked.
    // It will still get 400 for "no stake", but the error message should
    // NOT mention "invalid vote choice".
    assert_eq!(
        resp.status(),
        400,
        "Should get 400 for no stake, not for invalid choice"
    );
    let body: Value = resp.json().await.unwrap();
    let error = body["error"].as_str().unwrap();
    assert!(
        !error.contains("invalid vote choice"),
        "Whitespace should be trimmed — choice should parse as 'for'. Got error: {error}"
    );
}

// ---- economics: get_balance 200 (registered DID) ----

#[tokio::test]
async fn test_get_balance_registered_did_returns_200() {
    let lock = ENV_LOCK.lock().await;
    std::env::set_var("OMNIA_JWT_SECRET", JWT_SECRET);
    std::env::set_var("OMNIA_AUTHORIZED_CALLERS", ADMIN_CALLER);

    let (base_url, handle, _econ) = start_test_server_with_economics(|econ| {
        register_and_mint(econ, "did:test:balance-check", 5000);
    })
    .await;

    let _env_guard = EnvGuard {
        keys: vec!["OMNIA_JWT_SECRET", "OMNIA_AUTHORIZED_CALLERS"],
    };
    let client = reqwest::Client::new();
    let token = make_valid_token(REGULAR_CALLER);
    let resp = client
        .get(format!("{base_url}/api/v1/economics/balance/did:test:balance-check"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200, "Registered DID should return 200");
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["did"].as_str().unwrap(), "did:test:balance-check");
    assert!(
        body["balance"].as_u64().unwrap() >= 5000,
        "Balance should reflect minted amount"
    );
    assert!(body["is_registered"].as_bool().unwrap(), "Should be registered");

    drop(handle);
    drop(lock);
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
        .unwrap();

    assert_eq!(resp.status(), 400, "Zero amount should return 400");
    let body: Value = resp.json().await.unwrap();
    assert!(
        body["error"].as_str().unwrap().contains("must be greater than zero"),
        "Error should mention zero amount"
    );
}

// ---- economics: transfer 200 success ----

#[tokio::test]
async fn test_transfer_success_returns_200() {
    let lock = ENV_LOCK.lock().await;
    std::env::set_var("OMNIA_JWT_SECRET", JWT_SECRET);
    std::env::set_var("OMNIA_AUTHORIZED_CALLERS", ADMIN_CALLER);

    // Pre-register both DIDs in AppState.economics (the instance the
    // transfer handler reads from). REGULAR_CALLER is the JWT sub claim
    // which becomes from_did in the handler.
    let (base_url, handle, _econ) = start_test_server_with_economics(|econ| {
        register_and_mint(econ, REGULAR_CALLER, 10_000);
        register_and_mint(econ, "did:test:recipient", 100);
    })
    .await;

    let _env_guard = EnvGuard {
        keys: vec!["OMNIA_JWT_SECRET", "OMNIA_AUTHORIZED_CALLERS"],
    };

    let client = reqwest::Client::new();
    let token = make_valid_token(REGULAR_CALLER);

    let body = json!({
        "from_did": REGULAR_CALLER,
        "to_did": "did:test:recipient",
        "amount": 500
    });

    let resp = client
        .post(format!("{base_url}/api/v1/economics/transfer"))
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200, "Valid transfer should return 200");
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"].as_str().unwrap(), "completed");
    assert_eq!(body["amount"].as_u64().unwrap(), 500);
    assert!(
        body["new_balance"].as_u64().unwrap() >= 9_500,
        "New balance should reflect the spent amount"
    );

    drop(handle);
    drop(lock);
}

// ---- economics: transfer 400 insufficient balance ----

#[tokio::test]
async fn test_transfer_insufficient_balance_returns_400() {
    let lock = ENV_LOCK.lock().await;
    std::env::set_var("OMNIA_JWT_SECRET", JWT_SECRET);
    std::env::set_var("OMNIA_AUTHORIZED_CALLERS", ADMIN_CALLER);

    // Mint a small amount to the caller, not enough for the transfer.
    let (base_url, handle, _econ) = start_test_server_with_economics(|econ| {
        register_and_mint(econ, REGULAR_CALLER, 100);
        register_and_mint(econ, "did:test:recipient", 100);
    })
    .await;

    let _env_guard = EnvGuard {
        keys: vec!["OMNIA_JWT_SECRET", "OMNIA_AUTHORIZED_CALLERS"],
    };

    let client = reqwest::Client::new();
    let token = make_valid_token(REGULAR_CALLER);

    let body = json!({
        "from_did": REGULAR_CALLER,
        "to_did": "did:test:recipient",
        "amount": 10_000  // Much more than the 100 balance
    });

    let resp = client
        .post(format!("{base_url}/api/v1/economics/transfer"))
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400, "Insufficient balance should return 400");
    let body: Value = resp.json().await.unwrap();
    assert!(
        body["error"].as_str().unwrap().contains("Transfer failed"),
        "Error should mention transfer failure"
    );

    drop(handle);
    drop(lock);
}
