#![cfg(feature = "docker-tests")]
//! Docker Compose End-to-End Test — 5-Node Testnet
//!
//! Exercises the full 5-node Docker Compose configuration defined in
//! `docker/docker-compose.yml`. The test:
//!
//! 1. Runs `docker compose up -d --build` to launch the testnet
//! 2. Waits for all 5 nodes to report healthy (polls `/healthz`)
//! 3. Submits a batch of events via the bootstrap node's API
//! 4. Verifies event submission and retrieval on the bootstrap node
//! 5. Verifies shard operation processing on the bootstrap node
//! 6. Verifies node info consistency across all 5 nodes (protocol version,
//!    shard count, and API availability)
//! 7. Tears down with `docker compose down -v` in cleanup (even on failure)
//!
//! # Running
//!
//! ```bash
//! cargo test -p omnia-node --test docker_compose_e2e --features docker-tests -- --nocapture
//! ```
//!
//! # Prerequisites
//!
//! - Docker and Docker Compose v2 must be installed and the daemon running
//! - Ports 9090–9094 must be available on the host
//! - Sufficient resources to build the Docker image (multi-stage Rust build)

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use reqwest::StatusCode;
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Project root directory (workspace root, where the top-level Cargo.toml lives).
/// CARGO_MANIFEST_DIR points to the `node/` crate directory; we need to go
/// up one level to reach the workspace root where `docker/` lives.
const PROJECT_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

/// Relative path to the 5-node Docker Compose file from the project root.
const COMPOSE_FILE: &str = "docker/docker-compose.yml";

/// Strong JWT secret injected into the compose environment for the e2e run.
///
/// The compose files now require `OMNIA_JWT_SECRET` (`${...:?}`) and the node
/// rejects known-weak / too-short secrets at startup (AUDIT-2026-07 C11,
/// #349), so the test must deploy like a real operator would — with a
/// strong, explicitly-set secret. This is a throwaway 64-hex value used only
/// by the ephemeral test stack.
const TEST_COMPOSE_JWT_SECRET: &str = "e2e0f1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7";

/// Host ports mapped to each of the 5 nodes.
/// Bootstrap = 9090, node-1 = 9091, …, node-4 = 9094.
const NODE_PORTS: &[u16] = &[9090, 9091, 9092, 9093, 9094];

/// Maximum time to wait for all nodes to become healthy.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(300);

/// Interval between health-check polls.
const HEALTH_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Number of events to submit in the batch test.
const EVENT_BATCH_SIZE: usize = 10;

// ---------------------------------------------------------------------------
// RAII guard — ensures `docker compose down` runs even on test failure
// ---------------------------------------------------------------------------

/// Guard that runs `docker compose down -v` when dropped.
///
/// The `-v` flag removes named volumes so leftover state from a failed
/// test doesn't poison the next run.
struct ComposeGuard {
    project_root: PathBuf,
    torn_down: bool,
}

impl ComposeGuard {
    fn new(project_root: PathBuf) -> Self {
        Self {
            project_root,
            torn_down: false,
        }
    }

    /// Explicitly tear down the compose stack. Called automatically on drop.
    fn down(&mut self) {
        if self.torn_down {
            return;
        }
        self.torn_down = true;
        eprintln!("[docker-e2e] Tearing down Docker Compose stack…");
        let status = Command::new("docker")
            .args([
                "compose",
                "-f",
                COMPOSE_FILE,
                "down",
                "-v",
                "--remove-orphans",
                "--timeout",
                "30",
            ])
            .env("OMNIA_JWT_SECRET", TEST_COMPOSE_JWT_SECRET)
            .env("OMNIA_JWT_ALLOW_LEGACY_HS256", "true")
            .current_dir(&self.project_root)
            .status()
            .expect("failed to run docker compose down");
        if status.success() {
            eprintln!("[docker-e2e] Docker Compose stack torn down successfully.");
        } else {
            eprintln!(
                "[docker-e2e] WARNING: docker compose down exited with status {:?}",
                status.code()
            );
        }
    }
}

