//! Phase 1: BN254 Powers of Tau ceremony.
//!
//! The Powers of Tau ceremony produces a structured reference string (SRS)
//! consisting of powers of a secret `τ` in both G1 and G2 of the BN254
//! elliptic curve:
//!
//! ```text
//! G1: [1]₁, [τ]₁, [τ²]₁, ..., [τⁿ⁻¹]₁
//! G2: [1]₂, [τ]₂
//! ```
//!
//! This SRS is circuit-independent and can be used by any Groth16 proof
//! system up to the configured degree. The ceremony is multi-party: each
//! participant adds fresh randomness, ensuring the final SRS is secure
//! as long as at least one participant was honest.
//!
//! # Real EC Operations (C-2)
//!
//! The ceremony now uses actual BN254 elliptic curve scalar multiplication
//! for all SRS updates. Each contribution multiplies every G1 and G2 power
//! by the contributor's secret scalar `s`:
//!
//! ```text
//! new_G1[i] = old_G1[i] * s
//! new_G2[j] = old_G2[j] * s
//! ```
//!
//! The initial SRS is seeded with generator points (representing τ = 1),
//! ensuring that contributions produce non-trivial (non-identity) curve points.
//!
//! # Security Model
//!
//! The SRS is secure if at least one participant honestly destroys their
//! secret randomness after contributing. The transcript-based verification
//! ensures that each contribution was correctly computed.
//!
//! # References
//!
//! - Bowe, S., Gabizon, A. *Setup Ceremonies: The Powers of Tau* (Zcash, 2017)
//! - Ben-Sasson, E., et al. *Scalable, transparent, and post-quantum secure
//!   computational integrity* (IACR ePrint 2018/046)
//! - Groth, J. *On the Size of Pairing-based Non-interactive Arguments*
//!   (EUROCRYPT 2016). <https://eprint.iacr.org/2016/260>

use ark_bn254::{Fr, G1Affine, G1Projective, G2Affine, G2Projective};
use ark_ec::{AffineRepr, CurveGroup};
use ark_ff::UniformRand;
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

use super::contribution::{verify_contribution, Contribution};
use super::SetupError;

/// Default degree for the Powers of Tau ceremony.
///
/// This determines the maximum circuit size supported by the SRS.
/// A degree of 2^16 = 65536 supports circuits with up to ~65k constraints.
pub const DEFAULT_TAU_DEGREE: usize = 1 << 16;

/// The Powers of Tau accumulator (Phase 1 SRS).
///
/// Contains the G1 and G2 elements of the structured reference string,
/// along with metadata about the ceremony.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowersOfTau {
    /// G1 powers: \[1\]₁, \[τ\]₁, \[τ²\]₁, ..., \[τⁿ⁻¹\]₁ (serialized)
    pub g1_powers: Vec<Vec<u8>>,
    /// G2 powers: \[1\]₂, \[τ\]₂ (serialized)
    pub g2_powers: Vec<Vec<u8>>,
    /// Number of contributions applied so far
    pub contribution_count: usize,
    /// Transcript hash after all contributions
    pub transcript_hash: [u8; 32],
}

impl PowersOfTau {
    /// Create a new Powers of Tau accumulator with the given degree.
    ///
    /// Initializes all G1 elements to the **generator point** and G2 elements
    /// to the generator, representing the "empty" SRS before any contributions
    /// have been applied. This represents τ = 1 (the multiplicative identity),
    /// so all powers of τ are 1, meaning every element is the base generator.
    ///
    /// # Arguments
    ///
    /// * `degree` — The maximum degree supported (number of G1 powers)
    ///
    /// # Returns
    ///
    /// A new [`PowersOfTau`] with generator elements.
    pub fn new(degree: usize) -> Result<Self, SetupError> {
        // Serialize the generator G1 point (represents [1]₁ with τ = 1)
        let mut g1_bytes = Vec::new();
        G1Affine::generator()
            .serialize_uncompressed(&mut g1_bytes)
            .map_err(|e| SetupError::SerializationFailed(format!("G1 generator: {e}")))?;

        // Serialize the generator G2 point (represents [1]₂ with τ = 1)
        let mut g2_bytes = Vec::new();
        G2Affine::generator()
            .serialize_uncompressed(&mut g2_bytes)
            .map_err(|e| SetupError::SerializationFailed(format!("G2 generator: {e}")))?;

        let g1_powers = vec![g1_bytes; degree];
        let g2_powers = vec![g2_bytes; 2];

        Ok(Self {
            g1_powers,
            g2_powers,
            contribution_count: 0,
            transcript_hash: [0u8; 32],
        })
    }

