//! Proof generation and verification utilities.
//!
//! This module provides helper functions for working with ZK proofs
//! in the rollup context. Phase 0 uses hash-chain stubs; production
//! will integrate with arkworks for Groth16/PLONK proofs.

/// Minimum proof size in bytes for Phase 0 stub verification.
pub const MIN_PROOF_SIZE: usize = 32;

/// Generate a dummy proof for Phase 0.
///
/// In production, this would invoke the ZK prover (Groth16, PLONK, or STARK).
/// The dummy proof is a fixed-size byte vector that passes the Phase 0
/// stub verifier.
pub fn generate_dummy_proof() -> Vec<u8> {
    vec![0xAB; 192]
}

/// Verify a Phase 0 stub proof.
///
/// A stub proof is considered valid if it is non-empty and at least
/// [`MIN_PROOF_SIZE`] bytes long. This is a placeholder for the real
/// verification logic that will check Groth16/PLONK proofs on L1.
pub fn verify_stub_proof(proof: &[u8]) -> bool {
    !proof.is_empty() && proof.len() >= MIN_PROOF_SIZE
}

/// Compute a commitment hash for a batch of events.
///
/// This is used as a stand-in for a real ZK proof in Phase 0.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_dummy_proof() {
        let proof = generate_dummy_proof();
        assert_eq!(proof.len(), 192);
        assert!(proof.iter().all(|&b| b == 0xAB));
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
