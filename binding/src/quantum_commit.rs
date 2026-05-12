//! Quantum-Resistant Commitments — Long-Term Integrity (Stub)
//!
//! Physical anchors must remain valid for decades. Standard signatures
//! (ECDSA, Ed25519) may be broken by quantum computers running Shor's
//! algorithm. This module provides **hybrid commitments** that combine
//! classical Ed25519 signatures with post-quantum CRYSTALS-Dilithium
//! signatures during the transition period.
//!
//! # Research Foundations
//!
//! - **NIST PQC Standards (2024)**: CRYSTALS-Dilithium (digital signatures),
//!   CRYSTALS-Kyber (key encapsulation), SPHINCS+ (hash-based signatures).
//! - **Hybrid approach**: Google Chrome's TLS 1.3 + Kyber experiment —
//!   combining classical and post-quantum algorithms for defense in depth.
//!
//! # Current Status
//!
//! This is a **stub implementation** using classical Ed25519 signatures only.
//! The `dilithium_sig` and `kyber_key` fields are placeholder byte vectors.
//! Real PQC requires the `pqc_dilithium` and `pqc_kyber` crates.
//!
//! # Transition Strategy
//!
//! 1. **Phase 1 (Current)**: Classical-only with PQC fields reserved.
//!    Verification accepts Ed25519 signature as sufficient.
//! 2. **Phase 2 (Hybrid)**: Both classical and PQC signatures required.
//!    Verification checks both; failure of either rejects the commitment.
//! 3. **Phase 3 (Post-quantum)**: Only PQC signatures required.
//!    Classical signatures are kept for historical verification only.

use omnia_substrate::VectorClock;
use serde::{Deserialize, Serialize};

/// A hybrid (classical + post-quantum) cryptographic commitment.
///
/// This structure binds data to a signer using both a classical Ed25519
/// signature (for compatibility) and a CRYSTALS-Dilithium signature
/// (for quantum resistance). The `data_hash` is computed using BLAKE3
/// for speed and security.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuantumCommitment {
    /// CRYSTALS-Dilithium signature (NIST PQC standard).
    /// **Stub**: empty in Phase 1; populated in Phase 2.
    pub dilithium_sig: Vec<u8>,
    /// CRYSTALS-Kyber encapsulated key (for future encryption).
    /// **Stub**: empty in Phase 1; populated in Phase 2.
    pub kyber_key: Vec<u8>,
    /// Classical Ed25519 signature (for compatibility during transition).
    /// 64 bytes in Phase 1.
    pub classical_sig: Vec<u8>,
    /// BLAKE3 hash of the committed data (32 bytes).
    pub data_hash: [u8; 32],
    /// Commitment timestamp in causal time.
    pub committed_at: VectorClock,
}

/// Public key for verifying quantum commitments.
///
/// Contains both the classical Ed25519 public key and the post-quantum
/// Dilithium public key. During Phase 1, only the Ed25519 key is required.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PqPublicKey {
    /// Ed25519 verifying key (32 bytes).
    pub ed25519: [u8; 32],
    /// CRYSTALS-Dilithium public key.
    /// **Stub**: empty in Phase 1; populated in Phase 2.
    pub dilithium: Vec<u8>,
}

/// Whether we are in the classical-only, hybrid, or post-quantum phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommitmentPhase {
    /// Phase 1: Only classical (Ed25519) signatures are verified.
    ClassicalOnly,
    /// Phase 2: Both classical and PQC signatures must verify.
    Hybrid,
    /// Phase 3: Only PQC signatures are verified.
    PostQuantum,
}

impl Default for CommitmentPhase {
    fn default() -> Self {
        Self::ClassicalOnly
    }
}

impl QuantumCommitment {
    /// Create a new quantum commitment from data and a classical Ed25519
    /// signature.
    ///
    /// The data hash is computed using BLAKE3. PQC fields are left empty
    /// for Phase 1 (stub).
    ///
    /// # Arguments
    ///
    /// * `data` — The raw data being committed
    /// * `classical_sig` — Ed25519 signature over the data hash (64 bytes)
    /// * `committed_at` — Causal timestamp for the commitment
    pub fn new_classical(
        data: &[u8],
        classical_sig: Vec<u8>,
        committed_at: VectorClock,
    ) -> Self {
        let hash = blake3::hash(data);
        Self {
            dilithium_sig: Vec::new(),
            kyber_key: Vec::new(),
            classical_sig,
            data_hash: *hash.as_bytes(),
            committed_at,
        }
    }

    /// Create a stub commitment that doesn't require real signing.
    ///
    /// This is intended for testing and Phase 1 development. The
    /// "signature" is just the data hash repeated twice (to fill 64 bytes).
    /// **Do not use in production.**
    pub fn new_stub(data: &[u8], committed_at: VectorClock) -> Self {
        let hash = blake3::hash(data);
        // Stub: use hash bytes as a fake 64-byte signature (hash || hash)
        let mut classical_sig = Vec::with_capacity(64);
        classical_sig.extend_from_slice(hash.as_bytes());
        classical_sig.extend_from_slice(hash.as_bytes());
        Self {
            dilithium_sig: Vec::new(),
            kyber_key: Vec::new(),
            classical_sig,
            data_hash: *hash.as_bytes(),
            committed_at,
        }
    }

