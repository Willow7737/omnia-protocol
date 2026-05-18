//! # Trusted Setup Ceremony Orchestration
//!
//! This module implements the two-phase trusted setup ceremony for the
//! Omnia ZK-rollup's Groth16 proof system:
//!
//! 1. **Phase 1 — Powers of Tau**: A circuit-independent multi-party
//!    ceremony that produces a structured reference string (SRS) over
//!    the BN254 curve. Each participant contributes randomness, and
//!    the ceremony is secure as long as at least one participant is
//!    honest and destroys their secret.
//!
//! 2. **Phase 2 — Circuit-Specific Key Derivation**: Derives proving
//!    and verifying keys from the Phase 1 SRS for a specific circuit.
//!    This phase can also include a multi-party ceremony for additional
//!    security.
//!
//! # Architecture
//!
//! ```text
//! Phase 1: Powers of Tau Ceremony
//! ┌─────────────────────────────────────────┐
//! │  Participant 1 ──→ contribute(tau)       │
//! │  Participant 2 ──→ contribute(tau)       │
//! │  ...                                    │
//! │  Participant N ──→ contribute(tau)       │
//! │            ↓                            │
//! │  SRS = { [τ⁰]₁, [τ¹]₁, ..., [τⁿ⁻¹]₁, │
//! │          [τ⁰]₂, [τ¹]₂ }               │
//! └─────────────────────────────────────────┘
//!                     ↓
//! Phase 2: Circuit Key Derivation
//! ┌─────────────────────────────────────────┐
//! │  SRS + Circuit ──→ derive_keys_from_srs()│
//! │            ↓                            │
//! │  (ProvingKey, VerifyingKey)             │
//! └─────────────────────────────────────────┘
//! ```
//!
//! # Real EC Operations (C-2)
//!
//! The ceremony now uses actual BN254 elliptic curve scalar multiplication
//! for all SRS updates. The initial SRS is seeded with generator points
//! (representing τ = 1), and each contribution multiplies every G1 and G2
//! power by the contributor's secret scalar.
//!
//! # Security Considerations
//!
//! The trusted setup is the most security-critical part of any SNARK
//! system. If all participants collude (or are compromised), they can
//! create fake proofs. The multi-party ceremony mitigates this by
//! requiring ALL participants to be dishonest — a single honest
//! participant is sufficient for security.
//!
//! # References
//!
//! - Groth, J. *On the Size of Pairing-based Non-interactive Arguments*
//!   (EUROCRYPT 2016). <https://eprint.iacr.org/2016/260>
//! - Bowe, S., Gabizon, A., Green, M. *A Multi-Party Protocol for
//!   Constructing the Public Parameters of the Pinocchio zk-SNARK System*
//!   (Zcash, 2018). <https://eprint.iacr.org/2017/601>
//! - Ben-Sasson, E., et al. *Scalable, transparent, and post-quantum
//!   secure computational integrity* (IACR ePrint 2018/046)

pub mod circuit_setup;
pub mod contribution;
pub mod powers_of_tau;

use thiserror::Error;

// Re-export key types for convenience
pub use circuit_setup::{
    derive_keys, derive_keys_expanded, derive_keys_from_srs, verify_key_consistency, CircuitKeyPair,
};
pub use contribution::{
    contribute, initial_transcript_with_generators, verify_ceremony_transcript,
    verify_contribution, Contribution, ContributionProof,
};
pub use powers_of_tau::{run_ceremony, PowersOfTau, DEFAULT_TAU_DEGREE};

/// Errors that can occur during the trusted setup ceremony.
#[derive(Error, Debug)]
pub enum SetupError {
    /// A contribution failed verification.
    #[error("contribution verification failed: {0}")]
    InvalidContribution(String),
    /// Key derivation from the SRS failed.
    #[error("key derivation failed: {0}")]
    KeyDerivationFailed(String),
    /// Serialization or deserialization of SRS/keys failed.
    #[error("serialization failed: {0}")]
    SerializationFailed(String),
    /// The ceremony has insufficient participants.
    #[error("insufficient participants: need at least {0}, got {1}")]
    InsufficientParticipants(usize, usize),
    /// The Phase 1 SRS is not ready for Phase 2 key derivation.
    #[error("SRS not ready: {0}")]
    SrsNotReady(String),
}

/// Orchestrator for the full two-phase trusted setup ceremony.
///
/// Manages the lifecycle from Phase 1 (Powers of Tau) through Phase 2
/// (circuit-specific key derivation).
#[derive(Debug)]
pub struct SetupCeremony {
    /// The Phase 1 Powers of Tau accumulator.
    pub srs: PowersOfTau,
    /// Minimum number of participants required.
    pub min_participants: usize,
    /// Whether the ceremony is complete.
    pub completed: bool,
}

impl SetupCeremony {
    /// Create a new ceremony orchestrator.
    ///
    /// # Arguments
    ///
    /// * `degree` — The maximum degree for the Powers of Tau SRS
    /// * `min_participants` — Minimum number of Phase 1 participants required
    ///
    /// # Returns
    ///
    /// A new [`SetupCeremony`] ready to accept contributions.
    pub fn new(degree: usize, min_participants: usize) -> Self {
        Self {
            srs: PowersOfTau::new(degree)
                .expect("PowersOfTau initialization should not fail with valid degree"),
            min_participants,
            completed: false,
        }
    }

