//! Verify `fetch_finality` against a real confirmed testnet4 transaction.
//!
//! This example does NOT submit a new transaction — it calls `fetch_finality`
//! on an already-confirmed OP_RETURN anchor (`cbe1da89…`) and validates the
//! returned proof fields.  Use it to prove the full adapter round-trip
//! (submit → confirm → fetch_finality → PASSED) without waiting for a new
//! block.
//!
//! ```
//! export OMNIA_BITCOIN_RPC_URL=http://127.0.0.1:48332
//! export OMNIA_BITCOIN_RPC_USER=omnia
//! export OMNIA_BITCOIN_RPC_PASSWORD=<your-password>
//! export OMNIA_BITCOIN_MIN_CONFIRMATIONS=1
//! cargo run -p omnia-adapters --features bitcoin-live --example bitcoin_testnet4_finality_verify
//! ```

use omnia_adapters::settlement::bitcoin::{BitcoinConfig, BitcoinSettlementAdapter};
use omnia_adapters::settlement::SettlementAdapter;

/// The confirmed testnet4 anchor tx from our e2e run.
/// OP_RETURN contained: OMNIA1 + deadbeefcafebabe…
const CONFIRMED_TXID: &str = "cbe1da89872c718f4c2553efeaaf212a287d9b16962191f12ba8fc4b146c64e6";

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = BitcoinConfig::from_env()?;
    let adapter = BitcoinSettlementAdapter::new(config)?;

    // Decode the known txid into TxHash(bytes32).
    let mut tx_bytes = [0u8; 32];
    hex::decode_to_slice(CONFIRMED_TXID, &mut tx_bytes)?;
    let tx_hash = omnia_adapters::settlement::TxHash(tx_bytes);

    println!("Verifying fetch_finality against confirmed testnet4 tx:");
    println!("  TXID: {CONFIRMED_TXID}");
    println!();

    let proof = adapter.fetch_finality(tx_hash.clone()).await?;

    println!("  Block number:    {}", proof.block_number);
    println!("  Confirmations:   {}", proof.confirmation_count);
    println!("  TX hash:         {}", proof.tx_hash);
    println!("  Proof hash:      0x{}", hex::encode(proof.proof_hash));
    println!();

    assert!(
        proof.confirmation_count >= 1,
        "Expected >= 1 confirmation, got {}",
        proof.confirmation_count
    );
    assert!(
        proof.block_number > 0,
        "Expected block number > 0, got {}",
        proof.block_number
    );

    // Verify the proof_hash is derived correctly: blake3 keyed with "OMNIA-BTC-FINALITY" over the tx bytes.
    let expected_hash = blake3::derive_key("OMNIA-BTC-FINALITY", &tx_hash.0);
    assert_eq!(proof.proof_hash, expected_hash, "proof_hash mismatch");

    println!("PASSED — fetch_finality round-trip verified against real testnet4 block");
    Ok(())
}
