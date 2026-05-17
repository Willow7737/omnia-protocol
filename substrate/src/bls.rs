//! BLS Signature Aggregation on BLS12-381.
//!
//! This module implements Boneh-Lynn-Shacham (BLS) signatures with
//! aggregation support over the BLS12-381 elliptic curve. BLS signatures
//! enable **constant-time verification** of N signatures by aggregating
//! them into a single signature, reducing verification from O(N) to O(1).
//!
//! # Why BLS?
//!
//! In a BFT consensus system with N validators, each block typically
//! requires N individual signature verifications. BLS aggregation
//! reduces this to a single pairing check, dramatically improving
//! block verification throughput.
//!
//! # Aggregation Model
//!
//! ```text
//! Signer 1: sk₁, pk₁, σ₁ = Sign(sk₁, msg)
//! Signer 2: sk₂, pk₂, σ₂ = Sign(sk₂, msg)
//! ...
//! Signer N: skₙ, pkₙ, σₙ = Sign(skₙ, msg)
//!
//! Aggregate:
//!   σ_agg = σ₁ + σ₂ + ... + σₙ
//!   pk_agg = pk₁ + pk₂ + ... + pkₙ
//!
//! Verify:
//!   e(σ_agg, g₂) == e(H(msg), pk_agg)
//! ```
//!
//! # References
//!
//! - Boneh, D., Lynn, B., Shacham, H. *Short Signatures from the
//!   Weil Pairing* (ASIACRYPT 2001).
//!   <https://www.iacr.org/archive/asiacrypt2001/22480516.pdf>
//! - Boneh, D., Drijvers, M., Neven, G. *Compact Multi-Signatures
//!   for Smaller Blockchains* (ASIACRYPT 2018).
//!   <https://eprint.iacr.org/2018/483>
//! - Ethereum 2.0 Specification: BLS Signatures.
//!   <https://github.com/ethereum/consensus-specs/blob/dev/specs/phase0/beacon-chain.md#bls-signatures>

use blst::min_sig::{
    AggregatePublicKey, AggregateSignature, PublicKey as BlstPublicKey, SecretKey as BlstSecretKey,
    Signature as BlstSignature,
};
use blst::BLST_ERROR;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// BLS signature errors.
#[derive(Error, Debug)]
pub enum BlsError {
    /// Signature verification failed.
    #[error("BLS signature verification failed")]
    VerificationFailed,
    /// Invalid public key.
    #[error("invalid BLS public key: {0}")]
    InvalidPublicKey(String),
    /// Invalid signature.
    #[error("invalid BLS signature: {0}")]
    InvalidSignature(String),
    /// Aggregation failed (e.g., empty signature set).
    #[error("BLS aggregation failed: {0}")]
    AggregationFailed(String),
    /// Key generation failed.
    #[error("BLS key generation failed: {0}")]
    KeyGenerationFailed(String),
}

/// Size of a BLS12-381 secret key in bytes.
pub const SECRET_KEY_SIZE: usize = 32;

/// Size of a BLS12-381 G2 public key in compressed form (min_sig variant).
pub const PUBLIC_KEY_SIZE: usize = 96;

/// Size of a BLS12-381 G1 signature in compressed form (min_sig variant).
pub const SIGNATURE_SIZE: usize = 48;

/// Domain separation tag for BLS signatures.
///
/// Follows the IETF BLS signature specification (draft-irtf-cfrg-bls-signature-05)
/// using the `hash_to_curve` method with SHA-256.
const BLS_DST: &[u8] = b"BLS_SIG_BLS12381G1_XMD:SHA-256_SSWU_RO_NUL_";

/// Domain separation tag for BLS Proof-of-Possession.
///
/// Each validator must sign `H("BLS_POP_BLS12381G1" || pk_bytes)` to prove
/// they control the private key corresponding to their public key. This
/// prevents rogue-key attacks in aggregate signature schemes.
const BLS_POP_DST: &[u8] = b"BLS_POP_BLS12381G1";

