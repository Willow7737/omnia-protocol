//! Verify fetch_finality against a real confirmed testnet4 transaction.
//!
//! This proves the read path of the Bitcoin settlement adapter works on
//! a public network — no mining control, real block confirmations.
//!
//! ```
//! export OMNIA_BITCOIN_RPC_URL=http://127.0.0.1:48332
//! export OMNIA_BITCOIN_RPC_USER=omnia
//! export OMNIA_BITCOIN_RPC_PASSWORD=<password>
//! export OMNIA_BITCOIN_MIN_CONFIRMATIONS=1
//! OMNIA_BITCOIN_VERIFY_TXID=cbe1da89872c718f4c2553efeaaf212a287d9b16962191f12ba8fc4b146c64e6 \
//!   cargo run -p omnia-adapters --features bitcoin-live --example bitcoin_testnet4_verify
//! ```

use omnia_adapters::settlement::bitcoin::{BitcoinConfig, BitcoinSettlementAdapter};
use omnia_adapters::settlement::{SettlementAdapter, TxHash};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = BitcoinConfig::from_env()?;
    let adapter = BitcoinSettlementAdapter::new(config)?;

    let txid_str = std::env::var("OMNIA_BITCOIN_VERIFY_TXID")
        .map_err(|_| "missing env var OMNIA_BITCOIN_VERIFY_TXID")?;

    // Strip 0x prefix if present — TxHash is raw 32 bytes.
    let clean = txid_str.strip_prefix("0x").unwrap_or(&txid_str);
    let mut bytes = [0u8; 32];
    hex::decode_to_slice(clean, &mut bytes)?;
    let tx_hash = TxHash(bytes);

    println!("Verifying finality for tx: 0x{clean}");

    let proof = adapter.fetch_finality(tx_hash).await?;

    println!("  Block number:      {}", proof.block_number);
    println!("  Confirmations:     {}", proof.confirmation_count);
    println!("  TX hash:           {}", proof.tx_hash);
    println!("  Proof hash:        0x{}", hex::encode(proof.proof_hash));

    assert!(proof.confirmation_count >= 1, "Expected >= 1 confirmation");
    assert!(proof.block_number > 0, "Expected block number > 0");

    println!("\n✓ testnet4 finality verification PASSED");
    Ok(())
}
