#![allow(deprecated)]
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
use omnia_adapters::SettlementAdapter;
use omnia_economics::EconomicsState;
#[cfg(feature = "network")]
use omnia_network::{Multiaddr, OmniaNetwork};
use omnia_node::config::{CliArgs, CliCommand, NodeConfig};
use omnia_node::pipeline::{ColdWork, PipelineRouter};
use omnia_node::state::AppState;
#[cfg(feature = "metrics")]
use omnia_node::state::NodeMetrics;
use omnia_shards::{
    BiologicalShard, ComputationalShard, EconomicsShard, FeeSchedule, FinancialShard, IdentityShard, PhysicalShard,
    ShardRouter,
};
use omnia_substrate::{Substrate, SubstrateConfig};
use std::sync::atomic::AtomicBool;
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
            CliCommand::Keygen { output_dir, passphrase } => {
                return run_keygen(&output_dir, passphrase.as_deref());
            }
            #[cfg(feature = "zk")]
            CliCommand::SetupContribute {
                degree,
                min_participants,
                seed,
            } => {
                return run_setup_contribute(degree, min_participants, seed.as_deref());
            }
            #[cfg(feature = "zk")]
            CliCommand::SetupVerify {
                degree,
                num_contributions,
            } => {
                return run_setup_verify(degree, num_contributions);
            }
            #[cfg(not(feature = "zk"))]
            CliCommand::SetupContribute { .. } => {
                anyhow::bail!("ZK feature not enabled. Rebuild with --features zk");
            }
            #[cfg(not(feature = "zk"))]
            CliCommand::SetupVerify { .. } => {
                anyhow::bail!("ZK feature not enabled. Rebuild with --features zk");
            }
            CliCommand::Snapshot { output } => {
                return run_snapshot(&output);
            }
            CliCommand::Restore { input } => {
                return run_restore(&input);
            }
            CliCommand::Run => {}
            #[cfg(feature = "zk")]
            CliCommand::CeremonyServe {
                min_participants,
                max_participants,
                degree,
            } => {
                return run_ceremony_serve(min_participants, max_participants, degree);
            }
            #[cfg(feature = "zk")]
            CliCommand::CeremonyContribute { server_url, seed } => {
                return run_ceremony_contribute(&server_url, seed.as_deref()).await;
            }
            #[cfg(feature = "zk")]
            CliCommand::CeremonyVerify { server_url } => {
                return run_ceremony_verify(&server_url).await;
            }
            #[cfg(not(feature = "zk"))]
            CliCommand::CeremonyServe { .. } => {
                anyhow::bail!("ZK feature not enabled. Rebuild with --features zk");
            }
            #[cfg(not(feature = "zk"))]
            CliCommand::CeremonyContribute { .. } => {
                anyhow::bail!("ZK feature not enabled. Rebuild with --features zk");
            }
            #[cfg(not(feature = "zk"))]
            CliCommand::CeremonyVerify { .. } => {
                anyhow::bail!("ZK feature not enabled. Rebuild with --features zk");
            }
            CliCommand::GenesisInit { config, output } => {
                return run_genesis_init(&config, &output);
            }
            CliCommand::GenesisValidate { block } => {
                return run_genesis_validate(&block);
            }
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

    // 3b. Check for legacy sled database and migrate if present
    let sled_path = data_dir.join("sled");
    if sled_path.exists() {
        tracing::info!(
            sled_path = %sled_path.display(),
            "Detected legacy sled database, migrating to redb..."
        );
        let redb_path = data_dir.join("omnia.redb");
        omnia_substrate::migration::migrate_sled_to_redb(&sled_path, &redb_path)
            .context("Sled-to-redb migration failed")?;
        tracing::info!("Migration complete");
    }

    // 4. Initialize substrate components
    let node_id_bytes = config.node_id_bytes();
    let slashing_dir = config.slashing_dir();

    // Create the substrate runtime with slashing persistence configured
    let mut substrate_config = SubstrateConfig::new(node_id_bytes);
    substrate_config.slashing_data_dir = Some(slashing_dir.to_path_buf());
    substrate_config.max_payload_size = config.max_payload_size;
    substrate_config.pruning_depth = config.pruning_depth;
    substrate_config.snapshot_interval = config.snapshot_interval;
    substrate_config.nonce_data_dir = Some(config.nonce_dir());
    substrate_config.consensus_data_dir = Some(config.consensus_dir());
    let mut substrate = Substrate::new(substrate_config);

    // P0-1: Initialize the gossip protocol before wrapping substrate in Arc<RwLock.
    // This creates a GossipProtocol with a shared Arc<RwLock<CausalGraph>> that
    // will later be wired to the P2P network via start_with_network().
    #[cfg(feature = "network")]
    {
        substrate.init_gossip();
        tracing::info!("Gossip protocol initialized and wired to substrate");
    }
    tracing::info!(
        path = %slashing_dir.display(),
        "Substrate runtime initialized with persistent slashing engine"
    );

    // Create the shard router with standard fees and nonce persistence
    let shard_router = create_shard_router(Some(config.nonce_dir().as_path()))?;
    tracing::info!(shard_count = 6, "Shard router initialized with all shard types");

    // Create the economics state
    let economics = EconomicsState::new();
    tracing::info!("Economics state initialized (10% decay, 1000 UBC/month)");

    // Construct the settlement adapter based on enabled features.
    // By default, uses MockSettlementAdapter (zero alloy, compiles on MSRV 1.88).
    // When ethereum-live is enabled, uses EthereumSettlementAdapter (requires rustc >= 1.91).
    let settlement: Arc<dyn SettlementAdapter> = {
        #[cfg(feature = "ethereum-live")]
        {
            // Try to create a live Ethereum adapter from environment config.
            // Falls back to MockSettlementAdapter if config is missing or invalid.
            match create_ethereum_settlement_adapter() {
                Ok(adapter) => {
                    tracing::info!("Settlement: Ethereum live adapter (alloy-backed, requires rustc >= 1.91)");
                    adapter
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "Settlement: Falling back to MockSettlementAdapter — Ethereum live config invalid or missing"
                    );
                    Arc::new(omnia_adapters::MockSettlementAdapter::new())
                }
            }
        }
        #[cfg(not(feature = "ethereum-live"))]
        {
            tracing::info!("Settlement: Mock adapter (enable --features ethereum-live for live Ethereum)");
            Arc::new(omnia_adapters::MockSettlementAdapter::new())
        }
    };

    // Initialize Prometheus metrics
    #[cfg(feature = "metrics")]
    let metrics = NodeMetrics::new().context("Failed to initialize Prometheus metrics")?;
    #[cfg(feature = "metrics")]
    tracing::info!("Prometheus metrics initialized");

    // 5. Build shared application state
    // Clone the slashing engine BEFORE moving substrate into the Arc,
    // so both the substrate and the API share the same Arc<dyn SlashingStore>.
    let slashing_engine = substrate.slashing.clone();

    // Clone the substrate Arc for background tasks BEFORE wrapping in AppState
    let substrate_for_consensus = Arc::new(RwLock::new(substrate));

    // Initialize ceremony server when ZK feature is enabled
    #[cfg(feature = "zk")]
    let ceremony_server = {
        let ceremony_config = omnia_adapters::setup::CeremonyConfig::default();
        let server = omnia_adapters::setup::CeremonyServer::new(ceremony_config);
        if let Err(e) = server.start() {
            tracing::warn!(error = %e, "Failed to start ceremony server");
            None
        } else {
            tracing::info!("Ceremony server initialized");
            Some(Arc::new(RwLock::new(server)))
        }
    };

    let app_state = AppState {
        config: config.clone(),
        substrate: Arc::clone(&substrate_for_consensus),
        slashing: Arc::new(Mutex::new(slashing_engine)),
        shard_router: Arc::new(Mutex::new(shard_router)),
        economics: Arc::new(Mutex::new(economics)),
        event_store: Arc::new(RwLock::new(std::collections::HashMap::new())),
        peers: Arc::new(RwLock::new(Vec::new())),
        #[cfg(feature = "metrics")]
        metrics: Arc::new(metrics),
        started_at: Instant::now(),
        is_syncing: Arc::new(AtomicBool::new(false)),
        settlement,
        #[cfg(feature = "zk")]
        ceremony_server,
    };

    // 6. Build and start the HTTP server
    let app = omnia_node::http::build_http_router().with_state(app_state.clone());
    let listen_addr = format!("0.0.0.0:{}", config.http_port);

    let listener = tokio::net::TcpListener::bind(&listen_addr)
        .await
        .with_context(|| format!("Failed to bind HTTP server to {listen_addr}"))?;

    tracing::info!(listen_addr = %listen_addr, "HTTP server starting");

    // 7. Spawn background tasks for consensus loop and pipeline router
    // These are the critical integration pieces that enable the node to
    // participate in the network autonomously.
    let shutdown_tx = spawn_background_tasks(config.clone(), substrate_for_consensus, app_state.clone()).await?;

    // 8. Start the HTTP server with graceful shutdown
    let server_future = axum::serve(listener, app).with_graceful_shutdown(shutdown_signal());

    server_future.await.context("HTTP server error")?;

    // Signal background tasks to shut down
    let _ = shutdown_tx.send(());
    tracing::info!("Omnia node shut down gracefully");
    Ok(())
}

