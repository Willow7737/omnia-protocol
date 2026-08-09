//! Node configuration — CLI arguments, env var overrides, TOML config files, and validation
//!
//! This module defines the [`NodeConfig`] struct that captures all
//! configuration needed to run an Omnia node. Configuration values
//! can be set via CLI flags, environment variables (with the `OMNIA_`
//! prefix), TOML config files, or defaults.
//!
//! # Configuration Precedence
//!
//! 1. CLI flags (highest priority)
//! 2. Environment variables (`OMNIA_` prefix)
//! 3. TOML config file (via `--config` flag)
//! 4. Built-in defaults (lowest priority)

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use omnia_substrate::NodeId;
use serde::Deserialize;
use std::path::PathBuf;

/// Configuration for an Omnia node.
///
/// All fields can be overridden via environment variables using the
/// `OMNIA_` prefix (e.g., `OMNIA_NODE_ID`, `OMNIA_HTTP_PORT`).
/// CLI flags take precedence over environment variables.
#[derive(Debug, Clone)]
pub struct NodeConfig {
    /// Unique identifier for this node in the network.
    pub node_id: u64,
    /// Network listen address for P2P communication.
    pub listen_addr: String,
    /// Bootstrap peer multiaddresses for P2P discovery.
    pub bootstrap_nodes: Vec<String>,
    /// HTTP API server port.
    pub http_port: u16,
    /// Directory for persistent data (redb DB, slashing state, etc.).
    pub data_dir: PathBuf,
    /// Log level filter (trace, debug, info, warn, error).
    pub log_level: String,
    /// Maximum event payload size in bytes.
    pub max_payload_size: usize,
    /// Number of finalized rounds to retain before pruning (0 = archive).
    pub pruning_depth: u64,
    /// Interval (in event count) between automatic snapshots.
    pub snapshot_interval: u64,
    /// Directory for persistent slashing state.
    pub slashing_data_dir: Option<PathBuf>,
    /// Directory for persistent nonce state (redb). If None, nonce state is in-memory only.
    /// Production nodes MUST set this for replay protection across restarts.
    pub nonce_data_dir: Option<PathBuf>,
    /// Directory for persistent consensus state (redb). If None, consensus state is in-memory only.
    /// Production nodes MUST set this to avoid replaying all events from genesis after a crash.
    pub consensus_data_dir: Option<PathBuf>,
    /// Protocol version to advertise on the network.
    pub protocol_version: String,
    /// Hex-encoded Ed25519 public key authorized to mint on the financial
    /// shard, or `None` to disable minting entirely.
    ///
    /// **This is a network-wide genesis parameter, not a per-node one.**
    /// `FinancialState::apply` accepts a `Mint` only when the event's
    /// creator matches the configured authority, so every node must be
    /// configured with the *same* key. If node A used its own key and node
    /// B used its own, a mint created by A would be accepted by A and
    /// rejected by B — the two would disagree about total supply and every
    /// balance derived from it.
    ///
    /// Unset means minting is disabled. That is the safe default: a node
    /// that silently substitutes its own key would diverge from its peers
    /// the first time anyone minted.
    pub mint_authority: Option<[u8; 32]>,
    /// Minimum number of peers required for readiness (default: 1).
    pub readiness_min_peers: usize,
    /// Maximum age of last finalization in rounds for readiness (default: 600).
    pub readiness_max_finalization_age: u64,
    /// Enable fast sync on startup (downloads snapshot from peers).
    ///
    /// When `true`, a late-joining node will attempt to download a
    /// recent state snapshot from peers instead of replaying all events
    /// from genesis. Default: `false`.
    pub fast_sync: bool,
    /// Enable TCP transport fallback alongside QUIC.
    ///
    /// When `true`, the libp2p swarm listens on both QUIC and TCP,
    /// improving connectivity in networks that block UDP. Default: `true`.
    pub enable_tcp_fallback: bool,
}

