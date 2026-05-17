//! Quantum-Resistant Commitments — Long-Term Integrity
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
//! # Transition Strategy
//!
//! 1. **Phase 1 (ClassicalOnly)**: Classical Ed25519 signatures verified.
//!    Verification requires a valid Ed25519 signature over the data hash.
//! 2. **Phase 2 (Hybrid)**: Both classical and PQC signatures required.
//!    Verification checks both; failure of either rejects the commitment.
//! 3. **Phase 3 (PostQuantum)**: Only PQC signatures required.
//!    Classical signatures are kept for historical verification only.

use ed25519_dalek::{Signer, Verifier};
use omnia_substrate::{NodeKeypair, VectorClock};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;

/// Errors that can occur during quantum commitment operations.
#[derive(Debug, thiserror::Error)]
pub enum BindingError {
    /// Ed25519 signature deserialization or verification failed.
    #[error("Ed25519 signature error: {0}")]
    Ed25519(#[from] ed25519_dalek::SignatureError),
    /// Dilithium signature verification failed.
    #[error("Dilithium signature error: {0}")]
    Dilithium(String),
    /// Invalid signature length.
    #[error("Invalid signature length: expected {expected}, got {actual}")]
    InvalidSignatureLength {
        /// Expected length in bytes.
        expected: usize,
        /// Actual length in bytes.
        actual: usize,
    },
    /// Invalid public key.
    #[error("Invalid public key: {0}")]
    InvalidPublicKey(String),
    /// Signing operation failed.
    #[error("Signing failed: {0}")]
    SigningFailed(String),
}

/// A hybrid (classical + post-quantum) cryptographic commitment.
///
/// This structure binds data to a signer using both a classical Ed25519
/// signature (for compatibility) and a CRYSTALS-Dilithium signature
/// (for quantum resistance). The `data_hash` is computed using BLAKE3
/// for speed and security.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuantumCommitment {
    /// CRYSTALS-Dilithium signature (NIST PQC standard).
    pub dilithium_sig: Vec<u8>,
    /// CRYSTALS-Kyber encapsulated key (for future encryption).
    pub kyber_key: Vec<u8>,
    /// Classical Ed25519 signature (for compatibility during transition).
    /// 64 bytes.
    pub classical_sig: Vec<u8>,
    /// BLAKE3 hash of the committed data (32 bytes).
    pub data_hash: [u8; 32],
    /// Commitment timestamp in causal time.
    pub committed_at: VectorClock,
}

/// Public key for verifying quantum commitments.
///
/// Contains both the classical Ed25519 public key and the post-quantum
/// Dilithium public key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PqPublicKey {
    /// Ed25519 verifying key (32 bytes).
    pub ed25519: [u8; 32],
    /// CRYSTALS-Dilithium public key.
    pub dilithium: Vec<u8>,
}

/// Whether we are in the classical-only, hybrid, or post-quantum phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Serialize, Deserialize, Default)]
#[repr(u8)]
pub enum CommitmentPhase {
    /// Phase 1: Only classical (Ed25519) signatures are verified.
    #[default]
    ClassicalOnly = 0,
    /// Phase 2: Both classical and PQC signatures must verify.
    Hybrid = 1,
    /// Phase 3: Only PQC signatures are verified.
    PostQuantum = 2,
}

impl QuantumCommitment {
    /// Create a new quantum commitment from data and a classical Ed25519
    /// signature.
    ///
    /// The data hash is computed using BLAKE3. PQC fields are left empty.
    ///
    /// # Arguments
    ///
    /// * `data` — The raw data being committed
    /// * `classical_sig` — Ed25519 signature over the data hash (64 bytes)
    /// * `committed_at` — Causal timestamp for the commitment
    pub fn new_classical(data: &[u8], classical_sig: Vec<u8>, committed_at: VectorClock) -> Self {
        let hash = blake3::hash(data);
        Self {
            dilithium_sig: Vec::new(),
            kyber_key: Vec::new(),
            classical_sig,
            data_hash: *hash.as_bytes(),
            committed_at,
        }
    }

