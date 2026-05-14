//! # Omnia Node Binary
//!
//! The `omnia-node` binary runs a full Omnia Protocol node with:
//!
//! - **Layer 1 Substrate** — causal graph consensus, slashing, and gossip
//! - **Layer 2 Shard Router** — domain-specific state machines with fee enforcement
//! - **Layer 5 Economics** — UBC token, governance, and useful-work rewards
//! - **REST API** — health checks, metrics, and full API surface
//!
//! # Startup Sequence
//!
//! 1. Parse CLI args (with env var overrides via `OMNIA_` prefix)
//! 2. Initialize structured logging (tracing)
//! 3. Create data directory if it doesn't exist
//! 4. Initialize substrate components (consensus, slashing, shard router)
//! 5. Start the HTTP server (axum)
//! 6. Wait for SIGINT/SIGTERM for graceful shutdown
//!
//! # Example
//!
//! ```sh
//! omnia-node --node-id 1 --http-port 8080 --log-level info
//! OMNIA_NODE_ID=2 OMNIA_HTTP_PORT=8081 omnia-node
//! ```

use anyhow::{Context, Result};
use omnia_economics::EconomicsState;
use omnia_node::state::{AppState, NodeMetrics};
use omnia_shards::{
    BiologicalShard, ComputationalShard, EconomicsShard, FeeSchedule, FinancialShard,
    IdentityShard, PhysicalShard, ShardRouter,
};
use omnia_substrate::{SlashingEngine, SledSlashingStore, Substrate, SubstrateConfig};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Mutex, RwLock};
use tracing_subscriber::EnvFilter;

use omnia_node::config::NodeConfig;

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Parse configuration from CLI / environment
    let config = NodeConfig::from_cli();
    config.validate().context("Invalid configuration")?;

    // 2. Initialize tracing with the configured log level
    init_tracing(&config.log_level);

    tracing::info!(
        node_id = config.node_id,
        http_port = config.http_port,
        listen_addr = %config.listen_addr,
        data_dir = %config.data_dir.display(),
        log_level = %config.log_level,
        "Starting Omnia node"
    );

    // 3. Create data directory
    let data_dir = config
        .ensure_data_dir()
        .context("Failed to initialize data directory")?;
    tracing::info!(data_dir = %data_dir.display(), "Data directory ready");

    // 4. Initialize substrate components
    let node_id_bytes = config.node_id_bytes();

    // Create the substrate runtime
    let substrate_config = SubstrateConfig::new(node_id_bytes);
    let substrate = Substrate::new(substrate_config);
    tracing::info!("Substrate runtime initialized");

    // Create the slashing engine with sled persistence
    let slashing_dir = config.slashing_dir();
    let slashing_engine = create_slashing_engine(&slashing_dir)?;
    tracing::info!(path = %slashing_dir.display(), "Slashing engine initialized with sled persistence");

    // Create the shard router with standard fees
    let shard_router = create_shard_router()?;
    tracing::info!(
        shard_count = 6,
        "Shard router initialized with all shard types"
    );

    // Create the economics state
    let economics = EconomicsState::new();
    tracing::info!("Economics state initialized (10% decay, 1000 UBC/month)");

    // Initialize Prometheus metrics
    let metrics = NodeMetrics::new().context("Failed to initialize Prometheus metrics")?;
    tracing::info!("Prometheus metrics initialized");

    // 5. Build shared application state
    let app_state = AppState {
        config: config.clone(),
        substrate: Arc::new(RwLock::new(substrate)),
        slashing: Arc::new(Mutex::new(slashing_engine)),
        shard_router: Arc::new(Mutex::new(shard_router)),
        economics: Arc::new(Mutex::new(economics)),
        event_store: Arc::new(RwLock::new(std::collections::HashMap::new())),
        peers: Arc::new(RwLock::new(Vec::new())),
        metrics: Arc::new(metrics),
        started_at: Instant::now(),
    };

    // 6. Build and start the HTTP server
    let app = omnia_node::http::build_http_router().with_state(app_state);
    let listen_addr = format!("0.0.0.0:{}", config.http_port);

    let listener = tokio::net::TcpListener::bind(&listen_addr)
        .await
        .with_context(|| format!("Failed to bind HTTP server to {}", listen_addr))?;

    tracing::info!(listen_addr = %listen_addr, "HTTP server starting");

    // 7. Wait for shutdown signal
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("HTTP server error")?;

    tracing::info!("Omnia node shut down gracefully");
    Ok(())
}

/// Initialize the tracing subscriber with the specified log level.
///
/// Supports structured JSON output when the `RUST_LOG_FORMAT=json`
/// environment variable is set; otherwise uses the standard
/// human-readable format.
fn init_tracing(log_level: &str) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level));

    let format = std::env::var("RUST_LOG_FORMAT").unwrap_or_default();
    if format == "json" {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .json()
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(true)
            .with_thread_ids(false)
            .init();
    }
}

/// Create a slashing engine backed by sled for persistent storage.
///
/// If the sled database cannot be opened, falls back to an
/// in-memory slashing engine so the node can still operate.
fn create_slashing_engine(slashing_dir: &std::path::Path) -> Result<SlashingEngine> {
    match SledSlashingStore::open(slashing_dir) {
        Ok(store) => {
            let engine = SlashingEngine::with_store(Box::new(store))
                .context("Failed to load slashing state from sled store")?;
            Ok(engine)
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                path = %slashing_dir.display(),
                "Failed to open sled slashing store — falling back to in-memory"
            );
            Ok(SlashingEngine::new(
                omnia_substrate::DEFAULT_SLASH_THRESHOLD,
                omnia_substrate::DEFAULT_EJECTION_THRESHOLD,
            ))
        }
    }
}

/// Create a shard router with all six shard types registered.
///
/// Uses the standard fee schedule for operation pricing.
fn create_shard_router() -> Result<ShardRouter> {
    let fee_schedule = FeeSchedule::standard();
    let quota = omnia_economics::QuotaSystem::default_system();

    let mut router = ShardRouter::new(fee_schedule, quota);

    // Register all six domain shards
    router.register(Box::new(FinancialShard::new()));
    router.register(Box::new(ComputationalShard::new()));
    router.register(Box::new(PhysicalShard::new()));
    router.register(Box::new(BiologicalShard::new()));
    router.register(Box::new(IdentityShard::new()));
    router.register(Box::new(EconomicsShard::new()));

    Ok(router)
}

/// Wait for SIGINT (Ctrl+C) or SIGTERM for graceful shutdown.
///
/// This function completes when either signal is received, allowing
/// the axum server to stop accepting new connections and finish
/// serving in-flight requests.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Received Ctrl+C — shutting down");
        }
        _ = terminate => {
            tracing::info!("Received SIGTERM — shutting down");
        }
    }
}