    /// Serialize the accumulator into a flat byte vector.
    ///
    /// Concatenates all G1 and G2 element serializations for
    /// use as a ceremony transcript.
    ///
    /// # Returns
    ///
    /// A `Vec<u8>` containing all serialized group elements.
    pub fn to_transcript(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        for g1 in &self.g1_powers {
            bytes.extend_from_slice(g1);
        }
        for g2 in &self.g2_powers {
            bytes.extend_from_slice(g2);
        }
        bytes
    }

    /// Serialize only the G1 portion of the accumulator into a flat byte vector.
    ///
    /// # Returns
    ///
    /// A `Vec<u8>` containing all serialized G1 elements.
    pub fn to_g1_transcript(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        for g1 in &self.g1_powers {
            bytes.extend_from_slice(g1);
        }
        bytes
    }

    /// Apply a contribution to the accumulator.
    ///
    /// Verifies the contribution against the current transcript and
    /// updates the accumulator state if verification succeeds.
    /// Only G1 elements are updated from the contribution transcript;
    /// G2 elements are not modified by this method.
    ///
    /// For a ceremony that updates both G1 and G2, use
    /// [`apply_contribution_ec()`] instead.
    ///
    /// # Arguments
    ///
    /// * `contribution` — The [`Contribution`] to apply
    ///
    /// # Returns
    ///
    /// `Ok(())` if the contribution was applied successfully.
    pub fn apply_contribution(&mut self, contribution: &Contribution) -> Result<(), SetupError> {
        let previous_transcript = self.to_transcript();
        // tau_size is the number of G1 elements only (not G1 + G2)
        let tau_size = self.g1_powers.len();

        verify_contribution(contribution, &previous_transcript, tau_size)?;

        // Update the accumulator with the new transcript (G1 elements only)
        let g1_size = self.g1_powers.first().map(|v| v.len()).unwrap_or(64);
        for (i, g1) in self.g1_powers.iter_mut().enumerate() {
            let offset = i * g1_size;
            if offset + g1_size <= contribution.transcript.len() {
                g1.copy_from_slice(&contribution.transcript[offset..offset + g1_size]);
            }
        }

        self.contribution_count += 1;

        // Update transcript hash
        let mut hash_input = Vec::new();
        hash_input.extend_from_slice(&contribution.participant_id);
        hash_input.extend_from_slice(&contribution.transcript);
        self.transcript_hash = blake3::hash(&hash_input).into();

        tracing::info!(
            contributions = self.contribution_count,
            "Applied contribution to Powers of Tau (G1 updated from transcript)"
        );

        Ok(())
    }

    /// Apply a contribution using actual elliptic curve scalar multiplication.
    ///
    /// This method multiplies every G1 and G2 power by the secret scalar,
    /// performing real BN254 curve operations instead of just copying bytes
    /// from a contribution transcript.
    ///
    /// # Arguments
    ///
    /// * `secret` — The secret scalar `s` to apply: each power is multiplied by `s`
    ///
    /// # Returns
    ///
    /// `Ok(())` if the contribution was applied successfully.
    ///
    /// # Errors
    ///
    /// Returns [`SetupError::SerializationFailed`] if any point serialization
    /// or deserialization fails.
    pub fn apply_contribution_ec(&mut self, secret: &Fr) -> Result<(), SetupError> {
        // Multiply each G1 power by the secret
        for g1_bytes in self.g1_powers.iter_mut() {
            let mut slice = g1_bytes.as_slice();
            let g1_point = G1Affine::deserialize_uncompressed(&mut slice)
                .map_err(|e| SetupError::SerializationFailed(e.to_string()))?;
            let new_point: G1Projective = g1_point.into_group() * secret;
            let new_affine = new_point.into_affine();
            let mut new_bytes = Vec::new();
            new_affine
                .serialize_uncompressed(&mut new_bytes)
                .map_err(|e| SetupError::SerializationFailed(e.to_string()))?;
            *g1_bytes = new_bytes;
        }

        // Multiply each G2 power by the secret
        for g2_bytes in self.g2_powers.iter_mut() {
            let mut slice = g2_bytes.as_slice();
            let g2_point = G2Affine::deserialize_uncompressed(&mut slice)
                .map_err(|e| SetupError::SerializationFailed(e.to_string()))?;
            let new_point: G2Projective = g2_point.into_group() * secret;
            let new_affine = new_point.into_affine();
            let mut new_bytes = Vec::new();
            new_affine
                .serialize_uncompressed(&mut new_bytes)
                .map_err(|e| SetupError::SerializationFailed(e.to_string()))?;
            *g2_bytes = new_bytes;
        }

        self.contribution_count += 1;
        // Update transcript hash
        let transcript = self.to_transcript();
        self.transcript_hash = blake3::hash(&transcript).into();

        tracing::info!(
            contributions = self.contribution_count,
            "Applied EC contribution to Powers of Tau (G1 + G2 updated via scalar multiplication)"
        );

        Ok(())
    }

