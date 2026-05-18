//! Ethereum settlement layer adapter.
//!
//! Provides two modes:
//! - **Simulated** (default): Uses BLAKE3-based mock responses for testing
//! - **Live** (`ethereum-live` feature): Architecture for connecting to a real
//!   Ethereum RPC endpoint and interacting with the OmniaRollup smart contract.
//!
//! # Architecture
//!
//! The adapter implements the [`SettlementLayer`] trait, making it L1-agnostic
//! from the perspective of the rollup operator. The mode is determined at
//! construction time via [`EthereumAdapter::with_mode`].
//!
//! In simulated mode, `post_batch` returns a deterministic BLAKE3-derived
//! transaction hash, `verify_proof` returns `Ok(true)` for non-empty proofs,
//! and `latest_state_root` returns a tracked zero-initialized root.
//!
//! In live mode, the adapter validates configuration and is architecturally
//! ready for ethers-rs integration, but returns
//! [`SettlementError::NotImplemented`] pending contract deployment and the
//! ethers-rs dependency being added to `Cargo.toml`.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{SettlementError, SettlementLayer};
use crate::proof_bundle::ProofBundle;

/// Ethereum settlement layer configuration.
///
/// Holds all parameters needed to connect to an Ethereum node and interact
/// with the OmniaRollup smart contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EthereumConfig {
    /// Ethereum JSON-RPC endpoint URL (e.g., "ws://localhost:8545").
    pub rpc_url: String,
    /// OmniaRollup contract address (hex-encoded, 0x-prefixed, 42 characters).
    pub contract_address: String,
    /// Operator private key (hex-encoded, 0x-prefixed).
    pub operator_private_key: String,
    /// Gas limit for batch submission transactions.
    pub gas_limit: u64,
    /// Maximum fee per gas (in wei, as a decimal string).
    pub max_fee_per_gas: Option<String>,
    /// Number of confirmation blocks to wait for before considering a tx final.
    pub confirmation_blocks: u64,
}

impl Default for EthereumConfig {
    fn default() -> Self {
        Self {
            rpc_url: "ws://localhost:8545".to_string(),
            contract_address: "0x0000000000000000000000000000000000000000".to_string(),
            operator_private_key: String::new(),
            gas_limit: 1_000_000,
            max_fee_per_gas: None,
            confirmation_blocks: 3,
        }
    }
}

impl EthereumConfig {
    /// Validate the configuration.
    ///
    /// Checks that required fields are present and correctly formatted.
    /// This is called automatically when creating an adapter in live mode.
    pub fn validate(&self) -> Result<(), SettlementError> {
        if self.rpc_url.is_empty() {
            return Err(SettlementError::ConfigError(
                "RPC URL cannot be empty".to_string(),
            ));
        }
        if !self.rpc_url.starts_with("ws://")
            && !self.rpc_url.starts_with("wss://")
            && !self.rpc_url.starts_with("http://")
            && !self.rpc_url.starts_with("https://")
        {
            return Err(SettlementError::ConfigError(format!(
                "Invalid RPC URL scheme: {}",
                self.rpc_url
            )));
        }
        if self.contract_address.is_empty() {
            return Err(SettlementError::ConfigError(
                "Contract address cannot be empty".to_string(),
            ));
        }
        // Validate contract address is valid hex (0x-prefixed, 42 chars)
        if !self.contract_address.starts_with("0x") || self.contract_address.len() != 42 {
            return Err(SettlementError::ConfigError(format!(
                "Invalid contract address format: {}",
                self.contract_address
            )));
        }
        Ok(())
    }
}

/// Settlement mode for the Ethereum adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EthereumMode {
    /// Simulated mode using BLAKE3-based mock responses.
    Simulated,
    /// Live mode with real Ethereum RPC calls (requires `ethereum-live` feature
    /// and the ethers-rs dependency).
    Live,
}

impl fmt::Display for EthereumMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EthereumMode::Simulated => write!(f, "simulated"),
            EthereumMode::Live => write!(f, "live"),
        }
    }
}

/// Ethereum settlement adapter.
///
/// In default (simulated) mode, all operations return deterministic mock
/// responses based on BLAKE3. When created with [`EthereumMode::Live`],
/// the adapter validates configuration and is architecturally prepared for
/// ethers-rs integration, but returns [`SettlementError::NotImplemented`]
/// until the `ethers` crate is added as a dependency.
pub struct EthereumAdapter {
    config: EthereumConfig,
    /// Settlement mode: simulated or live.
    mode: EthereumMode,
    /// Latest state root (simulated tracking).
    latest_root: [u8; 32],
    /// Batch counter for mock transaction hashes.
    batch_counter: AtomicU64,
}

