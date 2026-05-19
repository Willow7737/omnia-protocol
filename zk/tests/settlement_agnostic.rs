#![allow(clippy::unwrap_used)]
//! Integration tests for settlement-agnostic ZK-rollup.
//!
//! These tests verify that:
//! - The Ethereum adapter works correctly in simulated mode
//! - The Ethereum adapter's new config/mode API functions correctly
//! - The Bitcoin, Solana, and Celestia stubs return `NotImplemented`
//! - The rollup operator works with any `SettlementLayer` adapter
//! - All four adapters implement the same trait and can be used as trait objects

use omnia_substrate::{Substrate, SubstrateConfig};
use omnia_zk::operator::RollupOperator;
use omnia_zk::settlement::{
    BitcoinAdapter, CelestiaAdapter, EthereumAdapter, EthereumConfig, EthereumMode,
    SettlementError, SettlementLayer, SolanaAdapter,
};
use std::sync::Arc;
use tokio::sync::RwLock;

fn test_node(id: u8) -> [u8; 32] {
    let mut node = [0u8; 32];
    node[0] = id;
    node
}

// ---------------------------------------------------------------------------
// Ethereum adapter — backward-compatible constructor
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_ethereum_adapter_chain_id() {
    let adapter = EthereumAdapter::new("http://localhost:8545", "0x1234", &[0u8; 32]);
    assert_eq!(adapter.chain_id(), "ethereum");
}

#[tokio::test]
async fn test_ethereum_adapter_post_batch() {
    let adapter = EthereumAdapter::new("http://localhost:8545", "0x1234", &[0u8; 32]);
    let result = adapter.post_batch(b"test batch data").await;
    assert!(result.is_ok());
    let tx = result.unwrap();
    assert!(tx.starts_with("0x"));
}

#[tokio::test]
async fn test_ethereum_adapter_verify_proof_simulated_non_empty() {
    let adapter = EthereumAdapter::new("http://localhost:8545", "0x1234", &[0u8; 32]);
    let proof = vec![0u8; 64];
    let result = adapter.verify_proof(&[0u8; 32], &[1u8; 32], &proof).await;
    // Simulated mode: non-empty proofs return Ok(true)
    assert!(
        result.is_ok(),
        "verify_proof should succeed in simulated mode"
    );
    assert!(
        result.unwrap(),
        "non-empty proof should verify as true in simulated mode"
    );
}

#[tokio::test]
async fn test_ethereum_adapter_verify_proof_simulated_empty() {
    let adapter = EthereumAdapter::new("http://localhost:8545", "0x1234", &[0u8; 32]);
    let result = adapter.verify_proof(&[0u8; 32], &[1u8; 32], &[]).await;
    // Simulated mode: empty proof returns Ok(false)
    assert!(
        result.is_ok(),
        "verify_proof should succeed in simulated mode"
    );
    assert!(
        !result.unwrap(),
        "empty proof should verify as false in simulated mode"
    );
}

#[tokio::test]
async fn test_ethereum_adapter_deposit() {
    let adapter = EthereumAdapter::new("http://localhost:8545", "0x1234", &[0u8; 32]);
    let result = adapter.deposit("did:omnia:test", 100).await;
    assert!(result.is_ok());
    let tx = result.unwrap();
    assert!(tx.starts_with("0x"));
}