/// Spawn background tasks for consensus loop, pipeline router, and P2P networking.
///
/// This function wires the core protocol components that enable the node to
/// run autonomously: the consensus loop (periodic round processing), the
/// pipeline router (hot/warm/cold path workers), and optionally P2P networking.
///
/// Returns a shutdown sender that can be used to signal all tasks to stop.
async fn spawn_background_tasks(
    config: NodeConfig,
    substrate: Arc<RwLock<Substrate>>,
    _app_state: AppState,
) -> Result<tokio::sync::broadcast::Sender<()>> {
    let (shutdown_tx, _shutdown_rx) = tokio::sync::broadcast::channel(1);

    // 7a. Spawn pipeline router with worker tasks
    let (pipeline, mut hot_rx, mut warm_rx, mut cold_rx) = PipelineRouter::new();
    let pipeline = Arc::new(pipeline);
    let _pipeline_clone = Arc::clone(&pipeline); // Reserved for future AppState wiring

    // Hot path worker — event validation and graph insertion
    let mut shutdown_hot = shutdown_tx.subscribe();
    tokio::spawn(async move {
        tracing::info!("Pipeline hot-path worker started");
        loop {
            tokio::select! {
                _ = shutdown_hot.recv() => {
                    tracing::info!("Hot-path worker shutting down");
                    break;
                }
                Some(work) = hot_rx.recv() => {
                    tracing::trace!(event_len = work.event_bytes.len(), "Hot path: processing event validation");
                }
                else => {
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                }
            }
        }
    });

    // Warm path worker — consensus processing, mempool, shard routing
    let mut shutdown_warm = shutdown_tx.subscribe();
    tokio::spawn(async move {
        tracing::info!("Pipeline warm-path worker started");
        loop {
            tokio::select! {
                _ = shutdown_warm.recv() => {
                    tracing::info!("Warm-path worker shutting down");
                    break;
                }
                Some(work) = warm_rx.recv() => {
                    tracing::trace!(event_id = ?&work.event_id[..4], "Warm path: processing consensus/shards");
                }
                else => {
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                }
            }
        }
    });

    // Cold path worker — ZK proofs, snapshots, settlement
    let mut shutdown_cold = shutdown_tx.subscribe();
    tokio::spawn(async move {
        tracing::info!("Pipeline cold-path worker started");
        loop {
            tokio::select! {
                _ = shutdown_cold.recv() => {
                    tracing::info!("Cold-path worker shutting down");
                    break;
                }
                Some(work) = cold_rx.recv() => {
                    match &work {
                        ColdWork::GenerateProof { event_ids } => {
                            tracing::info!(count = event_ids.len(), "Cold path: generating ZK proof");
                        }
                        ColdWork::SnapshotReplication { round } => {
                            tracing::info!(round, "Cold path: snapshot replication");
                        }
                        ColdWork::SettlementSubmit { batch_data } => {
                            tracing::info!(size = batch_data.len(), "Cold path: settlement submission");
                        }
                    }
                }
                else => {
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            }
        }
    });

    tracing::info!("Pipeline router initialized with hot/warm/cold workers");

    // 7b. Spawn the consensus background loop
    // This periodically calls check_round_timeout() and processes consensus
    let mut shutdown_consensus = shutdown_tx.subscribe();
    let substrate_consensus = Arc::clone(&substrate);
    tokio::spawn(async move {
        tracing::info!("Consensus background loop started");

        // Round timer: fires at the consensus round interval (default 1 second)
        let round_duration = tokio::time::Duration::from_millis(1000);
        let mut round_timer = tokio::time::interval(round_duration);

        loop {
            tokio::select! {
                _ = shutdown_consensus.recv() => {
                    tracing::info!("Consensus loop shutting down");
                    break;
                }
                _ = round_timer.tick() => {
                    // P0-1: Use process_consensus_round() which drains gossip
                    // events from the network, runs consensus, and feeds
                    // committed events to the shard processor. This replaces
                    // the previous process_consensus() call that skipped the
                    // gossip event drain step.
                    let mut substrate = substrate_consensus.write().await;
                    substrate.process_consensus_round().await;
                    drop(substrate);
                }
            }
        }
    });

    tracing::info!("Consensus background loop spawned (1s interval)");

    // 7c. Spawn P2P network initialization (when network feature is enabled)
    // P0-1: Wire OmniaNetwork → GossipProtocol → CausalGraph → Consensus
    // Instead of manually draining event_rx and only logging, we now delegate
    // to GossipProtocol::start_with_network() which wires the full pipeline:
    //   network.event_rx → gossip.network_rx → process_pending_events() →
    //   graph.insert() → unprocessed_events → process_consensus()
    #[cfg(feature = "network")]
    {
        let mut shutdown_network = shutdown_tx.subscribe();
        let listen_addr = config.listen_addr.clone();

        // We need a clone of the substrate Arc for wiring the network
        let substrate_for_network = Arc::clone(&substrate);

        tokio::spawn(async move {
            tracing::info!("P2P network initialization started");

            // Parse listen address into a Multiaddr for libp2p
            let listen_multiaddr: Multiaddr = match listen_addr.parse() {
                Ok(addr) => addr,
                Err(e) => {
                    tracing::error!(error = %e, addr = %listen_addr, "Failed to parse listen address as Multiaddr");
                    return;
                }
            };

            // Try to create the network, but don't block if it fails
            match OmniaNetwork::new(listen_multiaddr).await {
                Ok(mut network) => {
                    tracing::info!("P2P network initialized");

                    // Subscribe to the omnia_events gossip topic before
                    // starting the network run loop
                    if let Err(e) = network.subscribe("omnia_events") {
                        tracing::warn!(error = %e, "Failed to subscribe to omnia_events topic");
                    }

                    // Wire the network into the substrate's gossip protocol.
                    // This takes ownership of the network, spawns network.run_with_commands()
                    // internally, and stores event_rx in gossip.network_rx so that
                    // process_pending_events() can drain incoming events.
                    let mut substrate = substrate_for_network.write().await;
                    match substrate.wire_network(network).await {
                        Ok(()) => {
                            tracing::info!(
                                "P2P network wired to gossip protocol — events flow: \
                                 network → gossip → graph → consensus"
                            );
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "Failed to wire network to gossip protocol");
                        }
                    }
                    drop(substrate);

                    // Wait for shutdown signal — the network run loop is already
                    // spawned inside GossipProtocol::start_with_network()
                    let _ = shutdown_network.recv().await;
                    tracing::info!("P2P network task shutting down");
                }
                Err(e) => {
                    tracing::warn!(error = %e, "P2P network initialization failed — node running without P2P");
                }
            }
        });

        tracing::info!("P2P network task spawned");
    }

    #[cfg(not(feature = "network"))]
    {
        tracing::info!("P2P networking disabled (compile with --features network to enable)");
    }

    // Return shutdown sender so the main function can signal shutdown
    Ok(shutdown_tx)
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
        tracing_subscriber::fmt().with_env_filter(filter).json().init();
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
/// Uses the standard fee schedule for operation pricing. When `nonce_data_dir`
/// is `Some`, creates a `RedbNonceStore` for persistent replay protection;
/// otherwise falls back to in-memory nonce tracking.
fn create_shard_router(nonce_data_dir: Option<&std::path::Path>) -> Result<ShardRouter> {
    let fee_schedule = FeeSchedule::standard();
    let quota = omnia_economics::QuotaSystem::default_system();

    let mut router = match nonce_data_dir {
        Some(db_path) => {
            // Ensure the parent directory exists
            if let Some(parent) = db_path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create nonce directory: {}", parent.display()))?;
            }
            let nonce_store: Arc<dyn omnia_shards::NonceStore> = Arc::new(
                omnia_shards::RedbNonceStore::open(db_path)
                    .with_context(|| "Failed to open nonce store in redb database")?,
            );
            tracing::info!(path = %db_path.display(), "Shard router using persistent nonce store");
            ShardRouter::with_nonce_store(fee_schedule, quota, nonce_store)
        }
        None => {
            tracing::info!("Shard router using in-memory nonce store (no persistence)");
            ShardRouter::new(fee_schedule, quota)
        }
    };

    // Register all six domain shards
    router.register(Box::new(FinancialShard::new()));
    router.register(Box::new(ComputationalShard::new()));
    router.register(Box::new(PhysicalShard::new()));
    router.register(Box::new(BiologicalShard::new()));
    router.register(Box::new(IdentityShard::new()));
    router.register(Box::new(EconomicsShard::new()));

    Ok(router)
}

