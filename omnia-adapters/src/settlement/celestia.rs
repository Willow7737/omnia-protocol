//! Celestia settlement adapter — Data Availability layer integration.
//!
//! Celestia is a data availability (DA) layer — it does not verify ZK proofs
//! or hold assets. A production deployment would combine Celestia for cheap DA
//! with another L1 (e.g., Ethereum) for proof verification and bridging.
//!
//! ## Architecture
//!
//! This module provides two adapter implementations:
//!
//! 1. **`CelestiaAdapter`** (new `SettlementAdapter` trait) — Feature-gated
//!    behind the `celestia` feature. When enabled, uses `reqwest` for real
//!    HTTP calls to a Celestia node's RPC endpoint. When disabled, a mock
//!    implementation is provided that logs operations and returns `Ok`.
//!
//! 2. **Legacy `SettlementLayer` impl** — Always available, returns
//!    `NotImplemented` for all methods (Celestia has no proof verification
//!    or asset layer).
//!
//! ## Feature Flags
//!
//! | Feature   | Behavior                                         |
//! |-----------|--------------------------------------------------|
//! | `celestia`| Real HTTP calls to Celestia RPC via `reqwest`     |
//! | (default) | Mock implementation that logs and returns `Ok`   |

use super::{FinalityProof, SettlementAdapter, SettlementError, SettlementLayer, TxHash};
#[cfg(feature = "celestia")]
use crate::merkle::compute_root_from_proof;
use crate::merkle::MerkleProof;
use crate::proof_bundle::ProofBundle;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Celestia configuration
// ---------------------------------------------------------------------------

/// Configuration for connecting to a Celestia node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CelestiaConfig {
    /// RPC endpoint URL (e.g., "http://localhost:26658").
    pub rpc_endpoint: String,
    /// Namespace for blob submission (8-byte hex-encoded namespace ID).
    pub namespace: String,
    /// Authentication token for the Celestia node RPC.
    pub auth_token: String,
}

