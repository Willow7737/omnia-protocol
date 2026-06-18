//! Shared application state and Prometheus metrics
//!
//! This module defines the [`AppState`] struct that is shared across
//! all HTTP handlers via `axum::extract::State`, and the `NodeMetrics`
//! struct that tracks Prometheus counters and gauges.

// C-2 fix (audit v0.1.68): the previous comment referenced the now-removed
// crate-level `#![deprecated]` annotation on omnia-substrate. That
// annotation has been removed and the suppression is no longer needed.
use omnia_substrate::crypto::NodeKeypair;
use omnia_substrate::SlashingEngine;
use omnia_substrate::Substrate;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Instant;

// H-5 fix (audit v0.1.68): use IndexMap for the in-memory event store so
// that eviction is deterministic (oldest by insertion order, not arbitrary
// HashMap iteration order). IndexMap preserves insertion order while keeping
// O(1) lookup/insert/remove, which is exactly what we need for an LRU-style
// bounded cache.
use indexmap::IndexMap;

#[cfg(feature = "metrics")]
use prometheus::{Histogram, IntCounter, IntGauge};
use tokio::sync::{Mutex, RwLock};

use omnia_economics::EconomicsState;
use omnia_shards::ShardRouter;

#[cfg(feature = "zk")]
use omnia_adapters::setup::CeremonyServer;
use omnia_adapters::SettlementAdapter;

use crate::api::events::StoredEvent;
use crate::api::node::PeerInfo;
use crate::config::NodeConfig;

/// Maximum number of events to store in the in-memory event store.
/// When this limit is reached, the oldest 10% of events are evicted.
const MAX_STORED_EVENTS: usize = 100_000;

/// Type alias for the in-memory event store.
///
/// Uses `IndexMap` (insertion-order preserving) rather than `HashMap`
/// so that eviction at capacity is deterministic — the oldest 10% by
/// insertion order are removed, not arbitrary entries chosen by
/// HashMap iteration order. This is important for consensus-critical
/// state: if two honest nodes evict *different* events for the same
/// EventId, they may diverge in their responses to status queries.
///
/// H-5 fix (audit v0.1.68).
pub type EventStore = IndexMap<String, StoredEvent>;

/// Store an event in the bounded event store with deterministic LRU eviction.
///
/// When the store exceeds `MAX_STORED_EVENTS`, the oldest 10% of
/// entries (by insertion order) are removed to prevent unbounded
/// memory growth. `IndexMap::shift_remove_index(0)` removes the
/// first-inserted entry, guaranteeing deterministic eviction order
/// across nodes.
///
/// H-5 fix (audit v0.1.68): previously used `HashMap::keys().take(n)`,
/// which has non-deterministic iteration order — the "oldest 10%" claim
/// was inaccurate and could evict recently-inserted entries instead.
pub async fn store_event(
    event_store: &Arc<RwLock<EventStore>>,
    event_id: String,
    stored: StoredEvent,
) {
    let mut store = event_store.write().await;
    if store.len() >= MAX_STORED_EVENTS {
        // Remove oldest 10% by insertion order (deterministic).
        let to_remove = MAX_STORED_EVENTS / 10;
        for _ in 0..to_remove {
            // shift_remove_index(0) removes the first entry and shifts
            // subsequent entries down — so the next iteration's index 0
            // is again the oldest remaining entry.
            if store.shift_remove_index(0).is_none() {
                break; // store emptied early
            }
        }
    }
    store.insert(event_id, stored);
}

/// Global singleton for Prometheus metrics.
///
/// Using `OnceLock` ensures metrics are only registered once per process,
/// even when multiple test instances are created. Subsequent calls to
/// `NodeMetrics::new()` return clones of the same registered metrics.
#[cfg(feature = "metrics")]
static NODE_METRICS: OnceLock<NodeMetrics> = OnceLock::new();