impl fmt::Debug for EthereumAdapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EthereumAdapter")
            .field("mode", &self.mode)
            .field("config", &self.config)
            .finish()
    }
}

impl EthereumAdapter {
    /// Create a new simulated Ethereum adapter from individual parameters.
    ///
    /// This is the backward-compatible constructor. The adapter runs in
    /// simulated mode regardless of the parameters.
    ///
    /// # Arguments
    /// * `rpc_url` — Ethereum JSON-RPC endpoint (e.g., "http://localhost:8545")
    /// * `contract_address` — Deployed OmniaRollup contract address
    /// * `_operator_key` — Operator's private key (32 bytes, unused in simulated mode)
    pub fn new(rpc_url: &str, contract_address: &str, _operator_key: &[u8; 32]) -> Self {
        Self {
            config: EthereumConfig {
                rpc_url: rpc_url.to_string(),
                contract_address: contract_address.to_string(),
                operator_private_key: String::new(),
                ..Default::default()
            },
            mode: EthereumMode::Simulated,
            latest_root: [0u8; 32],
            batch_counter: AtomicU64::new(0),
        }
    }

    /// Create a new Ethereum adapter from a full configuration.
    ///
    /// The adapter runs in simulated mode.
    pub fn from_config(config: EthereumConfig) -> Self {
        Self {
            config,
            mode: EthereumMode::Simulated,
            latest_root: [0u8; 32],
            batch_counter: AtomicU64::new(0),
        }
    }

    /// Create a new Ethereum adapter with an explicit mode.
    ///
    /// If `mode` is [`EthereumMode::Live`], the configuration is validated
    /// before construction. Live mode requires the `ethereum-live` feature
    /// flag and the ethers-rs dependency at compile time, plus a deployed
    /// contract at runtime.
    pub fn with_mode(config: EthereumConfig, mode: EthereumMode) -> Result<Self, SettlementError> {
        if mode == EthereumMode::Live {
            config.validate()?;
        }
        Ok(Self {
            config,
            mode,
            latest_root: [0u8; 32],
            batch_counter: AtomicU64::new(0),
        })
    }

    /// Get the current settlement mode.
    pub fn mode(&self) -> EthereumMode {
        self.mode
    }

    /// Get a reference to the configuration.
    pub fn config(&self) -> &EthereumConfig {
        &self.config
    }

    /// Get the configured RPC URL.
    #[allow(dead_code)]
    pub fn rpc_url(&self) -> &str {
        &self.config.rpc_url
    }

    /// Get the configured contract address.
    #[allow(dead_code)]
    pub fn contract_address(&self) -> &str {
        &self.config.contract_address
    }

    /// Generate a deterministic mock transaction hash from batch data.
    ///
    /// Uses BLAKE3 keyed-hash with a domain separator to produce a
    /// 32-byte value that is formatted as a hex string with "0x" prefix.
    fn mock_tx_hash(&self, data: &[u8]) -> String {
        let hash = blake3::derive_key("OMNIA-ETH-MOCK-TX", data);
        format!("0x{}", hex::encode(hash))
    }

    /// Returns the standard error for live mode operations that are not yet
    /// implemented. This keeps the error messages consistent across all
    /// live-mode stubs.
    fn live_not_implemented(operation: &str) -> SettlementError {
        SettlementError::NotImplemented(format!(
            "Ethereum live mode: {} pending ethers-rs integration and contract deployment",
            operation
        ))
    }
}