/// Magic bytes identifying an encrypted Omnia key file.
///
/// Format: `OMNIAKEY01` (10 bytes) + nonce (12 bytes) + ciphertext+tag (80 bytes) = 102 bytes total.
const ENCRYPTED_KEY_MAGIC: &[u8; 10] = b"OMNIAKEY01";

/// Domain separation tag for BLAKE3 key derivation from passphrase.
const KEY_DERIVATION_CONTEXT: &str = "omnia-keygen-aes256gcm";

/// Generate a new validator keypair and save it to the specified output directory.
///
/// When a `passphrase` is provided, the private key is encrypted with
/// AES-256-GCM and saved as `validator_key.enc`. The key is derived from
/// the passphrase using BLAKE3 with domain separation.
///
/// When no passphrase is provided, the private key is written as raw bytes
/// to `validator_key.bin` with a prominent warning.
///
/// # Files Created
///
/// - `validator_pubkey.txt` — the hex-encoded public key
/// - `validator_key.enc` (encrypted) or `validator_key.bin` (unencrypted) — the private key
///
/// # Encrypted File Format
///
/// ```text
/// [OMNIAKEY01 (10B)] [nonce (12B)] [ciphertext+tag (80B)]
/// ```
///
/// # Errors
///
/// Returns an error if the output directory cannot be created, if
/// encryption fails, or if file writing fails.
fn run_keygen(output_dir: &str, passphrase: Option<&str>) -> Result<()> {
    use aes_gcm::aead::Aead;
    use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
    use omnia_substrate::crypto::generate_keypair;

    let dir = std::path::Path::new(output_dir);
    std::fs::create_dir_all(dir).with_context(|| format!("Failed to create output directory: {output_dir}"))?;

    let keypair = generate_keypair();
    let pubkey_bytes = keypair.verifying_key().to_bytes();

    // Write public key as hex
    let pubkey_path = dir.join("validator_pubkey.txt");
    std::fs::write(&pubkey_path, hex::encode(pubkey_bytes))
        .with_context(|| format!("Failed to write public key to {pubkey_path:?}"))?;

    let privkey_bytes = keypair.to_bytes();

    if let Some(pass) = passphrase {
        // ---- Encrypted path ----
        // Derive a 256-bit key from the passphrase using BLAKE3 with domain separation
        let derived_key = blake3::derive_key(KEY_DERIVATION_CONTEXT, pass.as_bytes());
        let cipher_key = aes_gcm::Key::<Aes256Gcm>::from_slice(&derived_key);
        let cipher = Aes256Gcm::new(cipher_key);

        // Generate a random 96-bit (12-byte) nonce
        let nonce_bytes: [u8; 12] = rand::random();
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Encrypt: ciphertext includes the 16-byte GCM authentication tag appended
        let ciphertext = cipher
            .encrypt(nonce, privkey_bytes.as_slice())
            .map_err(|e| anyhow::anyhow!("AES-256-GCM encryption failed: {e}"))?;

        // Build the file: magic + nonce + ciphertext+tag
        let mut file_bytes = Vec::with_capacity(10 + 12 + ciphertext.len());
        file_bytes.extend_from_slice(ENCRYPTED_KEY_MAGIC);
        file_bytes.extend_from_slice(&nonce_bytes);
        file_bytes.extend_from_slice(&ciphertext);

        let privkey_path = dir.join("validator_key.enc");
        std::fs::write(&privkey_path, &file_bytes)
            .with_context(|| format!("Failed to write encrypted private key to {privkey_path:?}"))?;

        // Set file permissions to 0600 (owner read/write only) on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&privkey_path, perms)
                .with_context(|| format!("Failed to set permissions on {privkey_path:?}"))?;
        }

        tracing::info!(
            output_dir = %dir.display(),
            pubkey = %hex::encode(&pubkey_bytes[..8]),
            encrypted = true,
            "Validator keypair generated and encrypted successfully"
        );

        println!("Validator keypair generated in {}", dir.display());
        println!("  Public key: {}", hex::encode(pubkey_bytes));
        println!("  Private key: {privkey_path:?} (AES-256-GCM encrypted)");
    } else {
        // ---- Unencrypted path (NOT recommended for production) ----
        let privkey_path = dir.join("validator_key.bin");
        std::fs::write(&privkey_path, privkey_bytes)
            .with_context(|| format!("Failed to write private key to {privkey_path:?}"))?;

        // Set file permissions to 0600 (owner read/write only) on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&privkey_path, perms)
                .with_context(|| format!("Failed to set permissions on {privkey_path:?}"))?;
        }

        tracing::warn!(
            output_dir = %dir.display(),
            "Private key written WITHOUT encryption — provide --passphrase for production use"
        );

        println!("Validator keypair generated in {}", dir.display());
        println!("  Public key: {}", hex::encode(pubkey_bytes));
        println!("╔══════════════════════════════════════════════════════════════╗");
        println!("║  ⚠  WARNING: Private key is stored UNENCRYPTED!            ║");
        println!("║     Provide --passphrase or set OMNIA_KEYGEN_PASSPHRASE    ║");
        println!("║     to encrypt the key with AES-256-GCM.                  ║");
        println!("║     Unencrypted keys are NOT suitable for production.     ║");
        println!("╚══════════════════════════════════════════════════════════════╝");
        println!("  Private key: {privkey_path:?}");
    }

    Ok(())
}