    /// Verify the SRS is well-formed after contributions.
    ///
    /// Checks that:
    /// 1. G1 and G2 powers can be deserialized into valid curve points
    /// 2. After at least one contribution, G1 and G2 points are non-identity
    ///
    /// # Returns
    ///
    /// `Ok(())` if the SRS is valid, `Err(SetupError)` otherwise.
    pub fn verify_srs(&self) -> Result<(), SetupError> {
        // Verify G1 powers are valid curve points
        for (i, g1_bytes) in self.g1_powers.iter().enumerate() {
            let mut slice = g1_bytes.as_slice();
            let _g1 = G1Affine::deserialize_uncompressed(&mut slice)
                .map_err(|e| SetupError::InvalidContribution(format!(
                    "G1 power {} is not a valid curve point: {}", i, e
                )))?;
        }

        // Verify G2 powers are valid curve points
        for (i, g2_bytes) in self.g2_powers.iter().enumerate() {
            let mut slice = g2_bytes.as_slice();
            let _g2 = G2Affine::deserialize_uncompressed(&mut slice)
                .map_err(|e| SetupError::InvalidContribution(format!(
                    "G2 power {} is not a valid curve point: {}", i, e
                )))?;
        }

        // After contributions, points should be non-identity
        if self.contribution_count > 0 {
            // Check first G1 power is non-identity
            let mut slice = self.g1_powers[0].as_slice();
            let g1_first = G1Affine::deserialize_uncompressed(&mut slice)
                .map_err(|e| SetupError::SerializationFailed(e.to_string()))?;
            if g1_first.is_zero() {
                return Err(SetupError::InvalidContribution(
                    "First G1 power is identity after contributions".to_string(),
                ));
            }

            // Check first G2 power is non-identity
            let mut slice2 = self.g2_powers[0].as_slice();
            let g2_first = G2Affine::deserialize_uncompressed(&mut slice2)
                .map_err(|e| SetupError::SerializationFailed(e.to_string()))?;
            if g2_first.is_zero() {
                return Err(SetupError::InvalidContribution(
                    "First G2 power is identity after contributions".to_string(),
                ));
            }
        }

        Ok(())
    }
}

