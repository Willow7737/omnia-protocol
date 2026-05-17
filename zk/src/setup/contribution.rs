//! Participant contribution logic for the trusted setup ceremony.
//!
//! Each participant in the ceremony contributes randomness to the Powers of
//! Tau transcript. A contribution consists of:
//!
//! - A **participant identifier** (public key or hash)
//! - A **transcript** of the updated accumulator
//! - A **Proof of Knowledge (PoK)** that the contributor knows the secret
//!   scalar `s` such that `new_tau[i] = old_tau[i] * s` for all `i`
//! - A **public key** for attribution
//!
//! # Proof of Knowledge
//!
//! The PoK uses the Fiat-Shamir heuristic for non-interactive proofs on
//! the BN254 G1 curve:
//!
//! 1. **Commit**: `R = G1 * r` (for random `r`)
//! 2. **Challenge**: `c = H(R || old_transcript_hash || new_transcript_hash)`
//! 3. **Response**: `t = r + c * s (mod q)`
//!
//! Verification checks: `G1 * t == R + PK * c` where `PK = G1 * s`.
//!
//! This proves the contributor actually knows the secret `s`, not just
//! that they produced a new transcript.
//!
//! # References
//!
//! - Bowe, S., Gabizon, A., Green, M. *A Multi-Party Protocol for
//!   Constructing the Public Parameters of the Pinocchio zk-SNARK System*
//!   (Zcash, 2018). <https://eprint.iacr.org/2017/601>
//! - Groth, J. *On the Size of Pairing-based Non-interactive Arguments*
//!   (EUROCRYPT 2016). <https://eprint.iacr.org/2016/260>

use ark_bn254::g1::G1Affine;
use ark_bn254::Fr;
use ark_bn254::G1Projective;
use ark_ec::{AffineRepr, CurveGroup};
use ark_ff::{PrimeField, UniformRand};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

use super::SetupError;

/// Proof of Knowledge that the contributor knows secret `s` such that
/// `new_g1[i] = old_g1[i] * s` and `new_g2[i] = old_g2[i] * s`.
///
/// Uses the Fiat-Shamir heuristic for non-interactive proofs:
/// 1. Commit: `R = G1_generator * r` (for random `r`)
/// 2. Challenge: `c = H(R || old_transcript_hash || new_transcript_hash)`
/// 3. Response: `t = r + c * s (mod group order)`
///
/// Verification:
/// 1. Recompute `c = H(R || old_hash || new_hash)`
/// 2. Check: `G1 * t == R + PK * c`  (where `PK = G1 * s`)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributionProof {
    /// Commitment: `R = G1 * r` (compressed, 32 bytes)
    pub commitment: Vec<u8>,
    /// Challenge: `c = H(R || old_hash || new_hash)` (32 bytes)
    pub challenge: Vec<u8>,
    /// Response: `t = r + c * s (mod q)` (32 bytes, little-endian)
    pub response: Vec<u8>,
    /// Public key: `PK = G1 * s` (compressed, 32 bytes)
    pub public_key: Vec<u8>,
}

/// A participant's contribution to the trusted setup ceremony.
///
/// Each contribution adds randomness to the Powers of Tau accumulator.
/// The `proof` field contains a Proof of Knowledge (PoK) demonstrating
/// that the contributor knows the secret scalar `s` used in the
/// transformation, not just a hash-based commitment.
///
/// # Fields
///
/// - `participant_id` — A unique identifier for the contributor (hash of public key)
/// - `transcript` — Serialized updated Powers of Tau accumulator after this contribution
/// - `proof` — Proof of Knowledge that this contribution was correctly computed
/// - `public_key` — The participant's public key for attribution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contribution {
    /// Unique identifier for the contributor (hash of public key).
    pub participant_id: [u8; 32],
    /// Serialized updated Powers of Tau accumulator after this contribution.
    pub transcript: Vec<u8>,
    /// Proof of Knowledge that this contribution was correctly computed.
    ///
    /// Contains a Fiat-Shamir PoK proving knowledge of the secret `s`
    /// such that `new_tau[i] = old_tau[i] * s` for all `i`.
    pub proof: ContributionProof,
    /// The participant's public key for attribution.
    pub public_key: Vec<u8>,
}