/// A BLS keypair consisting of a secret key and public key.
///
/// The secret key is a 32-byte scalar, and the public key is the
/// corresponding G2 point on BLS12-381 (min_sig variant).
///
/// # Example
///
/// ```ignore
/// use omnia_substrate::bls::BlsKeypair;
///
/// let keypair = BlsKeypair::generate(None);
/// let msg = b"hello world";
/// let sig = keypair.sign(msg);
/// assert!(keypair.public_key().verify(msg, &sig).is_ok());
/// ```
#[derive(Debug, Clone)]
pub struct BlsKeypair {
    /// The blst secret key.
    secret_key: BlstSecretKey,
    /// The corresponding blst public key.
    public_key: BlstPublicKey,
}

/// A BLS public key (G2 point on BLS12-381).
///
/// Stored in compressed form (96 bytes).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlsPublicKey(
    /// Compressed G2 point (96 bytes).
    #[serde(with = "serde_bytes")]
    Vec<u8>,
);

/// A BLS signature (G1 point on BLS12-381).
///
/// Stored in compressed form (48 bytes).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlsSignature(
    /// Compressed G1 point (48 bytes).
    #[serde(with = "serde_bytes")]
    Vec<u8>,
);

impl BlsKeypair {
    /// Generate a new BLS keypair.
    ///
    /// Uses the `blst` library's key derivation function to derive a
    /// secret key from the provided seed. If no seed is given, a
    /// default zero seed is used (for testing only — production code
    /// must provide cryptographic entropy).
    ///
    /// # Arguments
    ///
    /// * `seed` — Optional seed bytes for key generation. If `None`,
    ///   a zero seed is used (testing only).
    ///
    /// # Returns
    ///
    /// A new [`BlsKeypair`] with a random or seeded secret key.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use omnia_substrate::bls::BlsKeypair;
    ///
    /// let keypair = BlsKeypair::generate(None);
    /// ```
    pub fn generate(seed: Option<&[u8]>) -> Self {
        let ikm = seed.unwrap_or_else(|| {
            #[cfg(debug_assertions)]
            panic!("BlsKeypair::generate(None) is insecure — provide a seed or use BlsKeypair::generate_random()");
            #[cfg(not(debug_assertions))]
            {
                tracing::warn!("⚠️  BLS keypair generated with zero seed — INSECURE for production");
                &[0u8; 32]
            }
        });
        let sk = BlstSecretKey::key_gen(ikm, &[])
            .expect("BLS key generation should not fail with valid input");
        let pk = sk.sk_to_pk();
        Self {
            secret_key: sk,
            public_key: pk,
        }
    }

    /// Generate a BLS keypair with cryptographically random entropy.
    pub fn generate_random() -> Self {
        let mut ikm = [0u8; 32];
        getrandom::getrandom(&mut ikm).expect("Failed to generate random IKM for BLS key");
        let sk = BlstSecretKey::key_gen(&ikm, &[])
            .expect("BLS key generation should not fail with valid input");
        let pk = sk.sk_to_pk();
        Self {
            secret_key: sk,
            public_key: pk,
        }
    }

    /// Get the serializable public key.
    ///
    /// # Returns
    ///
    /// A [`BlsPublicKey`] (the wrapped, serializable form) associated
    /// with this keypair.
    pub fn public_key(&self) -> BlsPublicKey {
        BlsPublicKey(self.public_key.compress().as_slice().to_vec())
    }

    /// Sign a message using this keypair's secret key.
    ///
    /// The signature is a G1 point on BLS12-381. The message is hashed
    /// to a G1 point using the `hash_to_curve` algorithm with the BLS
    /// signature domain separator, then multiplied by the secret key.
    ///
    /// # Arguments
    ///
    /// * `message` — The message to sign
    ///
    /// # Returns
    ///
    /// A [`BlsSignature`] over the message.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use omnia_substrate::bls::BlsKeypair;
    ///
    /// let keypair = BlsKeypair::generate(None);
    /// let sig = keypair.sign(b"hello world");
    /// ```
    pub fn sign(&self, message: &[u8]) -> BlsSignature {
        let sig = self.secret_key.sign(message, BLS_DST, &[]);
        BlsSignature(sig.compress().as_slice().to_vec())
    }
}

impl BlsPublicKey {
    /// Create a public key from raw compressed bytes.
    ///
    /// # Arguments
    ///
    /// * `bytes` — The compressed G2 point bytes (96 bytes)
    ///
    /// # Returns
    ///
    /// A [`BlsPublicKey`], or [`BlsError::InvalidPublicKey`] if the bytes
    /// are not a valid G2 point.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, BlsError> {
        if bytes.len() != PUBLIC_KEY_SIZE {
            return Err(BlsError::InvalidPublicKey(format!(
                "expected {} bytes, got {}",
                PUBLIC_KEY_SIZE,
                bytes.len()
            )));
        }

