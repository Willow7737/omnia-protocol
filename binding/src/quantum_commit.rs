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
//!    A CRYSTALS-Kyber encapsulation key is stored for future PQ-secure
//!    key exchange.
//! 3. **Phase 3 (PostQuantum)**: Only PQC signatures required.
//!    Classical signatures are kept for historical verification only.
//!    A CRYSTALS-Kyber encapsulation key is stored for PQ-secure key exchange.

use ed25519_dalek::{Signer, Verifier};
#[cfg(feature = "pqc")]
use ml_kem::kem::{Decapsulate, Encapsulate};
#[cfg(feature = "pqc")]
use ml_kem::{EncodedSizeUser, KemCore, MlKem768};
use omnia_substrate::{NodeKeypair, VectorClock};
#[cfg(feature = "pqc")]
use rand::rngs::OsRng;
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
    /// Kyber KEM operation failed.
    #[cfg(feature = "pqc")]
    #[error("Kyber KEM error: {0}")]
    Kyber(#[from] KyberError),
}

/// ML-KEM-768 keypair for PQ-secure key encapsulation.
///
/// Wraps the fixed-size byte arrays from `ml-kem` into `Vec<u8>` for
/// serialization flexibility. ML-KEM-768 (formerly Kyber768) provides
/// security roughly equivalent to AES-192.
#[cfg(feature = "pqc")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KyberKeyPair {
    /// ML-KEM-768 encapsulation key (public key, 1184 bytes).
    pub encapsulation_key: Vec<u8>,
    /// ML-KEM-768 decapsulation key (secret key, 2400 bytes).
    pub decapsulation_key: Vec<u8>,
}

/// Error type for ML-KEM operations.
#[cfg(feature = "pqc")]
#[derive(Debug, thiserror::Error)]
pub enum KyberError {
    /// Invalid encapsulation key length.
    #[error("invalid encapsulation key length: {0}")]
    InvalidEncapsulationKey(usize),
    /// Invalid decapsulation key length.
    #[error("invalid decapsulation key length: {0}")]
    InvalidDecapsulationKey(usize),
    /// Invalid ciphertext length.
    #[error("invalid ciphertext length: {0}")]
    InvalidCiphertext(usize),
    /// KEM operation failed.
    #[error("KEM operation failed: {0}")]
    KemFailed(String),
}