/// TOML-deserializable configuration file structure.
///
/// All fields are optional — only the fields present in the TOML file
/// override the defaults. This allows minimal config files that only
/// specify the values that differ from defaults.
///
/// # Example TOML
///
/// ```toml
/// node_id = 1
/// http_port = 8080
/// listen_addr = "0.0.0.0:4001"
/// data_dir = "./data"
/// log_level = "info"
/// bootstrap_nodes = ["/ip4/1.2.3.4/udp/4001/quic/p2p/PeerId"]
/// max_payload_size = 1048576
/// pruning_depth = 0
/// snapshot_interval = 10000
/// ```
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeConfigFile {
    /// Unique node identifier in the network.
    pub node_id: Option<u64>,
    /// HTTP API server port.
    pub http_port: Option<u16>,
    /// Network listen address for P2P communication.
    pub listen_addr: Option<String>,
    /// Directory for persistent data storage.
    pub data_dir: Option<String>,
    /// Log level filter (trace, debug, info, warn, error).
    pub log_level: Option<String>,
    /// Bootstrap peer multiaddresses for P2P discovery.
    pub bootstrap_nodes: Option<Vec<String>>,
    /// Maximum allowed event payload size in bytes.
    pub max_payload_size: Option<usize>,
    /// Number of finalized rounds to retain before pruning (0 = archive).
    pub pruning_depth: Option<u64>,
    /// Interval (in event count) between automatic snapshots.
    pub snapshot_interval: Option<u64>,
    /// Directory for persistent slashing state.
    pub slashing_data_dir: Option<String>,
    /// Directory for persistent nonce state (redb). If None, nonce state is in-memory only.
    pub nonce_data_dir: Option<String>,
    /// Directory for persistent consensus state (redb). If None, consensus state is in-memory only.
    pub consensus_data_dir: Option<String>,
    /// Hex-encoded Ed25519 public key allowed to mint (64 hex characters).
    ///
    /// Must be identical on every node — see [`NodeConfig::mint_authority`].
    pub mint_authority: Option<String>,
    /// Minimum number of peers required for readiness (default: 1).
    pub readiness_min_peers: Option<usize>,
    /// Maximum age of last finalization in rounds for readiness (default: 600).
    pub readiness_max_finalization_age: Option<u64>,
    /// Enable fast sync on startup (default: false).
    pub fast_sync: Option<bool>,
    /// Enable TCP transport fallback alongside QUIC (default: true).
    pub enable_tcp_fallback: Option<bool>,
}

impl NodeConfigFile {
    /// Load configuration from a TOML file on disk.
    ///
    /// Reads the file, parses it as TOML, and returns the deserialized
    /// `NodeConfigFile`. Returns an error if the file cannot be read
    /// or contains invalid TOML.
    ///
    /// # Errors
    ///
    /// - `anyhow::Error` if the file cannot be read or parsed.
    pub fn from_file(path: &std::path::Path) -> Result<Self> {
        let content =
            std::fs::read_to_string(path).with_context(|| format!("Failed to read config file: {}", path.display()))?;
        Self::from_toml(&content)
    }

    /// Parse configuration from a TOML string.
    ///
    /// Useful for testing or when the TOML content comes from a source
    /// other than a file (e.g., environment variable, remote config).
    ///
    /// # Errors
    ///
    /// - `anyhow::Error` if the TOML is syntactically invalid or does not
    ///   match the `NodeConfigFile` schema.
    pub fn from_toml(toml_str: &str) -> Result<Self> {
        toml::from_str(toml_str).with_context(|| "Failed to parse TOML configuration".to_string())
    }
}

impl NodeConfig {
    /// Validate the configuration, returning an error if any value is invalid.
    ///
    /// Checks:
    /// - `node_id` must be non-zero
    /// - `http_port` must be non-zero
    /// - `data_dir` must be a valid path
    /// - `log_level` must be a valid tracing level
    pub fn validate(&self) -> Result<()> {
        if self.node_id == 0 {
            anyhow::bail!("node_id must be non-zero");
        }
        if self.http_port == 0 {
            anyhow::bail!("http_port must be non-zero");
        }
        if self.data_dir.as_os_str().is_empty() {
            anyhow::bail!("data_dir must not be empty");
        }
        match self.log_level.as_str() {
            "trace" | "debug" | "info" | "warn" | "error" => {}
            other => anyhow::bail!("invalid log_level: '{other}'"),
        }
        Ok(())
    }