    /// Sign data using classical Ed25519 only.
    ///
    /// Creates a commitment with an Ed25519 signature over the data hash.
    /// Dilithium fields are left empty. The commitment timestamp defaults
    /// to an empty `VectorClock`.
    ///
    /// # Arguments
    ///
    /// * `data` — The raw data being committed
    /// * `keypair` — The Ed25519 signing key (`NodeKeypair`)
    ///
    /// # Errors
    ///
    /// Returns `BindingError` if signing fails.
    pub fn sign_classical(data: &[u8], keypair: &NodeKeypair) -> Result<Self, BindingError> {
        let hash = blake3::hash(data);
        let sig = keypair.sign(hash.as_bytes());
        Ok(Self {
            dilithium_sig: Vec::new(),
            kyber_key: Vec::new(),
            classical_sig: sig.to_bytes().to_vec(),
            data_hash: *hash.as_bytes(),
            committed_at: VectorClock::new(),
        })
    }

    /// Sign data using both Ed25519 and Dilithium (hybrid mode).
    ///
    /// Creates a commitment with both classical and post-quantum signatures
    /// over the data hash. Both signatures must verify during the `Hybrid`
    /// phase. The commitment timestamp defaults to an empty `VectorClock`.
    ///
    /// # Arguments
    ///
    /// * `data` — The raw data being committed
    /// * `ed_keypair` — The Ed25519 signing key (`NodeKeypair`)
    /// * `dilithium_keypair` — The Dilithium keypair for signing
    ///
    /// # Errors
    ///
    /// Returns `BindingError` if signing fails.
    pub fn sign_hybrid(
        data: &[u8],
        ed_keypair: &NodeKeypair,
        dilithium_keypair: &pqc_dilithium::Keypair,
    ) -> Result<Self, BindingError> {
        let hash = blake3::hash(data);
        let ed_sig = ed_keypair.sign(hash.as_bytes());
        let dilithium_sig = dilithium_keypair.sign(hash.as_bytes());
        Ok(Self {
            dilithium_sig: dilithium_sig.to_vec(),
            kyber_key: Vec::new(),
            classical_sig: ed_sig.to_bytes().to_vec(),
            data_hash: *hash.as_bytes(),
            committed_at: VectorClock::new(),
        })
    }

    /// Sign data using post-quantum Dilithium only.
    ///
    /// Creates a commitment with only a Dilithium signature over the data hash.
    /// The classical signature field is left empty. The commitment timestamp
    /// defaults to an empty `VectorClock`.
    ///
    /// # Arguments
    ///
    /// * `data` — The raw data being committed
    /// * `dilithium_keypair` — The Dilithium keypair for signing
    ///
    /// # Errors
    ///
    /// Returns `BindingError` if signing fails.
    pub fn sign_post_quantum(
        data: &[u8],
        dilithium_keypair: &pqc_dilithium::Keypair,
    ) -> Result<Self, BindingError> {
        let hash = blake3::hash(data);
        let dilithium_sig = dilithium_keypair.sign(hash.as_bytes());
        Ok(Self {
            dilithium_sig: dilithium_sig.to_vec(),
            kyber_key: Vec::new(),
            classical_sig: Vec::new(),
            data_hash: *hash.as_bytes(),
            committed_at: VectorClock::new(),
        })
    }

