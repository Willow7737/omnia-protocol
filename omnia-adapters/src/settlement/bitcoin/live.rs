//! Live Bitcoin settlement adapter backed by `bitcoincore-rpc`.
//!
//! Implements [`SettlementAdapter`] by anchoring state roots to a Bitcoin
//! node via OP_RETURN transactions, rather than verifying proofs on-chain —
//! Bitcoin has no general-purpose smart contracts, so there is no
//! equivalent to the OmniaRollup contract's `submitBatch`. This adapter
//! therefore does not override `submit_batch_with_proof`; it inherits the
//! trait's default, which fails closed with a clear "not supported" error —
//! the same honest-failure posture the live Ethereum adapter uses for its
//! own disabled `submit_root` (see AUDIT-2026-07 C3, #341).
//!
//! Requires:
//! - The `bitcoin-live` feature flag at compile time
//! - A running Bitcoin Core node with an RPC-accessible wallet
//!   (regtest or testnet strongly recommended before mainnet)
//!
//! For testing and CI, use `MockSettlementAdapter` instead, which requires
//! no external dependencies.

use crate::merkle::MerkleProof;
use crate::settlement::{FinalityProof, SettlementAdapter, SettlementError, TxHash};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::OnceCell;

/// Configuration for connecting to a Bitcoin Core node's JSON-RPC interface.
///
/// Load via [`BitcoinConfig::from_env`] or construct directly.
///
/// | Env var | Purpose |
/// |---|---|
/// | `OMNIA_BITCOIN_RPC_URL` | e.g. `http://127.0.0.1:18443` (regtest) |
/// | `OMNIA_BITCOIN_RPC_USER` | RPC username |
/// | `OMNIA_BITCOIN_RPC_PASSWORD` | RPC password |
/// | `OMNIA_BITCOIN_MIN_CONFIRMATIONS` | optional, defaults to `1` |
#[derive(Debug, Clone)]
pub struct BitcoinConfig {
    /// JSON-RPC endpoint URL (e.g. `http://127.0.0.1:18443` for regtest).
    pub rpc_url: String,
    /// RPC username (from `bitcoin.conf` `rpcuser`).
    pub rpc_user: String,
    /// RPC password (from `bitcoin.conf` `rpcpassword`).
    pub rpc_password: String,
    /// Minimum block confirmations before a transaction is considered final.
    pub min_confirmations: u64,
}

impl BitcoinConfig {
    /// Validate that required config fields are non-empty.
    pub fn validate(&self) -> Result<(), SettlementError> {
        if self.rpc_url.is_empty() {
            return Err(SettlementError::ConfigError("rpc_url must not be empty".into()));
        }
        if self.rpc_user.is_empty() || self.rpc_password.is_empty() {
            return Err(SettlementError::ConfigError(
                "rpc_user and rpc_password must not be empty".into(),
            ));
        }
        Ok(())
    }

    /// Load configuration from environment variables (see table above).
    pub fn from_env() -> Result<Self, SettlementError> {
        let get = |k: &str| std::env::var(k).map_err(|_| SettlementError::ConfigError(format!("missing env var {k}")));
        let config = Self {
            rpc_url: get("OMNIA_BITCOIN_RPC_URL")?,
            rpc_user: get("OMNIA_BITCOIN_RPC_USER")?,
            rpc_password: get("OMNIA_BITCOIN_RPC_PASSWORD")?,
            min_confirmations: std::env::var("OMNIA_BITCOIN_MIN_CONFIRMATIONS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1),
        };
        config.validate()?;
        Ok(config)
    }
}

/// Live Bitcoin settlement adapter backed by `bitcoincore-rpc`.
pub struct BitcoinSettlementAdapter {
    config: BitcoinConfig,
    client: OnceCell<Arc<Client>>,
}

impl BitcoinSettlementAdapter {
    /// Create a new live Bitcoin settlement adapter. Does **not** connect
    /// to the node — connection is lazy, on first use.
    pub fn new(config: BitcoinConfig) -> Result<Self, SettlementError> {
        config.validate()?;
        Ok(Self {
            config,
            client: OnceCell::new(),
        })
    }

    async fn client(&self) -> Result<Arc<Client>, SettlementError> {
        self.client
            .get_or_try_init(|| async {
                let url = self.config.rpc_url.clone();
                let auth = Auth::UserPass(self.config.rpc_user.clone(), self.config.rpc_password.clone());
                tokio::task::spawn_blocking(move || Client::new(&url, auth))
                    .await
                    .map_err(|e| SettlementError::RpcError(format!("client init task panicked: {e}")))?
                    .map_err(|e| SettlementError::RpcError(format!("failed to connect to bitcoind: {e}")))
                    .map(Arc::new)
            })
            .await
            .cloned()
    }