        let pk = BlstPublicKey::from_bytes(bytes)
            .map_err(|e| BlsError::InvalidPublicKey(format!("{:?}", e)))?;

        // Validate the public key (group check)
        pk.validate()
            .map_err(|e| BlsError::InvalidPublicKey(format!("validation failed: {:?}", e)))?;

        Ok(BlsPublicKey(bytes.to_vec()))
    }

    /// Get the raw bytes of the public key.
    ///
    /// # Returns
    ///
    /// A slice of the compressed G2 point bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Verify a BLS signature against this public key and message.
    ///
    /// Uses the `blst` library's `verify` function to check that
    /// `e(σ, g₂) == e(H(m), pk)`, where `σ` is the signature, `g₂`
    /// is the G2 generator, `H(m)` is the hashed message, and `pk`
    /// is this public key.
    ///
    /// # Arguments
    ///
    /// * `message` — The message that was signed
    /// * `signature` — The [`BlsSignature`] to verify
    ///
    /// # Returns
    ///
    /// `Ok(())` if the signature is valid, `Err(BlsError)` otherwise.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use omnia_substrate::bls::{BlsKeypair, BlsPublicKey};
    ///
    /// let keypair = BlsKeypair::generate(None);
    /// let sig = keypair.sign(b"hello");
    /// let pk = keypair.public_key();
    /// pk.verify(b"hello", &sig)?;
    /// ```
    pub fn verify(&self, message: &[u8], signature: &BlsSignature) -> Result<(), BlsError> {
        let pk = BlstPublicKey::from_bytes(&self.0)
            .map_err(|e| BlsError::InvalidPublicKey(format!("{:?}", e)))?;

        let sig = BlstSignature::from_bytes(&signature.0)
            .map_err(|e| BlsError::InvalidSignature(format!("{:?}", e)))?;

        let result = sig.verify(true, message, BLS_DST, &[], &pk, false);

        match result {
            BLST_ERROR::BLST_SUCCESS => Ok(()),
            _ => Err(BlsError::VerificationFailed),
        }
    }

    /// Convert to the underlying blst public key type.
    fn to_blst(&self) -> Result<BlstPublicKey, BlsError> {
        BlstPublicKey::from_bytes(&self.0)
            .map_err(|e| BlsError::InvalidPublicKey(format!("{:?}", e)))
    }
}

impl BlsSignature {
    /// Create a signature from raw compressed bytes.
    ///
    /// # Arguments
    ///
    /// * `bytes` — The compressed G1 point bytes (48 bytes)
    ///
    /// # Returns
    ///
    /// A [`BlsSignature`], or [`BlsError::InvalidSignature`] if the bytes
    /// are not a valid G1 point.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, BlsError> {
        if bytes.len() != SIGNATURE_SIZE {
            return Err(BlsError::InvalidSignature(format!(
                "expected {} bytes, got {}",
                SIGNATURE_SIZE,
                bytes.len()
            )));
        }

        // Validate by attempting to deserialize
        let _sig = BlstSignature::from_bytes(bytes)
            .map_err(|e| BlsError::InvalidSignature(format!("{:?}", e)))?;

        Ok(BlsSignature(bytes.to_vec()))
    }

    /// Get the raw bytes of the signature.
    ///
    /// # Returns
    ///
    /// A slice of the compressed G1 point bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Convert to the underlying blst signature type.
    fn to_blst(&self) -> Result<BlstSignature, BlsError> {
        BlstSignature::from_bytes(&self.0)
            .map_err(|e| BlsError::InvalidSignature(format!("{:?}", e)))
    }
}

/// BLS Proof-of-Possession.
///
/// Prevents rogue-key attacks in aggregate signature schemes.
/// Each validator must submit a PoP before their public key is accepted
/// for aggregation. The PoP is a BLS signature on the public key itself.
pub struct BlsProofOfPossession {
    /// The public key this PoP is for.
    pub(crate) public_key: BlsPublicKey,
    /// BLS signature on H("BLS_POP_BLS12381G1" || public_key_bytes).
    pub(crate) proof: BlsSignature,
}

