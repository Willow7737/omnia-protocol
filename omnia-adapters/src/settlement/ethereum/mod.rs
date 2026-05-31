//! Ethereum settlement layer adapter.
//!
//! Provides two settlement modes via the legacy [`SettlementLayer`] trait:
//!
//! - **Simulated** (default): Uses BLAKE3-based mock responses for testing
//! - **Live** (`ethereum-live` feature): Connects to a real Ethereum RPC endpoint
//!   via the `alloy` crate and interacts with the OmniaRollup smart contract.
//!
//! Additionally, this module provides `EthereumSettlementAdapter` which
//! implements the new [`SettlementAdapter`](super::SettlementAdapter) trait
//! for the hybrid architecture (feature-gated behind `ethereum-live`).

#[cfg(feature = "ethereum-live")]
pub mod live;

#[cfg(feature = "ethereum-live")]
pub use live::EthereumSettlementAdapter;

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "ethereum-live")]
use tokio::sync::OnceCell;
use zeroize::Zeroize;

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
// OmniaRollup contract bindings (feature-gated, legacy SettlementLayer)
// ---------------------------------------------------------------------------

#[cfg(feature = "ethereum-live")]
alloy::sol! {
    #[sol(rpc)]
    OmniaRollupLegacy,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EthereumConfig {
    /// Ethereum JSON-RPC endpoint URL.
    pub rpc_url: String,
    /// OmniaRollup contract address (0x-prefixed, 42 chars).
    pub contract_address: String,
    /// Operator private key (0x-prefixed, 64 hex chars).
    #[serde(skip)]
    pub operator_private_key: String,
    /// Gas limit for batch submission transactions.
    pub gas_limit: u64,
    /// Maximum fee per gas (in wei, as a decimal string).
    pub max_fee_per_gas: Option<String>,
    /// Number of confirmation blocks to wait for.
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
    /// Validate the configuration for live mode.
    pub fn validate(&self) -> Result<(), SettlementError> {
        if self.rpc_url.is_empty() {
            return Err(SettlementError::ConfigError("RPC URL cannot be empty".to_string()));
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
        if self.contract_address == "0x0000000000000000000000000000000000000000" {
            return Err(SettlementError::ConfigError(
                "Contract address cannot be the zero address".to_string(),
            ));
        }
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

impl Drop for EthereumConfig {
    fn drop(&mut self) {
        // Zeroize the private key to prevent it from remaining in memory
        self.operator_private_key.zeroize();
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
    /// Live mode with real Ethereum RPC calls.
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
#[cfg(feature = "ethereum-live")]
pub struct EthereumLiveClient {
    rpc_url: String,
    /// Operator private key, zeroized on drop.
    operator_private_key: zeroize::Zeroizing<String>,
    contract_address: Address,
    gas_limit: u64,
    #[allow(dead_code)]
    max_fee_per_gas: Option<String>,
    confirmation_blocks: u64,
    /// Cached wallet to avoid re-parsing the private key on every call.
    wallet: OnceCell<PrivateKeySigner>,
}

#[cfg(feature = "ethereum-live")]
impl EthereumLiveClient {
    /// Create a new live client from configuration.
    pub fn connect(config: &EthereumConfig) -> Result<Self, SettlementError> {
        let contract_address: Address = config
            .contract_address
            .parse()
            .map_err(|e| SettlementError::ConfigError(format!("Invalid contract address: {e}")))?;

        let _: PrivateKeySigner = config
            .operator_private_key
            .parse()
            .map_err(|e| SettlementError::ConfigError(format!("Invalid operator private key: {e}")))?;

        Ok(Self {
            rpc_url: config.rpc_url.clone(),
            // SECURITY NOTE: The clone() creates a non-zeroized copy of the private key
            // before wrapping in Zeroizing. The original String in config remains in memory
            // until dropped. For better security, parse the key directly from config without
            // cloning, or zeroize the config field after first use.
            // TODO: Use Zeroizing<String> in the config struct itself.
            operator_private_key: zeroize::Zeroizing::new(config.operator_private_key.clone()),
            contract_address,
            gas_limit: config.gas_limit,
            max_fee_per_gas: config.max_fee_per_gas.clone(),
            confirmation_blocks: config.confirmation_blocks,
            wallet: OnceCell::new(),
        })
    }

    /// Get or initialize the cached wallet.
    ///
    /// Parses the private key once and caches the result for subsequent calls.
    async fn get_wallet(&self) -> Result<&PrivateKeySigner, SettlementError> {
        self.wallet
            .get_or_try_init(|| async {
                let key_str: &str = self.operator_private_key.as_ref();
                let key: PrivateKeySigner = key_str
                    .parse()
                    .map_err(|e| SettlementError::ConfigError(format!("Invalid operator key: {e}")))?;
                Ok(key)
            })
            .await
    }

    /// Build an alloy provider with wallet signing.
    async fn build_provider(&self) -> Result<impl Provider, SettlementError> {
        let wallet = self.get_wallet().await?.clone();

        let provider = ProviderBuilder::new().wallet(wallet).connect_http(
            self.rpc_url
                .parse()
                .map_err(|e| SettlementError::ConfigError(format!("Invalid RPC URL: {e}")))?,
        );

        Ok(provider)
    }

    /// Submit a batch to the OmniaRollup contract.
    pub async fn submit_batch_live(&self, bundle: &ProofBundle) -> Result<String, SettlementError> {
        let provider = self.build_provider().await?;
        let contract = OmniaRollupLegacy::new(self.contract_address, provider);

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

        let public_inputs = vec![
            U256::from_be_bytes::<32>(bundle.prev_state_root),
            U256::from_be_bytes::<32>(bundle.state_root),
            U256::from_be_bytes::<32>(bundle.batch_merkle_root),
        ];

        let new_state_root = B256::from(bundle.state_root);
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
        let builder = builder.gas(self.gas_limit);

        let pending_tx = builder
            .send()
            .await
            .map_err(|e| SettlementError::TxFailed(format!("submitBatch send failed: {e}")))?;

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
                "submitBatch transaction reverted: 0x{tx_hash:x}"
            )))
        }
    }

    /// Fetch the latest state root from the OmniaRollup contract.
    pub async fn latest_state_root_live(&self) -> Result<[u8; 32], SettlementError> {
        let provider = self.build_provider().await?;
        let contract = OmniaRollupLegacy::new(self.contract_address, provider);

        let root = contract
            .stateRoot()
            .call()
            .await
            .map_err(|e| SettlementError::ContractError(format!("stateRoot call failed: {e}")))?;

        Ok(root.0)
    }

    /// Deposit ETH into the rollup.
    pub async fn deposit_live(&self, l2_did: &str, amount: u64) -> Result<String, SettlementError> {
        let provider = self.build_provider().await?;
        let contract = OmniaRollupLegacy::new(self.contract_address, provider);

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
    pub async fn request_withdrawal_live(&self, l2_did: &str, amount: u64) -> Result<String, SettlementError> {
        let provider = self.build_provider().await?;
        let contract = OmniaRollupLegacy::new(self.contract_address, provider);

        let did_hash = blake3::derive_key("OMNIA-DID-MAP", l2_did.as_bytes());
        let l2_did_bytes = B256::from_slice(&did_hash);

        let builder = contract.requestWithdrawal(l2_did_bytes, U256::from(amount));

        let pending_tx = builder
            .send()
            .await
            .map_err(|e| SettlementError::TxFailed(format!("requestWithdrawal send failed: {e}")))?;

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

    /// Verify a Groth16 proof on-chain via eth_call simulation.
    ///
    /// # Arguments
    ///
    /// * `old_root` — The state root before the batch
    /// * `new_root` — The state root after the batch
    /// * `proof` — The serialized Groth16 proof (at least 256 bytes)
    /// * `batch_merkle_root` — The properly computed batch Merkle root
    ///   (NOT derived from the proof itself)
    pub async fn verify_proof_live(
        &self,
        old_root: &[u8; 32],
        new_root: &[u8; 32],
        proof: &[u8],
        batch_merkle_root: &[u8; 32],
    ) -> Result<bool, SettlementError> {
        if proof.len() < 256 {
            return Ok(false);
        }

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

        let public_inputs = vec![
            U256::from_be_bytes::<32>(*old_root),
            U256::from_be_bytes::<32>(*new_root),
            U256::from_be_bytes::<32>(*batch_merkle_root),
        ];

        let new_state_root = B256::from(*new_root);
        let batch_data_bytes = Bytes::copy_from_slice(proof);

        let provider = self.build_provider().await?;
        let contract = OmniaRollupLegacy::new(self.contract_address, provider);

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
                tracing::debug!("Proof verification simulation failed: {e}");
                Ok(false)
            }
        }
    }
}

/// Helper: convert a byte slice of exactly 32 bytes into a fixed array.
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
// EthereumAdapter (legacy SettlementLayer implementation)
// ---------------------------------------------------------------------------

/// Ethereum settlement adapter (legacy `SettlementLayer` implementation).
///
/// In default (simulated) mode, all operations return deterministic mock
/// responses based on BLAKE3. When created with [`EthereumMode::Live`],
/// the adapter validates configuration and uses alloy to interact with
/// a real Ethereum RPC endpoint and the OmniaRollup smart contract.
pub struct EthereumAdapter {
    config: EthereumConfig,
    mode: EthereumMode,
    latest_root: [u8; 32],
    batch_counter: AtomicU64,
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
    /// Create a new simulated Ethereum adapter.
    pub fn new(rpc_url: &str, contract_address: &str, _operator_key: &[u8; 32]) -> Self {
        Self {
            config: EthereumConfig {
                rpc_url: rpc_url.to_string(),
                contract_address: contract_address.to_string(),
                operator_private_key: String::new(),
                gas_limit: 1_000_000,
                max_fee_per_gas: None,
                confirmation_blocks: 3,
            },
            mode: EthereumMode::Simulated,
            latest_root: [0u8; 32],
            batch_counter: AtomicU64::new(0),
            #[cfg(feature = "ethereum-live")]
            live_client: None,
        }
    }

    /// Create a new Ethereum adapter from configuration (simulated mode).
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

    /// Generate a deterministic mock transaction hash.
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
                    let state_root: [u8; 32] = blake3::derive_key("OMNIA-ETH-POST-BATCH-STATE", batch_data);
                    let prev_state_root = self.latest_root;
                    let batch_merkle_root = blake3::derive_key("OMNIA-ETH-POST-BATCH-MERKLE", batch_data);

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
                    Err(SettlementError::ConfigError("Live client not initialized".to_string()))
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
                tracing::warn!("EthereumMode::Simulated accepts any non-empty proof — do not use in production!");
                Ok(!proof.is_empty())
            }
            EthereumMode::Live => {
                #[cfg(feature = "ethereum-live")]
                if let Some(ref client) = self.live_client {
                    // SECURITY: The batch_merkle_root must NOT be derived from the proof
                    // itself (that would be fabrication). Ideally it would be passed as a
                    // parameter, but the SettlementLayer trait doesn't support it yet.
                    // For now, derive from old_root || new_root which the verifier knows
                    // independently — still not ideal but better than deriving from the proof.
                    let mut root_input = [0u8; 64];
                    root_input[..32].copy_from_slice(old_root);
                    root_input[32..].copy_from_slice(new_root);
                    let batch_merkle_root = blake3::derive_key("OMNIA-ETH-BATCH-MERKLE", &root_input);
                    return client
                        .verify_proof_live(old_root, new_root, proof, &batch_merkle_root)
                        .await;
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
                    Err(SettlementError::ConfigError("Live client not initialized".to_string()))
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
                    Err(SettlementError::ConfigError("Live client not initialized".to_string()))
                }
            }
        }
    }

    async fn deposit(&self, l2_did: &str, amount: u64) -> Result<String, SettlementError> {
        match self.mode {
            EthereumMode::Simulated => Ok(format!("0xdeposit_{l2_did}_{amount}")),
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
                    Err(SettlementError::ConfigError("Live client not initialized".to_string()))
                }
            }
        }
    }

    async fn request_withdrawal(&self, l2_did: &str, amount: u64) -> Result<String, SettlementError> {
        match self.mode {
            EthereumMode::Simulated => Ok(format!("0xwithdraw_{l2_did}_{amount}")),
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
                    Err(SettlementError::ConfigError("Live client not initialized".to_string()))
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
                    Err(SettlementError::ConfigError("Live client not initialized".to_string()))
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
        assert_eq!(config.contract_address, "0x0000000000000000000000000000000000000000");
    }

    #[test]
    fn test_ethereum_config_validation_empty_rpc() {
        let config = EthereumConfig {
            rpc_url: "".to_string(),
            contract_address: "0x0000000000000000000000000000000000000000".to_string(),
            operator_private_key: String::new(),
            gas_limit: 1_000_000,
            max_fee_per_gas: None,
            confirmation_blocks: 3,
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_ethereum_config_validation_invalid_scheme() {
        let config = EthereumConfig {
            rpc_url: "ftp://invalid".to_string(),
            contract_address: "0x0000000000000000000000000000000000000000".to_string(),
            operator_private_key: String::new(),
            gas_limit: 1_000_000,
            max_fee_per_gas: None,
            confirmation_blocks: 3,
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_ethereum_config_validation_valid() {
        let config = EthereumConfig {
            rpc_url: "http://localhost:8545".to_string(),
            contract_address: "0x1234567890123456789012345678901234567890".to_string(),
            operator_private_key: "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80".to_string(),
            gas_limit: 1_000_000,
            max_fee_per_gas: None,
            confirmation_blocks: 3,
        };
        assert!(config.validate().is_ok());
    }

    #[tokio::test]
    async fn test_ethereum_simulated_post_batch() {
        let adapter = EthereumAdapter::new("http://localhost:8545", "0x1234", &[0u8; 32]);
        let result = adapter.post_batch(b"test batch data").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_ethereum_simulated_verify_proof() {
        let adapter = EthereumAdapter::new("http://localhost:8545", "0x1234", &[0u8; 32]);
        assert!(adapter.verify_proof(&[0u8; 32], &[1u8; 32], &[0xAA]).await.unwrap());
        assert!(!adapter.verify_proof(&[0u8; 32], &[1u8; 32], &[]).await.unwrap());
    }

    #[tokio::test]
    async fn test_ethereum_simulated_latest_state_root() {
        let adapter = EthereumAdapter::new("http://localhost:8545", "0x1234", &[0u8; 32]);
        assert_eq!(adapter.latest_state_root().await.unwrap(), [0u8; 32]);
    }

    #[tokio::test]
    async fn test_ethereum_simulated_deposit() {
        let adapter = EthereumAdapter::new("http://localhost:8545", "0x1234", &[0u8; 32]);
        let result = adapter.deposit("did:test", 100).await.unwrap();
        assert!(result.contains("did:test"));
    }

    #[tokio::test]
    async fn test_ethereum_simulated_submit_batch() {
        let adapter = EthereumAdapter::new("http://localhost:8545", "0x1234", &[0u8; 32]);
        let bundle = ProofBundle::new(
            [0u8; 32],
            [1u8; 32],
            vec![0xBB; 192],
            [2u8; 32],
            crate::proof_bundle::L1Anchor::new(1, 100, 1000),
        );
        let result = adapter.submit_batch(&bundle).await.unwrap();
        assert!(result.starts_with("0x"));
    }

    #[test]
    fn test_ethereum_mode_display() {
        assert_eq!(format!("{}", EthereumMode::Simulated), "simulated");
        assert_eq!(format!("{}", EthereumMode::Live), "live");
    }
}
