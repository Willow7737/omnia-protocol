//! Proof generation and verification utilities.
//!
//! This module provides helper functions for working with ZK proofs
//! in the rollup context. Production code uses Groth16 verification
//! via the [`prover`] module; legacy stub functions
//! are retained for test compatibility only.

use crate::prover::{self, Proof, ProverError, VerifyingKey};

/// Minimum proof size in bytes for compatibility checks.
///
/// A Groth16 proof on Bn254 consists of three group elements
/// (2 G1 + 1 G2), which serializes to a fixed size. This constant
/// is kept for backward compatibility with existing proof-bundle
/// integrity checks.
pub const MIN_PROOF_SIZE: usize = 32;

/// Verify a Groth16 proof against a verifying key and public inputs.
///
/// This is the production verification function. It deserializes the
/// proof from bytes and delegates to [`prover::verify_proof`].
///
/// # Arguments
///
/// * `vk` — The Groth16 verifying key
/// * `public_inputs` — The public inputs (expected new state root)
/// * `proof_bytes` — The serialized Groth16 proof
///
/// # Errors
///
/// Returns [`ProverError`] if deserialization or verification fails.
pub fn verify_groth16_proof(
    vk: &VerifyingKey,
    public_inputs: &[ark_bn254::Fr],
    proof_bytes: &[u8],
) -> Result<bool, ProverError> {
    let proof: Proof = prover::deserialize_proof(proof_bytes)?;
    prover::verify_proof(vk, public_inputs, &proof)
}

/// Compute a commitment hash for a batch of events.
///
/// This is used as a stand-in for a real ZK proof commitment.
/// The commitment is a BLAKE3 hash of the old state root, the
/// serialized events, and the new state root.
pub fn compute_batch_commitment(
    old_root: &[u8; 32],
    new_root: &[u8; 32],
    batch_data: &[u8],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(old_root);
    hasher.update(batch_data);
    hasher.update(new_root);
    *hasher.finalize().as_bytes()
}

// ---------------------------------------------------------------------------
// Test-only stub functions (legacy Phase 0 compatibility)
// ---------------------------------------------------------------------------

/// Generate a dummy proof for test compatibility.
///
/// This produces a fixed-size byte vector that passes the legacy
/// stub verifier. It is **not** a real ZK proof and must only be
/// used in tests.
#[cfg(test)]
pub fn generate_dummy_proof() -> Vec<u8> {
    vec![0xBB; 192]
}

/// Verify a Phase 0 stub proof (test-only).
///
/// A stub proof is considered valid if it is non-empty and at least
/// [`MIN_PROOF_SIZE`] bytes long. This is a legacy placeholder for
/// testing only — production code uses [`verify_groth16_proof`].
#[cfg(test)]
pub fn verify_stub_proof(proof: &[u8]) -> bool {
    !proof.is_empty() && proof.len() >= MIN_PROOF_SIZE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_dummy_proof() {
        let proof = generate_dummy_proof();
        assert_eq!(proof.len(), 192);
        assert!(proof.iter().all(|&b| b == 0xBB));
    }

    #[test]
    fn test_verify_stub_proof_valid() {
        let proof = vec![0u8; 64];
        assert!(verify_stub_proof(&proof));
    }

    #[test]
    fn test_verify_stub_proof_empty() {
        assert!(!verify_stub_proof(&[]));
    }

    #[test]
    fn test_verify_stub_proof_too_short() {
        let proof = vec![0u8; 16];
        assert!(!verify_stub_proof(&proof));
    }

    #[test]
    fn test_compute_batch_commitment() {
        let commitment1 = compute_batch_commitment(&[0u8; 32], &[1u8; 32], b"test data");
        let commitment2 = compute_batch_commitment(&[0u8; 32], &[1u8; 32], b"test data");
        let commitment3 = compute_batch_commitment(&[0u8; 32], &[2u8; 32], b"test data");

        assert_eq!(commitment1, commitment2); // Same inputs → same output
        assert_ne!(commitment1, commitment3); // Different inputs → different output
    }
}