    /// Convert the numeric `node_id` into the substrate's `[u8; 32]` `NodeId`.
    ///
    /// The node ID is placed in the first 8 bytes (little-endian) with the
    /// remaining bytes set to zero.
    pub fn node_id_bytes(&self) -> NodeId {
        let mut id = [0u8; 32];
        id[..8].copy_from_slice(&self.node_id.to_le_bytes());
        id
    }

    /// Resolve the data directory, creating it if it doesn't exist.
    ///
    /// Returns the canonical path to the data directory.
    pub fn ensure_data_dir(&self) -> Result<PathBuf> {
        std::fs::create_dir_all(&self.data_dir)
            .with_context(|| format!("Failed to create data directory: {:?}", self.data_dir))?;
        self.data_dir
            .canonicalize()
            .context("Failed to canonicalize data directory path")
    }

    /// Get the slashing store database file path.
    ///
    /// Returns `<data_dir>/slashing.redb` if not explicitly configured.
    /// redb uses a single file rather than a directory.
    pub fn slashing_dir(&self) -> PathBuf {
        self.slashing_data_dir
            .clone()
            .unwrap_or_else(|| self.data_dir.join("slashing.redb"))
    }

    /// Get the nonce store database file path.
    ///
    /// Returns `<data_dir>/nonces.redb` if not explicitly configured.
    /// redb uses a single file rather than a directory.
    pub fn nonce_dir(&self) -> PathBuf {
        self.nonce_data_dir
            .clone()
            .unwrap_or_else(|| self.data_dir.join("nonces.redb"))
    }

    /// Get the consensus store database file path.
    ///
    /// Returns `<data_dir>/consensus.redb` if not explicitly configured.
    /// redb uses a single file rather than a directory.
    pub fn consensus_dir(&self) -> PathBuf {
        self.consensus_data_dir
            .clone()
            .unwrap_or_else(|| self.data_dir.join("consensus.redb"))
    }

    /// Build the config from CLI arguments parsed by clap.
    ///
    /// If a `--config` flag is provided, the TOML file is loaded first
    /// and its values serve as defaults. CLI flags and environment variables
    /// override config file values.
    pub fn from_cli() -> Self {
        let args = CliArgs::parse();

        // Load TOML config file if specified
        let file_config =
            args.config
                .as_ref()
                .and_then(|path| match NodeConfigFile::from_file(std::path::Path::new(path)) {
                    Ok(fc) => {
                        tracing::info!(config_path = %path, "Loaded configuration file");
                        Some(fc)
                    }
                    Err(e) => {
                        tracing::error!(config_path = %path, error = %e, "Failed to load config file");
                        None
                    }
                });

        // Merge: CLI args > config file > defaults
        let node_id = args.node_id;
        let http_port = args.http_port;
        let listen_addr = args.listen_addr.clone();
        let data_dir = PathBuf::from(args.data_dir.clone());
        let log_level = args.log_level.clone();
        let protocol_version = args.protocol_version.clone();

        let bootstrap_nodes = args
            .bootstrap_nodes
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let max_payload_size = file_config
            .as_ref()
            .and_then(|fc| fc.max_payload_size)
            .unwrap_or(omnia_substrate::MAX_PAYLOAD_SIZE);

        let pruning_depth = file_config.as_ref().and_then(|fc| fc.pruning_depth).unwrap_or(0);

        let snapshot_interval = file_config
            .as_ref()
            .and_then(|fc| fc.snapshot_interval)
            .unwrap_or(10_000);

        let slashing_data_dir = file_config
            .as_ref()
            .and_then(|fc| fc.slashing_data_dir.clone())
            .map(PathBuf::from);

        let nonce_data_dir = file_config
            .as_ref()
            .and_then(|fc| fc.nonce_data_dir.clone())
            .map(PathBuf::from);

        let consensus_data_dir = file_config
            .as_ref()
            .and_then(|fc| fc.consensus_data_dir.clone())
            .map(PathBuf::from);

        // CLI/env wins over the config file, matching every other field.
        // A malformed key is refused rather than silently ignored: quietly
        // falling back to "minting disabled" would look identical to a
        // correct no-mint deployment, and the operator would only find out
        // when a mint they expected to work was rejected.
        let mint_authority = args
            .mint_authority
            .clone()
            .or_else(|| file_config.as_ref().and_then(|fc| fc.mint_authority.clone()))
            .filter(|s| !s.trim().is_empty())
            .map(|s| parse_mint_authority(s.as_str()))
            .transpose()
            .unwrap_or_else(|e| {
                panic!("Invalid mint_authority: {e}. Expected 64 hex characters (an Ed25519 public key).")
            });

        let readiness_min_peers = file_config.as_ref().and_then(|fc| fc.readiness_min_peers).unwrap_or(1);

        let readiness_max_finalization_age = file_config
            .as_ref()
            .and_then(|fc| fc.readiness_max_finalization_age)
            .unwrap_or(600);

        // CLI/env > TOML > default.
        // fast_sync: default false (opt-in for late-joining nodes).
        let fast_sync = args
            .fast_sync
            .or_else(|| file_config.as_ref().and_then(|fc| fc.fast_sync))
            .unwrap_or(false);

        // enable_tcp_fallback: default true (TCP is lightweight and helps
        // peers behind UDP-blocking firewalls).
        let enable_tcp_fallback = args
            .enable_tcp_fallback
            .or_else(|| file_config.as_ref().and_then(|fc| fc.enable_tcp_fallback))
            .unwrap_or(true);

        Self {
            node_id,
            listen_addr,
            bootstrap_nodes,
            http_port,
            data_dir,
            log_level,
            max_payload_size,
            pruning_depth,
            snapshot_interval,
            slashing_data_dir,
            nonce_data_dir,
            consensus_data_dir,
            protocol_version,
            mint_authority,
            readiness_min_peers,
            readiness_max_finalization_age,
            fast_sync,
            enable_tcp_fallback,
        }
    }
}

