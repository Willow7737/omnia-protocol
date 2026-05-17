#![allow(clippy::unwrap_used)]
//! Integration tests for settlement-agnostic ZK-rollup.
//!
//! These tests verify that:
//! - The Ethereum adapter works correctly (simulated)
//! - The Bitcoin, Solana, and Celestia stubs return `NotImplemented`
//! - The rollup operator works with any `SettlementLayer` adapter
//! - All four adapters implement the same trait and can be used as trait objects

use omnia_substrate::{Substrate, SubstrateConfig};
use omnia_zk::operator::RollupOperator;
use omnia_zk::settlement::{
    BitcoinAdapter, CelestiaAdapter, EthereumAdapter, SettlementLayer, SolanaAdapter,
};
use std::sync::Arc;
use tokio::sync::RwLock;

fn test_node(id: u8) -> [u8; 32] {
    let mut node = [0u8; 32];
    node[0] = id;
    node
}

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
async fn test_ethereum_adapter_verify_proof() {
    let adapter = EthereumAdapter::new("http://localhost:8545", "0x1234", &[0u8; 32]);
    let proof = vec![0u8; 64];
    let valid = adapter
        .verify_proof(&[0u8; 32], &[1u8; 32], &proof)
        .await
        .unwrap();
    assert!(valid);
}

#[tokio::test]
async fn test_ethereum_adapter_verify_proof_empty_fails() {
    let adapter = EthereumAdapter::new("http://localhost:8545", "0x1234", &[0u8; 32]);
    let valid = adapter
        .verify_proof(&[0u8; 32], &[1u8; 32], &[])
        .await
        .unwrap();
    assert!(!valid);
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
