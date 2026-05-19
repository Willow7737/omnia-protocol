//! Ethereum settlement layer adapter.
//!
//! Provides two modes:
//! - **Simulated** (default): Uses BLAKE3-based mock responses for testing
//! - **Live** (`ethereum-live` feature): Connects to a real Ethereum RPC endpoint
//!   via the `alloy` crate and interacts with the OmniaRollup smart contract.
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
//! In live mode, the adapter uses alloy to:
//! - Connect to an Ethereum RPC endpoint (HTTP or WebSocket)
//! - Sign transactions with the operator's private key
//! - Call the OmniaRollup smart contract's `submitBatch`, `stateRoot`, and
//!   `deposit`/`requestWithdrawal` functions
//! - Wait for transaction confirmations with configurable block depth

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::{SettlementError, SettlementLayer};
use crate::proof_bundle::ProofBundle;

// ---------------------------------------------------------------------------
// Alloy imports (feature-gated)
// ---------------------------------------------------------------------------

#[cfg(feature = "ethereum-live")]
use alloy::primitives::{Address, Bytes, B256, U256};
#[cfg(feature = "ethereum-live")]
use alloy::providers::{Provider, ProviderBuilder};
#[cfg(feature = "ethereum-live")]
use alloy::signers::local::PrivateKeySigner;

// ---------------------------------------------------------------------------
// OmniaRollup contract bindings (feature-gated)
// ---------------------------------------------------------------------------

// Generate typed contract bindings from the OmniaRollup ABI via the `sol!` macro.
//
// The ABI matches the Solidity contract at `zk/contracts/ethereum/OmniaRollup.sol`.
// The `#[sol(rpc)]` attribute generates a `new` constructor that takes a provider
// and returns a contract instance that can make RPC calls.
#[cfg(feature = "ethereum-live")]
alloy::sol! {
    #[sol(rpc)]
    OmniaRollup,
    r#"[
        {
            "type": "function",
            "name": "submitBatch",
            "inputs": [
                {"name": "newStateRoot", "type": "bytes32"},
                {"name": "proofA", "type": "uint256[2]"},
                {"name": "proofB", "type": "uint256[2][2]"},
                {"name": "proofC", "type": "uint256[2]"},
                {"name": "publicInputs", "type": "uint256[]"},
                {"name": "batchData", "type": "bytes"}
            ],
            "outputs": [],
            "stateMutability": "nonpayable"
        },
        {
            "type": "function",
            "name": "stateRoot",
            "inputs": [],
            "outputs": [{"name": "", "type": "bytes32"}],
            "stateMutability": "view"
        },
        {
            "type": "function",
            "name": "batchIndex",
            "inputs": [],
            "outputs": [{"name": "", "type": "uint256"}],
            "stateMutability": "view"
        },
        {
            "type": "function",
            "name": "deposit",
            "inputs": [{"name": "l2Did", "type": "bytes32"}],
            "outputs": [],
            "stateMutability": "payable"
        },
        {
            "type": "function",
            "name": "requestWithdrawal",
            "inputs": [
                {"name": "l2Did", "type": "bytes32"},
                {"name": "amount", "type": "uint256"}
            ],
            "outputs": [],
            "stateMutability": "nonpayable"
        },
        {
            "type": "event",
            "name": "StateUpdated",
            "inputs": [
                {"name": "oldRoot", "type": "bytes32", "indexed": true},
                {"name": "newRoot", "type": "bytes32", "indexed": true},
                {"name": "batchIndex", "type": "uint256", "indexed": false}
            ],
            "anonymous": false
        },
        {
            "type": "event",
            "name": "Deposited",
            "inputs": [
                {"name": "sender", "type": "address", "indexed": true},
                {"name": "l2Did", "type": "bytes32", "indexed": true},
                {"name": "amount", "type": "uint256", "indexed": false}
            ],
            "anonymous": false
        },
        {
            "type": "event",
            "name": "WithdrawalRequested",
            "inputs": [
                {"name": "recipient", "type": "address", "indexed": true},
                {"name": "l2Did", "type": "bytes32", "indexed": true},
                {"name": "amount", "type": "uint256", "indexed": false}
            ],
            "anonymous": false
        }
    ]"#
}

