//! End-to-end test for the Bitcoin settlement adapter.
//!
//! Works against regtest (mines a block on demand) or testnet4 (polls for real
//! confirmation).  The mode is selected via the `OMNIA_BITCOIN_NETWORK`
//! environment variable, defaulting to `regtest`.
//!
//! ## regtest (default)
//!
//! ```
//! export OMNIA_BITCOIN_RPC_URL=http://127.0.0.1:18443
//! export OMNIA_BITCOIN_RPC_USER=omnia
//! export OMNIA_BITCOIN_RPC_PASSWORD=devpassword
//! export OMNIA_BITCOIN_MIN_CONFIRMATIONS=1
//! cargo run -p omnia-adapters --features bitcoin-live --example bitcoin_regtest_e2e
//! ```
//!
//! ## testnet4
//!
//! ```
//! export OMNIA_BITCOIN_RPC_URL=http://127.0.0.1:48332
//! export OMNIA_BITCOIN_RPC_USER=omnia
//! export OMNIA_BITCOIN_RPC_PASSWORD=<your-password>
//! export OMNIA_BITCOIN_MIN_CONFIRMATIONS=1
//! OMNIA_BITCOIN_NETWORK=testnet4 \
//!   cargo run -p omnia-adapters --features bitcoin-live --example bitcoin_regtest_e2e
//! ```

use bitcoincore_rpc::{Auth, Client, RpcApi};
use omnia_adapters::settlement::bitcoin::{BitcoinConfig, BitcoinSettlementAdapter};
use omnia_adapters::settlement::SettlementAdapter;
use serde_json::json;
use std::time::Duration;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = BitcoinConfig::from_env()?;
    let adapter = BitcoinSettlementAdapter::new(config)?;

    let network = std::env::var("OMNIA_BITCOIN_NETWORK").unwrap_or_else(|_| "regtest".into());
    println!("Running e2e test on network: {network}");

    // --- Step 1: submit_root ---
    let test_root: [u8; 32] = [
        0xde, 0xad, 0xbe, 0xef, 0xca, 0xfe, 0xba, 0xbe, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a,
        0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18,
    ];

    println!("Submitting state root (OP_RETURN anchor)...");
    let tx_hash = adapter.submit_root(test_root).await?;
    println!("  TXID: {tx_hash}");

    // Bitcoin Core RPC expects bare hex (no 0x prefix).
    let txid_hex: String = format!("{tx_hash}")
        .strip_prefix("0x")
        .unwrap_or(&format!("{tx_hash}"))
        .to_string();

    // --- Step 2: wait for confirmation ---
    let url = std::env::var("OMNIA_BITCOIN_RPC_URL")?;
    let user = std::env::var("OMNIA_BITCOIN_RPC_USER")?;
    let pass = std::env::var("OMNIA_BITCOIN_RPC_PASSWORD")?;
    let rpc = Client::new(&url, Auth::UserPass(user, pass))?;

    match network.as_str() {
        "regtest" => {
            println!("\nMining 1 regtest block...");
            let addr: String = rpc.call("getnewaddress", &[])?;
            let _: Vec<String> = rpc.call("generatetoaddress", &[json!(1), json!(addr)])?;
            println!("  Block mined.");
        }
        _ => {
            // testnet4 / any public network — poll until the tx is confirmed.
            println!("\nWaiting for transaction to be confirmed (polling every 30s)...");
            let timeout = Duration::from_secs(1800); // 30 minutes hard limit
            let poll_interval = Duration::from_secs(30);
            let start = std::time::Instant::now();

            loop {
                let tx: serde_json::Value = rpc.call("gettransaction", &[json!(txid_hex)])?;
                let confirmations = tx["confirmations"].as_i64().unwrap_or(0);
                println!("  [{:?}] confirmations: {confirmations}", start.elapsed());
                if confirmations >= 1 {
                    break;
                }
                if start.elapsed() > timeout {
                    return Err("Timed out waiting for transaction confirmation after 1800s".into());
                }
                std::thread::sleep(poll_interval);
            }
        }
    }

    // --- Step 3: fetch_finality ---
    println!("\nFetching finality proof...");
    let proof = adapter.fetch_finality(tx_hash.clone()).await?;
    println!("  Block number: {}", proof.block_number);
    println!("  Confirmations: {}", proof.confirmation_count);
    println!("  TX hash: {}", proof.tx_hash);
    println!("  Proof hash: 0x{}", hex::encode(proof.proof_hash));

    // --- Verify ---
    assert!(proof.confirmation_count >= 1, "Expected >= 1 confirmation");
    assert!(proof.block_number > 0, "Expected block number > 0");

    println!("\n✓ End-to-end test PASSED");
    Ok(())
}