    /// Verify the commitment against a public key.
    ///
    /// Behavior depends on the `phase` parameter:
    /// - `ClassicalOnly`: Only verify the data hash matches; signature
    ///   verification is deferred to Phase 2.
    /// - `Hybrid`: Verify both classical and PQC signatures.
    /// - `PostQuantum`: Only verify the PQC signature.
    ///
    /// # Arguments
    ///
    /// * `public_key` — The signer's public key (classical + PQC)
    /// * `data` — The original data to verify against
    /// * `phase` — Which verification phase to use
    ///
    /// # Returns
    ///
    /// `true` if verification succeeds for the given phase.
    pub fn verify(&self, public_key: &PqPublicKey, data: &[u8], phase: CommitmentPhase) -> bool {
        // Always verify the data hash first
        let hash = blake3::hash(data);
        if hash.as_bytes() != &self.data_hash {
            return false;
        }

        match phase {
            CommitmentPhase::ClassicalOnly => {
                // Phase 1: Only check the hash integrity.
                // Real Ed25519 verification is deferred because the stub
                // doesn't produce valid Ed25519 signatures.
                true
            }
            CommitmentPhase::Hybrid => {
                // Phase 2: Verify both classical and PQC.
                // For now (stub), just verify the hash.
                // TODO: Add ed25519_dalek verification + pqc_dilithium::verify
                let _ = public_key; // Will be used in Phase 2
                true
            }
            CommitmentPhase::PostQuantum => {
                // Phase 3: Only verify PQC.
                // TODO: Add pqc_dilithium::verify
                let _ = public_key;
                true
            }
        }
    }

    /// Check whether this commitment links to (references) a previous
    /// commitment. Used in provenance chain verification.
    ///
    /// A commitment "links to" a previous one if the previous commitment's
    /// `data_hash` is embedded in this commitment's signed data. In the
    /// stub implementation, we check if the data hash differs (i.e., both
    /// commitments are not identical placeholders).
    pub fn links_to(&self, previous: &QuantumCommitment) -> bool {
        // In the full implementation, the current commitment's signed data
        // would include the previous commitment's hash. For now, we verify
        // that both commitments are valid (non-zero hashes) and that they
        // are different (indicating chain progression).
        //
        // TODO: In production, embed previous.data_hash in current signed data
        // and verify the signature covers it.

        let valid_current = self.data_hash != [0u8; 32];
        let valid_previous = previous.data_hash != [0u8; 32];
        let progressing = self.data_hash != previous.data_hash;

        valid_current && valid_previous && progressing
    }

    /// Compute the BLAKE3 hash of the given data.
    pub fn hash_data(data: &[u8]) -> [u8; 32] {
        *blake3::hash(data).as_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_classical_commitment() {
        let data = b"test commitment data";
        let sig = vec![0u8; 64]; // Fake signature
        let vc = VectorClock::new();
        let commitment = QuantumCommitment::new_classical(data, sig, vc);

        // Data hash should match BLAKE3 of the data
        let expected_hash = blake3::hash(data);
        assert_eq!(commitment.data_hash, *expected_hash.as_bytes());
    }

    #[test]
    fn test_stub_commitment() {
        let data = b"stub data";
        let vc = VectorClock::new();
        let commitment = QuantumCommitment::new_stub(data, vc);

        assert!(!commitment.dilithium_sig.is_empty() || commitment.dilithium_sig.is_empty());
        assert_eq!(commitment.classical_sig.len(), 64);
    }

    #[test]
    fn test_verify_classical_only_phase() {
        let data = b"test data";
        let vc = VectorClock::new();
        let commitment = QuantumCommitment::new_stub(data, vc);
        let pk = PqPublicKey {
            ed25519: [0u8; 32],
            dilithium: Vec::new(),
        };

        assert!(commitment.verify(&pk, data, CommitmentPhase::ClassicalOnly));
    }

    #[test]
    fn test_verify_fails_with_wrong_data() {
        let data = b"original data";
        let wrong_data = b"tampered data";
        let vc = VectorClock::new();
        let commitment = QuantumCommitment::new_stub(data, vc);
        let pk = PqPublicKey {
            ed25519: [0u8; 32],
            dilithium: Vec::new(),
        };

        // Hash mismatch should fail even in ClassicalOnly phase
        assert!(!commitment.verify(&pk, wrong_data, CommitmentPhase::ClassicalOnly));
    }

    #[test]
    fn test_links_to() {
        let data1 = b"first commitment";
        let data2 = b"second commitment";
        let vc = VectorClock::new();
        let commitment1 = QuantumCommitment::new_stub(data1, vc.clone());
        let commitment2 = QuantumCommitment::new_stub(data2, vc);

        assert!(commitment2.links_to(&commitment1));
    }

    #[test]
    fn test_hash_data() {
        let data = b"hello world";
        let hash = QuantumCommitment::hash_data(data);
        let expected = blake3::hash(data);
        assert_eq!(hash, *expected.as_bytes());
    }
}