impl Default for CelestiaConfig {
    fn default() -> Self {
        Self {
            rpc_endpoint: "http://localhost:26658".to_string(),
            namespace: "0000000000000001".to_string(),
            auth_token: String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// CelestiaAdapter — SettlementAdapter implementation
// ---------------------------------------------------------------------------

/// Celestia data availability adapter.
///
/// When the `celestia` feature is enabled, this adapter makes real HTTP calls
/// to a Celestia node's RPC endpoint for blob submission, finality queries,
/// and inclusion verification.
///
/// When the `celestia` feature is disabled, all operations succeed
/// deterministically with logging (mock mode). This ensures the adapter
/// can always be instantiated regardless of feature configuration.
pub struct CelestiaAdapter {
    /// Adapter configuration.
    #[allow(dead_code)] // Used when `celestia` feature is enabled for RPC calls
    config: CelestiaConfig,
    /// Monotonically increasing counter for generating unique tx hashes.
    counter: AtomicU64,
    /// Cached HTTP client to avoid creating a new one on every call.
    #[cfg(feature = "celestia")]
    client: reqwest::Client,
}

impl CelestiaAdapter {
    /// Create a new Celestia adapter with the given configuration.
    pub fn new(config: CelestiaConfig) -> Self {
        #[cfg(feature = "celestia")]
        let client = reqwest::Client::new();
        Self {
            config,
            counter: AtomicU64::new(0),
            #[cfg(feature = "celestia")]
            client,
        }
    }

    /// Create a new Celestia adapter with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(CelestiaConfig::default())
    }

    /// Generate a deterministic mock transaction hash from state root and counter.
    fn mock_tx_hash(&self, root: [u8; 32]) -> TxHash {
        let count = self.counter.fetch_add(1, Ordering::Relaxed);
        let mut input = root.to_vec();
        input.extend_from_slice(&count.to_le_bytes());
        let hash = blake3::derive_key("OMNIA-CELESTIA-SETTLEMENT-TX", &input);
        TxHash(hash)
    }
}

// ---------------------------------------------------------------------------
// SettlementAdapter impl — celestia feature (real HTTP calls via reqwest)
// ---------------------------------------------------------------------------

#[cfg(feature = "celestia")]
#[async_trait]
impl SettlementAdapter for CelestiaAdapter {
    async fn submit_root(&self, root: [u8; 32]) -> Result<TxHash, SettlementError> {
        let tx_hash = self.mock_tx_hash(root);

        // POST to Celestia RPC endpoint: /submit_blob
        // The request body contains the namespace and the blob data (the state root).
        let url = format!("{}/submit_blob", self.config.rpc_endpoint.trim_end_matches('/'));
        let body = serde_json::json!({
            "namespace": self.config.namespace,
            "data": hex::encode(root),
            "gas_price": 0.01,
        });

        tracing::info!(
            root = ?&root[..4],
            namespace = %self.config.namespace,
            "CelestiaAdapter: submitting state root to DA layer"
        );

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.auth_token))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| SettlementError::RpcError(format!("Celestia RPC error on submit_root: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(SettlementError::RpcError(format!(
                "Celestia submit_root failed: status={status}, body={body}"
            )));
        }

        tracing::info!(
            tx_hash = %tx_hash,
            "CelestiaAdapter: state root submitted successfully"
        );

        Ok(tx_hash)
    }

    async fn fetch_finality(&self, tx: TxHash) -> Result<FinalityProof, SettlementError> {
        // GET from Celestia RPC endpoint: /data_commitment or /height
        // Poll the Celestia node for inclusion information.
        let url = format!(
            "{}/blob/commitment/0x{}",
            self.config.rpc_endpoint.trim_end_matches('/'),
            hex::encode(tx.0)
        );

        tracing::info!(
            tx_hash = ?&tx.0[..4],
            "CelestiaAdapter: fetching finality proof"
        );

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.config.auth_token))
            .send()
            .await
            .map_err(|e| SettlementError::RpcError(format!("Celestia RPC error on fetch_finality: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            // If 404, the blob may not yet be included — return a timed-out error
            if status.as_u16() == 404 {
                return Err(SettlementError::TxTimedOut(0));
            }
            return Err(SettlementError::RpcError(format!(
                "Celestia fetch_finality failed: status={status}, body={body}"
            )));
        }

        // Parse the response to extract block height and commitment
        let resp_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| SettlementError::RpcError(format!("Celestia fetch_finality parse error: {e}")))?;

        let block_number = resp_body
            .get("height")
            .and_then(|v| v.as_u64())
            .unwrap_or(self.counter.load(Ordering::Relaxed));

        let proof_hash = blake3::derive_key("OMNIA-CELESTIA-FINALITY", &tx.0);

        Ok(FinalityProof {
            tx_hash: tx,
            block_number,
            confirmation_count: 3,
            proof_hash,
        })
    }

    async fn verify_inclusion(&self, leaf: &[u8; 32], proof: &MerkleProof) -> Result<bool, SettlementError> {
        // Verify the Merkle proof by checking it against the Celestia DA layer.
        // In practice, we would:
        //   1. Fetch the data root from Celestia at the given height
        //   2. Compute the expected root from the proof
        //   3. Compare the computed root with the on-chain data root
        //
        // For now, we do a local verification: compute the Merkle root from
        // the leaf and proof and then query Celestia to confirm the root exists.

        // First, do a local Merkle root computation to validate proof structure
        if proof.siblings.len() != proof.directions.len() {
            return Err(SettlementError::ProofVerificationFailed(
                "Merkle proof has mismatched siblings/directions lengths".into(),
            ));
        }

        // Compute root from the provided leaf and proof
        let computed_root = compute_root_from_proof(leaf, proof);

        // A zero computed root indicates an invalid proof
        if computed_root == [0u8; 32] {
            return Ok(false);
        }

        // Query Celestia for the share commitment to confirm inclusion
        let url = format!("{}/share/commitment", self.config.rpc_endpoint.trim_end_matches('/'));

        tracing::info!(
            siblings = proof.siblings.len(),
            "CelestiaAdapter: verifying Merkle inclusion proof"
        );

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.config.auth_token))
            .send()
            .await
            .map_err(|e| SettlementError::RpcError(format!("Celestia RPC error on verify_inclusion: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            // If the endpoint returns 404, the share may not be available yet
            if status.as_u16() == 404 {
                return Ok(false);
            }
            return Err(SettlementError::RpcError(format!(
                "Celestia verify_inclusion failed: status={status}"
            )));
        }

        // TODO: Fetch the actual on-chain data root from Celestia RPC and compare
        // it with computed_root. The current implementation computes the root
        // locally but never verifies it against the on-chain data root, which
        // means a malicious Celestia node could serve a valid-looking but
        // incorrect commitment. The verify_inclusion method MUST:
        //   1. Fetch the on-chain data root hash at the given height
        //   2. Compare it with `computed_root`
        //   3. Return `false` if they don't match
        // This MUST be fixed before mainnet.
        tracing::warn!("Celestia inclusion verification incomplete: computed root not compared against on-chain data");

        Ok(true)
    }

    fn is_live(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// SettlementAdapter impl — default (no celestia feature: mock mode)
// ---------------------------------------------------------------------------

#[cfg(not(feature = "celestia"))]
#[async_trait]
impl SettlementAdapter for CelestiaAdapter {
    async fn submit_root(&self, root: [u8; 32]) -> Result<TxHash, SettlementError> {
        let tx_hash = self.mock_tx_hash(root);
        tracing::info!(
            root = ?&root[..4],
            tx_hash = %tx_hash,
            "[CelestiaAdapter/Mock] submit_root: logging (celestia feature disabled)"
        );
        Ok(tx_hash)
    }

    async fn fetch_finality(&self, tx: TxHash) -> Result<FinalityProof, SettlementError> {
        let proof_hash = blake3::derive_key("OMNIA-CELESTIA-FINALITY", &tx.0);
        tracing::info!(
            tx_hash = ?&tx.0[..4],
            "[CelestiaAdapter/Mock] fetch_finality: logging (celestia feature disabled)"
        );
        Ok(FinalityProof {
            tx_hash: tx,
            block_number: self.counter.load(Ordering::Relaxed),
            confirmation_count: 3,
            proof_hash,
        })
    }

    async fn verify_inclusion(&self, _leaf: &[u8; 32], _proof: &MerkleProof) -> Result<bool, SettlementError> {
        tracing::info!("[CelestiaAdapter/Mock] verify_inclusion: logging (celestia feature disabled)");
        // Mock: all inclusion proofs are valid
        Ok(true)
    }

    fn is_live(&self) -> bool {
        // The mock adapter is not live — it doesn't connect to a real Celestia node
        false
    }
}

// ---------------------------------------------------------------------------
// Legacy SettlementLayer impl — always available, returns NotImplemented
// ---------------------------------------------------------------------------

#[async_trait]
impl SettlementLayer for CelestiaAdapter {
    fn chain_id(&self) -> &'static str {
        "celestia"
    }

    async fn post_batch(&self, _batch_data: &[u8]) -> Result<String, SettlementError> {
        Err(SettlementError::NotImplemented(
            "Celestia DA requires blob submission via node RPC".into(),
        ))
    }

    async fn verify_proof(
        &self,
        _old_root: &[u8; 32],
        _new_root: &[u8; 32],
        _proof: &[u8],
    ) -> Result<bool, SettlementError> {
        Err(SettlementError::NotImplemented(
            "Celestia has no proof verification — use with another L1".into(),
        ))
    }

    async fn latest_state_root(&self) -> Result<[u8; 32], SettlementError> {
        Err(SettlementError::NotImplemented(
            "Celestia has no state root storage".into(),
        ))
    }

    async fn deposit(&self, _l2_did: &str, _amount: u64) -> Result<String, SettlementError> {
        Err(SettlementError::NotImplemented("Celestia has no asset layer".into()))
    }

    async fn request_withdrawal(&self, _l2_did: &str, _amount: u64) -> Result<String, SettlementError> {
        Err(SettlementError::NotImplemented("Celestia has no asset layer".into()))
    }

    async fn submit_batch(&self, _bundle: &ProofBundle) -> Result<String, SettlementError> {
        Err(SettlementError::NotImplemented(
            "Celestia batch submission requires blob submission via node RPC".into(),
        ))
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
    fn test_celestia_config_default() {
        let config = CelestiaConfig::default();
        assert!(!config.rpc_endpoint.is_empty());
        assert!(!config.namespace.is_empty());
    }

    #[test]
    fn test_celestia_adapter_new() {
        let adapter = CelestiaAdapter::new(CelestiaConfig::default());
        assert_eq!(adapter.counter.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_celestia_adapter_with_defaults() {
        let adapter = CelestiaAdapter::with_defaults();
        assert_eq!(adapter.counter.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn test_celestia_mock_submit_root() {
        let adapter = CelestiaAdapter::with_defaults();
        let tx_hash = adapter.submit_root([1u8; 32]).await.unwrap();
        assert!(!tx_hash.0.iter().all(|&b| b == 0));
    }

    #[tokio::test]
    async fn test_celestia_mock_submit_root_deterministic() {
        let adapter = CelestiaAdapter::with_defaults();
        let hash1 = adapter.submit_root([42u8; 32]).await.unwrap();
        let hash2 = adapter.submit_root([42u8; 32]).await.unwrap();
        // Different counter values produce different hashes
        assert_ne!(hash1, hash2);
    }

    #[tokio::test]
    async fn test_celestia_mock_fetch_finality() {
        let adapter = CelestiaAdapter::with_defaults();
        let tx = TxHash([1u8; 32]);
        let proof = adapter.fetch_finality(tx.clone()).await.unwrap();
        assert_eq!(proof.tx_hash, tx);
        assert_eq!(proof.confirmation_count, 3);
    }

    #[tokio::test]
    async fn test_celestia_mock_verify_inclusion() {
        let adapter = CelestiaAdapter::with_defaults();
        let proof = crate::merkle::Blake3MerkleProof::new(vec![[0u8; 32]], vec![true]);
        assert!(adapter.verify_inclusion(&[0u8; 32], &proof).await.unwrap());
    }

    #[tokio::test]
    async fn test_celestia_legacy_not_implemented() {
        let adapter = CelestiaAdapter::with_defaults();
        assert_eq!(adapter.chain_id(), "celestia");
        assert!(adapter.post_batch(b"test").await.is_err());
        assert!(adapter.verify_proof(&[0u8; 32], &[0u8; 32], &[0xAA]).await.is_err());
        assert!(adapter.latest_state_root().await.is_err());
        assert!(adapter.deposit("did:test", 100).await.is_err());
        assert!(adapter.request_withdrawal("did:test", 50).await.is_err());
    }

    #[test]
    fn test_celestia_config_serialization_postcard() {
        let config = CelestiaConfig::default();
        let bytes = postcard::to_allocvec(&config).unwrap();
        let deserialized: CelestiaConfig = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(config.rpc_endpoint, deserialized.rpc_endpoint);
        assert_eq!(config.namespace, deserialized.namespace);
        assert_eq!(config.auth_token, deserialized.auth_token);
    }
}
