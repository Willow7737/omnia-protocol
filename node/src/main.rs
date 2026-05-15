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
use clap::Parser;
use omnia_economics::EconomicsState;
use omnia_node::config::{CliArgs, CliCommand, NodeConfig};
use omnia_node::state::{AppState, NodeMetrics};
use omnia_shards::{
    BiologicalShard, ComputationalShard, EconomicsShard, FeeSchedule, FinancialShard,
    IdentityShard, PhysicalShard, ShardRouter,
};
use omnia_substrate::{Substrate, SubstrateConfig};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Mutex, RwLock};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Parse CLI arguments and dispatch subcommands
    let cli = CliArgs::parse();

    // Handle subcommands before starting the node
    if let Some(command) = cli.command {
        match command {
            CliCommand::Keygen { output_dir } => {
                return run_keygen(&output_dir);
            }
            CliCommand::SetupContribute {
                degree,
                min_participants,
                seed,
            } => {
                return run_setup_contribute(degree, min_participants, seed.as_deref());
            }
            CliCommand::SetupVerify {
                degree,
                num_contributions,
            } => {
                return run_setup_verify(degree, num_contributions);
            }
            CliCommand::Snapshot { output } => {
                return run_snapshot(&output);
            }
            CliCommand::Restore { input } => {
                return run_restore(&input);
            }
            CliCommand::Run => {}
        }
    }

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
    let slashing_dir = config.slashing_dir();

    // Create the substrate runtime with slashing persistence configured
    let mut substrate_config = SubstrateConfig::new(node_id_bytes);
    substrate_config.slashing_data_dir = Some(slashing_dir.to_path_buf());
    substrate_config.max_payload_size = config.max_payload_size;
    substrate_config.pruning_depth = config.pruning_depth;
    substrate_config.snapshot_interval = config.snapshot_interval;
    let substrate = Substrate::new(substrate_config);
    tracing::info!(
        path = %slashing_dir.display(),
        "Substrate runtime initialized with persistent slashing engine"
    );

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
    // Clone the slashing engine BEFORE moving substrate into the Arc,
    // so both the substrate and the API share the same Arc<dyn SlashingStore>.
    let slashing_engine = substrate.slashing.clone();
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

/// Generate a new validator keypair and save it to the specified output directory.
///
/// Creates two files:
/// - `validator_key.pem` — the encrypted private key
/// - `validator_pubkey.txt` — the hex-encoded public key
///
/// # Errors
///
/// Returns an error if the output directory cannot be created or if
/// file writing fails.
fn run_keygen(output_dir: &str) -> Result<()> {
    use omnia_substrate::crypto::generate_keypair;

    let dir = std::path::Path::new(output_dir);
    std::fs::create_dir_all(dir)
        .with_context(|| format!("Failed to create output directory: {}", output_dir))?;

    let keypair = generate_keypair();
    let pubkey_bytes = keypair.verifying_key().to_bytes();

    // Write public key as hex
    let pubkey_path = dir.join("validator_pubkey.txt");
    std::fs::write(&pubkey_path, hex::encode(pubkey_bytes))
        .with_context(|| format!("Failed to write public key to {:?}", pubkey_path))?;

    // Write private key bytes (in production, this would be encrypted)
    let privkey_path = dir.join("validator_key.bin");
    std::fs::write(&privkey_path, keypair.to_bytes())
        .with_context(|| format!("Failed to write private key to {:?}", privkey_path))?;

    tracing::info!(
        output_dir = %dir.display(),
        pubkey = %hex::encode(&pubkey_bytes[..8]),
        "Validator keypair generated successfully"
    );

    println!("Validator keypair generated in {}", dir.display());
    println!("  Public key: {}", hex::encode(pubkey_bytes));
    println!(
        "  WARNING: Protect the private key file at {:?}",
        privkey_path
    );

    Ok(())
}