/// Prometheus metrics for the node.
///
/// All metrics are registered with the default Prometheus registry
/// so they can be exposed via the `/metrics` endpoint.
///
/// # Throughput-Specific Metrics (Sprint 0)
///
/// The following metrics were added for the Phase 0 throughput optimization
/// sprint to enable detailed monitoring of consensus and DAG performance:
///
/// - `omnia_consensus_tps` — counter incremented per finalized transaction
/// - `omnia_consensus_finality_latency_seconds` — histogram of time from event
///   creation to finality commitment
/// - `omnia_gossip_propagation_latency_seconds` — histogram of end-to-end event
///   propagation latency across the gossip network
/// - `omnia_dag_events_total` — counter for total events inserted into the DAG
/// - `omnia_dag_insertion_latency_seconds` — histogram of DAG event insertion latency
/// - `omnia_node_memory_rss_bytes` — gauge of process resident memory (RSS)
#[cfg(feature = "metrics")]
#[derive(Debug, Clone)]
pub struct NodeMetrics {
    // ── Existing metrics ───────────────────────────────────────────────
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

    // ── Throughput-specific metrics (Sprint 0) ─────────────────────────
    /// Counter for finalized transactions per second (incremented per tx).
    ///
    /// Combine with `rate()` in PromQL to get TPS:
    /// `rate(omnia_consensus_tps[1m])`
    pub consensus_tps: IntCounter,
    /// Histogram of consensus finality latency — time from event creation
    /// to finality commitment, in seconds.
    ///
    /// Buckets are chosen to capture sub-millisecond to multi-second ranges.
    pub consensus_finality_latency_seconds: Histogram,
    /// Histogram of gossip propagation latency — end-to-end event
    /// propagation delay across the network, in seconds.
    pub gossip_propagation_latency_seconds: Histogram,
    /// Counter for total events inserted into the DAG.
    pub dag_events_total: IntCounter,
    /// Histogram of DAG event insertion latency, in seconds.
    pub dag_insertion_latency_seconds: Histogram,
    /// Gauge for process resident memory (RSS) in bytes.
    ///
    /// On Linux, this reads from `/proc/self/status` VmRSS.
    /// On other platforms, reports 0.
    pub node_memory_rss_bytes: IntGauge,
}