#[async_trait]
impl SettlementLayer for EthereumAdapter {
    fn chain_id(&self) -> &'static str {
        "ethereum"
    }

    async fn post_batch(&self, batch_data: &[u8]) -> Result<String, SettlementError> {
        match self.mode {
            EthereumMode::Simulated => {
                let count = self.batch_counter.fetch_add(1, Ordering::Relaxed);
                let mut input = batch_data.to_vec();
                input.extend_from_slice(&count.to_le_bytes());
                let tx_hash = self.mock_tx_hash(&input);
                tracing::debug!(
                    "Simulated batch posted: tx_hash={}..",
                    &tx_hash[..16.min(tx_hash.len())]
                );
                Ok(tx_hash)
            }
            EthereumMode::Live => Err(Self::live_not_implemented("post_batch")),
        }
    }

    async fn verify_proof(
        &self,
        _old_root: &[u8; 32],
        _new_root: &[u8; 32],
        proof: &[u8],
    ) -> Result<bool, SettlementError> {
        match self.mode {
            EthereumMode::Simulated => {
                // Simulated: return true for non-empty proofs
                Ok(!proof.is_empty())
            }
            EthereumMode::Live => Err(Self::live_not_implemented("verify_proof")),
        }
    }

    async fn latest_state_root(&self) -> Result<[u8; 32], SettlementError> {
        match self.mode {
            EthereumMode::Simulated => Ok(self.latest_root),
            EthereumMode::Live => Err(Self::live_not_implemented("latest_state_root")),
        }
    }

    async fn deposit(&self, l2_did: &str, amount: u64) -> Result<String, SettlementError> {
        match self.mode {
            EthereumMode::Simulated => {
                let tx_hash = format!("0xdeposit_{}_{}", l2_did, amount);
                tracing::info!("[Ethereum] Deposit: {} UBC to {}", amount, l2_did);
                Ok(tx_hash)
            }
            EthereumMode::Live => Err(Self::live_not_implemented("deposit")),
        }
    }

    async fn request_withdrawal(
        &self,
        l2_did: &str,
        amount: u64,
    ) -> Result<String, SettlementError> {
        match self.mode {
            EthereumMode::Simulated => {
                let tx_hash = format!("0xwithdraw_{}_{}", l2_did, amount);
                tracing::info!(
                    "[Ethereum] Withdrawal request: {} UBC from {}",
                    amount,
                    l2_did
                );
                Ok(tx_hash)
            }
            EthereumMode::Live => Err(Self::live_not_implemented("request_withdrawal")),
        }
    }

    async fn submit_batch(&self, bundle: &ProofBundle) -> Result<String, SettlementError> {
        match self.mode {
            EthereumMode::Simulated => {
                let bundle_bytes = bundle
                    .to_bytes()
                    .map_err(|e| SettlementError::RpcError(e.to_string()))?;
                let tx_hash = self.mock_tx_hash(&bundle_bytes);
                tracing::info!(
                    "[Ethereum] Submitted proof bundle, state_root={}, tx: {}",
                    hex::encode(&bundle.state_root[..8]),
                    &tx_hash[..16.min(tx_hash.len())]
                );
                Ok(tx_hash)
            }
            EthereumMode::Live => Err(Self::live_not_implemented("submit_batch")),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_ethereum_simulated_mode_default() {
        let adapter = EthereumAdapter::new("http://localhost:8545", "0x1234", &[0u8; 32]);
        assert_eq!(adapter.mode(), EthereumMode::Simulated);
    }

    #[test]
    fn test_ethereum_config_default() {
        let config = EthereumConfig::default();
        assert_eq!(config.rpc_url, "ws://localhost:8545");
        assert_eq!(
            config.contract_address,
            "0x0000000000000000000000000000000000000000"
        );
        assert_eq!(config.gas_limit, 1_000_000);
        assert_eq!(config.confirmation_blocks, 3);
        assert!(config.max_fee_per_gas.is_none());
    }

    #[test]
    fn test_ethereum_config_validation_empty_rpc() {
        let config = EthereumConfig {
            rpc_url: "".to_string(),
            ..Default::default()
        };
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("RPC URL cannot be empty"));
    }

    #[test]
    fn test_ethereum_config_validation_invalid_scheme() {
        let config = EthereumConfig {
            rpc_url: "ftp://invalid".to_string(),
            ..Default::default()
        };
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("Invalid RPC URL scheme"));
    }

    #[test]
    fn test_ethereum_config_validation_empty_contract() {
        let config = EthereumConfig {
            contract_address: "".to_string(),
            ..Default::default()
        };
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("Contract address cannot be empty"));
    }

    #[test]
    fn test_ethereum_config_validation_short_contract() {
        let config = EthereumConfig {
            contract_address: "0x1234".to_string(),
            ..Default::default()
        };
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("Invalid contract address format"));
    }

    #[test]
    fn test_ethereum_config_validation_no_prefix() {
        let config = EthereumConfig {
            contract_address: "1234567890abcdef1234567890abcdef12345678".to_string(),
            ..Default::default()
        };
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("Invalid contract address format"));
    }

    #[test]
    fn test_ethereum_config_validation_valid() {
        let config = EthereumConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_ethereum_config_validation_http_scheme() {
        let config = EthereumConfig {
            rpc_url: "http://localhost:8545".to_string(),
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_ethereum_config_validation_https_scheme() {
        let config = EthereumConfig {
            rpc_url: "https://mainnet.infura.io/v3/key".to_string(),
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_ethereum_config_validation_wss_scheme() {
        let config = EthereumConfig {
            rpc_url: "wss://mainnet.infura.io/ws/v3/key".to_string(),
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_ethereum_with_mode_simulated() {
        let config = EthereumConfig::default();
        let adapter = EthereumAdapter::with_mode(config, EthereumMode::Simulated).unwrap();
        assert_eq!(adapter.mode(), EthereumMode::Simulated);
    }

    #[test]
    fn test_ethereum_with_mode_live_validates_config() {
        let config = EthereumConfig {
            rpc_url: "".to_string(),
            ..Default::default()
        };
        let result = EthereumAdapter::with_mode(config, EthereumMode::Live);
        assert!(result.is_err());
    }

    #[test]
    fn test_ethereum_from_config() {
        let config = EthereumConfig {
            rpc_url: "http://localhost:8545".to_string(),
            contract_address: "0x1234567890abcdef1234567890abcdef12345678".to_string(),
            gas_limit: 2_000_000,
            ..Default::default()
        };
        let adapter = EthereumAdapter::from_config(config);
        assert_eq!(adapter.mode(), EthereumMode::Simulated);
        assert_eq!(adapter.config().rpc_url, "http://localhost:8545");
        assert_eq!(adapter.config().gas_limit, 2_000_000);
    }

    #[tokio::test]
    async fn test_ethereum_simulated_post_batch() {
        let adapter = EthereumAdapter::new("http://localhost:8545", "0x1234", &[0u8; 32]);
        let result = adapter.post_batch(b"test batch data").await;
        assert!(result.is_ok());
        let tx = result.unwrap();
        assert!(tx.starts_with("0x"));
    }

    #[tokio::test]
    async fn test_ethereum_simulated_post_batch_deterministic() {
        let adapter = EthereumAdapter::new("http://localhost:8545", "0x1234", &[0u8; 32]);
        let result1 = adapter.post_batch(b"same data").await.unwrap();
        // Different batch counter makes each call unique
        let result2 = adapter.post_batch(b"same data").await.unwrap();
        // They differ because batch_counter increments
        assert_ne!(result1, result2);
    }

    #[tokio::test]
    async fn test_ethereum_simulated_verify_proof_non_empty() {
        let adapter = EthereumAdapter::new("http://localhost:8545", "0x1234", &[0u8; 32]);
        let result = adapter
            .verify_proof(&[0u8; 32], &[1u8; 32], &[0u8; 64])
            .await;
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[tokio::test]
    async fn test_ethereum_simulated_verify_proof_empty() {
        let adapter = EthereumAdapter::new("http://localhost:8545", "0x1234", &[0u8; 32]);
        let result = adapter.verify_proof(&[0u8; 32], &[1u8; 32], &[]).await;
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[tokio::test]
    async fn test_ethereum_simulated_latest_state_root() {
        let adapter = EthereumAdapter::new("http://localhost:8545", "0x1234", &[0u8; 32]);
        let root = adapter.latest_state_root().await.unwrap();
        assert_eq!(root, [0u8; 32]);
    }

    #[tokio::test]
    async fn test_ethereum_simulated_deposit() {
        let adapter = EthereumAdapter::new("http://localhost:8545", "0x1234", &[0u8; 32]);
        let result = adapter.deposit("did:omnia:test", 100).await;
        assert!(result.is_ok());
        let tx = result.unwrap();
        assert!(tx.starts_with("0x"));
    }

    #[tokio::test]
    async fn test_ethereum_simulated_withdrawal() {
        let adapter = EthereumAdapter::new("http://localhost:8545", "0x1234", &[0u8; 32]);
        let result = adapter.request_withdrawal("did:omnia:test", 50).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_ethereum_live_mode_returns_not_implemented() {
        let config = EthereumConfig::default();
        let adapter = EthereumAdapter::with_mode(config, EthereumMode::Live).unwrap();
        assert_eq!(adapter.mode(), EthereumMode::Live);

        let result = adapter.post_batch(b"test").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SettlementError::NotImplemented(_)));
        assert!(err.to_string().contains("ethers-rs"));
    }

    #[test]
    fn test_ethereum_mode_display() {
        assert_eq!(format!("{}", EthereumMode::Simulated), "simulated");
        assert_eq!(format!("{}", EthereumMode::Live), "live");
    }

    #[test]
    fn test_ethereum_debug_format() {
        let adapter = EthereumAdapter::new("http://localhost:8545", "0x1234", &[0u8; 32]);
        let debug = format!("{:?}", adapter);
        assert!(debug.contains("Simulated"));
    }
}
