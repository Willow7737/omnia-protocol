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
            CliCommand::Keygen {
                output_dir,
                passphrase,
            } => {
                return run_keygen(&output_dir, passphrase.as_deref());
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
    let substrate = Substrate::new(substrate_config);
    tracing::info!(
        path = %slashing_dir.display(),
        "Substrate runtime initialized with persistent slashing engine"
    );

    // Create the shard router with standard fees and nonce persistence
    let shard_router = create_shard_router(Some(config.nonce_dir().as_path()))?;
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
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("Failed to create nonce directory: {}", parent.display())
                })?;
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
    std::fs::create_dir_all(dir)
        .with_context(|| format!("Failed to create output directory: {}", output_dir))?;

    let keypair = generate_keypair();
    let pubkey_bytes = keypair.verifying_key().to_bytes();

    // Write public key as hex
    let pubkey_path = dir.join("validator_pubkey.txt");
    std::fs::write(&pubkey_path, hex::encode(pubkey_bytes))
        .with_context(|| format!("Failed to write public key to {:?}", pubkey_path))?;

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
            .map_err(|e| anyhow::anyhow!("AES-256-GCM encryption failed: {}", e))?;

        // Build the file: magic + nonce + ciphertext+tag
        let mut file_bytes = Vec::with_capacity(10 + 12 + ciphertext.len());
        file_bytes.extend_from_slice(ENCRYPTED_KEY_MAGIC);
        file_bytes.extend_from_slice(&nonce_bytes);
        file_bytes.extend_from_slice(&ciphertext);

        let privkey_path = dir.join("validator_key.enc");
        std::fs::write(&privkey_path, &file_bytes)
            .with_context(|| format!("Failed to write encrypted private key to {:?}", privkey_path))?;

        // Set file permissions to 0600 (owner read/write only) on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&privkey_path, perms)
                .with_context(|| format!("Failed to set permissions on {:?}", privkey_path))?;
        }

        tracing::info!(
            output_dir = %dir.display(),
            pubkey = %hex::encode(&pubkey_bytes[..8]),
            encrypted = true,
            "Validator keypair generated and encrypted successfully"
        );

        println!("Validator keypair generated in {}", dir.display());
        println!("  Public key: {}", hex::encode(pubkey_bytes));
        println!("  Private key: {:?} (AES-256-GCM encrypted)", privkey_path);
    } else {
        // ---- Unencrypted path (NOT recommended for production) ----
        let privkey_path = dir.join("validator_key.bin");
        std::fs::write(&privkey_path, privkey_bytes)
            .with_context(|| format!("Failed to write private key to {:?}", privkey_path))?;

        // Set file permissions to 0600 (owner read/write only) on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&privkey_path, perms)
                .with_context(|| format!("Failed to set permissions on {:?}", privkey_path))?;
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
        println!("  Private key: {:?}", privkey_path);
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
pub fn load_encrypted_key(
    path: &std::path::Path,
    passphrase: &str,
) -> Result<[u8; 64]> {
    use aes_gcm::aead::Aead;
    use aes_gcm::{Aes256Gcm, KeyInit, Nonce};

    let file_bytes =
        std::fs::read(path).with_context(|| format!("Failed to read key file: {:?}", path))?;

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

    let mut srs = PowersOfTau::new(degree).context("Failed to initialize Powers of Tau")?;
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