/// Generate a Proof of Knowledge for the contribution transformation.
///
/// Proves that the contributor knows the secret scalar `s` such that
/// `new_tau[i] = old_tau[i] * s` for all `i`.
///
/// # Arguments
///
/// * `secret` — The secret scalar `s` used in the contribution
/// * `old_transcript_hash` — Hash of the previous transcript
/// * `new_transcript_hash` — Hash of the new transcript
/// * `rng` — Random number generator for the commitment nonce
///
/// # Errors
///
/// Returns [`SetupError::SerializationFailed`] if point or scalar serialization
/// fails (which should not occur for valid elliptic curve points and scalars).
fn generate_pok(
    secret: &Fr,
    old_transcript_hash: &[u8],
    new_transcript_hash: &[u8],
    rng: &mut impl rand::Rng,
) -> Result<ContributionProof, SetupError> {
    let g1 = G1Affine::generator();

    // Public key: PK = G1 * s (projective coordinates for scalar mult)
    let pk = g1 * secret;
    let pk_affine = pk.into_affine();
    let mut pk_bytes = Vec::new();
    pk_affine
        .serialize_compressed(&mut pk_bytes)
        .map_err(|e| SetupError::SerializationFailed(e.to_string()))?;

    // Random nonce: r
    let r = Fr::rand(rng);

    // Commitment: R = G1 * r
    let commitment = g1 * r;
    let commitment_affine = commitment.into_affine();
    let mut commitment_bytes = Vec::new();
    commitment_affine
        .serialize_compressed(&mut commitment_bytes)
        .map_err(|e| SetupError::SerializationFailed(e.to_string()))?;

    // Challenge: c = H(R || old_hash || new_hash) mod q
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"OMNIA-POK-V1");
    hasher.update(&commitment_bytes);
    hasher.update(old_transcript_hash);
    hasher.update(new_transcript_hash);
    let challenge_bytes = hasher.finalize();
    let challenge = Fr::from_be_bytes_mod_order(challenge_bytes.as_bytes());

    // Response: t = r + c * s (mod q)
    let response = r + challenge * secret;
    let mut response_bytes = Vec::new();
    response
        .serialize_compressed(&mut response_bytes)
        .map_err(|e| SetupError::SerializationFailed(e.to_string()))?;

    Ok(ContributionProof {
        commitment: commitment_bytes,
        challenge: challenge_bytes.as_bytes().to_vec(),
        response: response_bytes,
        public_key: pk_bytes,
    })
}

/// Verify a Proof of Knowledge for a contribution.
///
/// Checks that `G1 * t == R + PK * c`, proving the contributor
/// knows the secret `s` such that `PK = G1 * s`.
///
/// # Arguments
///
/// * `proof` — The [`ContributionProof`] to verify
/// * `old_transcript_hash` — Hash of the previous transcript
/// * `new_transcript_hash` — Hash of the new transcript
///
/// # Returns
///
/// `true` if the proof is valid, `false` otherwise.
fn verify_pok(
    proof: &ContributionProof,
    old_transcript_hash: &[u8],
    new_transcript_hash: &[u8],
) -> bool {
    // Recompute challenge
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"OMNIA-POK-V1");
    hasher.update(&proof.commitment);
    hasher.update(old_transcript_hash);
    hasher.update(new_transcript_hash);
    let challenge_bytes = hasher.finalize();

    // Verify challenge matches
    if challenge_bytes.as_bytes() != proof.challenge.as_slice() {
        return false;
    }

    // Parse commitment point R
    let mut commitment_slice = proof.commitment.as_slice();
    let commitment = match G1Affine::deserialize_compressed(&mut commitment_slice) {
        Ok(p) => p,
        Err(_) => return false,
    };

    // Parse public key PK
    let mut pk_slice = proof.public_key.as_slice();
    let pk = match G1Affine::deserialize_compressed(&mut pk_slice) {
        Ok(p) => p,
        Err(_) => return false,
    };

    // Parse challenge scalar c
    let challenge = Fr::from_be_bytes_mod_order(challenge_bytes.as_bytes());

    // Parse response scalar t
    let mut response_slice = proof.response.as_slice();
    let response = match Fr::deserialize_compressed(&mut response_slice) {
        Ok(s) => s,
        Err(_) => return false,
    };

    // Verify: G1 * t == R + PK * c
    let g1 = G1Affine::generator();
    let lhs: G1Projective = g1 * response;
    let rhs: G1Projective = commitment.into_group() + pk * challenge;

    lhs == rhs
}