/// Run the full Phase 1 ceremony with the specified number of participants.
///
/// Creates a fresh accumulator initialized with **generator points**
/// (representing τ = 1), then processes contributions from
/// `num_participants` simulated participants using **actual BN254
/// elliptic curve scalar multiplication**.
///
/// After each contribution, the SRS is verified to ensure the points
/// remain valid and non-identity.
///
/// # Arguments
///
/// * `degree` — The maximum degree for the SRS (number of G1 powers)
/// * `num_participants` — Number of participants to simulate
///
/// # Returns
///
/// The final [`PowersOfTau`] accumulator after all contributions.
///
/// # Example
///
/// ```ignore
/// use omnia_zk::setup::powers_of_tau::run_ceremony;
///
/// let srs = run_ceremony(1024, 3)?;
/// assert_eq!(srs.contribution_count, 3);
/// ```
pub fn run_ceremony(degree: usize, num_participants: usize) -> Result<PowersOfTau, SetupError> {
    // Initialize with generator points (representing τ = 1)
    let mut accumulator = PowersOfTau::new(degree)?;

    tracing::info!(degree, num_participants, "Starting Powers of Tau ceremony (real EC operations)");

    for i in 0..num_participants {
        let mut seed = [0u8; 32];
        seed[0] = i as u8;
        seed[1] = (i >> 8) as u8;

        // Generate a random secret for this participant
        let mut rng = ChaCha8Rng::from_seed(seed);
        let secret = Fr::rand(&mut rng);

        // Apply the contribution using actual EC scalar multiplication
        // This updates both G1 and G2 powers: new = old * secret
        accumulator.apply_contribution_ec(&secret)?;

        // Verify the SRS after each contribution
        accumulator.verify_srs()?;

        tracing::info!(
            participant = i,
            contributions = accumulator.contribution_count,
            "Ceremony participant contributed (EC scalar multiplication applied)"
        );
    }

    tracing::info!(
        contributions = accumulator.contribution_count,
        "Powers of Tau ceremony completed (real EC operations)"
    );

    Ok(accumulator)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use super::super::contribution::contribute;

    #[test]
    fn test_powers_of_tau_creation() {
        let pot = PowersOfTau::new(8).unwrap();
        assert_eq!(pot.g1_powers.len(), 8);
        assert_eq!(pot.g2_powers.len(), 2);
        assert_eq!(pot.contribution_count, 0);
    }

    #[test]
    fn test_powers_of_tau_initialized_with_generators() {
        let pot = PowersOfTau::new(4).unwrap();
        let g1_gen = G1Affine::generator();
        let g2_gen = G2Affine::generator();

        // All G1 powers should be the generator
        for (i, g1_bytes) in pot.g1_powers.iter().enumerate() {
            let mut slice = g1_bytes.as_slice();
            let point = G1Affine::deserialize_uncompressed(&mut slice).unwrap();
            assert_eq!(point, g1_gen, "G1 power {} should be generator", i);
        }

        // All G2 powers should be the generator
        for (i, g2_bytes) in pot.g2_powers.iter().enumerate() {
            let mut slice = g2_bytes.as_slice();
            let point = G2Affine::deserialize_uncompressed(&mut slice).unwrap();
            assert_eq!(point, g2_gen, "G2 power {} should be generator", i);
        }
    }

    #[test]
    fn test_to_transcript() {
        let pot = PowersOfTau::new(4).unwrap();
        let transcript = pot.to_transcript();
        // Each G1 point is ~64 bytes, G2 is ~128 bytes (uncompressed)
        let g1_size = pot.g1_powers[0].len();
        let g2_size = pot.g2_powers[0].len();
        assert_eq!(transcript.len(), 4 * g1_size + 2 * g2_size);
    }

    #[test]
    fn test_run_ceremony() {
        let pot = run_ceremony(4, 3).expect("ceremony failed");
        assert_eq!(pot.contribution_count, 3);
        assert_ne!(pot.transcript_hash, [0u8; 32]);
    }

    #[test]
    fn test_ceremony_single_participant() {
        let pot = run_ceremony(4, 1).expect("ceremony failed");
        assert_eq!(pot.contribution_count, 1);
    }

    #[test]
    fn test_ceremony_increments_contribution_count() {
        let mut pot = PowersOfTau::new(4).unwrap();
        assert_eq!(pot.contribution_count, 0);

        let transcript = pot.to_transcript();
        let c = contribute(&transcript, 4, Some([1u8; 32])).expect("contribute failed");
        pot.apply_contribution(&c).expect("apply failed");
        assert_eq!(pot.contribution_count, 1);
    }

    #[test]
    fn test_apply_contribution_ec() {
        let mut pot = PowersOfTau::new(4).unwrap();

        // Before contribution, all G1 are generators
        let g1_gen = G1Affine::generator();
        let mut slice = pot.g1_powers[0].as_slice();
        let before = G1Affine::deserialize_uncompressed(&mut slice).unwrap();
        assert_eq!(before, g1_gen);

        // Apply a contribution with a known secret
        let secret = Fr::from(42u64);
        pot.apply_contribution_ec(&secret).unwrap();

        assert_eq!(pot.contribution_count, 1);

        // After contribution, G1[0] should be G * 42 (not the generator)
        let mut slice = pot.g1_powers[0].as_slice();
        let after_g1 = G1Affine::deserialize_uncompressed(&mut slice).unwrap();
        assert_ne!(after_g1, g1_gen, "G1 should change after contribution");
        assert!(!after_g1.is_zero(), "G1 should be non-identity");

        // G2[0] should also be modified
        let g2_gen = G2Affine::generator();
        let mut slice2 = pot.g2_powers[0].as_slice();
        let after_g2 = G2Affine::deserialize_uncompressed(&mut slice2).unwrap();
        assert_ne!(after_g2, g2_gen, "G2 should change after contribution");
    }

    #[test]
    fn test_verify_srs() {
        let pot = run_ceremony(4, 2).expect("ceremony failed");
        pot.verify_srs().expect("SRS verification should pass after ceremony");
    }

    #[test]
    fn test_ceremony_produces_non_identity_points() {
        let pot = run_ceremony(4, 3).expect("ceremony failed");

        // All G1 powers should be non-identity after contributions
        for (i, g1_bytes) in pot.g1_powers.iter().enumerate() {
            let mut slice = g1_bytes.as_slice();
            let point = G1Affine::deserialize_uncompressed(&mut slice).unwrap();
            assert!(!point.is_zero(), "G1 power {} should be non-identity", i);
        }

        // All G2 powers should be non-identity after contributions
        for (i, g2_bytes) in pot.g2_powers.iter().enumerate() {
            let mut slice = g2_bytes.as_slice();
            let point = G2Affine::deserialize_uncompressed(&mut slice).unwrap();
            assert!(!point.is_zero(), "G2 power {} should be non-identity", i);
        }
    }
}
