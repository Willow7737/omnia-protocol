//! Node configuration — CLI arguments, env var overrides, and validation
//!
//! This module defines the [`NodeConfig`] struct that captures all
//! configuration needed to run an Omnia node. Configuration values
//! can be set via CLI flags, environment variables (with the `OMNIA_`
//! prefix), or defaults.

use anyhow::{Context, Result};
use clap::Parser;
use omnia_substrate::NodeId;
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
    /// Directory for persistent data (sled DB, slashing state, etc.).
    pub data_dir: PathBuf,
    /// Log level filter (trace, debug, info, warn, error).
    pub log_level: String,
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

    /// Get the slashing store subdirectory path.
    pub fn slashing_dir(&self) -> PathBuf {
        self.data_dir.join("slashing")
    }

    /// Build the config from CLI arguments parsed by clap.
    pub fn from_cli() -> Self {
        let args = CliArgs::parse();
        Self {
            node_id: args.node_id,
            listen_addr: args.listen_addr,
            bootstrap_nodes: args
                .bootstrap_nodes
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            http_port: args.http_port,
            data_dir: PathBuf::from(args.data_dir),
            log_level: args.log_level,
        }
    }
}

/// CLI argument definitions using clap derive.
///
/// Each field supports environment variable overrides via the `OMNIA_` prefix.
#[derive(Debug, Clone, clap::Parser)]
#[command(name = "omnia-node", version, about = "Omnia Protocol full node")]
struct CliArgs {
    /// Unique node identifier in the network.
    #[arg(long, env = "OMNIA_NODE_ID", default_value = "1")]
    node_id: u64,

    /// P2P listen address.
    #[arg(long, env = "OMNIA_LISTEN_ADDR", default_value = "0.0.0.0:4001")]
    listen_addr: String,

    /// Comma-separated list of bootstrap peer multiaddresses.
    #[arg(long, env = "OMNIA_BOOTSTRAP_NODES", default_value = "")]
    bootstrap_nodes: String,

    /// HTTP API server port.
    #[arg(long, env = "OMNIA_HTTP_PORT", default_value_t = 8080)]
    http_port: u16,

    /// Directory for persistent data storage.
    #[arg(long, env = "OMNIA_DATA_DIR", default_value = "./data")]
    data_dir: String,

    /// Log level (trace, debug, info, warn, error).
    #[arg(long, env = "OMNIA_LOG_LEVEL", default_value = "info")]
    log_level: String,
}