/// Parse a hex-encoded Ed25519 public key into a mint authority.
///
/// Rejects input that is not hex, not 32 bytes, or does not decompress to
/// a point on the curve. Catching those at startup matters because the
/// alternative is a node that boots happily and then rejects every mint
/// at runtime — a much harder failure to trace back to a typo.
///
/// This is not a full weak-key check: `VerifyingKey::from_bytes`
/// decompresses but does not reject small-order or non-canonical
/// encodings (all-zero and all-`ff` both parse). Those are caught where
/// it counts — `verify_strict` on the signature path refuses them — so a
/// weak key configured here yields a mint authority that can never
/// successfully authorize anything, rather than one that can be forged.
fn parse_mint_authority(value: &str) -> Result<[u8; 32], String> {
    let trimmed = value.trim().trim_start_matches("0x");
    let bytes = hex::decode(trimmed).map_err(|e| format!("not valid hex: {e}"))?;
    let key: [u8; 32] = bytes
        .try_into()
        .map_err(|_| format!("expected 32 bytes, got {} hex characters", trimmed.len()))?;
    omnia_substrate::crypto::VerifyingKey::from_bytes(&key)
        .map_err(|e| format!("not a valid Ed25519 public key: {e}"))?;
    Ok(key)
}

/// CLI argument definitions using clap derive.
///
/// Each field supports environment variable overrides via the `OMNIA_` prefix.
#[derive(Debug, Clone, Parser)]
#[command(name = "omnia-node", version, about = "Omnia Protocol full node")]
pub struct CliArgs {
    /// Unique node identifier in the network.
    #[arg(long, env = "OMNIA_NODE_ID", default_value = "1")]
    pub node_id: u64,

    /// P2P listen address (multiaddr format).
    ///
    /// The libp2p swarm uses QUIC transport by default. The listen address
    /// must be a valid multiaddr that matches a configured transport:
    /// - QUIC: `/ip4/0.0.0.0/udp/4001/quic-v1` (default, recommended)
    /// - TCP: `/ip4/0.0.0.0/tcp/4001` (only if TCP fallback is enabled)
    ///
    /// A plain `host:port` string (e.g., `0.0.0.0:4001`) is automatically
    /// converted to `/ip4/0.0.0.0/udp/{port}/quic-v1` for QUIC compatibility.
    #[arg(long, env = "OMNIA_LISTEN_ADDR", default_value = "/ip4/0.0.0.0/udp/4001/quic-v1")]
    pub listen_addr: String,