impl BlsProofOfPossession {
    /// Generate a Proof-of-Possession for the given keypair.
    ///
    /// Signs `H("BLS_POP_BLS12381G1" || pk_bytes)` with the keypair's
    /// secret key to produce a BLS signature that proves ownership
    /// of the private key corresponding to the public key.
    pub fn generate(keypair: &BlsKeypair) -> Self {
        let message = Self::pop_message(&keypair.public_key());
        let proof = keypair.sign(&message);
        Self {
            public_key: keypair.public_key(),
            proof,
        }
    }

    /// Verify this Proof-of-Possession.
    ///
    /// Checks that the proof signature is a valid BLS signature on
    /// the PoP message under the claimed public key.
    pub fn verify(&self) -> bool {
        let message = Self::pop_message(&self.public_key);
        self.public_key.verify(&message, &self.proof).is_ok()
    }

    /// Construct the PoP message: `"BLS_POP_BLS12381G1" || public_key_bytes`.
    pub fn pop_message(public_key: &BlsPublicKey) -> Vec<u8> {
        let mut msg = Vec::with_capacity(BLS_POP_DST.len() + public_key.as_bytes().len());
        msg.extend_from_slice(BLS_POP_DST);
        msg.extend_from_slice(public_key.as_bytes());
        msg
    }
}

/// Aggregate multiple BLS signatures into a single signature.
///
/// Combines N G1 points (signatures) into one by point addition.
/// The resulting aggregate signature can be verified against the
/// aggregate public key for the same message.
///
/// # Arguments
///
/// * `signatures` — Slice of [`BlsSignature`]s to aggregate
///
/// # Returns
///
/// An aggregated [`BlsSignature`], or [`BlsError::AggregationFailed`]
/// if the input is empty or any signature is invalid.
///
/// # Example
///
/// ```ignore
/// use omnia_substrate::bls::{BlsKeypair, aggregate_signatures};
///
/// let kp1 = BlsKeypair::generate(None);
/// let kp2 = BlsKeypair::generate(None);
/// let msg = b"same message";
///
/// let sig1 = kp1.sign(msg);
/// let sig2 = kp2.sign(msg);
/// let agg_sig = aggregate_signatures(&[sig1, sig2])?;
/// ```
pub fn aggregate_signatures(signatures: &[BlsSignature]) -> Result<BlsSignature, BlsError> {
    if signatures.is_empty() {
        return Err(BlsError::AggregationFailed(
            "cannot aggregate empty signature set".to_string(),
        ));
    }

    if signatures.len() == 1 {
        return Ok(signatures[0].clone());
    }

    // Deserialize all signatures to blst types
    let blst_sigs: Vec<BlstSignature> = signatures
        .iter()
        .map(|s| s.to_blst())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| BlsError::AggregationFailed(format!("deserialization: {}", e)))?;

    // Create references for the blst aggregate function
    let sig_refs: Vec<&BlstSignature> = blst_sigs.iter().collect();

    let agg = AggregateSignature::aggregate(&sig_refs, false)
        .map_err(|e| BlsError::AggregationFailed(format!("blst aggregation: {:?}", e)))?;

    let compressed = agg.to_signature().compress();
    Ok(BlsSignature(compressed.to_vec()))
}

/// Aggregate multiple BLS public keys into a single public key.
///
/// Combines N G2 points (public keys) into one by point addition.
/// Used together with [`aggregate_signatures`] for efficient batch
/// verification.
///
/// # Arguments
///
/// * `public_keys` — Slice of [`BlsPublicKey`]s to aggregate
///
/// # Returns
///
/// An aggregated [`BlsPublicKey`], or [`BlsError::AggregationFailed`]
/// if the input is empty or any public key is invalid.
///
/// # Example
///
/// ```ignore
/// use omnia_substrate::bls::{BlsKeypair, aggregate_public_keys};
///
/// let kp1 = BlsKeypair::generate(None);
/// let kp2 = BlsKeypair::generate(None);
/// let agg_pk = aggregate_public_keys(&[kp1.public_key(), kp2.public_key()])?;
/// ```
pub fn aggregate_public_keys(public_keys: &[BlsPublicKey]) -> Result<BlsPublicKey, BlsError> {
    if public_keys.is_empty() {
        return Err(BlsError::AggregationFailed(
            "cannot aggregate empty public key set".to_string(),
        ));
    }

    if public_keys.len() == 1 {
        return Ok(public_keys[0].clone());
    }

    // Deserialize all public keys to blst types
    let blst_pks: Vec<BlstPublicKey> = public_keys
        .iter()
        .map(|pk| pk.to_blst())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| BlsError::AggregationFailed(format!("deserialization: {}", e)))?;

    // Create references for the blst aggregate function
    let pk_refs: Vec<&BlstPublicKey> = blst_pks.iter().collect();

    let agg = AggregatePublicKey::aggregate(&pk_refs, false)
        .map_err(|e| BlsError::AggregationFailed(format!("blst pk aggregation: {:?}", e)))?;

    let compressed = agg.to_public_key().compress();
    Ok(BlsPublicKey(compressed.to_vec()))
}