#[tokio::test]
async fn test_ethereum_adapter_withdrawal() {
    let adapter = EthereumAdapter::new("http://localhost:8545", "0x1234", &[0u8; 32]);
    let result = adapter.request_withdrawal("did:omnia:test", 50).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_ethereum_adapter_latest_state_root() {
    let adapter = EthereumAdapter::new("http://localhost:8545", "0x1234", &[0u8; 32]);
    let root = adapter.latest_state_root().await.unwrap();
    assert_eq!(root, [0u8; 32]);
}

// ---------------------------------------------------------------------------
// Ethereum adapter — new config/mode API
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_ethereum_adapter_from_config() {
    let config = EthereumConfig {
        rpc_url: "http://localhost:8545".to_string(),
        contract_address: "0x1234567890abcdef1234567890abcdef12345678".to_string(),
        gas_limit: 2_000_000,
        ..Default::default()
    };
    let adapter = EthereumAdapter::from_config(config);
    assert_eq!(adapter.mode(), EthereumMode::Simulated);
    assert_eq!(adapter.chain_id(), "ethereum");

    let result = adapter.post_batch(b"config-based batch").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_ethereum_adapter_with_mode_simulated() {
    let config = EthereumConfig::default();
    let adapter = EthereumAdapter::with_mode(config, EthereumMode::Simulated).unwrap();
    assert_eq!(adapter.mode(), EthereumMode::Simulated);
}

#[tokio::test]
async fn test_ethereum_adapter_with_mode_live_validates() {
    let config = EthereumConfig {
        rpc_url: "".to_string(),
        ..Default::default()
    };
    let result = EthereumAdapter::with_mode(config, EthereumMode::Live);
    assert!(result.is_err());
    match result.unwrap_err() {
        SettlementError::ConfigError(msg) => assert!(msg.contains("RPC URL")),
        other => panic!("Expected ConfigError, got: {}", other),
    }
}

#[tokio::test]
async fn test_ethereum_adapter_with_mode_live_requires_feature() {
    // Live mode now requires the 'ethereum-live' feature flag and a valid
    // operator private key. Without the feature or with invalid config,
    // it returns a ConfigError instead of NotImplemented at call time.
    let config = EthereumConfig {
        rpc_url: "http://localhost:8545".to_string(),
        contract_address: "0x1234567890abcdef1234567890abcdef12345678".to_string(),
        operator_private_key: "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
            .to_string(),
        ..Default::default()
    };
    let result = EthereumAdapter::with_mode(config, EthereumMode::Live);
    // Without the ethereum-live feature, this returns ConfigError
    // With the ethereum-live feature, it should succeed (creates a live client)
    #[cfg(not(feature = "ethereum-live"))]
    {
        assert!(result.is_err());
        match result.unwrap_err() {
            SettlementError::ConfigError(msg) => {
                assert!(msg.contains("ethereum-live"));
            }
            other => panic!("Expected ConfigError, got: {}", other),
        }
    }
    #[cfg(feature = "ethereum-live")]
    {
        assert!(result.is_ok());
        let adapter = result.unwrap();
        assert_eq!(adapter.mode(), EthereumMode::Live);
    }
}

#[test]
fn test_ethereum_config_validation_valid() {
    let config = EthereumConfig {
        rpc_url: "http://localhost:8545".to_string(),
        contract_address: "0x0000000000000000000000000000000000000000".to_string(),
        operator_private_key: "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
            .to_string(),
        ..Default::default()
    };
    assert!(config.validate().is_ok());
}

#[test]
fn test_ethereum_config_validation_invalid_rpc() {
    let config = EthereumConfig {
        rpc_url: "ftp://bad".to_string(),
        ..Default::default()
    };
    assert!(config.validate().is_err());
}

#[test]
fn test_ethereum_config_validation_invalid_contract() {
    let config = EthereumConfig {
        contract_address: "0xbad".to_string(),
        ..Default::default()
    };
    assert!(config.validate().is_err());
}

// ---------------------------------------------------------------------------
// Other adapters — stubs
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_bitcoin_adapter_not_implemented() {
    let adapter = BitcoinAdapter;
    assert_eq!(adapter.chain_id(), "bitcoin");

    let result = adapter.post_batch(b"test").await;
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("Not implemented") || err_msg.contains("NotImplemented"),
        "Unexpected error: {}",
        err_msg
    );
}

#[tokio::test]
async fn test_bitcoin_adapter_verify_proof_not_implemented() {
    let adapter = BitcoinAdapter;
    let result = adapter
        .verify_proof(&[0u8; 32], &[1u8; 32], &[0u8; 64])
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_solana_adapter_not_implemented() {
    let adapter = SolanaAdapter;
    assert_eq!(adapter.chain_id(), "solana");

    let result = adapter.verify_proof(&[0u8; 32], &[1u8; 32], &[]).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_celestia_adapter_not_implemented() {
    let adapter = CelestiaAdapter;
    assert_eq!(adapter.chain_id(), "celestia");

    let result = adapter.deposit("did:omnia:test", 100).await;
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Rollup operator integration
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_operator_with_ethereum_adapter() {
    let config = SubstrateConfig::with_network_size(test_node(1), 1);
    let substrate = Arc::new(RwLock::new(Substrate::new(config)));

    let adapter = Box::new(EthereumAdapter::new(
        "http://localhost:8545",
        "0x1234",
        &[0u8; 32],
    ));
    let mut operator = RollupOperator::new(substrate, adapter, 10);

    // No events in the graph, so run_batch should succeed with "No new events"
    let result = operator.run_batch().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_settlement_layer_trait_object() {
    // Verify that all adapters can be used as trait objects
    let adapters: Vec<Box<dyn SettlementLayer>> = vec![
        Box::new(EthereumAdapter::new(
            "http://localhost:8545",
            "0x1234",
            &[0u8; 32],
        )),
        Box::new(BitcoinAdapter),
        Box::new(SolanaAdapter),
        Box::new(CelestiaAdapter),
    ];

    let ids: Vec<&str> = adapters.iter().map(|a| a.chain_id()).collect();
    assert_eq!(ids, vec!["ethereum", "bitcoin", "solana", "celestia"]);
}

#[tokio::test]
async fn test_operator_chain_id() {
    let config = SubstrateConfig::with_network_size(test_node(1), 1);
    let substrate = Arc::new(RwLock::new(Substrate::new(config)));

    let adapter = Box::new(EthereumAdapter::new(
        "http://localhost:8545",
        "0x1234",
        &[0u8; 32],
    ));
    let operator = RollupOperator::new(substrate, adapter, 10);

    assert_eq!(operator.chain_id(), "ethereum");
}