    /// Comma-separated list of bootstrap peer multiaddresses.
    #[arg(long, env = "OMNIA_BOOTSTRAP_NODES", default_value = "")]
    pub bootstrap_nodes: String,

    /// HTTP API server port.
    #[arg(long, env = "OMNIA_HTTP_PORT", default_value_t = 8080)]
    pub http_port: u16,

    /// Directory for persistent data storage.
    #[arg(long, env = "OMNIA_DATA_DIR", default_value = "./data")]
    pub data_dir: String,

    /// Log level (trace, debug, info, warn, error).
    #[arg(long, env = "OMNIA_LOG_LEVEL", default_value = "info")]
    pub log_level: String,

    /// Path to a TOML configuration file.
    ///
    /// Values in the config file serve as defaults; CLI flags and
    /// environment variables take precedence.
    #[arg(long, env = "OMNIA_CONFIG")]
    pub config: Option<String>,

    /// Hex-encoded Ed25519 public key authorized to mint on the financial
    /// shard (64 hex characters). Omit to disable minting.
    ///
    /// Must be IDENTICAL on every node in the network — it is a genesis
    /// parameter, not a per-node identity. Nodes configured with different
    /// authorities will disagree about which mints are valid.
    #[arg(long, env = "OMNIA_MINT_AUTHORITY")]
    pub mint_authority: Option<String>,

    /// Protocol version to advertise on the network.
    #[arg(long, env = "OMNIA_PROTOCOL_VERSION", default_value = "4.0.0")]
    pub protocol_version: String,

    /// Enable fast sync on startup (downloads snapshot from peers).
    ///
    /// When set, a late-joining node downloads a recent state snapshot
    /// from peers and replays only delta events since that snapshot,
    /// instead of replaying all events from genesis.
    #[arg(long, env = "OMNIA_FAST_SYNC")]
    pub fast_sync: Option<bool>,

    /// Enable TCP transport fallback alongside QUIC.
    ///
    /// When enabled (default), the libp2p swarm listens on both QUIC
    /// and TCP, improving connectivity in networks that block UDP.
    /// Set `--no-enable-tcp-fallback` or `OMNIA_ENABLE_TCP_FALLBACK=false`
    /// to disable.
    #[arg(long, env = "OMNIA_ENABLE_TCP_FALLBACK")]
    pub enable_tcp_fallback: Option<bool>,

    /// Optional subcommand (e.g., keygen).
    #[command(subcommand)]
    pub command: Option<CliCommand>,
}

/// CLI subcommands for the omnia-node binary.
#[derive(Debug, Clone, Subcommand)]
pub enum CliCommand {
    /// Run the Omnia node (default behavior).
    Run,

    /// Generate a new validator keypair and save it to disk.
    Keygen {
        /// Output directory for the generated keypair files.
        #[arg(long, default_value = ".")]
        output_dir: String,
        /// Passphrase to encrypt the private key file (required for production).
        /// If not provided, the key will be written unencrypted with a WARNING.
        #[arg(long, env = "OMNIA_KEYGEN_PASSPHRASE")]
        passphrase: Option<String>,
    },

    /// Contribute to the Powers of Tau trusted setup ceremony (Phase 1).
    SetupContribute {
        /// Degree of the Powers of Tau SRS.
        #[arg(long, default_value_t = 65536)]
        degree: usize,
        /// Minimum number of participants required to finalize the ceremony.
        #[arg(long, default_value_t = 1)]
        min_participants: usize,
        /// Optional hex-encoded 32-byte seed for deterministic contribution (testing only).
        #[arg(long)]
        seed: Option<String>,
    },

    /// Verify a completed Powers of Tau ceremony transcript.
    SetupVerify {
        /// Degree of the Powers of Tau SRS.
        #[arg(long, default_value_t = 65536)]
        degree: usize,
        /// Number of contributions to replay and verify.
        #[arg(long, default_value_t = 3)]
        num_contributions: usize,
    },

    /// Take a state snapshot and write it to a file.
    Snapshot {
        /// Output path for the snapshot file.
        #[arg(long, default_value = "snapshot.bin")]
        output: String,
    },

    /// Restore node state from a snapshot file.
    Restore {
        /// Path to the snapshot file to restore from.
        #[arg(long)]
        input: String,
    },