/// Load and decrypt an encrypted validator private key from disk.
///
/// Reads the file at `path`, validates the `OMNIAKEY01` magic header,
/// extracts the nonce and ciphertext, decrypts with AES-256-GCM using
/// the key derived from `passphrase` via BLAKE3, and returns the raw
/// 64-byte Ed25519 keypair bytes.
///
/// # Expected File Format
///
/// ```text
/// [OMNIAKEY01 (10B)] [nonce (12B)] [ciphertext+tag (80B)]
/// ```
///
/// # Errors
///
/// Returns an error if:
/// - The file cannot be read
/// - The magic header does not match `OMNIAKEY01`
/// - The file is too short to contain the header, nonce, and tag
/// - Decryption fails (wrong passphrase, corrupted data, or tampering)
pub fn load_encrypted_key(path: &std::path::Path, passphrase: &str) -> Result<[u8; 64]> {
    use aes_gcm::aead::Aead;
    use aes_gcm::{Aes256Gcm, KeyInit, Nonce};

    let file_bytes = std::fs::read(path).with_context(|| format!("Failed to read key file: {path:?}"))?;

    // Validate minimum length: magic(10) + nonce(12) + tag(16) = 38 bytes minimum
    // (ciphertext must be at least 0 bytes + 16-byte tag, but Ed25519 keypair is 64 bytes,
    //  so the real minimum is 10 + 12 + 64 + 16 = 102)
    if file_bytes.len() < 38 {
        anyhow::bail!(
            "Key file too short ({} bytes), expected at least 38 bytes",
            file_bytes.len()
        );
    }

    // Validate magic header
    if &file_bytes[..10] != ENCRYPTED_KEY_MAGIC {
        anyhow::bail!(
            "Invalid key file magic: expected {:?}, got {:?}",
            std::str::from_utf8(ENCRYPTED_KEY_MAGIC),
            std::str::from_utf8(&file_bytes[..10]).unwrap_or("<invalid UTF-8>")
        );
    }

    let nonce_bytes = &file_bytes[10..22];
    let ciphertext = &file_bytes[22..];

    if ciphertext.len() < 16 {
        anyhow::bail!(
            "Ciphertext too short ({} bytes), must include 16-byte GCM tag",
            ciphertext.len()
        );
    }

    // Derive the same key from the passphrase
    let derived_key = blake3::derive_key(KEY_DERIVATION_CONTEXT, passphrase.as_bytes());
    let cipher_key = aes_gcm::Key::<Aes256Gcm>::from_slice(&derived_key);
    let cipher = Aes256Gcm::new(cipher_key);
    let nonce = Nonce::from_slice(nonce_bytes);

    // Decrypt — this also verifies the authentication tag
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| anyhow::anyhow!("Decryption failed: wrong passphrase or corrupted file"))?;

    if plaintext.len() != 64 {
        anyhow::bail!(
            "Decrypted key has unexpected length: expected 64 bytes, got {}",
            plaintext.len()
        );
    }

    let mut key_bytes = [0u8; 64];
    key_bytes.copy_from_slice(&plaintext);
    Ok(key_bytes)
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
#[cfg(feature = "zk")]
fn run_setup_contribute(degree: usize, min_participants: usize, seed_hex: Option<&str>) -> Result<()> {
    use omnia_adapters::setup::{contribute, PowersOfTau};

    // Initialize minimal tracing for the ceremony
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(true)
        .init();

    let mut srs = PowersOfTau::new(degree).context("Failed to initialize Powers of Tau")?;
    println!("Powers of Tau ceremony initialized (degree={degree})");

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
    let contribution =
        contribute(&transcript, tau_size, seed).map_err(|e| anyhow::anyhow!("Contribution failed: {e}"))?;

    srs.apply_contribution(&contribution)
        .map_err(|e| anyhow::anyhow!("Failed to apply contribution: {e}"))?;

    println!("Contribution accepted!");
    println!("  Participant ID: {}", hex::encode(&contribution.participant_id[..4]));
    println!("  Contribution count: {}", srs.contribution_count);
    println!("  Transcript hash: {}", hex::encode(&srs.transcript_hash[..8]));

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
#[cfg(feature = "zk")]
fn run_setup_verify(degree: usize, num_contributions: usize) -> Result<()> {
    use omnia_adapters::setup::run_ceremony;

    // Initialize minimal tracing
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(true)
        .init();

    println!("Verifying Powers of Tau ceremony (degree={degree}, contributions={num_contributions})...");

    let srs =
        run_ceremony(degree, num_contributions).map_err(|e| anyhow::anyhow!("Ceremony verification failed: {e}"))?;

    println!("Ceremony verification successful!");
    println!("  Total contributions: {}", srs.contribution_count);
    println!("  Transcript hash: {}", hex::encode(&srs.transcript_hash[..8]));
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

    let snapshot =
        StateSnapshot::take(&graph, &slashing, &nonces, 0).map_err(|e| anyhow::anyhow!("Snapshot failed: {e}"))?;

    snapshot
        .write_to_file(std::path::Path::new(output_path))
        .map_err(|e| anyhow::anyhow!("Failed to write snapshot: {e}"))?;

    println!("Snapshot written to {output_path}");
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
        .map_err(|e| anyhow::anyhow!("Failed to read snapshot: {e}"))?;

    snapshot
        .verify()
        .map_err(|e| anyhow::anyhow!("Snapshot integrity check failed: {e}"))?;

    println!("Snapshot restored from {input_path}");
    println!("  Version: {}", snapshot.version);
    println!("  Height: {}", snapshot.height);
    println!("  Event count: {}", snapshot.event_count);
    println!("  State root: {}", hex::encode(&snapshot.state_root[..8]));
    println!("  Timestamp: {}", snapshot.timestamp);
    println!("  Integrity: OK");

    Ok(())
}

/// Generate a genesis block from a TOML configuration file.
///
/// Reads the genesis configuration, validates it (minimum 3 validators,
/// unique node IDs, non-zero stakes), generates a deterministic genesis
/// block, and writes it to the output path.
fn run_genesis_init(config_path: &str, output_path: &str) -> Result<()> {
    use omnia_substrate::genesis::{generate_genesis, GenesisConfig};

    // Initialize minimal tracing
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(true)
        .init();

    println!("Loading genesis configuration from {config_path}");

    let config_content = std::fs::read_to_string(config_path)
        .with_context(|| format!("Failed to read genesis config: {config_path}"))?;

    let genesis_config: GenesisConfig =
        toml::from_str(&config_content).with_context(|| "Failed to parse genesis configuration TOML")?;

    println!("  Chain ID: {}", genesis_config.chain_id);
    println!("  Network: {}", genesis_config.network_name);
    println!("  Validators: {}", genesis_config.initial_validators.len());

    let genesis_block =
        generate_genesis(&genesis_config).map_err(|e| anyhow::anyhow!("Genesis generation failed: {e}"))?;

    println!("\nGenesis block generated successfully!");
    println!("  State root: {}", hex::encode(&genesis_block.state_root[..8]));
    println!("  Hash: {}", hex::encode(&genesis_block.hash[..8]));
    println!("  Validators: {}", genesis_block.validators.len());

    // Serialize and write
    let bytes = postcard::to_allocvec(&genesis_block).map_err(|e| anyhow::anyhow!("Serialization failed: {e}"))?;
    std::fs::write(output_path, &bytes).with_context(|| format!("Failed to write genesis block to {output_path}"))?;

    println!("\nGenesis block written to {output_path}");
    println!("  Size: {} bytes", bytes.len());

    Ok(())
}

/// Validate a genesis block file.
///
/// Reads the genesis block, verifies its integrity by re-deriving the
/// expected hash, and prints summary information.
fn run_genesis_validate(block_path: &str) -> Result<()> {
    use omnia_substrate::genesis::{validate_genesis, GenesisBlock};

    // Initialize minimal tracing
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(true)
        .init();

    println!("Loading genesis block from {block_path}");

    let bytes = std::fs::read(block_path).with_context(|| format!("Failed to read genesis block: {block_path}"))?;

    let genesis_block: GenesisBlock =
        postcard::from_bytes(&bytes).map_err(|e| anyhow::anyhow!("Deserialization failed: {e}"))?;

    println!("  Chain ID: {}", genesis_block.chain_id);
    println!("  Validators: {}", genesis_block.validators.len());
    println!("  Hash: {}", hex::encode(&genesis_block.hash[..8]));
    println!("  State root: {}", hex::encode(&genesis_block.state_root[..8]));

    validate_genesis(&genesis_block).map_err(|e| anyhow::anyhow!("Genesis validation failed: {e}"))?;

    println!("\nGenesis block is VALID");
    Ok(())
}

/// Create a live Ethereum settlement adapter from environment configuration.
///
/// Reads the following environment variables:
/// - `OMNIA_ETH_RPC_URL`: Ethereum JSON-RPC endpoint (required)
/// - `OMNIA_ETH_CONTRACT_ADDRESS`: OmniaRollup contract address (required)
/// - `OMNIA_ETH_OPERATOR_KEY`: Operator private key (required)
/// - `OMNIA_ETH_GAS_LIMIT`: Gas limit for transactions (optional, default 1M)
/// - `OMNIA_ETH_CONFIRMATION_BLOCKS`: Blocks to wait for finality (optional, default 3)
///
/// Returns an error if any required variable is missing or invalid.
#[cfg(feature = "ethereum-live")]
fn create_ethereum_settlement_adapter() -> Result<Arc<dyn SettlementAdapter>> {
    use omnia_adapters::{EthereumConfig, EthereumSettlementAdapter};

    let rpc_url = std::env::var("OMNIA_ETH_RPC_URL").context("OMNIA_ETH_RPC_URL environment variable not set")?;
    let contract_address = std::env::var("OMNIA_ETH_CONTRACT_ADDRESS")
        .context("OMNIA_ETH_CONTRACT_ADDRESS environment variable not set")?;
    let operator_key =
        std::env::var("OMNIA_ETH_OPERATOR_KEY").context("OMNIA_ETH_OPERATOR_KEY environment variable not set")?;

    let gas_limit = std::env::var("OMNIA_ETH_GAS_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1_000_000);
    let confirmation_blocks = std::env::var("OMNIA_ETH_CONFIRMATION_BLOCKS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3);

    let config = EthereumConfig {
        rpc_url,
        contract_address,
        operator_private_key: operator_key,
        gas_limit,
        max_fee_per_gas: None,
        confirmation_blocks,
    };

    let adapter = EthereumSettlementAdapter::new(config)
        .map_err(|e| anyhow::anyhow!("Failed to create Ethereum settlement adapter: {}", e))?;

    Ok(Arc::new(adapter))
}

/// Wait for SIGINT (Ctrl+C) or SIGTERM for graceful shutdown.
///
/// This function completes when either signal is received, allowing
/// the axum server to stop accepting new connections and finish
/// serving in-flight requests.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.expect("Failed to install Ctrl+C handler");
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

/// Start a multi-party trusted setup ceremony server.
///
/// Creates a [`CeremonyServer`] and runs it with an HTTP endpoint
/// for accepting contributions. The ceremony proceeds through
/// three phases:
/// 1. `NotStarted` → `AcceptingContributions` (on `start()`)
/// 2. Participants contribute via HTTP API
/// 3. `AcceptingContributions` → `Finalized` (on `finalize()`)
///
/// For now, this runs a local ceremony simulation that accepts
/// contributions from the command line. The full network ceremony
/// server with HTTP endpoints will be implemented in a follow-up.
#[cfg(feature = "zk")]
fn run_ceremony_serve(min_participants: usize, max_participants: usize, degree: usize) -> Result<()> {
    use omnia_adapters::setup::{contribute, CeremonyConfig, CeremonyServer};

    // Initialize minimal tracing
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(true)
        .init();

    let config = CeremonyConfig {
        min_participants,
        max_participants,
        ceremony_id: 1,
        degree,
    };

    let server = CeremonyServer::new(config);
    server.start().context("Failed to start ceremony")?;

    println!("Ceremony server started (degree={degree})");
    println!("  Min participants: {min_participants} / Max participants: {max_participants}");

    // Simulate contributions (in a real server, these would come over HTTP)
    println!("\nSimulating {min_participants} participant contributions...");
    for i in 0..min_participants {
        let (transcript, tau_size) = server.get_srs_state().context("Failed to get SRS state")?;
        let mut seed = [0u8; 32];
        seed[0] = i as u8;
        seed[1] = (i >> 8) as u8;
        let contribution = contribute(&transcript, tau_size, Some(seed))
            .map_err(|e| anyhow::anyhow!("Contribution {i} failed: {e}"))?;
        let receipt = server
            .accept_contribution(contribution)
            .map_err(|e| anyhow::anyhow!("Accept contribution {i} failed: {e}"))?;
        println!("  Contribution {i} accepted (index={})", receipt.contribution_index);
    }

    // Finalize
    let circuit = omnia_adapters::circuit::RollupCircuit::empty();
    let key_pair = server
        .finalize(&circuit)
        .map_err(|e| anyhow::anyhow!("Finalize failed: {e}"))?;

    println!("\nCeremony finalized!");
    println!("  Total contributions: {}", server.contribution_count());
    println!("  Proving key size: {} bytes", key_pair.proving_key.len());
    println!("  Verifying key size: {} bytes", key_pair.verifying_key.len());
    println!("  Transcript hash: {}", hex::encode(&key_pair.tau_hash[..8]));

    // Export and display transcript info
    let transcript = server
        .export_transcript()
        .map_err(|e| anyhow::anyhow!("Export transcript failed: {e}"))?;
    println!("\nTranscript exported:");
    println!("  Contributions: {}", transcript.contribution_count);
    println!("  Final hash: {}", hex::encode(&transcript.final_transcript_hash[..8]));

    Ok(())
}

/// Contribute to a remote ceremony server.
///
/// Connects to the ceremony server at `server_url`, fetches the
/// current SRS state, generates a contribution locally, and
/// submits it to the server.
///
/// # API Contract
///
/// - `GET {server_url}/ceremony/state` → `{ "transcript": [...], "tau_size": N }`
/// - `POST {server_url}/ceremony/contribute` → `ContributionReceipt` (JSON)
#[cfg(feature = "zk")]
async fn run_ceremony_contribute(server_url: &str, seed_hex: Option<&str>) -> Result<()> {
    use omnia_adapters::setup::ceremony_server::ContributionReceipt;
    use omnia_adapters::setup::CeremonyClient;

    // Initialize minimal tracing
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(true)
        .init();

    println!("Connecting to ceremony server at {server_url}...");

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

    // Build HTTP client
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .context("Failed to build HTTP client")?;

    // 1. Fetch current ceremony state
    println!("Fetching current ceremony state...");
    let state_url = format!("{server_url}/ceremony/state");
    let state_resp = client
        .get(&state_url)
        .send()
        .await
        .with_context(|| format!("Failed to connect to ceremony server at {state_url}"))?;

    if !state_resp.status().is_success() {
        let status = state_resp.status();
        let body = state_resp.text().await.unwrap_or_default();
        anyhow::bail!("Ceremony server returned error {status} when fetching state: {body}");
    }

    let state: CeremonyStateResponse = state_resp
        .json()
        .await
        .context("Failed to deserialize ceremony state response")?;

    println!(
        "  Received state: transcript {} bytes, tau_size = {}",
        state.transcript.len(),
        state.tau_size
    );

    // 2. Generate contribution locally
    println!("Generating contribution...");
    let (contribution, _proof) = CeremonyClient::generate_contribution(&state.transcript, state.tau_size, seed)
        .map_err(|e| anyhow::anyhow!("Failed to generate contribution: {e}"))?;

    println!(
        "  Contribution generated: participant_id = {}",
        hex::encode(&contribution.participant_id[..8])
    );

    // 3. Submit contribution to server
    println!("Submitting contribution to server...");
    let contribute_url = format!("{server_url}/ceremony/contribute");
    let contribute_resp = client
        .post(&contribute_url)
        .json(&contribution)
        .send()
        .await
        .with_context(|| format!("Failed to submit contribution to {contribute_url}"))?;

    if !contribute_resp.status().is_success() {
        let status = contribute_resp.status();
        let body = contribute_resp.text().await.unwrap_or_default();
        anyhow::bail!("Ceremony server rejected contribution (HTTP {status}): {body}");
    }

    let receipt: ContributionReceipt = contribute_resp
        .json()
        .await
        .context("Failed to deserialize contribution receipt")?;

    println!("\n✓ Contribution accepted!");
    println!("  Contribution index: {}", receipt.contribution_index);
    println!("  Transcript hash: {}", hex::encode(&receipt.transcript_hash[..8]));
    println!("  Proof commitment: {}", hex::encode(&receipt.proof.commitment[..8]));

    Ok(())
}

/// Verify a ceremony transcript from a remote server.
///
/// Downloads the full transcript and independently verifies each
/// contribution's Proof of Knowledge.
///
/// # API Contract
///
/// - `GET {server_url}/ceremony/transcript` → `CeremonyTranscript` (JSON)
#[cfg(feature = "zk")]
async fn run_ceremony_verify(server_url: &str) -> Result<()> {
    use omnia_adapters::setup::ceremony_server::CeremonyTranscript;
    use omnia_adapters::setup::CeremonyClient;

    // Initialize minimal tracing
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(true)
        .init();

    println!("Fetching ceremony transcript from {server_url}...");

    // Build HTTP client
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .context("Failed to build HTTP client")?;

    // 1. Fetch the full transcript
    let transcript_url = format!("{server_url}/ceremony/transcript");
    let resp = client
        .get(&transcript_url)
        .send()
        .await
        .with_context(|| format!("Failed to connect to ceremony server at {transcript_url}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Ceremony server returned error {status} when fetching transcript: {body}");
    }

    let transcript: CeremonyTranscript = resp.json().await.context("Failed to deserialize ceremony transcript")?;

    println!("  Received transcript:");
    println!("    Contributions: {}", transcript.contribution_count);
    println!("    Ceremony ID: {}", transcript.config.ceremony_id);
    println!("    Degree: {}", transcript.config.degree);
    println!(
        "    Final hash: {}",
        hex::encode(&transcript.final_transcript_hash[..8])
    );

    // 2. Verify the transcript independently
    println!("\nVerifying transcript...");
    let degree = transcript.config.degree;
    let contribution_count = transcript.contribution_count;

    match CeremonyClient::verify_transcript(&transcript, degree) {
        Ok(true) => {
            println!("\n✓ Transcript verification succeeded!");
            println!("  All {contribution_count} contributions verified");
            println!("  Final transcript hash matches");
        }
        Ok(false) => {
            anyhow::bail!("Transcript verification failed: final hash mismatch");
        }
        Err(e) => {
            anyhow::bail!("Transcript verification failed: {e}");
        }
    }

    Ok(())
}

/// Response body for `GET /ceremony/state`.
///
/// Contains the current SRS transcript bytes and the number of G1 powers
/// needed to generate a contribution.
#[cfg(feature = "zk")]
#[derive(serde::Deserialize)]
struct CeremonyStateResponse {
    /// Current SRS transcript bytes.
    transcript: Vec<u8>,
    /// Number of G1 powers in the ceremony.
    tau_size: usize,
}
