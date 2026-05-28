//! Shared ZK proof verification utilities.
//!
//! Centralises the minimum-proof-length constant and layout-check helper
//! so that both the Biological and Computational shards use the same
//! definition. When the `real_verification` feature is disabled, every
//! ZK proof is **rejected** — the placeholder that previously accepted
//! any well-formed proof has been removed as a security hardening measure.

/// Minimum length of a Groth16 proof in bytes.
///
/// A Groth16 proof consists of three curve points:
/// - 2 × G1 point (48 bytes uncompressed each) = 96 bytes
/// - 1 × G2 point (96 bytes uncompressed) = 96 bytes
///
/// Total minimum: 192 bytes. We use the conservative lower bound of 128
/// to allow for compressed representations.
pub const MIN_GROTH16_PROOF_LEN: usize = 128;

/// Check if proof bytes have a valid ZK proof layout (length check only).
///
/// This does **not** verify cryptographic validity — it only rejects
/// obviously malformed proofs (too short to contain the expected
/// curve-point data). Real verification requires the `real_verification`
/// feature flag.
pub fn is_valid_zk_proof_layout(proof_bytes: &[u8]) -> bool {
    proof_bytes.len() >= MIN_GROTH16_PROOF_LEN
}

/// Verify a ZK proof. When the `real_verification` feature is disabled,
/// this always returns an error to prevent accepting unverified proofs.
pub fn verify_zk_proof(proof_bytes: &[u8], context: &str) -> Result<(), crate::shard::ShardError> {
    if !is_valid_zk_proof_layout(proof_bytes) {
        return Err(crate::shard::ShardError::ValidationFailed(
            format!("{}: invalid ZK proof layout ({} bytes, minimum {})", context, proof_bytes.len(), MIN_GROTH16_PROOF_LEN)
        ));
    }

    #[cfg(feature = "real_verification")]
    {
        // Real verification logic would go here.
        // For now, still reject until real verification is implemented.
        todo!("Real ZK proof verification not yet implemented")
    }

    #[cfg(not(feature = "real_verification"))]
    {
        let _ = context;
        Err(crate::shard::ShardError::ValidationFailed(
            format!("{}: ZK proof verification requires 'real_verification' feature", context)
        ))
    }
}