    /// Start a multi-party trusted setup ceremony server.
    ///
    /// Coordinates contributions from multiple participants and
    /// derives circuit-specific keys when enough participants
    /// have contributed.
    CeremonyServe {
        /// Minimum number of participants required to finalize.
        #[arg(long, default_value_t = 3)]
        min_participants: usize,
        /// Maximum number of participants allowed.
        #[arg(long, default_value_t = 100)]
        max_participants: usize,
        /// Degree of the Powers of Tau SRS.
        #[arg(long, default_value_t = 65536)]
        degree: usize,
    },

    /// Contribute to a remote ceremony server.
    ///
    /// Generates a random secret scalar, applies it to the current
    /// SRS, and submits the contribution with a Proof of Knowledge.
    CeremonyContribute {
        /// URL of the ceremony server.
        #[arg(long)]
        server_url: String,
        /// Optional hex-encoded 32-byte seed for deterministic contribution (testing only).
        #[arg(long)]
        seed: Option<String>,
    },

    /// Verify a ceremony transcript from a remote server.
    ///
    /// Downloads the full transcript and independently verifies
    /// each contribution's Proof of Knowledge.
    CeremonyVerify {
        /// URL of the ceremony server.
        #[arg(long)]
        server_url: String,
    },

    /// Generate a genesis block from a TOML configuration file.
    ///
    /// Creates a deterministic genesis block containing the initial
    /// validator set, economic parameters, and governance configuration.
    /// The genesis block hash is computed from BLAKE3 of the configuration,
    /// ensuring all nodes produce the same block from the same config.
    GenesisInit {
        /// Path to the genesis configuration TOML file.
        #[arg(long)]
        config: String,
        /// Output path for the serialized genesis block.
        #[arg(long, default_value = "genesis.bin")]
        output: String,
    },

    /// Validate a genesis block file.
    ///
    /// Reads a genesis block, re-derives the expected hash from the
    /// embedded validator set, and verifies integrity. Useful for
    /// ensuring all nodes have the same genesis block before launch.
    GenesisValidate {
        /// Path to the genesis block file to validate.
        #[arg(long)]
        block: String,
    },
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_config_file_minimal_toml() {
        let toml = r#"
            node_id = 42
            http_port = 9090
        "#;
        let config = NodeConfigFile::from_toml(toml).expect("parse minimal TOML");
        assert_eq!(config.node_id, Some(42));
        assert_eq!(config.http_port, Some(9090));
        assert!(config.listen_addr.is_none());
        assert!(config.data_dir.is_none());
        assert!(config.log_level.is_none());
        assert!(config.bootstrap_nodes.is_none());
        assert!(config.max_payload_size.is_none());
        assert!(config.pruning_depth.is_none());
        assert!(config.snapshot_interval.is_none());
        assert!(config.slashing_data_dir.is_none());
    }

    #[test]
    fn test_config_file_full_toml() {
        let toml = r#"
            node_id = 5
            http_port = 8081
            listen_addr = "0.0.0.0:5001"
            data_dir = "/var/lib/omnia"
            log_level = "debug"
            bootstrap_nodes = ["/ip4/1.2.3.4/udp/4001/quic/p2p/12D3KooWTest"]
            max_payload_size = 2097152
            pruning_depth = 10000
            snapshot_interval = 5000
            slashing_data_dir = "/var/lib/omnia/slashing"
        "#;
        let config = NodeConfigFile::from_toml(toml).expect("parse full TOML");
        assert_eq!(config.node_id, Some(5));
        assert_eq!(config.http_port, Some(8081));
        assert_eq!(config.listen_addr.as_deref(), Some("0.0.0.0:5001"));
        assert_eq!(config.data_dir.as_deref(), Some("/var/lib/omnia"));
        assert_eq!(config.log_level.as_deref(), Some("debug"));
        assert_eq!(
            config.bootstrap_nodes,
            Some(vec!["/ip4/1.2.3.4/udp/4001/quic/p2p/12D3KooWTest".to_string()])
        );
        assert_eq!(config.max_payload_size, Some(2_097_152));
        assert_eq!(config.pruning_depth, Some(10_000));
        assert_eq!(config.snapshot_interval, Some(5_000));
        assert_eq!(config.slashing_data_dir, Some("/var/lib/omnia/slashing".to_string()));
    }