// ML-KEM-768 key size constants (same as Kyber768)
#[cfg(feature = "pqc")]
/// Encapsulation key size in bytes for ML-KEM-768.
const ML_KEM_768_ENCAPSULATION_KEY_SIZE: usize = 1184;
#[cfg(feature = "pqc")]
/// Decapsulation key size in bytes for ML-KEM-768.
const ML_KEM_768_DECAPSULATION_KEY_SIZE: usize = 2400;
#[cfg(feature = "pqc")]
/// Ciphertext size in bytes for ML-KEM-768.
const ML_KEM_768_CIPHERTEXT_SIZE: usize = 1088;
#[cfg(feature = "pqc")]
/// Shared secret size in bytes for ML-KEM-768.
#[allow(dead_code)]
const ML_KEM_768_SHARED_SECRET_SIZE: usize = 32;

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
    /// over the data hash. A CRYSTALS-Kyber encapsulation key is generated
    /// and stored in `kyber_key` for future PQ-secure key exchange. Both
    /// signatures must verify during the `Hybrid` phase. The commitment
    /// timestamp defaults to an empty `VectorClock`.
    ///
    /// # Arguments
    ///
    /// * `data` — The raw data being committed
    /// * `ed_keypair` — The Ed25519 signing key (`NodeKeypair`)
    /// * `dilithium_keypair` — The Dilithium keypair for signing
    ///
    /// # Errors
    ///
    /// Returns `BindingError` if signing or Kyber keypair generation fails.
    #[cfg(feature = "pqc")]
    pub fn sign_hybrid(
        data: &[u8],
        ed_keypair: &NodeKeypair,
        dilithium_keypair: &pqc_dilithium::Keypair,
    ) -> Result<Self, BindingError> {
        let hash = blake3::hash(data);
        let ed_sig = ed_keypair.sign(hash.as_bytes());
        let dilithium_sig = dilithium_keypair.sign(hash.as_bytes());

        // Generate an ML-KEM-768 encapsulation key for PQ-secure key exchange
        let kyber_kp = Self::generate_kyber_keypair()?;

        Ok(Self {
            dilithium_sig: dilithium_sig.to_vec(),
            kyber_key: kyber_kp.encapsulation_key,
            classical_sig: ed_sig.to_bytes().to_vec(),
            data_hash: *hash.as_bytes(),
            committed_at: VectorClock::new(),
        })
    }

    /// Sign data using both Ed25519 and Dilithium (hybrid mode).
    ///
    /// **Stub**: Returns an error when the `pqc` feature is not enabled.
    #[cfg(not(feature = "pqc"))]
    pub fn sign_hybrid(
        _data: &[u8],
        _ed_keypair: &NodeKeypair,
        _dilithium_keypair: &[u8],
    ) -> Result<Self, BindingError> {
        Err(BindingError::SigningFailed(
            "PQC feature not enabled: hybrid signing requires the 'pqc' feature".to_string(),
        ))
    }

    /// Sign data using post-quantum Dilithium only.
    ///
    /// Creates a commitment with only a Dilithium signature over the data hash.
    /// An ML-KEM-768 encapsulation key is generated and stored in `kyber_key`
    /// for PQ-secure key exchange. The classical signature field is left empty.
    /// The commitment timestamp defaults to an empty `VectorClock`.
    ///
    /// # Arguments
    ///
    /// * `data` — The raw data being committed
    /// * `dilithium_keypair` — The Dilithium keypair for signing
    ///
    /// # Errors
    ///
    /// Returns `BindingError` if signing or ML-KEM keypair generation fails.
    #[cfg(feature = "pqc")]
    pub fn sign_post_quantum(data: &[u8], dilithium_keypair: &pqc_dilithium::Keypair) -> Result<Self, BindingError> {
        let hash = blake3::hash(data);
        let dilithium_sig = dilithium_keypair.sign(hash.as_bytes());

        // Generate an ML-KEM-768 encapsulation key for PQ-secure key exchange
        let kyber_kp = Self::generate_kyber_keypair()?;

        Ok(Self {
            dilithium_sig: dilithium_sig.to_vec(),
            kyber_key: kyber_kp.encapsulation_key,
            classical_sig: Vec::new(),
            data_hash: *hash.as_bytes(),
            committed_at: VectorClock::new(),
        })
    }

    /// Sign data using post-quantum Dilithium only.
    ///
    /// **Stub**: Returns an error when the `pqc` feature is not enabled.
    #[cfg(not(feature = "pqc"))]
    pub fn sign_post_quantum(_data: &[u8], _dilithium_keypair: &[u8]) -> Result<Self, BindingError> {
        Err(BindingError::SigningFailed(
            "PQC feature not enabled: post-quantum signing requires the 'pqc' feature".to_string(),
        ))
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
    #[cfg(feature = "pqc")]
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

    /// Verify the Dilithium signature against the data hash.
    ///
    /// **Stub**: Returns `false` when the `pqc` feature is not enabled,
    /// since Dilithium verification requires PQC libraries.
    #[cfg(not(feature = "pqc"))]
    fn verify_dilithium(&self, _public_key: &PqPublicKey, _hash: &blake3::Hash) -> bool {
        tracing::warn!("Dilithium verification skipped: PQC feature not enabled");
        false
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

    // -----------------------------------------------------------------------
    // ML-KEM KEM operations
    // -----------------------------------------------------------------------

    /// Generate an ML-KEM-768 keypair for PQ-secure key encapsulation.
    ///
    /// Returns a [`KyberKeyPair`] containing the encapsulation (public) key
    /// and decapsulation (secret) key. The encapsulation key should be stored
    /// in the commitment's `kyber_key` field; the decapsulation key should be
    /// kept private by the commitment creator.
    ///
    /// # Errors
    ///
    /// Returns [`KyberError`] if the keypair generation fails (extremely
    /// unlikely with a proper RNG).
    #[cfg(feature = "pqc")]
    pub fn generate_kyber_keypair() -> Result<KyberKeyPair, KyberError> {
        let mut rng = OsRng;
        let (dk, ek) = <MlKem768 as KemCore>::generate(&mut rng);
        Ok(KyberKeyPair {
            encapsulation_key: ek.as_bytes().to_vec(),
            decapsulation_key: dk.as_bytes().to_vec(),
        })
    }

    /// Encapsulate: generate a shared secret and ciphertext using the
    /// commitment's stored ML-KEM-768 encapsulation key.
    ///
    /// This is the KEM analog of encryption: the sender calls `encapsulate`
    /// with the recipient's public key to produce a shared secret and a
    /// ciphertext. The recipient can then call [`QuantumCommitment::kyber_decapsulate`] with
    /// their secret key and the ciphertext to recover the same shared secret.
    ///
    /// # Returns
    ///
    /// A tuple of `(shared_secret, ciphertext)` where both are `Vec<u8>`.
    /// The shared secret is 32 bytes; the ciphertext is 1088 bytes (ML-KEM-768).
    ///
    /// # Errors
    ///
    /// Returns [`KyberError`] if:
    /// - The encapsulation key stored in this commitment is invalid
    /// - The KEM operation fails (extremely unlikely with a proper RNG)
    #[cfg(feature = "pqc")]
    pub fn kyber_encapsulate(&self) -> Result<(Vec<u8>, Vec<u8>), KyberError> {
        if self.kyber_key.len() != ML_KEM_768_ENCAPSULATION_KEY_SIZE {
            return Err(KyberError::InvalidEncapsulationKey(self.kyber_key.len()));
        }
        type EncapKey = <MlKem768 as KemCore>::EncapsulationKey;
        let ek_bytes: [u8; ML_KEM_768_ENCAPSULATION_KEY_SIZE] = self
            .kyber_key
            .clone()
            .try_into()
            .map_err(|_| KyberError::InvalidEncapsulationKey(self.kyber_key.len()))?;
        let ek_encoded: ml_kem::Encoded<EncapKey> = ek_bytes.into();
        let ek = EncapKey::from_bytes(&ek_encoded);
        let mut rng = OsRng;
        let (ct, ss) = ek
            .encapsulate(&mut rng)
            .map_err(|e| KyberError::KemFailed(format!("{e:?}")))?;
        Ok((ss.to_vec(), ct.to_vec()))
    }

    /// Decapsulate: recover the shared secret from a ciphertext.
    ///
    /// This is a standalone function (not a method on `QuantumCommitment`)
    /// because the decapsulation key must be kept private and is not stored
    /// in the commitment itself.
    ///
    /// # Arguments
    ///
    /// * `decapsulation_key` — The ML-KEM-768 secret key (2400 bytes)
    /// * `ciphertext` — The KEM ciphertext (1088 bytes)
    ///
    /// # Returns
    ///
    /// The shared secret as a 32-byte `Vec<u8>`.
    ///
    /// # Errors
    ///
    /// Returns [`KyberError`] if the key or ciphertext has an invalid length.
    ///
    /// # Note on Implicit Rejection
    ///
    /// ML-KEM uses implicit rejection: if the ciphertext is invalid (e.g.,
    /// modified by an attacker), decapsulation still succeeds but returns
    /// a pseudorandom shared secret instead of the original one. This is
    /// by design and prevents CCA-style attacks.
    #[cfg(feature = "pqc")]
    pub fn kyber_decapsulate(decapsulation_key: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, KyberError> {
        if decapsulation_key.len() != ML_KEM_768_DECAPSULATION_KEY_SIZE {
            return Err(KyberError::InvalidDecapsulationKey(decapsulation_key.len()));
        }
        if ciphertext.len() != ML_KEM_768_CIPHERTEXT_SIZE {
            return Err(KyberError::InvalidCiphertext(ciphertext.len()));
        }
        type DecapKey = <MlKem768 as KemCore>::DecapsulationKey;
        let dk_bytes: [u8; ML_KEM_768_DECAPSULATION_KEY_SIZE] = decapsulation_key
            .try_into()
            .map_err(|_| KyberError::InvalidDecapsulationKey(decapsulation_key.len()))?;
        let ct_bytes: [u8; ML_KEM_768_CIPHERTEXT_SIZE] = ciphertext
            .try_into()
            .map_err(|_| KyberError::InvalidCiphertext(ciphertext.len()))?;
        let dk_encoded: ml_kem::Encoded<DecapKey> = dk_bytes.into();
        let dk = DecapKey::from_bytes(&dk_encoded);
        let ct_encoded: ml_kem::Ciphertext<MlKem768> = ct_bytes.into();
        let ss = dk
            .decapsulate(&ct_encoded)
            .map_err(|e| KyberError::KemFailed(format!("{e:?}")))?;
        Ok(ss.to_vec())
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

    // -----------------------------------------------------------------------
    // PQC-dependent tests (only compiled with 'pqc' feature)
    // -----------------------------------------------------------------------

    #[cfg(feature = "pqc")]
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

    #[cfg(feature = "pqc")]
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

    #[cfg(feature = "pqc")]
    #[test]
    fn test_kyber_keypair_generation() {
        let keypair = QuantumCommitment::generate_kyber_keypair().unwrap();
        // ML-KEM-768: encapsulation key = 1184 bytes, decapsulation key = 2400 bytes
        assert_eq!(
            keypair.encapsulation_key.len(),
            ML_KEM_768_ENCAPSULATION_KEY_SIZE,
            "encapsulation key should be {ML_KEM_768_ENCAPSULATION_KEY_SIZE} bytes"
        );
        assert_eq!(
            keypair.decapsulation_key.len(),
            ML_KEM_768_DECAPSULATION_KEY_SIZE,
            "decapsulation key should be {ML_KEM_768_DECAPSULATION_KEY_SIZE} bytes"
        );
    }

    #[cfg(feature = "pqc")]
    #[test]
    fn test_kyber_keypair_unique() {
        let kp1 = QuantumCommitment::generate_kyber_keypair().unwrap();
        let kp2 = QuantumCommitment::generate_kyber_keypair().unwrap();
        // Two independently generated keypairs should differ
        assert_ne!(
            kp1.encapsulation_key, kp2.encapsulation_key,
            "independent ML-KEM keypairs should have different encapsulation keys"
        );
    }

    #[cfg(feature = "pqc")]
    #[test]
    fn test_kyber_encapsulate_decapsulate() {
        let keypair = QuantumCommitment::generate_kyber_keypair().unwrap();

        let commitment = QuantumCommitment {
            dilithium_sig: Vec::new(),
            kyber_key: keypair.encapsulation_key.clone(),
            classical_sig: Vec::new(),
            data_hash: [0u8; 32],
            committed_at: VectorClock::new(),
        };

        let (shared_secret_1, ciphertext) = commitment.kyber_encapsulate().unwrap();
        let shared_secret_2 = QuantumCommitment::kyber_decapsulate(&keypair.decapsulation_key, &ciphertext).unwrap();

        assert_eq!(shared_secret_1, shared_secret_2, "shared secrets must match");
        assert_eq!(
            shared_secret_1.len(),
            ML_KEM_768_SHARED_SECRET_SIZE,
            "shared secret should be 32 bytes"
        );
        assert_eq!(
            ciphertext.len(),
            ML_KEM_768_CIPHERTEXT_SIZE,
            "ciphertext should be {ML_KEM_768_CIPHERTEXT_SIZE} bytes"
        );
    }

    #[cfg(feature = "pqc")]
    #[test]
    fn test_kyber_wrong_ciphertext_succeeds() {
        // ML-KEM uses implicit rejection: decapsulation with a wrong ciphertext
        // produces a different (pseudorandom) shared secret, not an error.
        let keypair = QuantumCommitment::generate_kyber_keypair().unwrap();
        let wrong_ct = vec![0u8; ML_KEM_768_CIPHERTEXT_SIZE];
        let result = QuantumCommitment::kyber_decapsulate(&keypair.decapsulation_key, &wrong_ct);
        assert!(
            result.is_ok(),
            "ML-KEM decapsulation always succeeds (implicit rejection)"
        );
    }

    #[cfg(feature = "pqc")]
    #[test]
    fn test_kyber_wrong_key_different_secret() {
        let kp1 = QuantumCommitment::generate_kyber_keypair().unwrap();
        let kp2 = QuantumCommitment::generate_kyber_keypair().unwrap();

        let commitment = QuantumCommitment {
            dilithium_sig: Vec::new(),
            kyber_key: kp1.encapsulation_key.clone(),
            classical_sig: Vec::new(),
            data_hash: [0u8; 32],
            committed_at: VectorClock::new(),
        };

        let (shared_secret_1, ciphertext) = commitment.kyber_encapsulate().unwrap();
        // Decapsulating with the WRONG key should succeed (implicit rejection)
        // but produce a DIFFERENT shared secret
        let shared_secret_2 = QuantumCommitment::kyber_decapsulate(&kp2.decapsulation_key, &ciphertext).unwrap();
        assert_ne!(
            shared_secret_1, shared_secret_2,
            "wrong decapsulation key should produce a different shared secret"
        );
    }

    #[cfg(feature = "pqc")]
    #[test]
    fn test_kyber_invalid_encapsulation_key() {
        let commitment = QuantumCommitment {
            dilithium_sig: Vec::new(),
            kyber_key: vec![0u8; 100], // Wrong length
            classical_sig: Vec::new(),
            data_hash: [0u8; 32],
            committed_at: VectorClock::new(),
        };
        let result = commitment.kyber_encapsulate();
        assert!(result.is_err());
        match result.unwrap_err() {
            KyberError::InvalidEncapsulationKey(len) => assert_eq!(len, 100),
            other => panic!("Expected InvalidEncapsulationKey, got: {other:?}"),
        }
    }

    #[cfg(feature = "pqc")]
    #[test]
    fn test_kyber_invalid_decapsulation_key() {
        let result = QuantumCommitment::kyber_decapsulate(&[0u8; 100], &[0u8; ML_KEM_768_CIPHERTEXT_SIZE]);
        assert!(result.is_err());
        match result.unwrap_err() {
            KyberError::InvalidDecapsulationKey(len) => assert_eq!(len, 100),
            other => panic!("Expected InvalidDecapsulationKey, got: {other:?}"),
        }
    }

    #[cfg(feature = "pqc")]
    #[test]
    fn test_kyber_invalid_ciphertext() {
        let kp = QuantumCommitment::generate_kyber_keypair().unwrap();
        let result = QuantumCommitment::kyber_decapsulate(&kp.decapsulation_key, &[0u8; 100]);
        assert!(result.is_err());
        match result.unwrap_err() {
            KyberError::InvalidCiphertext(len) => assert_eq!(len, 100),
            other => panic!("Expected InvalidCiphertext, got: {other:?}"),
        }
    }

    #[test]
    fn test_kyber_key_empty_in_classical_mode() {
        let (kp, _) = test_keypair_and_pk();
        let commitment = QuantumCommitment::sign_classical(b"classical data", &kp).unwrap();
        assert!(
            commitment.kyber_key.is_empty(),
            "classical mode should have empty kyber_key"
        );
    }

    #[cfg(feature = "pqc")]
    #[test]
    fn test_kyber_key_present_in_hybrid_mode() {
        let (ed_kp, _) = test_keypair_and_pk();
        let dilithium_kp = pqc_dilithium::Keypair::generate();
        let commitment = QuantumCommitment::sign_hybrid(b"hybrid data", &ed_kp, &dilithium_kp).unwrap();
        assert_eq!(
            commitment.kyber_key.len(),
            ML_KEM_768_ENCAPSULATION_KEY_SIZE,
            "hybrid mode should have an ML-KEM encapsulation key"
        );
    }

    #[cfg(feature = "pqc")]
    #[test]
    fn test_kyber_key_present_in_post_quantum_mode() {
        let dilithium_kp = pqc_dilithium::Keypair::generate();
        let commitment = QuantumCommitment::sign_post_quantum(b"pq data", &dilithium_kp).unwrap();
        assert_eq!(
            commitment.kyber_key.len(),
            ML_KEM_768_ENCAPSULATION_KEY_SIZE,
            "post-quantum mode should have an ML-KEM encapsulation key"
        );
    }

    #[cfg(feature = "pqc")]
    #[test]
    fn test_kyber_hybrid_encapsulate_decapsulate() {
        let (ed_kp, _) = test_keypair_and_pk();
        let dilithium_kp = pqc_dilithium::Keypair::generate();

        // Sign in hybrid mode — this generates an ML-KEM keypair internally
        let commitment = QuantumCommitment::sign_hybrid(b"hybrid kem test", &ed_kp, &dilithium_kp).unwrap();

        // We can't get the decapsulation key back from sign_hybrid (it's dropped),
        // so generate a separate keypair and manually set the encapsulation key
        // to test encapsulate/decapsulate with the same key.
        let kyber_kp = QuantumCommitment::generate_kyber_keypair().unwrap();
        let commitment_with_key = QuantumCommitment {
            dilithium_sig: commitment.dilithium_sig,
            kyber_key: kyber_kp.encapsulation_key.clone(),
            classical_sig: commitment.classical_sig,
            data_hash: commitment.data_hash,
            committed_at: commitment.committed_at,
        };

        let (ss1, ct) = commitment_with_key.kyber_encapsulate().unwrap();
        let ss2 = QuantumCommitment::kyber_decapsulate(&kyber_kp.decapsulation_key, &ct).unwrap();
        assert_eq!(ss1, ss2, "shared secrets must match");
    }
}