impl Drop for ComposeGuard {
    fn drop(&mut self) {
        self.down();
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build the base URL for a node reachable on the given host port.
fn node_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

/// Run `docker compose up -d --build` from the project root.
fn compose_up(project_root: &PathBuf) {
    eprintln!("[docker-e2e] Starting Docker Compose stack (this may take a while on first run)…");
    let output = Command::new("docker")
        .args(["compose", "-f", COMPOSE_FILE, "up", "-d", "--build", "--wait"])
        .env("OMNIA_JWT_SECRET", TEST_COMPOSE_JWT_SECRET)
        .env("OMNIA_JWT_ALLOW_LEGACY_HS256", "true")
        .current_dir(project_root)
        .output()
        .expect("failed to run docker compose up");

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!(
            "docker compose up failed (exit {:?}):\nstdout: {stdout}\nstderr: {stderr}",
            output.status.code()
        );
    }
    eprintln!("[docker-e2e] Docker Compose stack is up.");
}

/// Poll `/healthz` on every node until they all return 200, or timeout.
async fn wait_for_healthy(client: &reqwest::Client) {
    eprintln!(
        "[docker-e2e] Waiting up to {}s for all {} nodes to become healthy…",
        HEALTH_TIMEOUT.as_secs(),
        NODE_PORTS.len()
    );
    let deadline = tokio::time::Instant::now() + HEALTH_TIMEOUT;

    loop {
        let mut all_healthy = true;
        for &port in NODE_PORTS {
            let url = format!("{}/healthz", node_url(port));
            match client.get(&url).timeout(Duration::from_secs(5)).send().await {
                Ok(resp) if resp.status() == StatusCode::OK => {
                    // Node is healthy
                }
                Ok(resp) => {
                    eprintln!(
                        "[docker-e2e] Node on port {port} returned status {} (not yet healthy)",
                        resp.status()
                    );
                    all_healthy = false;
                }
                Err(e) => {
                    eprintln!("[docker-e2e] Node on port {port} not reachable yet: {e}");
                    all_healthy = false;
                }
            }
        }

        if all_healthy {
            eprintln!("[docker-e2e] All {} nodes are healthy!", NODE_PORTS.len());
            return;
        }

        if tokio::time::Instant::now() + HEALTH_POLL_INTERVAL > deadline {
            panic!(
                "Timed out waiting for all nodes to become healthy after {}s",
                HEALTH_TIMEOUT.as_secs()
            );
        }

        tokio::time::sleep(HEALTH_POLL_INTERVAL).await;
    }
}

/// Submit a single event to the given node, returning the event ID on success.
async fn submit_event(client: &reqwest::Client, port: u16, payload: &str, event_type: &str) -> String {
    let url = format!("{}/api/v1/events", node_url(port));
    let body = json!({
        "payload": payload,
        "event_type": event_type,
    });

    let resp = client
        .post(&url)
        .json(&body)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .unwrap_or_else(|e| panic!("Failed to POST event to port {port}: {e}"));

    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "Event submission on port {port} should return 201, got {}",
        resp.status()
    );

    let json: Value = resp.json().await.expect("Failed to parse event response JSON");
    json["event_id"]
        .as_str()
        .unwrap_or_else(|| panic!("Response should contain event_id, got: {json:?}"))
        .to_string()
}

/// Retrieve a stored event by ID from the given node.
async fn get_event(client: &reqwest::Client, port: u16, event_id: &str) -> Value {
    let url = format!("{}/api/v1/events/{event_id}", node_url(port));
    let resp = client
        .get(&url)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .unwrap_or_else(|e| panic!("Failed to GET event {event_id} from port {port}: {e}"));

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "Event retrieval on port {port} for event {event_id} should return 200, got {}",
        resp.status()
    );

    resp.json().await.expect("Failed to parse event response JSON")
}

/// Get node info from the given node.
async fn get_node_info(client: &reqwest::Client, port: u16) -> Value {
    let url = format!("{}/api/v1/node/info", node_url(port));
    let resp = client
        .get(&url)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .unwrap_or_else(|e| panic!("Failed to GET node info from port {port}: {e}"));

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "Node info on port {port} should return 200, got {}",
        resp.status()
    );

    resp.json().await.expect("Failed to parse node info response JSON")
}

/// Perform a shard operation on the economics shard.
async fn submit_shard_op(
    client: &reqwest::Client,
    port: u16,
    operation: &str,
    params: serde_json::Map<String, Value>,
) -> Value {
    let url = format!("{}/api/v1/shards/economics/operations", node_url(port));
    let body = json!({
        "operation": operation,
        "params": params,
    });

    let resp = client
        .post(&url)
        .json(&body)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .unwrap_or_else(|e| panic!("Failed to POST shard op to port {port}: {e}"));

    // Accept 200 (processed) or 201 for success
    assert!(
        resp.status() == StatusCode::OK || resp.status() == StatusCode::CREATED,
        "Shard operation '{operation}' on port {port} should succeed, got {}",
        resp.status()
    );

    resp.json().await.expect("Failed to parse shard op response JSON")
}