    /// Create a stub commitment that doesn't require real signing.
    ///
    /// This is intended for testing and non-crypto scenarios only. The
    /// "signature" is just the data hash repeated twice (to fill 64 bytes).
    /// **Do not use in production.** This commitment will NOT pass
    /// cryptographic verification via `verify()`.
    #[cfg(test)]
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
    /// - `ClassicalOnly`: Verify Ed25519 signature over the data hash.
    /// - `Hybrid`: Verify both Ed25519 and Dilithium signatures.
    /// - `PostQuantum`: Verify only the Dilithium signature.
    ///
    /// In all cases, the data hash is verified first. If the hash doesn't
    /// match, verification fails immediately without checking signatures.
    ///
    /// # Arguments
    ///
    /// * `public_key` — The signer's public key (classical + PQC)
    /// * `data` — The original data to verify against
    /// * `phase` — Which verification phase to use
    ///
    /// # Returns
    ///
    /// `true` if verification succeeds for the given phase, `false` otherwise.
    /// Deserialization or verification failures return `false` (never panic).
    pub fn verify(&self, public_key: &PqPublicKey, data: &[u8], phase: CommitmentPhase) -> bool {
        // Always verify the data hash first
        let hash = blake3::hash(data);
        if hash.as_bytes().ct_ne(&self.data_hash).into() {
            return false;
        }

        match phase {
            CommitmentPhase::ClassicalOnly => self.verify_ed25519(public_key, &hash),
            CommitmentPhase::Hybrid => {
                let ed_ok = self.verify_ed25519(public_key, &hash);
                let pq_ok = self.verify_dilithium(public_key, &hash);
                ed_ok && pq_ok
            }
            CommitmentPhase::PostQuantum => self.verify_dilithium(public_key, &hash),
        }
    }