/// Verify an aggregated BLS signature against an aggregated public key.
///
/// Checks that `e(σ_agg, g₂) == e(H(msg), pk_agg)`, where `σ_agg`
/// is the aggregate signature, `g₂` is the G2 generator, `H(msg)`
/// is the hashed message, and `pk_agg` is the aggregate public key.
///
/// This reduces N individual verifications to a single pairing check.
///
/// # Arguments
///
/// * `message` — The message that all signers signed
/// * `aggregate_public_key` — The aggregated [`BlsPublicKey`]
/// * `aggregate_signature` — The aggregated [`BlsSignature`]
///
/// # Returns
///
/// `Ok(())` if the aggregate signature is valid, `Err(BlsError)` otherwise.
///
/// # Example
///
/// ```ignore
/// use omnia_substrate::bls::{
///     BlsKeypair, aggregate_signatures, aggregate_public_keys, verify_aggregate,
/// };
///
/// let kp1 = BlsKeypair::generate(None);
/// let kp2 = BlsKeypair::generate(None);
/// let msg = b"same message";
///
/// let sig1 = kp1.sign(msg);
/// let sig2 = kp2.sign(msg);
/// let agg_sig = aggregate_signatures(&[sig1, sig2])?;
/// let agg_pk = aggregate_public_keys(&[kp1.public_key(), kp2.public_key()])?;
///
/// verify_aggregate(msg, &agg_pk, &agg_sig)?;
/// ```
pub fn verify_aggregate(
    message: &[u8],
    aggregate_public_key: &BlsPublicKey,
    aggregate_signature: &BlsSignature,
) -> Result<(), BlsError> {
    // Aggregate verification is equivalent to single-key verification
    // with the aggregated key and signature
    aggregate_public_key.verify(message, aggregate_signature)
}

