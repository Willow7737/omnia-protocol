//! Shared application state and Prometheus metrics
//!
//! This module defines the [`AppState`] struct that is shared across
//! all HTTP handlers via `axum::extract::State`, and the [`NodeMetrics`]
//! struct that tracks Prometheus counters and gauges.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Instant;

use prometheus::{IntCounter, IntGauge};
use tokio::sync::{Mutex, RwLock};

use omnia_economics::EconomicsState;
use omnia_shards::ShardRouter;
use omnia_substrate::SlashingEngine;
use omnia_substrate::Substrate;

use crate::api::events::StoredEvent;
use crate::api::node::PeerInfo;
use crate::config::NodeConfig;

/// Global singleton for Prometheus metrics.
///
/// Using `OnceLock` ensures metrics are only registered once per process,
/// even when multiple test instances are created. Subsequent calls to
/// `NodeMetrics::new()` return clones of the same registered metrics.
static NODE_METRICS: OnceLock<NodeMetrics> = OnceLock::new();

/// Prometheus metrics for the node.
///
/// All metrics are registered with the default Prometheus registry
/// so they can be exposed via the `/metrics` endpoint.
#[derive(Debug, Clone)]
pub struct NodeMetrics {
    /// Total number of events submitted via the API.
    pub events_submitted: IntCounter,
    /// Total number of events finalized by consensus.
    pub events_finalized: IntCounter,
    /// Current number of connected peers.
    pub peers_connected: IntGauge,
    /// Current consensus round.
    pub consensus_round: IntGauge,
    /// Total number of shard operations processed.
    pub shard_ops_total: IntCounter,
    /// Total number of HTTP requests served.
    pub http_requests_total: IntCounter,
}

impl NodeMetrics {
    /// Create and register all node metrics with the default Prometheus registry.
    ///
    /// Uses a `OnceLock` to ensure metrics are only registered once per process.
    /// Subsequent calls return a clone of the already-registered metrics.
    /// This is safe for parallel test execution.
    pub fn new() -> Result<Self, prometheus::Error> {
        let _ = NODE_METRICS
            .get_or_init(|| {
                let events_submitted = IntCounter::new(
                    "omnia_node_events_submitted_total",
                    "Total events submitted via the API",
                )
                .expect("Failed to create events_submitted counter");
                let events_finalized = IntCounter::new(
                    "omnia_node_events_finalized_total",
                    "Total events finalized by consensus",
                )
                .expect("Failed to create events_finalized counter");
                let peers_connected = IntGauge::new(
                    "omnia_node_peers_connected",
                    "Current number of connected peers",
                )
                .expect("Failed to create peers_connected gauge");
                let consensus_round =
                    IntGauge::new("omnia_node_consensus_round", "Current consensus round")
                        .expect("Failed to create consensus_round gauge");
                let shard_ops_total = IntCounter::new(
                    "omnia_node_shard_operations_total",
                    "Total shard operations processed",
                )
                .expect("Failed to create shard_ops_total counter");
                let http_requests_total = IntCounter::new(
                    "omnia_node_http_requests_total",
                    "Total HTTP requests served",
                )
                .expect("Failed to create http_requests_total counter");

                let registry = prometheus::default_registry();
                // Ignore AlreadyReg errors — metrics may have been registered
                // in a previous test or call
                let _ = registry.register(Box::new(events_submitted.clone()));
                let _ = registry.register(Box::new(events_finalized.clone()));
                let _ = registry.register(Box::new(peers_connected.clone()));
                let _ = registry.register(Box::new(consensus_round.clone()));
                let _ = registry.register(Box::new(shard_ops_total.clone()));
                let _ = registry.register(Box::new(http_requests_total.clone()));

                Self {
                    events_submitted,
                    events_finalized,
                    peers_connected,
                    consensus_round,
                    shard_ops_total,
                    http_requests_total,
                }
            })
            .clone();
        Ok(NODE_METRICS
            .get()
            .expect("metrics just initialized")
            .clone())
    }
}

/// Shared application state accessible to all HTTP handlers.
///
/// This struct is wrapped in `Arc` by `axum` and passed to handlers
/// via `State(state): State<AppState>`. All mutable fields use
/// interior mutability (`Mutex` or `RwLock`) for safe concurrent access.
///
/// Implements `Clone` by cloning the `Arc` pointers — the underlying
/// data is shared, not duplicated.
#[derive(Clone)]
pub struct AppState {
    /// Node configuration (immutable after startup).
    pub config: NodeConfig,
    /// The substrate runtime — holds the causal graph and consensus engine.
    pub substrate: Arc<RwLock<Substrate>>,
    /// Slashing engine with redb persistence.
    pub slashing: Arc<Mutex<SlashingEngine>>,
    /// Shard router for dispatching operations to domain shards.
    pub shard_router: Arc<Mutex<ShardRouter>>,
    /// Economics state — UBC token balances, governance, and quota tracking.
    pub economics: Arc<Mutex<EconomicsState>>,
    /// In-memory event store for API retrieval.
    pub event_store: Arc<RwLock<HashMap<String, StoredEvent>>>,
    /// Known peers in the network.
    pub peers: Arc<RwLock<Vec<PeerInfo>>>,
    /// Prometheus metrics counters and gauges.
    pub metrics: Arc<NodeMetrics>,
    /// Time when the node was started (for uptime calculation).
    pub started_at: Instant,
}
