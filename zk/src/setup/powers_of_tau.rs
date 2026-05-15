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

use ark_bn254::{G1Affine, G2Affine};
use ark_ec::AffineRepr;
use ark_serialize::CanonicalSerialize;
use serde::{Deserialize, Serialize};

use super::contribution::{contribute, verify_contribution, Contribution};
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
    /// Initializes all G1 elements to the identity point and G2 elements
    /// to the generator, representing the "empty" SRS before any
    /// contributions have been applied.
    ///
    /// # Arguments
    ///
    /// * `degree` — The maximum degree supported (number of G1 powers minus one)
    ///
    /// # Returns
    ///
    /// A new [`PowersOfTau`] with identity elements.
    pub fn new(degree: usize) -> Self {
        // Serialize the identity G1 point
        let mut g1_bytes = Vec::new();
        G1Affine::identity()
            .serialize_uncompressed(&mut g1_bytes)
            .expect("G1 identity serialization should not fail");

        // Serialize the generator G2 point
        let mut g2_bytes = Vec::new();
        G2Affine::generator()
            .serialize_uncompressed(&mut g2_bytes)
            .expect("G2 generator serialization should not fail");

        let g1_powers = vec![g1_bytes; degree];
        let g2_powers = vec![g2_bytes; 2];

        Self {
            g1_powers,
            g2_powers,
            contribution_count: 0,
            transcript_hash: [0u8; 32],
        }
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

    /// Apply a contribution to the accumulator.
    ///
    /// Verifies the contribution against the current transcript and
    /// updates the accumulator state if verification succeeds.
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
        let tau_size = self.g1_powers.len() + self.g2_powers.len();

        verify_contribution(contribution, &previous_transcript, tau_size)?;

        // Update the accumulator with the new transcript
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
            "Applied contribution to Powers of Tau"
        );

        Ok(())
    }
}

/// Run the full Phase 1 ceremony with the specified number of participants.
///
/// Creates a fresh accumulator and processes contributions from
/// `num_participants` simulated participants, each using deterministic
/// seeds derived from their index.
///
/// # Arguments
///
/// * `degree` — The maximum degree for the SRS
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
    let mut accumulator = PowersOfTau::new(degree);

    tracing::info!(degree, num_participants, "Starting Powers of Tau ceremony");

    for i in 0..num_participants {
        let mut seed = [0u8; 32];
        seed[0] = i as u8;
        seed[1] = (i >> 8) as u8;

        let transcript = accumulator.to_transcript();
        let contribution = contribute(&transcript, degree + 2, Some(seed))?;
        accumulator.apply_contribution(&contribution)?;
    }

    tracing::info!(
        contributions = accumulator.contribution_count,
        "Powers of Tau ceremony completed"
    );

    Ok(accumulator)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_powers_of_tau_creation() {
        let pot = PowersOfTau::new(8);
        assert_eq!(pot.g1_powers.len(), 8);
        assert_eq!(pot.g2_powers.len(), 2);
        assert_eq!(pot.contribution_count, 0);
    }

    #[test]
    fn test_to_transcript() {
        let pot = PowersOfTau::new(4);
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
        let mut pot = PowersOfTau::new(4);
        assert_eq!(pot.contribution_count, 0);

        let transcript = pot.to_transcript();
        let c = contribute(&transcript, 6, Some([1u8; 32])).expect("contribute failed");
        pot.apply_contribution(&c).expect("apply failed");
        assert_eq!(pot.contribution_count, 1);
    }
}
