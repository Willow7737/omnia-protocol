//! Live Ethereum settlement adapter backed by alloy.
//!
//! This adapter implements [`SettlementAdapter`] using the `alloy` crate
//! to interact with a real Ethereum RPC endpoint and the OmniaRollup smart
//! contract. It is only compiled when:
//!
//! 1. The `ethereum-live` feature flag is enabled
//! 2. The Rust compiler version is >= 1.91 (required by alloy >= 1.7)
//!
//! The `build.rs` script in this crate detects the compiler version and
//! sets the `rustc_version_compatible` cfg flag accordingly.

use crate::merkle::MerkleProof;
use crate::settlement::ethereum::EthereumConfig;
use crate::settlement::{FinalityProof, SettlementAdapter, SettlementError, TxHash};

// ---------------------------------------------------------------------------
// Alloy imports (feature-gated)
// ---------------------------------------------------------------------------

#[cfg(feature = "ethereum-live")]
use alloy::primitives::{Address, B256};
#[cfg(feature = "ethereum-live")]
use alloy::providers::{Provider, ProviderBuilder};
#[cfg(feature = "ethereum-live")]
use alloy::signers::local::PrivateKeySigner;

// ---------------------------------------------------------------------------
// OmniaRollup contract bindings (feature-gated)
// ---------------------------------------------------------------------------

#[cfg(feature = "ethereum-live")]
alloy::sol! {
    #[sol(rpc)]
    OmniaRollup,
    r#"[
        {
            "type": "function",
            "name": "submitRoot",
            "inputs": [
                {"name": "newStateRoot", "type": "bytes32"}
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
            "type": "event",
            "name": "StateUpdated",
            "inputs": [
                {"name": "oldRoot", "type": "bytes32", "indexed": true},
                {"name": "newRoot", "type": "bytes32", "indexed": true},
                {"name": "batchIndex", "type": "uint256", "indexed": false}
            ],
            "anonymous": false
        }
    ]"#
}

// ---------------------------------------------------------------------------
// EthereumSettlementAdapter
// ---------------------------------------------------------------------------

/// Live Ethereum settlement adapter backed by alloy.
///
/// Connects to an Ethereum RPC endpoint and submits state roots to the
/// OmniaRollup smart contract. This adapter requires:
///
/// - The `ethereum-live` feature flag at compile time
/// - Rust compiler >= 1.91 (alloy dependency requirement)
/// - A running Ethereum node (or Anvil for local testing)
/// - A deployed OmniaRollup contract
///
/// For testing and CI, use [`MockSettlementAdapter`](super::MockSettlementAdapter)
/// instead, which requires no external dependencies.
#[cfg(feature = "ethereum-live")]
pub struct EthereumSettlementAdapter {
    config: EthereumConfig,
    contract_address: Address,
}

#[cfg(feature = "ethereum-live")]
impl EthereumSettlementAdapter {
    /// Create a new live Ethereum settlement adapter.
    ///
    /// Validates the configuration and parses the contract address.
    /// Does **not** connect to the network — connection is lazy.
    pub fn new(config: EthereumConfig) -> Result<Self, SettlementError> {
        config.validate()?;

        let contract_address: Address = config
            .contract_address
            .parse()
            .map_err(|e| SettlementError::ConfigError(format!("Invalid contract address: {e}")))?;

        Ok(Self {
            config,
            contract_address,
        })
    }

    /// Build an alloy provider with wallet signing.
    async fn build_provider(&self) -> Result<impl Provider, SettlementError> {
        let wallet: PrivateKeySigner = self
            .config
            .operator_private_key
            .parse()
            .map_err(|e| SettlementError::ConfigError(format!("Invalid operator key: {e}")))?;

        let provider = ProviderBuilder::new().wallet(wallet).connect_http(
            self.config
                .rpc_url
                .parse()
                .map_err(|e| SettlementError::ConfigError(format!("Invalid RPC URL: {e}")))?,
        );

        Ok(provider)
    }
}

#[cfg(feature = "ethereum-live")]
#[async_trait::async_trait]
impl SettlementAdapter for EthereumSettlementAdapter {
    async fn submit_root(&self, root: [u8; 32]) -> Result<TxHash, SettlementError> {
        let provider = self.build_provider().await?;
        let contract = OmniaRollup::new(self.contract_address, provider);

        let new_state_root = B256::from(root);

        let builder = contract.submitRoot(new_state_root);
        let builder = builder.gas(self.config.gas_limit);

        let pending_tx = builder
            .send()
            .await
            .map_err(|e| SettlementError::TxFailed(format!("submitRoot send failed: {e}")))?;

        let tx_hash = *pending_tx.tx_hash();
        let hash_bytes: [u8; 32] = tx_hash.into();

        // Wait for confirmations
        let receipt = pending_tx
            .with_required_confirmations(self.config.confirmation_blocks)
            .get_receipt()
            .await
            .map_err(|_e| SettlementError::TxTimedOut(self.config.confirmation_blocks))?;

        if receipt.status() {
            Ok(TxHash(hash_bytes))
        } else {
            Err(SettlementError::TxFailed(format!(
                "submitRoot transaction reverted: 0x{}",
                hex::encode(hash_bytes)
            )))
        }
    }

    async fn fetch_finality(&self, tx: TxHash) -> Result<FinalityProof, SettlementError> {
        let provider = self.build_provider().await?;

        // Get the transaction receipt to determine block number
        let receipt = provider
            .get_transaction_receipt(B256::from(tx.0))
            .await
            .map_err(|e| SettlementError::RpcError(format!("Failed to fetch receipt: {e}")))?
            .ok_or_else(|| SettlementError::RpcError("Transaction receipt not found".to_string()))?;

        let block_number = receipt.block_number.unwrap_or(0);
        let proof_hash = blake3::derive_key("OMNIA-ETH-FINALITY", &tx.0);

        Ok(FinalityProof {
            tx_hash: tx,
            block_number,
            confirmation_count: self.config.confirmation_blocks,
            proof_hash,
        })
    }

    async fn verify_inclusion(&self, proof: &MerkleProof) -> Result<bool, SettlementError> {
        // For the Ethereum adapter, inclusion verification is done on-chain
        // by the smart contract. We delegate to the contract's state root
        // verification. For now, we compute the root from the proof and
        // compare against the on-chain state root.
        let provider = self.build_provider().await?;
        let contract = OmniaRollup::new(self.contract_address, provider);

        // Fetch on-chain state root
        let on_chain_root = contract
            .stateRoot()
            .call()
            .await
            .map_err(|e| SettlementError::ContractError(format!("stateRoot call failed: {e}")))?;

        // Compute root from the provided proof
        let leaf = proof.siblings.first().copied().unwrap_or([0u8; 32]);
        let computed = crate::merkle::compute_root_from_proof(&leaf, proof);

        Ok(computed == on_chain_root.0)
    }

    fn is_live(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "ethereum-live"))]
mod tests {
    use super::*;

    #[test]
    fn test_ethereum_settlement_adapter_config_validation() {
        let config = EthereumConfig {
            rpc_url: "".to_string(),
            ..Default::default()
        };
        let result = EthereumSettlementAdapter::new(config);
        assert!(result.is_err());
    }
}