    #[test]
    fn test_config_file_empty_toml() {
        let toml = "";
        let config = NodeConfigFile::from_toml(toml).expect("parse empty TOML");
        assert!(config.node_id.is_none());
        assert!(config.http_port.is_none());
    }

    #[test]
    fn test_config_file_invalid_toml() {
        let toml = "this is not valid toml {{{{";
        let result = NodeConfigFile::from_toml(toml);
        assert!(result.is_err(), "Invalid TOML should return an error");
    }

    #[test]
    fn test_config_file_unknown_fields_rejected() {
        let toml = r#"
            node_id = 1
            unknown_field = "should fail"
        "#;
        let result = NodeConfigFile::from_toml(toml);
        assert!(result.is_err(), "Unknown fields should cause a parse error");
    }

    #[test]
    fn test_config_file_from_nonexistent_path() {
        let result = NodeConfigFile::from_file(std::path::Path::new("/nonexistent/config.toml"));
        assert!(result.is_err(), "Nonexistent file should return an error");
    }

    #[test]
    fn test_node_config_validate_valid() {
        let config = NodeConfig {
            node_id: 1,
            listen_addr: "0.0.0.0:4001".to_string(),
            bootstrap_nodes: vec![],
            http_port: 8080,
            data_dir: PathBuf::from("./data"),
            log_level: "info".to_string(),
            max_payload_size: 1024 * 1024,
            pruning_depth: 0,
            snapshot_interval: 10_000,
            slashing_data_dir: None,
            nonce_data_dir: None,
            consensus_data_dir: None,
            protocol_version: "4.0.0".to_string(),
            mint_authority: None,
            readiness_min_peers: 1,
            readiness_max_finalization_age: 600,
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_node_config_validate_zero_node_id() {
        let config = NodeConfig {
            node_id: 0,
            listen_addr: "0.0.0.0:4001".to_string(),
            bootstrap_nodes: vec![],
            http_port: 8080,
            data_dir: PathBuf::from("./data"),
            log_level: "info".to_string(),
            max_payload_size: 1024 * 1024,
            pruning_depth: 0,
            snapshot_interval: 10_000,
            slashing_data_dir: None,
            nonce_data_dir: None,
            consensus_data_dir: None,
            protocol_version: "4.0.0".to_string(),
            mint_authority: None,
            readiness_min_peers: 1,
            readiness_max_finalization_age: 600,
        };
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("node_id"));
    }

    #[test]
    fn test_node_config_validate_zero_http_port() {
        let config = NodeConfig {
            node_id: 1,
            listen_addr: "0.0.0.0:4001".to_string(),
            bootstrap_nodes: vec![],
            http_port: 0,
            data_dir: PathBuf::from("./data"),
            log_level: "info".to_string(),
            max_payload_size: 1024 * 1024,
            pruning_depth: 0,
            snapshot_interval: 10_000,
            slashing_data_dir: None,
            nonce_data_dir: None,
            consensus_data_dir: None,
            protocol_version: "4.0.0".to_string(),
            mint_authority: None,
            readiness_min_peers: 1,
            readiness_max_finalization_age: 600,
        };
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("http_port"));
    }

