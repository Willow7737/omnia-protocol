#![allow(clippy::unwrap_used)]
#![allow(deprecated)]
//! Integration tests for the Omnia node HTTP API
//!
//! These tests start the full HTTP server and verify that each
//! endpoint returns the expected status codes and response shapes.

use anyhow::Result;
use omnia_economics::EconomicsState;
use omnia_node::config::NodeConfig;
use omnia_node::http;
use omnia_node::state::AppState;
#[cfg(feature = "metrics")]
use omnia_node::state::NodeMetrics;
use omnia_shards::{
    BiologicalShard, ComputationalShard, EconomicsShard, FeeSchedule, FinancialShard, IdentityShard, PhysicalShard,
    ShardRouter,
};
use omnia_substrate::{Substrate, SubstrateConfig};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Mutex, RwLock};

/// Helper: start the node HTTP server on a random port and return
/// the base URL and a shutdown handle.
///
/// The server runs in a background tokio task and will be stopped
/// when the shutdown handle is dropped.
async fn start_test_server() -> (String, tokio::task::JoinHandle<()>) {
    // Pick a random available port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind to random port");
    let port = listener.local_addr().unwrap().port();

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

    #[cfg(feature = "metrics")]
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

    let app_state = AppState {
        config,
        substrate: Arc::new(RwLock::new(substrate)),
        slashing: Arc::new(Mutex::new(slashing)),
        shard_router: Arc::new(std::sync::Mutex::new(shard_router)),
        economics: Arc::new(Mutex::new(economics)),
        event_store: Arc::new(RwLock::new(HashMap::new())),
        peers: Arc::new(RwLock::new(Vec::new())),
        #[cfg(feature = "metrics")]
        metrics: Arc::new(metrics),
        started_at: Instant::now(),
        is_syncing: Arc::new(AtomicBool::new(false)),
        settlement: Arc::new(omnia_adapters::MockSettlementAdapter::new()),
        #[cfg(feature = "zk")]
        ceremony_server: None,
    };

    let app = http::build_http_router().with_state(app_state);

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("Test server error");
    });

    // Give the server a moment to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    (format!("http://127.0.0.1:{port}"), handle)
}

#[tokio::test]
async fn test_health_endpoint() -> Result<()> {
    let (base_url, _handle) = start_test_server().await;

    let client = reqwest::Client::new();
    let resp = client.get(format!("{base_url}/health")).send().await?;

    assert_eq!(resp.status(), 200);

    let body: Value = resp.json().await?;
    assert_eq!(body["status"], "alive");
    assert_eq!(body["node_id"], 42);

    Ok(())
}

#[tokio::test]
#[cfg(feature = "metrics")]
async fn test_metrics_endpoint() -> Result<()> {
    let (base_url, _handle) = start_test_server().await;

    let client = reqwest::Client::new();
    let resp = client.get(format!("{base_url}/metrics")).send().await?;

    assert_eq!(resp.status(), 200);

    let body = resp.text().await?;
    // Verify Prometheus exposition format contains our metrics
    assert!(
        body.contains("omnia_node_events_submitted_total"),
        "Metrics should contain omnia_node_events_submitted_total, got: {}",
        &body[..body.len().min(500)]
    );

    Ok(())
}

/// When metrics feature is disabled, /metrics should return 404
#[tokio::test]
#[cfg(not(feature = "metrics"))]
async fn test_metrics_endpoint_disabled() -> Result<()> {
    let (base_url, _handle) = start_test_server().await;

    let client = reqwest::Client::new();
    let resp = client.get(format!("{base_url}/metrics")).send().await?;

    assert_eq!(
        resp.status(),
        404,
        "Metrics endpoint should be 404 when feature disabled"
    );

    Ok(())
}

#[tokio::test]
async fn test_submit_event() -> Result<()> {
    let (base_url, _handle) = start_test_server().await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base_url}/api/v1/events"))
        .json(&json!({
            "payload": hex::encode(b"hello omnia"),
            "event_type": "test"
        }))
        .send()
        .await?;

    assert_eq!(resp.status(), 201);

    let body: Value = resp.json().await?;
    assert!(body["event_id"].is_string(), "Response should contain event_id");
    assert_eq!(body["status"], "submitted");

    Ok(())
}

#[tokio::test]
async fn test_get_event_not_found() -> Result<()> {
    let (base_url, _handle) = start_test_server().await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{base_url}/api/v1/events/nonexistent"))
        .send()
        .await?;

    assert_eq!(resp.status(), 404);

    // The response body may be empty for 404s from axum's default handler,
    // or JSON from our custom handler. Either way, the status code is the
    // primary assertion.
    let body_text = resp.text().await?;
    if !body_text.is_empty() {
        let body: Value = serde_json::from_str(&body_text)?;
        assert!(body["error"].is_string());
    }

    Ok(())
}

/// Test that Swagger UI is accessible when the feature is enabled.
#[tokio::test]
#[cfg(feature = "swagger-ui")]
async fn test_swagger_ui() -> Result<()> {
    let (base_url, _handle) = start_test_server().await;
    let client = reqwest::Client::new();
    let resp = client.get(format!("{base_url}/swagger-ui/")).send().await?;
    assert_eq!(
        resp.status(),
        200,
        "Swagger UI should be accessible when feature enabled"
    );
    Ok(())
}

/// Test that Swagger UI returns 404 when the feature is disabled.
#[tokio::test]
#[cfg(not(feature = "swagger-ui"))]
async fn test_swagger_ui_disabled() -> Result<()> {
    let (base_url, _handle) = start_test_server().await;
    let client = reqwest::Client::new();
    let resp = client.get(format!("{base_url}/swagger-ui/")).send().await?;
    assert_eq!(resp.status(), 404, "Swagger UI should be 404 when feature disabled");
    Ok(())
}

/// Test that OpenAPI JSON is accessible when the feature is enabled.
#[tokio::test]
#[cfg(feature = "swagger-ui")]
async fn test_openapi_json() -> Result<()> {
    let (base_url, _handle) = start_test_server().await;
    let client = reqwest::Client::new();
    let resp = client.get(format!("{base_url}/api-docs/openapi.json")).send().await?;
    assert_eq!(
        resp.status(),
        200,
        "OpenAPI JSON should be accessible when feature enabled"
    );
    let body: Value = resp.json().await?;
    // Verify the OpenAPI spec contains our paths
    let paths = body["paths"].as_object().expect("paths should be an object");
    assert!(paths.keys().any(|k| k.contains("events")), "Should contain events path");
    assert!(
        paths.keys().any(|k| k.contains("governance")),
        "Should contain governance path"
    );
    assert!(
        paths.keys().any(|k| k.contains("economics")),
        "Should contain economics path"
    );
    assert!(paths.keys().any(|k| k.contains("node")), "Should contain node path");
    Ok(())
}

/// Test that OpenAPI JSON returns 404 when the feature is disabled.
#[tokio::test]
#[cfg(not(feature = "swagger-ui"))]
async fn test_openapi_json_disabled() -> Result<()> {
    let (base_url, _handle) = start_test_server().await;
    let client = reqwest::Client::new();
    let resp = client.get(format!("{base_url}/api-docs/openapi.json")).send().await?;
    assert_eq!(resp.status(), 404, "OpenAPI JSON should be 404 when feature disabled");
    Ok(())
}