    /// Run a JSON-RPC call on a blocking thread (the underlying client is
    /// synchronous) and map errors into [`SettlementError`].
    async fn rpc_call<T>(&self, method: &'static str, params: Vec<serde_json::Value>) -> Result<T, SettlementError>
    where
        T: for<'de> serde::de::Deserialize<'de> + Send + 'static,
    {
        let client = self.client().await?;
        tokio::task::spawn_blocking(move || client.call::<T>(method, &params))
            .await
            .map_err(|e| SettlementError::RpcError(format!("{method} task panicked: {e}")))?
            .map_err(|e| SettlementError::RpcError(format!("{method} failed: {e}")))
    }
}

#[async_trait::async_trait]
impl SettlementAdapter for BitcoinSettlementAdapter {
    /// Anchor a state root as an OP_RETURN output in a Bitcoin transaction.
    ///
    /// Builds, funds, signs, and broadcasts via the node's own wallet —
    /// `fundrawtransaction` handles UTXO selection, change, and fee
    /// estimation, so this adapter does not implement its own coin
    /// selection.
    async fn submit_root(&self, root: [u8; 32]) -> Result<TxHash, SettlementError> {
        let mut data = Vec::with_capacity(6 + 32);
        data.extend_from_slice(b"OMNIA1");
        data.extend_from_slice(&root);

        let raw_hex: String = self
            .rpc_call(
                "createrawtransaction",
                vec![json!([]), json!([{ "data": hex::encode(&data) }])],
            )
            .await?;

        let funded: serde_json::Value = self.rpc_call("fundrawtransaction", vec![json!(raw_hex)]).await?;
        let funded_hex = funded["hex"]
            .as_str()
            .ok_or_else(|| SettlementError::RpcError("fundrawtransaction returned no hex".into()))?;

        let signed: serde_json::Value = self
            .rpc_call("signrawtransactionwithwallet", vec![json!(funded_hex)])
            .await?;
        if !signed["complete"].as_bool().unwrap_or(false) {
            return Err(SettlementError::TxFailed(
                "wallet could not fully sign the anchor transaction".into(),
            ));
        }
        let signed_hex = signed["hex"]
            .as_str()
            .ok_or_else(|| SettlementError::RpcError("signrawtransactionwithwallet returned no hex".into()))?;

        let txid: String = self.rpc_call("sendrawtransaction", vec![json!(signed_hex)]).await?;

        let mut bytes = [0u8; 32];
        hex::decode_to_slice(&txid, &mut bytes)
            .map_err(|e| SettlementError::RpcError(format!("node returned a malformed txid: {e}")))?;
        Ok(TxHash(bytes))
    }

    // No override for `submit_batch_with_proof` — Bitcoin has no on-chain
    // verifier, so the trait's default ("this settlement adapter does not
    // support proof-carrying batch submission") is the honest answer here.

    /// Look up confirmations and block height for a previously anchored
    /// transaction.
    async fn fetch_finality(&self, tx: TxHash) -> Result<FinalityProof, SettlementError> {
        let txid = hex::encode(tx.0);
        let info: serde_json::Value = self.rpc_call("gettransaction", vec![json!(txid)]).await?;

        // bitcoind reports `confirmations` as signed: negative means the
        // transaction was conflicted out of the chain.
        let confirmations = info["confirmations"].as_i64().unwrap_or(0);
        if confirmations < 0 {
            return Err(SettlementError::TxFailed(
                "anchor transaction was conflicted out of the chain".into(),
            ));
        }
        let confirmations = confirmations as u64;
        if confirmations < self.config.min_confirmations {
            return Err(SettlementError::TxTimedOut(self.config.min_confirmations));
        }

        let block_number = info["blockheight"].as_u64().unwrap_or(0);
        let proof_hash = blake3::derive_key("OMNIA-BTC-FINALITY", &tx.0);

        Ok(FinalityProof {
            tx_hash: tx,
            block_number,
            confirmation_count: confirmations,
            proof_hash,
        })
    }

    /// Not yet implemented. Bitcoin has no queryable "current state root"
    /// the way the OmniaRollup contract does — recovering the last
    /// anchored root needs either a scan of this wallet's OP_RETURN
    /// history or a locally tracked pointer, and neither is wired up yet.
    /// Fails closed rather than risk comparing against a stale root.
    async fn verify_inclusion(&self, _leaf: &[u8; 32], _proof: &MerkleProof) -> Result<bool, SettlementError> {
        Err(SettlementError::NotImplemented(
            "Bitcoin inclusion verification needs an OP_RETURN history scan — not wired up yet".into(),
        ))
    }

    fn is_live(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_config() {
        let config = BitcoinConfig {
            rpc_url: String::new(),
            rpc_user: String::new(),
            rpc_password: String::new(),
            min_confirmations: 1,
        };
        assert!(BitcoinSettlementAdapter::new(config).is_err());
    }
}