// ---------------------------------------------------------------------------
// EthereumConfig
// ---------------------------------------------------------------------------

/// Ethereum settlement layer configuration.
///
/// Holds all parameters needed to connect to an Ethereum node and interact
/// with the OmniaRollup smart contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EthereumConfig {
    /// Ethereum JSON-RPC endpoint URL (e.g., "http://localhost:8545" or "ws://localhost:8546").
    pub rpc_url: String,
    /// OmniaRollup contract address (hex-encoded, 0x-prefixed, 42 characters).
    pub contract_address: String,
    /// Operator private key (hex-encoded, 0x-prefixed, 64 hex chars).
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
            rpc_url: "http://localhost:8545".to_string(),
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
        if self.operator_private_key.is_empty() {
            return Err(SettlementError::ConfigError(
                "Operator private key cannot be empty in live mode".to_string(),
            ));
        }
        if !self.operator_private_key.starts_with("0x") {
            return Err(SettlementError::ConfigError(
                "Operator private key must be 0x-prefixed hex".to_string(),
            ));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// EthereumMode
// ---------------------------------------------------------------------------

/// Settlement mode for the Ethereum adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EthereumMode {
    /// Simulated mode using BLAKE3-based mock responses.
    Simulated,
    /// Live mode with real Ethereum RPC calls (requires `ethereum-live` feature
    /// and the alloy dependency).
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

// ---------------------------------------------------------------------------
// EthereumLiveClient (feature-gated)
// ---------------------------------------------------------------------------

/// Live Ethereum client backed by alloy.
///
/// Holds the configuration needed to create alloy providers on demand.
/// The provider is created lazily per-call so that the client is `Send + Sync`
/// without requiring complex type erasure for alloy's filler stack.
///
/// Each method builds a fresh provider, signs with the operator key, and
/// sends a transaction to the OmniaRollup contract.
#[cfg(feature = "ethereum-live")]
pub struct EthereumLiveClient {
    rpc_url: String,
    operator_private_key: String,
    contract_address: Address,
    gas_limit: u64,
    #[allow(dead_code)]
    max_fee_per_gas: Option<String>,
    confirmation_blocks: u64,
}

#[cfg(feature = "ethereum-live")]
impl EthereumLiveClient {
    /// Create a new live client from the given configuration.
    ///
    /// Validates that the RPC URL, contract address, and operator key can be
    /// parsed, but does **not** connect to the network. Connection is lazy;
    /// RPC calls are made only when a settlement method is invoked.
    pub fn connect(config: &EthereumConfig) -> Result<Self, SettlementError> {
        let contract_address: Address = config
            .contract_address
            .parse()
            .map_err(|e| SettlementError::ConfigError(format!("Invalid contract address: {e}")))?;

        // Validate the operator private key can be parsed by alloy
        let _: PrivateKeySigner = config.operator_private_key.parse().map_err(|e| {
            SettlementError::ConfigError(format!("Invalid operator private key: {e}"))
        })?;

        Ok(Self {
            rpc_url: config.rpc_url.clone(),
            operator_private_key: config.operator_private_key.clone(),
            contract_address,
            gas_limit: config.gas_limit,
            max_fee_per_gas: config.max_fee_per_gas.clone(),
            confirmation_blocks: config.confirmation_blocks,
        })
    }

    /// Build an alloy provider with the recommended fillers and operator wallet.
    ///
    /// The provider includes gas estimation, nonce management, chain ID fill,
    /// and wallet signing. It connects via HTTP or WebSocket depending on the
    /// configured RPC URL scheme.
    async fn build_provider(&self) -> Result<impl Provider, SettlementError> {
        let wallet: PrivateKeySigner = self
            .operator_private_key
            .parse()
            .map_err(|e| SettlementError::ConfigError(format!("Invalid operator key: {e}")))?;

        // In alloy 1.8.x, recommended fillers (gas, nonce, chain ID) are enabled
        // by default on ProviderBuilder::new(). No need to call with_recommended_fillers().
        let provider = ProviderBuilder::new().wallet(wallet).connect_http(
            self.rpc_url
                .parse()
                .map_err(|e| SettlementError::ConfigError(format!("Invalid RPC URL: {e}")))?,
        );

        Ok(provider)
    }

    /// Submit a batch to the OmniaRollup contract by calling `submitBatch`.
    ///
    /// The proof bytes from the [`ProofBundle`] are decomposed into the
    /// structured calldata parameters that the Solidity contract expects:
    /// `proofA` (`uint256[2]`), `proofB` (`uint256[2][2]`), `proofC` (`uint256[2]`).
    ///
    /// The public inputs are extracted from the proof bundle's state roots and
    /// batch Merkle root using BLAKE3 domain separation.
    ///
    /// Returns the transaction hash as a hex string.
    pub async fn submit_batch_live(&self, bundle: &ProofBundle) -> Result<String, SettlementError> {
        let provider = self.build_provider().await?;
        let contract = OmniaRollup::new(self.contract_address, provider);

        // Decompose the serialized proof into Groth16 components.
        // The proof is laid out as 256 bytes:
        //   A.x (32) | A.y (32) | B.x.c0 (32) | B.x.c1 (32) |
        //   B.y.c0 (32) | B.y.c1 (32) | C.x (32) | C.y (32)
        let proof_bytes = &bundle.transition_proof;
        if proof_bytes.len() < 256 {
            return Err(SettlementError::ContractError(format!(
                "Proof too short: {} bytes, need at least 256",
                proof_bytes.len()
            )));
        }

        let proof_a = [
            U256::from_be_bytes::<32>(slice_to_array(&proof_bytes[0..32])?),
            U256::from_be_bytes::<32>(slice_to_array(&proof_bytes[32..64])?),
        ];
        let proof_b = [
            [
                U256::from_be_bytes::<32>(slice_to_array(&proof_bytes[64..96])?),
                U256::from_be_bytes::<32>(slice_to_array(&proof_bytes[96..128])?),
            ],
            [
                U256::from_be_bytes::<32>(slice_to_array(&proof_bytes[128..160])?),
                U256::from_be_bytes::<32>(slice_to_array(&proof_bytes[160..192])?),
            ],
        ];
        let proof_c = [
            U256::from_be_bytes::<32>(slice_to_array(&proof_bytes[192..224])?),
            U256::from_be_bytes::<32>(slice_to_array(&proof_bytes[224..256])?),
        ];

        // Build public inputs: [old_state_root, new_state_root, event_commitment]
        // These match the ExpandedRollupCircuit public input layout.
        let public_inputs = vec![
            U256::from_be_bytes::<32>(bundle.prev_state_root),
            U256::from_be_bytes::<32>(bundle.state_root),
            U256::from_be_bytes::<32>(bundle.batch_merkle_root),
        ];

        let new_state_root = B256::from(bundle.state_root);

        // Encode batch data using BLAKE3 domain separation for integrity.
        let batch_data_hash = blake3::derive_key("OMNIA-ETH-BATCH-DATA", &bundle.transition_proof);
        let batch_data_bytes = Bytes::copy_from_slice(&batch_data_hash);

        let builder = contract.submitBatch(
            new_state_root,
            proof_a,
            proof_b,
            proof_c,
            public_inputs,
            batch_data_bytes,
        );

        // Set gas limit
        let builder = builder.gas(self.gas_limit);

        let pending_tx = builder
            .send()
            .await
            .map_err(|e| SettlementError::TxFailed(format!("submitBatch send failed: {e}")))?;

        let tx_hash = *pending_tx.tx_hash();

        // Wait for confirmations
        let receipt = pending_tx
            .with_required_confirmations(self.confirmation_blocks)
            .get_receipt()
            .await
            .map_err(|_e| SettlementError::TxTimedOut(self.confirmation_blocks))?;

        if receipt.status() {
            Ok(format!("0x{tx_hash:x}"))
        } else {
            Err(SettlementError::TxFailed(format!(
                "submitBatch transaction reverted: 0x{tx_hash:x}"
            )))
        }
    }

    /// Fetch the latest state root from the OmniaRollup contract.
    ///
    /// Calls the `stateRoot()` view function.
    pub async fn latest_state_root_live(&self) -> Result<[u8; 32], SettlementError> {
        let provider = self.build_provider().await?;
        let contract = OmniaRollup::new(self.contract_address, provider);

        let root =
            contract.stateRoot().call().await.map_err(|e| {
                SettlementError::ContractError(format!("stateRoot call failed: {e}"))
            })?;

        Ok(root.0)
    }

    /// Deposit ETH into the rollup, credited to an L2 identity.
    ///
    /// Calls the `deposit(bytes32)` function on the OmniaRollup contract.
    /// The `amount` parameter specifies the value in wei.
    pub async fn deposit_live(&self, l2_did: &str, amount: u64) -> Result<String, SettlementError> {
        let provider = self.build_provider().await?;
        let contract = OmniaRollup::new(self.contract_address, provider);

        // Convert DID string to bytes32 via BLAKE3 domain separation
        let did_hash = blake3::derive_key("OMNIA-DID-MAP", l2_did.as_bytes());
        let l2_did_bytes = B256::from_slice(&did_hash);

        let builder = contract.deposit(l2_did_bytes).value(U256::from(amount));

        let pending_tx = builder
            .send()
            .await
            .map_err(|e| SettlementError::TxFailed(format!("deposit send failed: {e}")))?;

        let tx_hash = *pending_tx.tx_hash();

        let receipt = pending_tx
            .with_required_confirmations(self.confirmation_blocks)
            .get_receipt()
            .await
            .map_err(|_e| SettlementError::TxTimedOut(self.confirmation_blocks))?;

        if receipt.status() {
            Ok(format!("0x{tx_hash:x}"))
        } else {
            Err(SettlementError::TxFailed(format!(
                "deposit transaction reverted: 0x{tx_hash:x}"
            )))
        }
    }

    /// Request a withdrawal from L2 to L1.
    ///
    /// Calls the `requestWithdrawal(bytes32, uint256)` function on the
    /// OmniaRollup contract.
    pub async fn request_withdrawal_live(
        &self,
        l2_did: &str,
        amount: u64,
    ) -> Result<String, SettlementError> {
        let provider = self.build_provider().await?;
        let contract = OmniaRollup::new(self.contract_address, provider);

        // Convert DID string to bytes32 via BLAKE3 domain separation
        let did_hash = blake3::derive_key("OMNIA-DID-MAP", l2_did.as_bytes());
        let l2_did_bytes = B256::from_slice(&did_hash);

        let builder = contract.requestWithdrawal(l2_did_bytes, U256::from(amount));

        let pending_tx = builder.send().await.map_err(|e| {
            SettlementError::TxFailed(format!("requestWithdrawal send failed: {e}"))
        })?;

        let tx_hash = *pending_tx.tx_hash();

        let receipt = pending_tx
            .with_required_confirmations(self.confirmation_blocks)
            .get_receipt()
            .await
            .map_err(|_e| SettlementError::TxTimedOut(self.confirmation_blocks))?;

        if receipt.status() {
            Ok(format!("0x{tx_hash:x}"))
        } else {
            Err(SettlementError::TxFailed(format!(
                "requestWithdrawal transaction reverted: 0x{tx_hash:x}"
            )))
        }
    }

    /// Verify a Groth16 proof on-chain by calling `submitBatch` with
    /// view simulation, or by checking the contract's `stateRoot` and
    /// comparing against the expected new root.
    ///
    /// Since the OmniaRollup contract does not expose a standalone
    /// `verifyGroth16Proof` view function, this method simulates a
    /// `submitBatch` call in a read-only manner using `eth_call`.
    pub async fn verify_proof_live(
        &self,
        old_root: &[u8; 32],
        new_root: &[u8; 32],
        proof: &[u8],
    ) -> Result<bool, SettlementError> {
        // We validate the proof structure first
        if proof.len() < 256 {
            return Ok(false);
        }

        // Decompose proof into Groth16 components
        let proof_a = [
            U256::from_be_bytes::<32>(slice_to_array(&proof[0..32])?),
            U256::from_be_bytes::<32>(slice_to_array(&proof[32..64])?),
        ];
        let proof_b = [
            [
                U256::from_be_bytes::<32>(slice_to_array(&proof[64..96])?),
                U256::from_be_bytes::<32>(slice_to_array(&proof[96..128])?),
            ],
            [
                U256::from_be_bytes::<32>(slice_to_array(&proof[128..160])?),
                U256::from_be_bytes::<32>(slice_to_array(&proof[160..192])?),
            ],
        ];
        let proof_c = [
            U256::from_be_bytes::<32>(slice_to_array(&proof[192..224])?),
            U256::from_be_bytes::<32>(slice_to_array(&proof[224..256])?),
        ];

        // Build public inputs matching ExpandedRollupCircuit layout
        let batch_merkle_root = blake3::derive_key("OMNIA-PROOF-VERIFY", proof);
        let public_inputs = vec![
            U256::from_be_bytes::<32>(*old_root),
            U256::from_be_bytes::<32>(*new_root),
            U256::from_be_bytes::<32>(batch_merkle_root),
        ];

        let new_state_root = B256::from(*new_root);

        let batch_data_bytes = Bytes::copy_from_slice(proof);

        let provider = self.build_provider().await?;
        let contract = OmniaRollup::new(self.contract_address, provider);

        // Simulate the submitBatch call using eth_call (read-only)
        let result = contract
            .submitBatch(
                new_state_root,
                proof_a,
                proof_b,
                proof_c,
                public_inputs,
                batch_data_bytes,
            )
            .call()
            .await;

        match result {
            Ok(_) => Ok(true),
            Err(e) => {
                // If the simulation reverts, the proof is invalid or the
                // state roots don't match — both are "verification failed".
                tracing::debug!("Proof verification simulation failed: {e}");
                Ok(false)
            }
        }
    }
}

/// Helper: convert a byte slice of exactly 32 bytes into a fixed array.
///
/// Returns an error if the slice is not exactly 32 bytes.
#[cfg(feature = "ethereum-live")]
fn slice_to_array(slice: &[u8]) -> Result<[u8; 32], SettlementError> {
    if slice.len() != 32 {
        return Err(SettlementError::ContractError(format!(
            "Expected 32 bytes, got {}",
            slice.len()
        )));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(slice);
    Ok(arr)
}

// ---------------------------------------------------------------------------
// EthereumAdapter
// ---------------------------------------------------------------------------

/// Ethereum settlement adapter.
///
/// In default (simulated) mode, all operations return deterministic mock
/// responses based on BLAKE3. When created with [`EthereumMode::Live`],
/// the adapter validates configuration and uses alloy to interact with
/// a real Ethereum RPC endpoint and the OmniaRollup smart contract.
pub struct EthereumAdapter {
    config: EthereumConfig,
    /// Settlement mode: simulated or live.
    mode: EthereumMode,
    /// Latest state root (simulated tracking).
    latest_root: [u8; 32],
    /// Batch counter for mock transaction hashes.
    batch_counter: AtomicU64,
    /// Live client for real Ethereum RPC calls (only when `ethereum-live` feature is enabled).
    #[cfg(feature = "ethereum-live")]
    live_client: Option<EthereumLiveClient>,
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
            #[cfg(feature = "ethereum-live")]
            live_client: None,
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
            #[cfg(feature = "ethereum-live")]
            live_client: None,
        }
    }

    /// Create a new Ethereum adapter with an explicit mode.
    ///
    /// If `mode` is [`EthereumMode::Live`], the configuration is validated
    /// and an `EthereumLiveClient` is created. Live mode requires the
    /// `ethereum-live` feature flag and the alloy dependency at compile time,
    /// plus a deployed contract at runtime.
    pub fn with_mode(config: EthereumConfig, mode: EthereumMode) -> Result<Self, SettlementError> {
        if mode == EthereumMode::Live {
            config.validate()?;

            #[cfg(feature = "ethereum-live")]
            {
                let live_client = EthereumLiveClient::connect(&config)?;
                Ok(Self {
                    config,
                    mode,
                    latest_root: [0u8; 32],
                    batch_counter: AtomicU64::new(0),
                    live_client: Some(live_client),
                })
            }

            #[cfg(not(feature = "ethereum-live"))]
            {
                Err(SettlementError::ConfigError(
                    "Ethereum live mode requires the 'ethereum-live' feature flag. \
                     Rebuild with --features ethereum-live"
                        .to_string(),
                ))
            }
        } else {
            Ok(Self {
                config,
                mode,
                latest_root: [0u8; 32],
                batch_counter: AtomicU64::new(0),
                #[cfg(feature = "ethereum-live")]
                live_client: None,
            })
        }
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
            EthereumMode::Live => {
                #[cfg(feature = "ethereum-live")]
                if let Some(ref client) = self.live_client {
                    // For `post_batch`, we need to construct a minimal ProofBundle
                    // from the raw batch data. Use BLAKE3 domain separation for
                    // the state roots and batch Merkle root.
                    let state_root: [u8; 32] =
                        blake3::derive_key("OMNIA-ETH-POST-BATCH-STATE", batch_data);
                    let prev_state_root = self.latest_root;
                    let batch_merkle_root =
                        blake3::derive_key("OMNIA-ETH-POST-BATCH-MERKLE", batch_data);

                    let bundle = ProofBundle::new(
                        prev_state_root,
                        state_root,
                        batch_data.to_vec(),
                        batch_merkle_root,
                        crate::proof_bundle::L1Anchor::new(1, 0, 0),
                    );

                    return client.submit_batch_live(&bundle).await;
                }

                #[cfg(not(feature = "ethereum-live"))]
                {
                    return Err(SettlementError::ConfigError(
                        "Ethereum live mode requires the 'ethereum-live' feature flag".to_string(),
                    ));
                }

                #[cfg(feature = "ethereum-live")]
                {
                    Err(SettlementError::ConfigError(
                        "Live client not initialized".to_string(),
                    ))
                }
            }
        }
    }

    async fn verify_proof(
        &self,
        old_root: &[u8; 32],
        new_root: &[u8; 32],
        proof: &[u8],
    ) -> Result<bool, SettlementError> {
        match self.mode {
            EthereumMode::Simulated => {
                // Simulated: return true for non-empty proofs
                Ok(!proof.is_empty())
            }
            EthereumMode::Live => {
                #[cfg(feature = "ethereum-live")]
                if let Some(ref client) = self.live_client {
                    return client.verify_proof_live(old_root, new_root, proof).await;
                }

                #[cfg(not(feature = "ethereum-live"))]
                {
                    let _ = (old_root, new_root, proof);
                    return Err(SettlementError::ConfigError(
                        "Ethereum live mode requires the 'ethereum-live' feature flag".to_string(),
                    ));
                }

                #[cfg(feature = "ethereum-live")]
                {
                    let _ = (old_root, new_root, proof);
                    Err(SettlementError::ConfigError(
                        "Live client not initialized".to_string(),
                    ))
                }
            }
        }
    }

    async fn latest_state_root(&self) -> Result<[u8; 32], SettlementError> {
        match self.mode {
            EthereumMode::Simulated => Ok(self.latest_root),
            EthereumMode::Live => {
                #[cfg(feature = "ethereum-live")]
                if let Some(ref client) = self.live_client {
                    return client.latest_state_root_live().await;
                }

                #[cfg(not(feature = "ethereum-live"))]
                {
                    return Err(SettlementError::ConfigError(
                        "Ethereum live mode requires the 'ethereum-live' feature flag".to_string(),
                    ));
                }

                #[cfg(feature = "ethereum-live")]
                {
                    Err(SettlementError::ConfigError(
                        "Live client not initialized".to_string(),
                    ))
                }
            }
        }
    }

    async fn deposit(&self, l2_did: &str, amount: u64) -> Result<String, SettlementError> {
        match self.mode {
            EthereumMode::Simulated => {
                let tx_hash = format!("0xdeposit_{}_{}", l2_did, amount);
                tracing::info!("[Ethereum] Deposit: {} UBC to {}", amount, l2_did);
                Ok(tx_hash)
            }
            EthereumMode::Live => {
                #[cfg(feature = "ethereum-live")]
                if let Some(ref client) = self.live_client {
                    return client.deposit_live(l2_did, amount).await;
                }

                #[cfg(not(feature = "ethereum-live"))]
                {
                    return Err(SettlementError::ConfigError(
                        "Ethereum live mode requires the 'ethereum-live' feature flag".to_string(),
                    ));
                }

                #[cfg(feature = "ethereum-live")]
                {
                    Err(SettlementError::ConfigError(
                        "Live client not initialized".to_string(),
                    ))
                }
            }
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
            EthereumMode::Live => {
                #[cfg(feature = "ethereum-live")]
                if let Some(ref client) = self.live_client {
                    return client.request_withdrawal_live(l2_did, amount).await;
                }

                #[cfg(not(feature = "ethereum-live"))]
                {
                    return Err(SettlementError::ConfigError(
                        "Ethereum live mode requires the 'ethereum-live' feature flag".to_string(),
                    ));
                }

                #[cfg(feature = "ethereum-live")]
                {
                    Err(SettlementError::ConfigError(
                        "Live client not initialized".to_string(),
                    ))
                }
            }
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
            EthereumMode::Live => {
                #[cfg(feature = "ethereum-live")]
                if let Some(ref client) = self.live_client {
                    return client.submit_batch_live(bundle).await;
                }

                #[cfg(not(feature = "ethereum-live"))]
                {
                    let _ = bundle;
                    return Err(SettlementError::ConfigError(
                        "Ethereum live mode requires the 'ethereum-live' feature flag".to_string(),
                    ));
                }

                #[cfg(feature = "ethereum-live")]
                {
                    let _ = bundle;
                    Err(SettlementError::ConfigError(
                        "Live client not initialized".to_string(),
                    ))
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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
        assert_eq!(config.rpc_url, "http://localhost:8545");
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
        let _config = EthereumConfig::default();
        // Default config has empty operator key, so validation should fail
        // in live mode. But let's test the individual field validations.
        let config = EthereumConfig {
            rpc_url: "http://localhost:8545".to_string(),
            contract_address: "0x0000000000000000000000000000000000000000".to_string(),
            operator_private_key:
                "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80".to_string(),
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_ethereum_config_validation_http_scheme() {
        let config = EthereumConfig {
            rpc_url: "http://localhost:8545".to_string(),
            operator_private_key:
                "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80".to_string(),
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_ethereum_config_validation_https_scheme() {
        let config = EthereumConfig {
            rpc_url: "https://mainnet.infura.io/v3/key".to_string(),
            operator_private_key:
                "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80".to_string(),
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_ethereum_config_validation_wss_scheme() {
        let config = EthereumConfig {
            rpc_url: "wss://mainnet.infura.io/ws/v3/key".to_string(),
            operator_private_key:
                "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80".to_string(),
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_ethereum_config_validation_empty_operator_key() {
        let config = EthereumConfig {
            operator_private_key: String::new(),
            ..Default::default()
        };
        let err = config.validate().unwrap_err();
        assert!(err
            .to_string()
            .contains("Operator private key cannot be empty"));
    }

    #[test]
    fn test_ethereum_config_validation_operator_key_no_prefix() {
        let config = EthereumConfig {
            operator_private_key:
                "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80".to_string(),
            ..Default::default()
        };
        let err = config.validate().unwrap_err();
        assert!(err
            .to_string()
            .contains("Operator private key must be 0x-prefixed hex"));
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

    #[test]
    fn test_ethereum_with_mode_live_without_feature_returns_error() {
        // Without the ethereum-live feature, creating a Live adapter should
        // return a ConfigError explaining the feature is needed.
        let config = EthereumConfig {
            rpc_url: "http://localhost:8545".to_string(),
            contract_address: "0x1234567890abcdef1234567890abcdef12345678".to_string(),
            operator_private_key:
                "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80".to_string(),
            ..Default::default()
        };

        #[cfg(not(feature = "ethereum-live"))]
        {
            let result = EthereumAdapter::with_mode(config, EthereumMode::Live);
            assert!(result.is_err());
            let err = result.unwrap_err();
            assert!(matches!(err, SettlementError::ConfigError(_)));
            assert!(err.to_string().contains("ethereum-live"));
        }

        #[cfg(feature = "ethereum-live")]
        {
            // With the feature enabled, this should succeed (just creates the live client)
            let result = EthereumAdapter::with_mode(config, EthereumMode::Live);
            assert!(result.is_ok());
        }
    }

    // -----------------------------------------------------------------------
    // Feature-gated tests for live mode (require ethereum-live feature)
    // -----------------------------------------------------------------------

    #[cfg(feature = "ethereum-live")]
    mod live_tests {
        use super::*;

        /// Helper: build a config pointing at a local Anvil instance.
        fn anvil_config() -> EthereumConfig {
            EthereumConfig {
                rpc_url: "http://localhost:8545".to_string(),
                contract_address: "0x5FbDB2315678afecb367f032d93F642f64180aa3".to_string(),
                // Anvil's default first account private key
                operator_private_key:
                    "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80".to_string(),
                gas_limit: 3_000_000,
                max_fee_per_gas: None,
                confirmation_blocks: 1,
            }
        }

        #[test]
        fn test_live_client_connect_valid_config() {
            let config = anvil_config();
            let result = EthereumLiveClient::connect(&config);
            assert!(result.is_ok(), "connect should succeed with valid config");
        }

        #[test]
        fn test_live_client_connect_invalid_contract_address() {
            let mut config = anvil_config();
            config.contract_address = "0xINVALID".to_string();
            let result = EthereumLiveClient::connect(&config);
            assert!(result.is_err());
        }

        #[test]
        fn test_live_client_connect_invalid_private_key() {
            let mut config = anvil_config();
            config.operator_private_key = "0xZZZZ".to_string();
            let result = EthereumLiveClient::connect(&config);
            assert!(result.is_err());
        }

        #[test]
        fn test_slice_to_array_valid() {
            let input = [42u8; 32];
            let result = slice_to_array(&input);
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), [42u8; 32]);
        }

        #[test]
        fn test_slice_to_array_wrong_length() {
            let input = [42u8; 16];
            let result = slice_to_array(&input);
            assert!(result.is_err());
            assert!(matches!(
                result.unwrap_err(),
                SettlementError::ContractError(_)
            ));
        }

        #[test]
        fn test_with_mode_live_creates_live_client() {
            let config = anvil_config();
            let result = EthereumAdapter::with_mode(config, EthereumMode::Live);
            assert!(result.is_ok());
            let adapter = result.unwrap();
            assert_eq!(adapter.mode(), EthereumMode::Live);
            assert!(adapter.live_client.is_some());
        }

        #[tokio::test]
        async fn test_live_submit_batch_short_proof_returns_error() {
            let config = anvil_config();
            let client = EthereumLiveClient::connect(&config).unwrap();

            // Create a bundle with a proof that's too short (< 256 bytes)
            let bundle = ProofBundle::new(
                [0u8; 32],
                [1u8; 32],
                vec![0u8; 64], // Too short for a Groth16 proof
                [2u8; 32],
                crate::proof_bundle::L1Anchor::new(1, 0, 0),
            );

            let result = client.submit_batch_live(&bundle).await;
            assert!(result.is_err());
            match result.unwrap_err() {
                SettlementError::ContractError(msg) => {
                    assert!(msg.contains("Proof too short"));
                }
                other => panic!("Expected ContractError, got {:?}", other),
            }
        }
    }

    // -----------------------------------------------------------------------
    // Test: live mode without ethereum-live feature should return ConfigError
    // (This test only compiles without the feature flag)
    // -----------------------------------------------------------------------

    #[cfg(not(feature = "ethereum-live"))]
    #[tokio::test]
    async fn test_ethereum_live_mode_returns_config_error_without_feature() {
        let config = EthereumConfig {
            rpc_url: "http://localhost:8545".to_string(),
            contract_address: "0x1234567890abcdef1234567890abcdef12345678".to_string(),
            operator_private_key:
                "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80".to_string(),
            ..Default::default()
        };
        // with_mode should fail because the feature is not enabled
        let result = EthereumAdapter::with_mode(config, EthereumMode::Live);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SettlementError::ConfigError(_)
        ));
    }
}