    #[test]
    fn test_node_config_validate_invalid_log_level() {
        let config = NodeConfig {
            node_id: 1,
            listen_addr: "0.0.0.0:4001".to_string(),
            bootstrap_nodes: vec![],
            http_port: 8080,
            data_dir: PathBuf::from("./data"),
            log_level: "verbose".to_string(),
            max_payload_size: 1024 * 1024,
            pruning_depth: 0,
            snapshot_interval: 10_000,
            slashing_data_dir: None,
            nonce_data_dir: None,
            consensus_data_dir: None,
            protocol_version: "4.0.0".to_string(),
            mint_authority: None,
            readiness_min_peers: 1,
            readiness_max_finalization_age: 600,
        };
        let result = config.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("log_level"));
    }

    #[test]
    fn test_node_config_slashing_dir_default() {
        let config = NodeConfig {
            node_id: 1,
            listen_addr: "0.0.0.0:4001".to_string(),
            bootstrap_nodes: vec![],
            http_port: 8080,
            data_dir: PathBuf::from("./data"),
            log_level: "info".to_string(),
            max_payload_size: 1024 * 1024,
            pruning_depth: 0,
            snapshot_interval: 10_000,
            slashing_data_dir: None,
            nonce_data_dir: None,
            consensus_data_dir: None,
            protocol_version: "4.0.0".to_string(),
            mint_authority: None,
            readiness_min_peers: 1,
            readiness_max_finalization_age: 600,
        };
        assert_eq!(config.slashing_dir(), PathBuf::from("./data/slashing.redb"));
    }

    #[test]
    fn test_node_config_slashing_dir_custom() {
        let config = NodeConfig {
            node_id: 1,
            listen_addr: "0.0.0.0:4001".to_string(),
            bootstrap_nodes: vec![],
            http_port: 8080,
            data_dir: PathBuf::from("./data"),
            log_level: "info".to_string(),
            max_payload_size: 1024 * 1024,
            pruning_depth: 0,
            snapshot_interval: 10_000,
            slashing_data_dir: Some(PathBuf::from("/custom/slashing")),
            nonce_data_dir: None,
            consensus_data_dir: None,
            protocol_version: "4.0.0".to_string(),
            mint_authority: None,
            readiness_min_peers: 1,
            readiness_max_finalization_age: 600,
        };
        assert_eq!(config.slashing_dir(), PathBuf::from("/custom/slashing"));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod mint_authority_tests {
    use super::*;

    /// A real Ed25519 public key (seed [3u8; 32]) — the same one the
    /// wallet's cross-language vectors use.
    const VALID_KEY: &str = "ed4928c628d1c2c6eae90338905995612959273a5c63f93636c14614ac8737d1";

    #[test]
    fn parses_a_valid_key() {
        let parsed = parse_mint_authority(VALID_KEY).expect("valid key should parse");
        assert_eq!(hex::encode(parsed), VALID_KEY);
    }

    #[test]
    fn accepts_a_0x_prefix_and_surrounding_whitespace() {
        // Operators paste keys from all sorts of places.
        assert!(parse_mint_authority(&format!("  0x{VALID_KEY}  ")).is_ok());
    }

    #[test]
    fn rejects_malformed_input() {
        // Not hex at all.
        assert!(parse_mint_authority("not-a-key").is_err());
        // Right alphabet, wrong length — a truncated paste.
        assert!(parse_mint_authority("ed4928c6").is_err());
        // 64 hex chars that do not decompress to a point on the curve.
        // Accepting this would start the node happily and then reject
        // every mint at runtime, which is far harder to diagnose than
        // failing at boot.
        let mut off_curve = [0u8; 32];
        off_curve[0] = 0x02;
        assert!(
            parse_mint_authority(&hex::encode(off_curve)).is_err(),
            "an off-curve key must be refused at startup"
        );
    }

    /// Documents a real limit rather than pretending it away: decompression
    /// accepts some degenerate encodings. They are harmless here because
    /// `verify_strict` on the signature path refuses them, so the result is
    /// an authority that can never authorize anything — not a forgeable one.
    #[test]
    fn does_not_claim_to_be_a_weak_key_check() {
        assert!(parse_mint_authority(&"ff".repeat(32)).is_ok());
        assert!(parse_mint_authority(&"00".repeat(32)).is_ok());
    }

    #[test]
    fn config_file_round_trips_the_key() {
        let toml = format!(
            r#"
            node_id = 1
            mint_authority = "{VALID_KEY}"
        "#
        );
        let parsed = NodeConfigFile::from_toml(&toml).expect("parse TOML");
        assert_eq!(parsed.mint_authority.as_deref(), Some(VALID_KEY));
    }

    #[test]
    fn config_file_without_a_key_leaves_minting_disabled() {
        // Absence must stay absence. A node that quietly substitutes its
        // own key here would diverge from peers the first time anyone
        // minted, so "unset" has to survive the round trip.
        let parsed = NodeConfigFile::from_toml("node_id = 1").expect("parse TOML");
        assert!(parsed.mint_authority.is_none());
    }
}