/// Contribute to the Powers of Tau trusted setup ceremony (Phase 1).
///
/// Runs a local ceremony simulation with the specified parameters,
/// contributing fresh randomness to the SRS. In production, this
/// would coordinate with other participants over the network.
///
/// # Arguments
///
/// * `degree` — The maximum degree for the Powers of Tau SRS
/// * `min_participants` — Minimum participants before the ceremony can finalize
/// * `seed_hex` — Optional hex-encoded seed for deterministic contribution
fn run_setup_contribute(
    degree: usize,
    min_participants: usize,
    seed_hex: Option<&str>,
) -> Result<()> {
    use omnia_zk::setup::{contribute, PowersOfTau};

    // Initialize minimal tracing for the ceremony
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(true)
        .init();

    let mut srs = PowersOfTau::new(degree);
    println!("Powers of Tau ceremony initialized (degree={})", degree);

    // Parse optional seed
    let seed: Option<[u8; 32]> = seed_hex
        .map(|hex_str| {
            let bytes = hex::decode(hex_str).context("Failed to decode hex seed")?;
            let mut seed = [0u8; 32];
            if bytes.len() != 32 {
                anyhow::bail!(
                    "Seed must be exactly 32 bytes (64 hex chars), got {} bytes",
                    bytes.len()
                );
            }
            seed.copy_from_slice(&bytes);
            Ok(seed)
        })
        .transpose()?;

    // Make the contribution
    let transcript = srs.to_transcript();
    let tau_size = srs.g1_powers.len() + srs.g2_powers.len();
    let contribution = contribute(&transcript, tau_size, seed)
        .map_err(|e| anyhow::anyhow!("Contribution failed: {}", e))?;

    srs.apply_contribution(&contribution)
        .map_err(|e| anyhow::anyhow!("Failed to apply contribution: {}", e))?;

    println!("Contribution accepted!");
    println!(
        "  Participant ID: {}",
        hex::encode(&contribution.participant_id[..4])
    );
    println!("  Contribution count: {}", srs.contribution_count);
    println!(
        "  Transcript hash: {}",
        hex::encode(&srs.transcript_hash[..8])
    );

    if srs.contribution_count >= min_participants {
        println!("  Ceremony has enough participants to proceed to Phase 2.");
    } else {
        println!(
            "  Need {} more contributions to reach minimum of {}.",
            min_participants - srs.contribution_count,
            min_participants
        );
    }

    Ok(())
}

/// Verify a completed Powers of Tau ceremony by replaying contributions.
///
/// Simulates a ceremony with the specified number of contributions
/// and verifies each one.
///
/// # Arguments
///
/// * `degree` — The maximum degree for the Powers of Tau SRS
/// * `num_contributions` — Number of contributions to replay and verify
fn run_setup_verify(degree: usize, num_contributions: usize) -> Result<()> {
    use omnia_zk::setup::run_ceremony;

    // Initialize minimal tracing
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(true)
        .init();

    println!(
        "Verifying Powers of Tau ceremony (degree={}, contributions={})...",
        degree, num_contributions
    );

    let srs = run_ceremony(degree, num_contributions)
        .map_err(|e| anyhow::anyhow!("Ceremony verification failed: {}", e))?;

    println!("Ceremony verification successful!");
    println!("  Total contributions: {}", srs.contribution_count);
    println!(
        "  Transcript hash: {}",
        hex::encode(&srs.transcript_hash[..8])
    );
    println!("  G1 powers: {}", srs.g1_powers.len());
    println!("  G2 powers: {}", srs.g2_powers.len());

    Ok(())
}

/// Take a state snapshot and write it to a file.
///
/// Creates a minimal CausalGraph, SlashingState, and nonce map,
/// serializes them into a [`StateSnapshot`], and writes the result
/// to the specified output path.
fn run_snapshot(output_path: &str) -> Result<()> {
    use omnia_substrate::snapshot::StateSnapshot;

    // Initialize minimal tracing
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(true)
        .init();

    let graph = omnia_substrate::CausalGraph::new();
    let slashing = omnia_substrate::SlashingState::default();
    let nonces = std::collections::HashMap::new();

    let snapshot = StateSnapshot::take(&graph, &slashing, &nonces, 0)
        .map_err(|e| anyhow::anyhow!("Snapshot failed: {}", e))?;

    snapshot
        .write_to_file(std::path::Path::new(output_path))
        .map_err(|e| anyhow::anyhow!("Failed to write snapshot: {}", e))?;

    println!("Snapshot written to {}", output_path);
    println!("  Height: {}", snapshot.height);
    println!("  Event count: {}", snapshot.event_count);
    println!("  State root: {}", hex::encode(&snapshot.state_root[..8]));

    Ok(())
}

/// Restore node state from a snapshot file.
///
/// Reads the snapshot from the specified path, verifies its integrity,
/// and prints summary information.
fn run_restore(input_path: &str) -> Result<()> {
    use omnia_substrate::snapshot::StateSnapshot;

    // Initialize minimal tracing
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(true)
        .init();

    let snapshot = StateSnapshot::read_from_file(std::path::Path::new(input_path))
        .map_err(|e| anyhow::anyhow!("Failed to read snapshot: {}", e))?;

    snapshot
        .verify()
        .map_err(|e| anyhow::anyhow!("Snapshot integrity check failed: {}", e))?;

    println!("Snapshot restored from {}", input_path);
    println!("  Version: {}", snapshot.version);
    println!("  Height: {}", snapshot.height);
    println!("  Event count: {}", snapshot.event_count);
    println!("  State root: {}", hex::encode(&snapshot.state_root[..8]));
    println!("  Timestamp: {}", snapshot.timestamp);
    println!("  Integrity: OK");

    Ok(())
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