#[cfg(feature = "metrics")]
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
                let peers_connected = IntGauge::new("omnia_node_peers_connected", "Current number of connected peers")
                    .expect("Failed to create peers_connected gauge");
                let consensus_round = IntGauge::new("omnia_node_consensus_round", "Current consensus round")
                    .expect("Failed to create consensus_round gauge");
                let shard_ops_total =
                    IntCounter::new("omnia_node_shard_operations_total", "Total shard operations processed")
                        .expect("Failed to create shard_ops_total counter");
                let http_requests_total =
                    IntCounter::new("omnia_node_http_requests_total", "Total HTTP requests served")
                        .expect("Failed to create http_requests_total counter");

                // ── Throughput-specific metrics (Sprint 0) ───────────────
                let consensus_tps = IntCounter::new(
                    "omnia_consensus_tps",
                    "Counter for finalized transactions (use rate() for TPS)",
                )
                .expect("Failed to create consensus_tps counter");
                let consensus_finality_latency_seconds = Histogram::with_opts(
                    prometheus::HistogramOpts::new(
                        "omnia_consensus_finality_latency_seconds",
                        "Time from event creation to finality commitment",
                    )
                    .buckets(vec![
                        0.000_1, 0.000_5, 0.001, 0.002, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
                    ]),
                )
                .expect("Failed to create consensus_finality_latency histogram");
                let gossip_propagation_latency_seconds = Histogram::with_opts(
                    prometheus::HistogramOpts::new(
                        "omnia_gossip_propagation_latency_seconds",
                        "End-to-end event propagation latency across the gossip network",
                    )
                    .buckets(vec![
                        0.000_1, 0.000_5, 0.001, 0.002, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
                    ]),
                )
                .expect("Failed to create gossip_propagation_latency histogram");
                let dag_events_total = IntCounter::new("omnia_dag_events_total", "Total events inserted into the DAG")
                    .expect("Failed to create dag_events_total counter");
                let dag_insertion_latency_seconds = Histogram::with_opts(
                    prometheus::HistogramOpts::new(
                        "omnia_dag_insertion_latency_seconds",
                        "Latency of DAG event insertion",
                    )
                    .buckets(vec![
                        0.000_01, 0.000_05, 0.000_1, 0.000_5, 0.001, 0.002, 0.005, 0.01, 0.025, 0.05, 0.1,
                    ]),
                )
                .expect("Failed to create dag_insertion_latency histogram");
                let node_memory_rss_bytes =
                    IntGauge::new("omnia_node_memory_rss_bytes", "Process resident memory (RSS) in bytes")
                        .expect("Failed to create node_memory_rss_bytes gauge");

                let registry = prometheus::default_registry();
                // Ignore AlreadyReg errors — metrics may have been registered
                // in a previous test or call
                let _ = registry.register(Box::new(events_submitted.clone()));
                let _ = registry.register(Box::new(events_finalized.clone()));
                let _ = registry.register(Box::new(peers_connected.clone()));
                let _ = registry.register(Box::new(consensus_round.clone()));
                let _ = registry.register(Box::new(shard_ops_total.clone()));
                let _ = registry.register(Box::new(http_requests_total.clone()));
                let _ = registry.register(Box::new(consensus_tps.clone()));
                let _ = registry.register(Box::new(consensus_finality_latency_seconds.clone()));
                let _ = registry.register(Box::new(gossip_propagation_latency_seconds.clone()));
                let _ = registry.register(Box::new(dag_events_total.clone()));
                let _ = registry.register(Box::new(dag_insertion_latency_seconds.clone()));
                let _ = registry.register(Box::new(node_memory_rss_bytes.clone()));

                Self {
                    events_submitted,
                    events_finalized,
                    peers_connected,
                    consensus_round,
                    shard_ops_total,
                    http_requests_total,
                    consensus_tps,
                    consensus_finality_latency_seconds,
                    gossip_propagation_latency_seconds,
                    dag_events_total,
                    dag_insertion_latency_seconds,
                    node_memory_rss_bytes,
                }
            })
            .clone();
        Ok(NODE_METRICS.get().expect("metrics just initialized").clone())
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
    ///
    /// Uses `std::sync::Mutex` instead of `tokio::sync::Mutex` because:
    /// - The same `ShardRouter` must be shared with the Substrate's
    ///   `EventProcessor` (which is a synchronous trait).
    /// - `ShardRouter` operations are CPU-only (no I/O), so the lock
    ///   is held for only a few microseconds, making `std::sync::Mutex`
    ///   both safe and more efficient than its tokio counterpart.
    pub shard_router: Arc<std::sync::Mutex<ShardRouter>>,
    /// Economics state — UBC token balances, governance, and quota tracking.
    pub economics: Arc<Mutex<EconomicsState>>,
    /// In-memory event store for API retrieval.
    ///
    /// Uses `IndexMap` for deterministic insertion-order eviction
    /// (see [`store_event`] and the `EventStore` type alias). H-5 fix.
    pub event_store: Arc<RwLock<EventStore>>,
    /// Known peers in the network.
    pub peers: Arc<RwLock<Vec<PeerInfo>>>,
    /// Prometheus metrics counters and gauges.
    #[cfg(feature = "metrics")]
    pub metrics: Arc<NodeMetrics>,
    /// Time when the node was started (for uptime calculation).
    pub started_at: Instant,
    /// Whether the node is currently in fast-sync mode.
    ///
    /// When `true`, the node is still catching up with the network
    /// and should not be considered ready to serve traffic.
    pub is_syncing: Arc<AtomicBool>,
    /// Persistent node keypair for signing events and shard operations.
    ///
    /// This keypair is loaded or generated at startup and used for all
    /// event signing operations. Using a persistent keypair ensures that
    /// events can be verified as originating from this node, unlike
    /// ephemeral keypairs which create a new key per request.
    ///
    /// If `None`, API endpoints that require signing will return
    /// `500 Internal Server Error`.
    pub keypair: Option<NodeKeypair>,
    /// Settlement adapter for L1 batch submissions.
    ///
    /// Uses `MockSettlementAdapter` by default (zero alloy, MSRV 1.88).
    /// When the `ethereum-live` feature is enabled and a valid config is
    /// provided, uses `EthereumSettlementAdapter` instead (requires rustc >= 1.91).
    pub settlement: Arc<dyn SettlementAdapter>,
    /// Optional ceremony server for multi-party trusted setup.
    ///
    /// When present, the ceremony HTTP API endpoints are functional.
    /// When absent, ceremony endpoints return 503 Service Unavailable.
    #[cfg(feature = "zk")]
    pub ceremony_server: Option<Arc<RwLock<CeremonyServer>>>,
}
