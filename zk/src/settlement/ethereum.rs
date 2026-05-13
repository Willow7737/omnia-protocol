//! Ethereum settlement adapter.
//!
//! Uses a Solidity smart contract for proof verification and bridging.
//! Batch data is posted as Ethereum calldata (Phase 0).
//! Future: migrate to EIP-4844 blobs for cheaper DA.

use super::{SettlementError, SettlementLayer};
use crate::proof_bundle::ProofBundle;
use async_trait::async_trait;

/// Ethereum settlement adapter.
///
/// This adapter interacts with the [`OmniaRollup`] Solidity contract
/// deployed on Ethereum. In Phase 0, all interactions are simulated —
/// no real RPC calls are made. Production will use `ethers-rs` for
/// on-chain transactions.
///
/// [`OmniaRollup`]: ../../contracts/ethereum/OmniaRollup.sol
pub struct EthereumAdapter {
    rpc_url: String,
    contract_address: String,
    operator_key: [u8; 32],
}

impl EthereumAdapter {
    /// Create a new Ethereum adapter.
    ///
    /// # Arguments
    /// * `rpc_url` — Ethereum JSON-RPC endpoint (e.g., "http://localhost:8545")
    /// * `contract_address` — Deployed OmniaRollup contract address
    /// * `operator_key` — Operator's private key (32 bytes)
    pub fn new(rpc_url: &str, contract_address: &str, operator_key: &[u8; 32]) -> Self {
        Self {
            rpc_url: rpc_url.to_string(),
            contract_address: contract_address.to_string(),
            operator_key: *operator_key,
        }
    }

    /// Get the configured RPC URL.
    #[allow(dead_code)]
    pub fn rpc_url(&self) -> &str {
        &self.rpc_url
    }

    /// Get the configured contract address.
    #[allow(dead_code)]
    pub fn contract_address(&self) -> &str {
        &self.contract_address
    }
}

#[async_trait]
impl SettlementLayer for EthereumAdapter {
    fn chain_id(&self) -> &'static str {
        "ethereum"
    }

    async fn post_batch(&self, batch_data: &[u8]) -> Result<String, SettlementError> {
        // Phase 0: Simulate posting to Ethereum.
        // In production, this would use ethers-rs to send a transaction
        // to the OmniaRollup contract with the batch data as calldata.
        let tx_hash = format!("0x{}", hex::encode(blake3::hash(batch_data).as_bytes()));
        tracing::info!("[Ethereum] Posted batch, tx: {}", &tx_hash[..16]);
        Ok(tx_hash)
    }

    async fn verify_proof(
        &self,
        _old_root: &[u8; 32],
        _new_root: &[u8; 32],
        proof: &[u8],
    ) -> Result<bool, SettlementError> {
        // Phase 0: Stub verification.
        // In production, this would call the Solidity verifier contract.
        // The contract uses a pre-compiled verifying key to check Groth16 proofs.
        let valid = !proof.is_empty() && proof.len() >= 32;
        tracing::info!("[Ethereum] Proof verification: {}", valid);
        Ok(valid)
    }

    async fn latest_state_root(&self) -> Result<[u8; 32], SettlementError> {
        // Phase 0: Return a dummy state root.
        // In production, query the Solidity contract's stateRoot() getter.
        Ok([0u8; 32])
    }

    async fn deposit(&self, l2_did: &str, amount: u64) -> Result<String, SettlementError> {
        let tx_hash = format!("0xdeposit_{}_{}", l2_did, amount);
        tracing::info!("[Ethereum] Deposit: {} UBC to {}", amount, l2_did);
        Ok(tx_hash)
    }

    async fn request_withdrawal(
        &self,
        l2_did: &str,
        amount: u64,
    ) -> Result<String, SettlementError> {
        let tx_hash = format!("0xwithdraw_{}_{}", l2_did, amount);
        tracing::info!(
            "[Ethereum] Withdrawal request: {} UBC from {}",
            amount,
            l2_did
        );
        Ok(tx_hash)
    }

    async fn submit_batch(&self, bundle: &ProofBundle) -> Result<String, SettlementError> {
        // Phase 0: Simulate submitting a proof bundle to Ethereum.
        // In production, this would serialize the bundle and send it to the
        // OmniaRollup contract, which verifies the ZK proof and updates the
        // committed state root on-chain.
        let bundle_bytes = bundle.to_bytes().map_err(|e| SettlementError::RpcError(e.to_string()))?;
        let tx_hash = format!("0x{}", hex::encode(blake3::hash(&bundle_bytes).as_bytes()));
        tracing::info!(
            "[Ethereum] Submitted proof bundle, state_root={}, tx: {}",
            hex::encode(&bundle.state_root[..8]),
            &tx_hash[..16]
        );
        Ok(tx_hash)
    }
}