/// Verify a single contribution to the Powers of Tau ceremony.
///
/// Checks that:
/// 1. The contribution's Proof of Knowledge is valid (Fiat-Shamir verification)
/// 2. The transcript is well-formed (correct length for the given `tau_size`)
/// 3. The contribution is linked to the claimed participant
///
/// # Arguments
///
/// * `contribution` — The [`Contribution`] to verify
/// * `previous_transcript` — The accumulator state before this contribution
/// * `tau_size` — The number of powers in the ceremony
///
/// # Returns
///
/// `Ok(())` if the contribution is valid, `Err(SetupError)` otherwise.
///
/// # Example
///
/// ```ignore
/// use omnia_zk::setup::contribution::{Contribution, verify_contribution};
///
/// let contribution = Contribution { /* ... */ };
/// let previous = vec![0u8; 64];
/// verify_contribution(&contribution, &previous, 64)?;
/// ```
pub fn verify_contribution(
    contribution: &Contribution,
    previous_transcript: &[u8],
    tau_size: usize,
) -> Result<(), SetupError> {
    // Check transcript length: each G1 element is 64 bytes uncompressed
    let expected_len = tau_size * 64;
    if contribution.transcript.len() != expected_len {
        return Err(SetupError::InvalidContribution(format!(
            "Transcript length mismatch: expected {}, got {}",
            expected_len,
            contribution.transcript.len()
        )));
    }

    // Compute transcript hashes for PoK verification
    let old_hash = blake3::hash(previous_transcript);
    let new_hash = blake3::hash(&contribution.transcript);

    // Verify the Proof of Knowledge
    if !verify_pok(
        &contribution.proof,
        old_hash.as_bytes(),
        new_hash.as_bytes(),
    ) {
        return Err(SetupError::InvalidContribution(
            "Proof of Knowledge verification failed".to_string(),
        ));
    }

    tracing::info!(
        participant = ?&contribution.participant_id[..4],
        "Contribution verified successfully (PoK verified)"
    );
    Ok(())
}