/// Verify aggregate signature with proof-of-possession.
///
/// This is the safe version of [`verify_aggregate`] — it requires that all
/// participants have submitted valid PoPs, preventing rogue-key attacks.
///
/// # Arguments
///
/// * `message` — The message that all signers signed
/// * `public_keys` — Slice of [`BlsPublicKey`]s from all signers
/// * `pops` — Slice of [`BlsProofOfPossession`]s, one per signer
/// * `aggregate_signature` — The aggregated [`BlsSignature`]
///
/// # Returns
///
/// `true` if all PoPs are valid and the aggregate signature verifies.
/// `false` if any PoP is invalid, counts mismatch, or verification fails.
pub fn verify_aggregate_with_pop(
    message: &[u8],
    public_keys: &[BlsPublicKey],
    pops: &[BlsProofOfPossession],
    aggregate_signature: &BlsSignature,
) -> bool {
    // All signers must have a valid PoP
    if public_keys.len() != pops.len() {
        return false;
    }

    // Verify each PoP
    for (pk, pop) in public_keys.iter().zip(pops.iter()) {
        if pk != &pop.public_key || !pop.verify() {
            return false;
        }
    }

    // With valid PoPs, standard aggregate verification is safe
    let agg_pk = match aggregate_public_keys(public_keys) {
        Ok(pk) => pk,
        Err(_) => return false,
    };

    verify_aggregate(message, &agg_pk, aggregate_signature).is_ok()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_bls_keypair_generate() {
        let kp = BlsKeypair::generate(Some(&[1u8; 32]));
        let pk = kp.public_key();
        assert_eq!(pk.0.len(), PUBLIC_KEY_SIZE);
    }

    #[test]
    fn test_bls_keypair_deterministic() {
        let seed = [42u8; 32];
        let kp1 = BlsKeypair::generate(Some(&seed));
        let kp2 = BlsKeypair::generate(Some(&seed));
        assert_eq!(kp1.public_key(), kp2.public_key());
    }

    #[test]
    fn test_bls_sign_and_verify() {
        let kp = BlsKeypair::generate(Some(&[1u8; 32]));
        let msg = b"test message for BLS";
        let sig = kp.sign(msg);
        assert_eq!(sig.0.len(), SIGNATURE_SIZE);

        let pk = kp.public_key();
        pk.verify(msg, &sig).expect("verification should succeed");
    }

    #[test]
    fn test_bls_verify_wrong_message() {
        let kp = BlsKeypair::generate(Some(&[2u8; 32]));
        let sig = kp.sign(b"correct message");

        let pk = kp.public_key();
        let result = pk.verify(b"wrong message", &sig);
        assert!(result.is_err());
    }

    #[test]
    fn test_bls_verify_wrong_key() {
        let kp1 = BlsKeypair::generate(Some(&[3u8; 32]));
        let kp2 = BlsKeypair::generate(Some(&[4u8; 32]));
        let msg = b"same message";
        let sig = kp1.sign(msg);

        let pk2 = kp2.public_key();
        let result = pk2.verify(msg, &sig);
        assert!(result.is_err());
    }

    #[test]
    fn test_bls_aggregate_signatures() {
        let kp1 = BlsKeypair::generate(Some(&[10u8; 32]));
        let kp2 = BlsKeypair::generate(Some(&[20u8; 32]));
        let kp3 = BlsKeypair::generate(Some(&[30u8; 32]));
        let msg = b"same message for all";

        let sig1 = kp1.sign(msg);
        let sig2 = kp2.sign(msg);
        let sig3 = kp3.sign(msg);

        let agg_sig =
            aggregate_signatures(&[sig1, sig2, sig3]).expect("aggregation should succeed");
        assert_eq!(agg_sig.0.len(), SIGNATURE_SIZE);
    }

    #[test]
    fn test_bls_aggregate_public_keys() {
        let kp1 = BlsKeypair::generate(Some(&[11u8; 32]));
        let kp2 = BlsKeypair::generate(Some(&[22u8; 32]));

        let agg_pk = aggregate_public_keys(&[kp1.public_key(), kp2.public_key()])
            .expect("aggregation should succeed");
        assert_eq!(agg_pk.0.len(), PUBLIC_KEY_SIZE);
    }

    #[test]
    fn test_bls_verify_aggregate() {
        let kp1 = BlsKeypair::generate(Some(&[15u8; 32]));
        let kp2 = BlsKeypair::generate(Some(&[25u8; 32]));
        let kp3 = BlsKeypair::generate(Some(&[35u8; 32]));
        let msg = b"aggregate verify test";

        let sig1 = kp1.sign(msg);
        let sig2 = kp2.sign(msg);
        let sig3 = kp3.sign(msg);

        let agg_sig =
            aggregate_signatures(&[sig1, sig2, sig3]).expect("sig aggregation should succeed");
        let agg_pk = aggregate_public_keys(&[kp1.public_key(), kp2.public_key(), kp3.public_key()])
            .expect("pk aggregation should succeed");

        verify_aggregate(msg, &agg_pk, &agg_sig).expect("aggregate verification should succeed");
    }

    #[test]
    fn test_bls_aggregate_empty_fails() {
        let result = aggregate_signatures(&[]);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            BlsError::AggregationFailed(_)
        ));

        let result = aggregate_public_keys(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_bls_single_aggregate_equals_original() {
        let kp = BlsKeypair::generate(Some(&[77u8; 32]));
        let msg = b"single signer";
        let sig = kp.sign(msg);

        let agg_sig = aggregate_signatures(&[sig.clone()]).expect("single sig aggregation");
        let agg_pk = aggregate_public_keys(&[kp.public_key()]).expect("single pk aggregation");

        verify_aggregate(msg, &agg_pk, &agg_sig).expect("single aggregate should verify");
    }

    #[test]
    fn test_bls_public_key_from_bytes() {
        let kp = BlsKeypair::generate(Some(&[88u8; 32]));
        let pk = kp.public_key();
        let pk2 = BlsPublicKey::from_bytes(pk.as_bytes()).expect("from_bytes should work");
        assert_eq!(pk, pk2);
    }

    #[test]
    fn test_bls_signature_from_bytes() {
        let kp = BlsKeypair::generate(Some(&[99u8; 32]));
        let sig = kp.sign(b"test");
        let sig2 = BlsSignature::from_bytes(sig.as_bytes()).expect("from_bytes should work");
        assert_eq!(sig, sig2);
    }

    #[test]
    fn test_bls_public_key_wrong_length() {
        let wrong_len = vec![0u8; 10];
        let result = BlsPublicKey::from_bytes(&wrong_len);
        assert!(result.is_err());
    }

    #[test]
    fn test_bls_signature_wrong_length() {
        let wrong_len = vec![0u8; 10];
        let result = BlsSignature::from_bytes(&wrong_len);
        assert!(result.is_err());
    }

    #[test]
    fn test_bls_different_messages_cannot_aggregate_verify() {
        let kp1 = BlsKeypair::generate(Some(&[55u8; 32]));
        let kp2 = BlsKeypair::generate(Some(&[66u8; 32]));

        // Sign different messages — aggregate verify should fail
        let sig1 = kp1.sign(b"message one");
        let sig2 = kp2.sign(b"message two");

        let agg_sig = aggregate_signatures(&[sig1, sig2]).expect("aggregation should succeed");
        let agg_pk =
            aggregate_public_keys(&[kp1.public_key(), kp2.public_key()]).expect("pk aggregation");

        // Aggregate verify against a single message should fail
        // because the signers signed different messages
        let result = verify_aggregate(b"message one", &agg_pk, &agg_sig);
        assert!(result.is_err());
    }

    // --- Proof-of-Possession tests (Phase C2) ---

    #[test]
    fn test_pop_generation_and_verification() {
        let keypair = BlsKeypair::generate(Some(&[1u8; 32]));
        let pop = BlsProofOfPossession::generate(&keypair);
        assert!(pop.verify());
    }

    #[test]
    fn test_pop_wrong_key_fails() {
        let keypair1 = BlsKeypair::generate(Some(&[1u8; 32]));
        let keypair2 = BlsKeypair::generate(Some(&[2u8; 32]));

        // Generate a valid PoP for keypair1
        let mut pop = BlsProofOfPossession::generate(&keypair1);
        // Swap the public key — PoP should no longer verify
        pop.public_key = keypair2.public_key();
        assert!(!pop.verify());
    }

    #[test]
    fn test_aggregate_with_pop_prevents_rogue_key() {
        let kp1 = BlsKeypair::generate(Some(&[10u8; 32]));
        let kp2 = BlsKeypair::generate(Some(&[20u8; 32]));
        let msg = b"protected message";

        let sig1 = kp1.sign(msg);
        let sig2 = kp2.sign(msg);

        let pop1 = BlsProofOfPossession::generate(&kp1);
        let pop2 = BlsProofOfPossession::generate(&kp2);

        let agg_sig = aggregate_signatures(&[sig1, sig2]).unwrap();

        // Both PoPs valid → verification succeeds
        assert!(verify_aggregate_with_pop(
            msg,
            &[kp1.public_key(), kp2.public_key()],
            &[pop1, pop2],
            &agg_sig,
        ));
    }

    #[test]
    fn test_aggregate_with_pop_rejects_invalid_pop() {
        let kp1 = BlsKeypair::generate(Some(&[10u8; 32]));
        let kp2 = BlsKeypair::generate(Some(&[20u8; 32]));
        let msg = b"protected message";

        let sig1 = kp1.sign(msg);
        let sig2 = kp2.sign(msg);

        let pop1 = BlsProofOfPossession::generate(&kp1);
        // Create a PoP for a different keypair but present it for kp2
        let kp3 = BlsKeypair::generate(Some(&[30u8; 32]));
        let pop3 = BlsProofOfPossession::generate(&kp3);

        let agg_sig = aggregate_signatures(&[sig1, sig2]).unwrap();

        // PoP mismatch: pop3 is for kp3, not kp2 → verification fails
        assert!(!verify_aggregate_with_pop(
            msg,
            &[kp1.public_key(), kp2.public_key()],
            &[pop1, pop3],
            &agg_sig,
        ));
    }
}