    /// Accept a contribution from a ceremony participant.
    ///
    /// Uses the `contribute()` + `apply_contribution()` flow, which updates
    /// only G1 elements from the contribution transcript. For a ceremony
    /// that updates both G1 and G2, use [`run_ceremony`] instead.
    ///
    /// # Arguments
    ///
    /// * `seed` — Optional deterministic seed for the contribution (testing only)
    ///
    /// # Returns
    ///
    /// `Ok(())` if the contribution was accepted, `Err(SetupError)` otherwise.
    pub fn accept_contribution(&mut self, seed: Option<[u8; 32]>) -> Result<(), SetupError> {
        let transcript = self.srs.to_transcript();
        // tau_size is the number of G1 elements only (not G1 + G2)
        let tau_size = self.srs.g1_powers.len();
        let c = contribute(&transcript, tau_size, seed)?;
        self.srs.apply_contribution(&c)?;
        tracing::info!(
            total_contributions = self.srs.contribution_count,
            "Ceremony accepted contribution"
        );
        Ok(())
    }

    /// Complete Phase 1 and derive Phase 2 keys for the basic circuit.
    ///
    /// Uses [`derive_keys_from_srs`] which verifies the SRS has contributions
    /// and is well-formed before deriving keys.
    ///
    /// # Arguments
    ///
    /// * `circuit` — The [`RollupCircuit`](crate::circuit::RollupCircuit) to derive keys for
    ///
    /// # Returns
    ///
    /// A [`CircuitKeyPair`] if the ceremony has enough participants,
    /// or [`SetupError::InsufficientParticipants`] otherwise.
    pub fn finalize_basic(
        &mut self,
        circuit: &crate::circuit::RollupCircuit,
    ) -> Result<CircuitKeyPair, SetupError> {
        if self.srs.contribution_count < self.min_participants {
            return Err(SetupError::InsufficientParticipants(
                self.min_participants,
                self.srs.contribution_count,
            ));
        }

        let keypair = derive_keys_from_srs(&self.srs, circuit)?;
        self.completed = true;
        tracing::info!("Trusted setup ceremony finalized (basic circuit)");
        Ok(keypair)
    }

    /// Complete Phase 1 and derive Phase 2 keys for the expanded circuit.
    ///
    /// # Arguments
    ///
    /// * `num_events` — Number of events per batch
    /// * `merkle_depth` — Depth of each Merkle inclusion proof
    ///
    /// # Returns
    ///
    /// A [`CircuitKeyPair`] if the ceremony has enough participants,
    /// or [`SetupError::InsufficientParticipants`] otherwise.
    pub fn finalize_expanded(
        &mut self,
        num_events: usize,
        merkle_depth: usize,
    ) -> Result<CircuitKeyPair, SetupError> {
        if self.srs.contribution_count < self.min_participants {
            return Err(SetupError::InsufficientParticipants(
                self.min_participants,
                self.srs.contribution_count,
            ));
        }

        let keypair = derive_keys_expanded(&self.srs, num_events, merkle_depth)?;
        self.completed = true;
        tracing::info!("Trusted setup ceremony finalized (expanded circuit)");
        Ok(keypair)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::circuit::RollupCircuit;

    #[test]
    fn test_ceremony_orchestration() {
        let mut ceremony = SetupCeremony::new(8, 2);
        assert!(!ceremony.completed);

        // First contribution
        ceremony
            .accept_contribution(Some([1u8; 32]))
            .expect("contribution 1 failed");

        // Second contribution
        ceremony
            .accept_contribution(Some([2u8; 32]))
            .expect("contribution 2 failed");

        // Now we can finalize
        let circuit = RollupCircuit::empty();
        let keypair = ceremony.finalize_basic(&circuit).expect("finalize failed");
        assert!(ceremony.completed);
        assert!(!keypair.proving_key.is_empty());
        assert!(!keypair.verifying_key.is_empty());
    }

    #[test]
    fn test_ceremony_insufficient_participants() {
        let mut ceremony = SetupCeremony::new(8, 3);
        ceremony
            .accept_contribution(Some([1u8; 32]))
            .expect("contribution failed");

        let circuit = RollupCircuit::empty();
        let result = ceremony.finalize_basic(&circuit);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            SetupError::InsufficientParticipants(3, 1)
        ));
    }

    #[test]
    fn test_ceremony_expanded_circuit() {
        let mut ceremony = SetupCeremony::new(8, 1);
        ceremony
            .accept_contribution(Some([42u8; 32]))
            .expect("contribution failed");

        let keypair = ceremony
            .finalize_expanded(2, 4)
            .expect("finalize expanded failed");
        assert!(ceremony.completed);
        assert!(!keypair.proving_key.is_empty());
    }

    #[test]
    fn test_setup_error_display() {
        let err = SetupError::InvalidContribution("bad proof".to_string());
        assert!(err.to_string().contains("bad proof"));

        let err = SetupError::InsufficientParticipants(5, 2);
        assert!(err.to_string().contains("5"));
    }
}