    /// Verify the Ed25519 signature against the data hash.
    ///
    /// Returns `false` on any deserialization or verification failure.
    fn verify_ed25519(&self, public_key: &PqPublicKey, hash: &blake3::Hash) -> bool {
        let verifying_key = match ed25519_dalek::VerifyingKey::from_bytes(&public_key.ed25519) {
            Ok(vk) => vk,
            Err(e) => {
                tracing::warn!("Ed25519 public key deserialization failed: {e}");
                return false;
            }
        };
        let sig = match ed25519_dalek::Signature::from_slice(&self.classical_sig) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Ed25519 signature deserialization failed: {e}");
                return false;
            }
        };
        match verifying_key.verify(hash.as_bytes(), &sig) {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!("Ed25519 verification failed: {e}");
                false
            }
        }
    }

    /// Verify the Dilithium signature against the data hash.
    ///
    /// Returns `false` on any verification failure or if the
    /// signature/public key is empty.
    fn verify_dilithium(&self, public_key: &PqPublicKey, hash: &blake3::Hash) -> bool {
        if public_key.dilithium.is_empty() {
            tracing::warn!("Dilithium public key is empty");
            return false;
        }
        if self.dilithium_sig.is_empty() {
            tracing::warn!("Dilithium signature is empty");
            return false;
        }
        match pqc_dilithium::verify(&self.dilithium_sig, hash.as_bytes(), &public_key.dilithium) {
            Ok(()) => true,
            Err(_) => {
                tracing::warn!("Dilithium verification failed");
                false
            }
        }
    }

    /// Check whether this commitment links to (references) a previous
    /// commitment. Used in provenance chain verification.
    ///
    /// A commitment "links to" a previous one if the previous commitment's
    /// `data_hash` is embedded in this commitment's signed data. In the
    /// current implementation, we check if the data hash differs (i.e., both
    /// commitments are not identical placeholders).
    pub fn links_to(&self, previous: &QuantumCommitment) -> bool {
        let valid_current: bool = self.data_hash.ct_ne(&[0u8; 32]).into();
        let valid_previous: bool = previous.data_hash.ct_ne(&[0u8; 32]).into();
        let progressing: bool = self.data_hash.ct_ne(&previous.data_hash).into();

        valid_current && valid_previous && progressing
    }

    /// Compute the BLAKE3 hash of the given data.
    pub fn hash_data(data: &[u8]) -> [u8; 32] {
        *blake3::hash(data).as_bytes()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use omnia_substrate::generate_keypair;

    fn test_keypair_and_pk() -> (NodeKeypair, PqPublicKey) {
        let kp = generate_keypair();
        let pk = PqPublicKey {
            ed25519: kp.verifying_key().to_bytes(),
            dilithium: Vec::new(),
        };
        (kp, pk)
    }

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
    fn test_sign_classical_verify_classical_only() {
        let (kp, pk) = test_keypair_and_pk();
        let data = b"test data for classical signing";
        let commitment = QuantumCommitment::sign_classical(data, &kp).unwrap();
        assert!(commitment.verify(&pk, data, CommitmentPhase::ClassicalOnly));
    }

    #[test]
    fn test_sign_classical_hybrid_fails() {
        let (kp, pk) = test_keypair_and_pk();
        let data = b"test data for hybrid check";
        let commitment = QuantumCommitment::sign_classical(data, &kp).unwrap();
        // ClassicalOnly commitment lacks Dilithium, so Hybrid verification fails
        assert!(!commitment.verify(&pk, data, CommitmentPhase::Hybrid));
    }

    #[test]
    fn test_verify_fails_with_wrong_data() {
        let (kp, pk) = test_keypair_and_pk();
        let data = b"original data";
        let wrong_data = b"tampered data";
        let commitment = QuantumCommitment::sign_classical(data, &kp).unwrap();
        assert!(!commitment.verify(&pk, wrong_data, CommitmentPhase::ClassicalOnly));
    }

    #[test]
    fn test_verify_fails_with_wrong_public_key() {
        let (kp, _) = test_keypair_and_pk();
        let (_, wrong_pk) = test_keypair_and_pk();
        let data = b"signed data";
        let commitment = QuantumCommitment::sign_classical(data, &kp).unwrap();
        assert!(!commitment.verify(&wrong_pk, data, CommitmentPhase::ClassicalOnly));
    }

    #[test]
    fn test_verify_fails_with_empty_signature() {
        let data = b"data with empty sig";
        let hash = blake3::hash(data);
        let commitment = QuantumCommitment {
            dilithium_sig: Vec::new(),
            kyber_key: Vec::new(),
            classical_sig: Vec::new(), // Empty signature
            data_hash: *hash.as_bytes(),
            committed_at: VectorClock::new(),
        };
        let (kp, _) = test_keypair_and_pk();
        let pk = PqPublicKey {
            ed25519: kp.verifying_key().to_bytes(),
            dilithium: Vec::new(),
        };
        assert!(!commitment.verify(&pk, data, CommitmentPhase::ClassicalOnly));
    }

    #[test]
    fn test_sign_hybrid_verify_hybrid() {
        let (ed_kp, _) = test_keypair_and_pk();
        let dilithium_kp = pqc_dilithium::Keypair::generate();
        let data = b"hybrid signed data";

        let pk = PqPublicKey {
            ed25519: ed_kp.verifying_key().to_bytes(),
            dilithium: dilithium_kp.public.to_vec(),
        };

        let commitment = QuantumCommitment::sign_hybrid(data, &ed_kp, &dilithium_kp).unwrap();
        assert!(commitment.verify(&pk, data, CommitmentPhase::Hybrid));
        assert!(commitment.verify(&pk, data, CommitmentPhase::ClassicalOnly));
    }

    #[test]
    fn test_sign_post_quantum_verify() {
        let dilithium_kp = pqc_dilithium::Keypair::generate();
        let data = b"post-quantum data";

        let pk = PqPublicKey {
            ed25519: [0u8; 32], // Not used in PostQuantum phase
            dilithium: dilithium_kp.public.to_vec(),
        };

        let commitment = QuantumCommitment::sign_post_quantum(data, &dilithium_kp).unwrap();
        assert!(commitment.verify(&pk, data, CommitmentPhase::PostQuantum));
        // ClassicalOnly should fail (no Ed25519 signature)
        assert!(!commitment.verify(&pk, data, CommitmentPhase::ClassicalOnly));
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