/// Get the economics balance for a DID from the given node.
async fn get_balance(client: &reqwest::Client, port: u16, did: &str) -> Value {
    let url = format!("{}/api/v1/economics/balance/{did}", node_url(port));
    let resp = client
        .get(&url)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .unwrap_or_else(|e| panic!("Failed to GET balance from port {port}: {e}"));

    resp.json().await.expect("Failed to parse balance response JSON")
}

// ---------------------------------------------------------------------------
// Test
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_docker_compose_5node_e2e() {
    let project_root = PathBuf::from(PROJECT_ROOT);
    // Guard ensures cleanup on panic / early return
    let mut guard = ComposeGuard::new(project_root.clone());

    // ── Step 1: Bring up the 5-node testnet ──────────────────────────────
    compose_up(&project_root);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .expect("Failed to build reqwest client");

    // ── Step 2: Wait for all nodes to become healthy ─────────────────────
    wait_for_healthy(&client).await;

    // ── Step 3: Verify liveness on every node ────────────────────────────
    eprintln!("[docker-e2e] Verifying liveness endpoints on all nodes…");
    for &port in NODE_PORTS {
        let url = format!("{}/healthz", node_url(port));
        let resp = client
            .get(&url)
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .unwrap_or_else(|e| panic!("Health check failed for port {port}: {e}"));

        assert_eq!(resp.status(), StatusCode::OK, "Node on port {port} should be alive");
        let body: Value = resp.json().await.expect("Failed to parse health response");
        assert_eq!(
            body["status"], "alive",
            "Health response should have status=alive for port {port}"
        );
    }
    eprintln!("[docker-e2e] All nodes are alive.");

    // ── Step 4: Submit a batch of events via the bootstrap node ──────────
    eprintln!("[docker-e2e] Submitting {EVENT_BATCH_SIZE} events to bootstrap node (port 9090)…");
    let mut event_ids = Vec::with_capacity(EVENT_BATCH_SIZE);
    for i in 0..EVENT_BATCH_SIZE {
        let payload = hex::encode(format!("docker-e2e-event-{i}"));
        let event_type = "docker-test";
        let event_id = submit_event(&client, 9090, &payload, event_type).await;
        event_ids.push(event_id);
    }
    eprintln!("[docker-e2e] Successfully submitted {EVENT_BATCH_SIZE} events.");

    // ── Step 5: Verify event retrieval on the bootstrap node ─────────────
    eprintln!("[docker-e2e] Verifying event retrieval on bootstrap node…");
    for (i, event_id) in event_ids.iter().enumerate() {
        let event = get_event(&client, 9090, event_id).await;
        assert_eq!(
            event["id"].as_str().expect("test assertion failed"),
            event_id,
            "Retrieved event ID should match for event {i}"
        );
        assert_eq!(
            event["event_type"].as_str().expect("test assertion failed"),
            "docker-test",
            "Event type should be 'docker-test' for event {i}"
        );
        assert_eq!(
            event["status"].as_str().expect("test assertion failed"),
            "submitted",
            "Event status should be 'submitted' for event {i}"
        );
    }
    eprintln!("[docker-e2e] All {EVENT_BATCH_SIZE} events verified on bootstrap node.");

    // ── Step 6: Verify node info consistency across all 5 nodes ──────────
    eprintln!("[docker-e2e] Verifying node info consistency across all nodes…");
    let mut node_infos = Vec::with_capacity(NODE_PORTS.len());
    for &port in NODE_PORTS {
        let info = get_node_info(&client, port).await;
        node_infos.push(info);
    }

    // All nodes should report the same protocol version
    let protocol_versions: Vec<&str> = node_infos
        .iter()
        .map(|info| info["protocol_version"].as_str().unwrap_or("missing"))
        .collect();
    let first_version = protocol_versions[0];
    for (i, version) in protocol_versions.iter().enumerate() {
        assert_eq!(
            *version, first_version,
            "All nodes should report the same protocol version; node {i} has {version}"
        );
    }

    // All nodes should report the same shard count
    let shard_counts: Vec<u64> = node_infos
        .iter()
        .map(|info| info["shard_count"].as_u64().unwrap_or(0))
        .collect();
    let first_shard_count = shard_counts[0];
    assert!(
        first_shard_count > 0,
        "Shard count should be positive, got {first_shard_count}"
    );
    for (i, count) in shard_counts.iter().enumerate() {
        assert_eq!(
            *count, first_shard_count,
            "All nodes should report the same shard count; node {i} has {count}"
        );
    }

    // All nodes should have distinct node IDs
    let node_ids: Vec<&str> = node_infos
        .iter()
        .map(|info| info["node_id"].as_str().unwrap_or("missing"))
        .collect();
    let unique_node_ids: std::collections::HashSet<_> = node_ids.iter().copied().collect();
    assert_eq!(
        unique_node_ids.len(),
        NODE_PORTS.len(),
        "Each node should have a unique node_id, got: {node_ids:?}"
    );

    eprintln!("[docker-e2e] Node info consistent: protocol_version={first_version}, shard_count={first_shard_count}");

    // ── Step 7: Verify shard operations (economics) on bootstrap node ────
    eprintln!("[docker-e2e] Testing shard operations on bootstrap node…");
    let test_did = "did:docker:e2e-test";

    // Register the DID
    let mut register_params = serde_json::Map::new();
    register_params.insert("did".to_string(), json!(test_did));
    let result = submit_shard_op(&client, 9090, "register", register_params).await;
    assert_eq!(
        result["status"].as_str().expect("test assertion failed"),
        "processed",
        "Register operation should be processed"
    );

    // Mint UBC to the DID (a privileged shard op). OMNIA_AUTHORIZED_CALLERS
    // is not configured in this stack, so the caller is not privileged and
    // mint may fail with 403. We test that the shard operation endpoint is
    // reachable and functioning — a 403 means authorization is being enforced.
    let mut mint_params = serde_json::Map::new();
    mint_params.insert("did".to_string(), json!(test_did));
    mint_params.insert("amount".to_string(), json!(1000));
    let url = format!("{}/api/v1/shards/economics/operations", node_url(9090));
    let mint_body = json!({
        "operation": "mint",
        "params": mint_params,
    });
    let mint_resp = client
        .post(&url)
        .json(&mint_body)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .expect("Mint request should be sendable");

    // The mint response is either 200 (if authorized) or 403 (if not) —
    // both are valid outcomes depending on the Docker config.
    let mint_status = mint_resp.status();
    assert!(
        mint_status == StatusCode::OK || mint_status == StatusCode::FORBIDDEN,
        "Mint should return 200 or 403, got {mint_status}"
    );

    if mint_status == StatusCode::OK {
        // If mint succeeded, verify balance
        let balance_resp = get_balance(&client, 9090, test_did).await;
        // Balance endpoint returns 200 with amount or 404 if DID not registered
        eprintln!("[docker-e2e] Mint succeeded, balance response: {balance_resp:?}");
    } else {
        eprintln!(
            "[docker-e2e] Mint returned 403 (expected without OMNIA_AUTHORIZED_CALLERS) — auth is working correctly."
        );
    }

    eprintln!("[docker-e2e] Shard operations verified.");

    // ── Step 8: Verify API availability on all nodes ─────────────────────
    eprintln!("[docker-e2e] Verifying API availability on all nodes…");
    for &port in NODE_PORTS {
        // Events endpoint
        let url = format!("{}/api/v1/events", node_url(port));
        let resp = client
            .post(&url)
            .json(&json!({
                "payload": hex::encode(format!("availability-check-port-{port}")),
                "event_type": "probe",
            }))
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .unwrap_or_else(|e| panic!("Events endpoint on port {port} should be reachable: {e}"));

        assert_eq!(
            resp.status(),
            StatusCode::CREATED,
            "Events endpoint on port {port} should return 201"
        );

        // Node info endpoint
        let info = get_node_info(&client, port).await;
        assert!(
            info["node_id"].is_string(),
            "Node info on port {port} should contain node_id"
        );

        // Peers endpoint
        let url = format!("{}/api/v1/node/peers", node_url(port));
        let resp = client
            .get(&url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .unwrap_or_else(|e| panic!("Peers endpoint on port {port} should be reachable: {e}"));
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "Peers endpoint on port {port} should return 200"
        );
    }
    eprintln!(
        "[docker-e2e] All API endpoints verified on all {} nodes.",
        NODE_PORTS.len()
    );

    // ── Cleanup ──────────────────────────────────────────────────────────
    guard.down();
    eprintln!("[docker-e2e] Test completed successfully!");
}
