//! Participant contribution logic for the trusted setup ceremony.
//!
//! Each participant in the ceremony contributes randomness to the Powers of
//! Tau transcript. A contribution consists of:
//!
//! - A **participant identifier** (public key or hash)
//! - A **transcript** of the updated accumulator
//! - A **proof** that the contribution was computed correctly
//! - A **public key** for attribution
//!
//! # References
//!
//! - Bowe, S., Gabizon, A., Green, M. *A Multi-Party Protocol for
//!   Constructing the Public Parameters of the Pinocchio zk-SNARK System*
//!   (Zcash, 2018). <https://eprint.iacr.org/2017/601>
//! - Groth, J. *On the Size of Pairing-based Non-interactive Arguments*
//!   (EUROCRYPT 2016). <https://eprint.iacr.org/2016/260>

use ark_ff::UniformRand;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

use super::SetupError;

/// A participant's contribution to the trusted setup ceremony.
///
/// Each contribution adds randomness to the Powers of Tau accumulator.
/// The `proof` field demonstrates that the contribution was correctly
/// computed without knowledge of any previous contributor's secret.
///
/// # Fields
///
/// - `participant_id` — A unique identifier for the contributor (hash of public key)
/// - `transcript` — Serialized updated Powers of Tau accumulator after this contribution
/// - `proof` — Proof that this contribution was correctly computed
/// - `public_key` — The participant's public key for attribution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contribution {
    /// Unique identifier for the contributor (hash of public key).
    pub participant_id: [u8; 32],
    /// Serialized updated Powers of Tau accumulator after this contribution.
    pub transcript: Vec<u8>,
    /// Proof that this contribution was correctly computed.
    ///
    /// In a full implementation this would be a PoK of the secret `s` such
    /// that `new_tau[i] = old_tau[i] * s` for all `i`. Here we store a
    /// hash-based commitment to enable offline verification.
    pub proof: Vec<u8>,
    /// The participant's public key for attribution.
    pub public_key: Vec<u8>,
}

/// Verify a single contribution to the Powers of Tau ceremony.
///
/// Checks that:
/// 1. The contribution's proof is valid (hash-based commitment check)
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

    // Check that the proof is non-empty
    if contribution.proof.is_empty() {
        return Err(SetupError::InvalidContribution(
            "Proof must not be empty".to_string(),
        ));
    }

    // Compute a hash-based commitment: H(participant_id || previous_transcript || new_transcript)
    let mut preimage = Vec::new();
    preimage.extend_from_slice(&contribution.participant_id);
    preimage.extend_from_slice(previous_transcript);
    preimage.extend_from_slice(&contribution.transcript);
    let commitment = blake3::hash(&preimage);

    // The proof should match the commitment (simplified verification)
    if contribution.proof != commitment.as_bytes().as_slice() {
        return Err(SetupError::InvalidContribution(
            "Proof commitment mismatch".to_string(),
        ));
    }

    tracing::info!(
        participant = ?&contribution.participant_id[..4],
        "Contribution verified successfully"
    );
    Ok(())
}

/// Create a new contribution to the Powers of Tau ceremony.
///
/// Generates fresh randomness, updates the accumulator, and produces
/// a proof of correct computation.
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

    // Sample a random scalar for the contribution
    let _secret = ark_bn254::Fr::rand(&mut rng);

    // Generate a participant ID from the randomness
    let mut participant_id = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rng, &mut participant_id);

    // Generate a public key (simplified: hash of participant_id)
    let public_key = blake3::hash(&participant_id).as_bytes().to_vec();

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

    // Compute the proof: H(participant_id || previous_transcript || new_transcript)
    let mut proof_preimage = Vec::new();
    proof_preimage.extend_from_slice(&participant_id);
    proof_preimage.extend_from_slice(previous_transcript);
    proof_preimage.extend_from_slice(&new_transcript);
    let proof = blake3::hash(&proof_preimage).as_bytes().to_vec();

    tracing::info!(
        participant = ?&participant_id[..4],
        tau_size,
        "Created new ceremony contribution"
    );

    Ok(Contribution {
        participant_id,
        transcript: new_transcript,
        proof,
        public_key,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contribute_and_verify() {
        let tau_size = 8;
        let initial_transcript = vec![0u8; tau_size * 64];

        let contribution =
            contribute(&initial_transcript, tau_size, Some([42u8; 32])).expect("contribute failed");

        assert_eq!(contribution.transcript.len(), tau_size * 64);
        assert!(!contribution.proof.is_empty());
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
}