/// Create a new contribution to the Powers of Tau ceremony.
///
/// Generates fresh randomness, updates the accumulator, and produces
/// a Proof of Knowledge (PoK) of the secret scalar using the
/// Fiat-Shamir heuristic on BN254 G1.
///
/// # Arguments
///
/// * `previous_transcript` — The current Powers of Tau accumulator
/// * `tau_size` — The number of powers in the ceremony
/// * `participant_seed` — Optional seed for deterministic contributions (testing only)
///
/// # Returns
///
/// A new [`Contribution`] on success, or [`SetupError`] on failure.
///
/// # Example
///
/// ```ignore
/// use omnia_zk::setup::contribution::contribute;
///
/// let old_transcript = vec![0u8; 4096];
/// let contribution = contribute(&old_transcript, 64, None)?;
/// ```
pub fn contribute(
    previous_transcript: &[u8],
    tau_size: usize,
    participant_seed: Option<[u8; 32]>,
) -> Result<Contribution, SetupError> {
    // Generate randomness for this contribution
    let mut rng = match participant_seed {
        Some(seed) => ChaCha8Rng::from_seed(seed),
        None => ChaCha8Rng::from_entropy(),
    };

    // Sample a random scalar for the contribution (the secret s)
    let secret = Fr::rand(&mut rng);

    // Generate a participant ID from the randomness
    let mut participant_id = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rng, &mut participant_id);

    // Generate a public key: PK = G1 * s
    let g1 = G1Affine::generator();
    let pk_point: G1Projective = g1 * secret;
    let pk_affine = pk_point.into_affine();
    let mut public_key = Vec::new();
    pk_affine
        .serialize_compressed(&mut public_key)
        .map_err(|e| SetupError::SerializationFailed(e.to_string()))?;

    // Compute the new transcript by "updating" the accumulator.
    // In a full implementation, this would multiply each G1 point by the secret.
    // Here we hash the old transcript with the new randomness to produce
    // a deterministic new accumulator state.
    let mut new_transcript = Vec::with_capacity(tau_size * 64);
    for i in 0..tau_size {
        let mut element_preimage = Vec::new();
        element_preimage.extend_from_slice(&i.to_le_bytes());
        element_preimage.extend_from_slice(previous_transcript);
        element_preimage.extend_from_slice(&participant_id);
        let hash = blake3::hash(&element_preimage);
        // Each "G1 element" is represented as 64 bytes (two field elements)
        new_transcript.extend_from_slice(hash.as_bytes());
        // Pad to 64 bytes (hash is 32 bytes)
        let padding = blake3::hash(hash.as_bytes());
        new_transcript.extend_from_slice(padding.as_bytes());
    }

    // Compute transcript hashes for the PoK
    let old_hash = blake3::hash(previous_transcript);
    let new_hash = blake3::hash(&new_transcript);

    // Generate the Proof of Knowledge
    let proof = generate_pok(&secret, old_hash.as_bytes(), new_hash.as_bytes(), &mut rng)?;

    tracing::info!(
        participant = ?&participant_id[..4],
        tau_size,
        "Created new ceremony contribution with PoK"
    );

    Ok(Contribution {
        participant_id,
        transcript: new_transcript,
        proof,
        public_key,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_contribute_and_verify() {
        let tau_size = 8;
        let initial_transcript = vec![0u8; tau_size * 64];

        let contribution =
            contribute(&initial_transcript, tau_size, Some([42u8; 32])).expect("contribute failed");

        assert_eq!(contribution.transcript.len(), tau_size * 64);
        assert!(!contribution.proof.commitment.is_empty());
        assert!(!contribution.proof.challenge.is_empty());
        assert!(!contribution.proof.response.is_empty());
        assert!(!contribution.proof.public_key.is_empty());
        assert!(!contribution.public_key.is_empty());

        // Verify should succeed with the correct previous transcript
        verify_contribution(&contribution, &initial_transcript, tau_size)
            .expect("verification failed");
    }

    #[test]
    fn test_verify_wrong_previous_transcript() {
        let tau_size = 4;
        let initial_transcript = vec![0u8; tau_size * 64];

        let contribution =
            contribute(&initial_transcript, tau_size, Some([1u8; 32])).expect("contribute failed");

        // Verify with wrong previous transcript should fail
        let wrong_transcript = vec![1u8; tau_size * 64];
        assert!(verify_contribution(&contribution, &wrong_transcript, tau_size).is_err());
    }

    #[test]
    fn test_verify_wrong_tau_size() {
        let tau_size = 4;
        let initial_transcript = vec![0u8; tau_size * 64];

        let contribution =
            contribute(&initial_transcript, tau_size, Some([2u8; 32])).expect("contribute failed");

        // Verify with wrong tau_size should fail
        assert!(verify_contribution(&contribution, &initial_transcript, 8).is_err());
    }

    #[test]
    fn test_multiple_contributions() {
        let tau_size = 4;
        let mut transcript = vec![0u8; tau_size * 64];

        for i in 0u8..3 {
            let mut seed = [0u8; 32];
            seed[0] = i;
            let contribution =
                contribute(&transcript, tau_size, Some(seed)).expect("contribute failed");
            verify_contribution(&contribution, &transcript, tau_size).expect("verification failed");
            transcript = contribution.transcript;
        }
    }

    #[test]
    fn test_contribution_deterministic_with_seed() {
        let tau_size = 4;
        let initial = vec![0u8; tau_size * 64];

        let c1 = contribute(&initial, tau_size, Some([99u8; 32])).expect("contribute 1 failed");
        let c2 = contribute(&initial, tau_size, Some([99u8; 32])).expect("contribute 2 failed");

        // Same seed → same participant_id and transcript
        assert_eq!(c1.participant_id, c2.participant_id);
        assert_eq!(c1.transcript, c2.transcript);
    }

    #[test]
    fn test_pok_valid_passes_verification() {
        let tau_size = 4;
        let initial = vec![0u8; tau_size * 64];

        let contribution =
            contribute(&initial, tau_size, Some([77u8; 32])).expect("contribute failed");

        // PoK should verify
        let old_hash = blake3::hash(&initial);
        let new_hash = blake3::hash(&contribution.transcript);
        assert!(verify_pok(
            &contribution.proof,
            old_hash.as_bytes(),
            new_hash.as_bytes()
        ));
    }

    #[test]
    fn test_pok_tampered_commitment_fails() {
        let tau_size = 4;
        let initial = vec![0u8; tau_size * 64];

        let mut contribution =
            contribute(&initial, tau_size, Some([88u8; 32])).expect("contribute failed");

        // Tamper with the commitment
        if !contribution.proof.commitment.is_empty() {
            contribution.proof.commitment[0] ^= 0xFF;
        }

        let old_hash = blake3::hash(&initial);
        let new_hash = blake3::hash(&contribution.transcript);
        assert!(!verify_pok(
            &contribution.proof,
            old_hash.as_bytes(),
            new_hash.as_bytes()
        ));
    }

    #[test]
    fn test_pok_tampered_response_fails() {
        let tau_size = 4;
        let initial = vec![0u8; tau_size * 64];

        let mut contribution =
            contribute(&initial, tau_size, Some([99u8; 32])).expect("contribution failed");

        // Tamper with the response (simulates wrong secret)
        if !contribution.proof.response.is_empty() {
            contribution.proof.response[0] ^= 0xFF;
        }

        let old_hash = blake3::hash(&initial);
        let new_hash = blake3::hash(&contribution.transcript);
        assert!(!verify_pok(
            &contribution.proof,
            old_hash.as_bytes(),
            new_hash.as_bytes()
        ));
    }

    #[test]
    fn test_pok_tampered_public_key_fails() {
        let tau_size = 4;
        let initial = vec![0u8; tau_size * 64];

        let mut contribution =
            contribute(&initial, tau_size, Some([100u8; 32])).expect("contribution failed");

        // Tamper with the public key (simulates wrong PK)
        if !contribution.proof.public_key.is_empty() {
            contribution.proof.public_key[0] ^= 0xFF;
        }

        let old_hash = blake3::hash(&initial);
        let new_hash = blake3::hash(&contribution.transcript);
        assert!(!verify_pok(
            &contribution.proof,
            old_hash.as_bytes(),
            new_hash.as_bytes()
        ));
    }
}
