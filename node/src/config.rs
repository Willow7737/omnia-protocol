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
    /// Minimum number of peers required for readiness (default: 1).
    pub readiness_min_peers: usize,
    /// Maximum age of last finalization in rounds for readiness (default: 600).
    pub readiness_max_finalization_age: u64,
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
    /// Minimum number of peers required for readiness (default: 1).
    pub readiness_min_peers: Option<usize>,
    /// Maximum age of last finalization in rounds for readiness (default: 600).
    pub readiness_max_finalization_age: Option<u64>,
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
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;
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
            other => anyhow::bail!("invalid log_level: '{}'", other),
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
        let file_config = args.config.as_ref().and_then(|path| {
            match NodeConfigFile::from_file(std::path::Path::new(path)) {
                Ok(fc) => {
                    tracing::info!(config_path = %path, "Loaded configuration file");
                    Some(fc)
                }
                Err(e) => {
                    tracing::error!(config_path = %path, error = %e, "Failed to load config file");
                    None
                }
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

        let pruning_depth = file_config
            .as_ref()
            .and_then(|fc| fc.pruning_depth)
            .unwrap_or(0);

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

        let readiness_min_peers = file_config
            .as_ref()
            .and_then(|fc| fc.readiness_min_peers)
            .unwrap_or(1);

        let readiness_max_finalization_age = file_config
            .as_ref()
            .and_then(|fc| fc.readiness_max_finalization_age)
            .unwrap_or(600);

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
            readiness_min_peers,
            readiness_max_finalization_age,
        }
    }
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

    /// P2P listen address.
    #[arg(long, env = "OMNIA_LISTEN_ADDR", default_value = "0.0.0.0:4001")]
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

    /// Protocol version to advertise on the network.
    #[arg(long, env = "OMNIA_PROTOCOL_VERSION", default_value = "4.0.0")]
    pub protocol_version: String,

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
            Some(vec![
                "/ip4/1.2.3.4/udp/4001/quic/p2p/12D3KooWTest".to_string()
            ])
        );
        assert_eq!(config.max_payload_size, Some(2_097_152));
        assert_eq!(config.pruning_depth, Some(10_000));
        assert_eq!(config.snapshot_interval, Some(5_000));
        assert_eq!(
            config.slashing_data_dir,
            Some("/var/lib/omnia/slashing".to_string())
        );
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
            readiness_min_peers: 1,
            readiness_max_finalization_age: 600,
        };
        assert_eq!(config.slashing_dir(), PathBuf::from("/custom/slashing"));
    }
}
