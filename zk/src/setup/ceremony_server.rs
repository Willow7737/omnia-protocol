//! Multi-party trusted setup ceremony server.
//!
//! Coordinates a production ceremony where multiple participants contribute
//! sequentially to the Powers of Tau SRS. Each participant:
//! 1. Fetches the current ceremony state
//! 2. Generates a random secret scalar
//! 3. Applies their contribution (EC scalar multiplication on G1 via `contribute()`)
//! 4. Submits the contribution with a built-in Proof of Knowledge
//!
//! The server verifies each contribution before applying it and maintains
//! a full transcript for independent verification.
//!
//! # Flow
//!
//! ```text
//! Client                                    Server
//!   |                                         |
//!   |--- GET /ceremony/state ---------------->|
//!   |<-- { srs_transcript, tau_size } --------|
//!   |                                         |
//!   |--- contribute(transcript, tau_size) ----|  (local)
//!   |                                         |
//!   |--- POST /ceremony/contribute ---------->|
//!   |    { Contribution (includes PoK) }      |
//!   |                                         |
//!   |     verify_contribution() + apply()     |  (server-side)
//!   |                                         |
//!   |<-- { receipt, contribution_index } -----|
//!   |                                         |
//!   |--- POST /ceremony/finalize ----------->|  (after min_participants)
//!   |<-- { CircuitKeyPair } ------------------|
//! ```
//!
//! # Security Considerations
//!
//! - Each contribution includes a Fiat-Shamir Proof of Knowledge (PoK)
//!   proving the contributor knows the secret scalar used
//! - The server verifies each PoK before applying the contribution
//! - The full transcript is maintained for independent third-party verification
//! - BLAKE3 domain separation is used for all hash operations
//! - Constant-time comparisons are used where applicable
//!
//! # References
//!
//! - Bowe, S., Gabizon, A., Green, M. *A Multi-Party Protocol for
//!   Constructing the Public Parameters of the Pinocchio zk-SNARK System*
//!   (Zcash, 2018). <https://eprint.iacr.org/2017/601>

use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::circuit_setup::{derive_keys_from_srs, CircuitKeyPair};
use super::contribution::{verify_contribution, Contribution, ContributionProof};
use super::powers_of_tau::PowersOfTau;
use super::SetupError;
use crate::circuit::RollupCircuit;

/// Errors that can occur during ceremony operations.
#[derive(Debug, Error)]
pub enum CeremonyError {
    /// The ceremony has not started yet.
    #[error("ceremony has not started")]
    NotStarted,
    /// The ceremony has already been finalized.
    #[error("ceremony already finalized")]
    AlreadyFinalized,
    /// Insufficient participants for finalization.
    #[error("insufficient participants: {current}/{required}")]
    InsufficientParticipants {
        /// Current number of contributions.
        current: usize,
        /// Required minimum participants.
        required: usize,
    },
    /// Contribution verification failed.
    #[error("contribution verification failed: {0}")]
    VerificationFailed(String),
    /// Key derivation failed.
    #[error("key derivation failed: {0}")]
    KeyDerivationFailed(String),
    /// The ceremony is not in a state to accept contributions.
    #[error("ceremony not accepting contributions")]
    NotAcceptingContributions,
    /// Internal error.
    #[error("internal error: {0}")]
    Internal(String),
    /// Invalid degree parameter.
    #[error("invalid degree: {0}")]
    InvalidDegree(String),
}

impl From<SetupError> for CeremonyError {
    fn from(e: SetupError) -> Self {
        CeremonyError::Internal(e.to_string())
    }
}

/// Configuration for a ceremony.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CeremonyConfig {
    /// Minimum number of participants required.
    pub min_participants: usize,
    /// Maximum number of participants allowed.
    pub max_participants: usize,
    /// Ceremony identifier.
    pub ceremony_id: u64,
    /// Degree for the Powers of Tau SRS.
    pub degree: usize,
}

impl Default for CeremonyConfig {
    fn default() -> Self {
        Self {
            min_participants: 3,
            max_participants: 100,
            ceremony_id: 1,
            degree: 65536,
        }
    }
}

/// Current state of the ceremony.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CeremonyPhase {
    /// Ceremony has not started yet.
    NotStarted,
    /// Accepting contributions from participants.
    AcceptingContributions,
    /// Ceremony is finalized, keys derived.
    Finalized,
}

/// Receipt given to a participant after their contribution is accepted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributionReceipt {
    /// The participant's contribution index (0-based).
    pub contribution_index: usize,
    /// Hash of the SRS after this contribution.
    pub transcript_hash: [u8; 32],
    /// The proof of knowledge for this contribution.
    pub proof: ContributionProof,
}

/// Full transcript of the ceremony for independent verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CeremonyTranscript {
    /// Ceremony configuration.
    pub config: CeremonyConfig,
    /// All contributions in order.
    pub contributions: Vec<Contribution>,
    /// Final SRS transcript hash.
    pub final_transcript_hash: [u8; 32],
    /// Total number of contributions.
    pub contribution_count: usize,
}

/// Multi-party ceremony server.
///
/// Coordinates a production trusted setup where multiple participants
/// contribute sequentially. Each contribution is verified before being
/// applied to the SRS. The full transcript is maintained for independent
/// verification after the ceremony is finalized.
pub struct CeremonyServer {
    /// Current SRS state.
    srs: Arc<RwLock<Option<PowersOfTau>>>,
    /// All accepted contributions.
    contributions: Arc<RwLock<Vec<Contribution>>>,
    /// Ceremony configuration.
    config: CeremonyConfig,
    /// Current ceremony phase.
    phase: Arc<RwLock<CeremonyPhase>>,
}

impl CeremonyServer {
    /// Create a new ceremony server with the given configuration.
    ///
    /// The server starts in the `NotStarted` phase. Call [`Self::start`]
    /// to initialize the SRS and begin accepting contributions.
    pub fn new(config: CeremonyConfig) -> Self {
        Self {
            srs: Arc::new(RwLock::new(None)),
            contributions: Arc::new(RwLock::new(Vec::new())),
            config,
            phase: Arc::new(RwLock::new(CeremonyPhase::NotStarted)),
        }
    }

    /// Start the ceremony — initializes the SRS with generator points.
    ///
    /// Creates a new [`PowersOfTau`] with the configured degree and
    /// transitions the ceremony to the `AcceptingContributions` phase.
    pub fn start(&self) -> Result<(), CeremonyError> {
        {
            let mut phase = self
                .phase
                .write()
                .map_err(|e| CeremonyError::Internal(e.to_string()))?;
            if !matches!(*phase, CeremonyPhase::NotStarted) {
                return Err(CeremonyError::AlreadyFinalized);
            }
            *phase = CeremonyPhase::AcceptingContributions;
        }

        // Initialize the SRS with generator points
        let srs = PowersOfTau::new(self.config.degree)
            .map_err(|e| CeremonyError::InvalidDegree(e.to_string()))?;

        {
            let mut srs_guard = self
                .srs
                .write()
                .map_err(|e| CeremonyError::Internal(e.to_string()))?;
            *srs_guard = Some(srs);
        }

        tracing::info!(
            ceremony_id = self.config.ceremony_id,
            min_participants = self.config.min_participants,
            max_participants = self.config.max_participants,
            degree = self.config.degree,
            "Ceremony started"
        );
        Ok(())
    }

    /// Get the current ceremony phase.
    pub fn phase(&self) -> CeremonyPhase {
        self.phase
            .read()
            .map(|p| p.clone())
            .unwrap_or(CeremonyPhase::NotStarted)
    }

    /// Get the current number of contributions.
    pub fn contribution_count(&self) -> usize {
        self.contributions.read().map(|c| c.len()).unwrap_or(0)
    }

    /// Get the current SRS transcript and tau_size for clients to generate contributions.
    ///
    /// Returns the current SRS transcript bytes and the number of G1 powers.
    /// Clients use this to call [`contribute`] locally.
    pub fn get_srs_state(&self) -> Result<(Vec<u8>, usize), CeremonyError> {
        let srs_guard = self
            .srs
            .read()
            .map_err(|e| CeremonyError::Internal(e.to_string()))?;
        let srs = srs_guard.as_ref().ok_or(CeremonyError::NotStarted)?;
        Ok((srs.to_transcript(), srs.g1_powers.len()))
    }

    /// Accept a contribution from a participant.
    ///
    /// The contribution is verified before being applied. If verification
    /// fails, the contribution is rejected and the SRS is not modified.
    ///
    /// The `Contribution` struct includes the `ContributionProof` (PoK)
    /// internally, so they are submitted together.
    pub fn accept_contribution(
        &self,
        contribution: Contribution,
    ) -> Result<ContributionReceipt, CeremonyError> {
        // Check ceremony is accepting
        {
            let phase = self
                .phase
                .read()
                .map_err(|e| CeremonyError::Internal(e.to_string()))?;
            if !matches!(*phase, CeremonyPhase::AcceptingContributions) {
                return Err(CeremonyError::NotAcceptingContributions);
            }
        }

        // Check max participants
        {
            let contributions = self
                .contributions
                .read()
                .map_err(|e| CeremonyError::Internal(e.to_string()))?;
            if contributions.len() >= self.config.max_participants {
                return Err(CeremonyError::AlreadyFinalized);
            }
        }

        // Get the current SRS transcript and tau_size for verification
        let (previous_transcript, tau_size) = {
            let srs_guard = self
                .srs
                .read()
                .map_err(|e| CeremonyError::Internal(e.to_string()))?;
            let srs = srs_guard.as_ref().ok_or(CeremonyError::NotStarted)?;
            (srs.to_transcript(), srs.g1_powers.len())
        };

        // Verify the contribution's Proof of Knowledge and consistency
        verify_contribution(&contribution, &previous_transcript, tau_size)
            .map_err(|e| CeremonyError::VerificationFailed(e.to_string()))?;

        // Apply the contribution to the SRS
        {
            let mut srs_guard = self
                .srs
                .write()
                .map_err(|e| CeremonyError::Internal(e.to_string()))?;
            let srs = srs_guard.as_mut().ok_or(CeremonyError::NotStarted)?;
            srs.apply_contribution(&contribution)
                .map_err(|e| CeremonyError::VerificationFailed(e.to_string()))?;
        }

        // Store the contribution
        let index = {
            let mut contributions = self
                .contributions
                .write()
                .map_err(|e| CeremonyError::Internal(e.to_string()))?;
            let index = contributions.len();
            contributions.push(contribution.clone());
            index
        };

        // Get transcript hash
        let transcript_hash = {
            let srs_guard = self
                .srs
                .read()
                .map_err(|e| CeremonyError::Internal(e.to_string()))?;
            let srs = srs_guard.as_ref().ok_or(CeremonyError::NotStarted)?;
            srs.transcript_hash
        };

        tracing::info!(
            contribution_index = index,
            ceremony_id = self.config.ceremony_id,
            "Contribution accepted and verified"
        );

        Ok(ContributionReceipt {
            contribution_index: index,
            transcript_hash,
            proof: contribution.proof.clone(),
        })
    }

    /// Finalize the ceremony — verify the SRS and derive circuit-specific keys.
    ///
    /// Requires at least `min_participants` contributions. Derives keys
    /// for the given circuit using [`derive_keys_from_srs`].
    pub fn finalize(&self, circuit: &RollupCircuit) -> Result<CircuitKeyPair, CeremonyError> {
        let contribution_count = self.contribution_count();
        if contribution_count < self.config.min_participants {
            return Err(CeremonyError::InsufficientParticipants {
                current: contribution_count,
                required: self.config.min_participants,
            });
        }

        // Derive keys from the final SRS
        let key_pair = {
            let srs_guard = self
                .srs
                .read()
                .map_err(|e| CeremonyError::Internal(e.to_string()))?;
            let srs = srs_guard.as_ref().ok_or(CeremonyError::NotStarted)?;
            derive_keys_from_srs(srs, circuit)
                .map_err(|e| CeremonyError::KeyDerivationFailed(e.to_string()))?
        };

        // Update phase
        {
            let mut phase = self
                .phase
                .write()
                .map_err(|e| CeremonyError::Internal(e.to_string()))?;
            *phase = CeremonyPhase::Finalized;
        }

        tracing::info!(
            ceremony_id = self.config.ceremony_id,
            contributions = contribution_count,
            "Ceremony finalized"
        );

        Ok(key_pair)
    }

    /// Export the full transcript for independent verification.
    ///
    /// The transcript contains all contributions in order along with
    /// the ceremony configuration and final SRS hash. An independent
    /// verifier can replay the ceremony and verify each contribution.
    pub fn export_transcript(&self) -> Result<CeremonyTranscript, CeremonyError> {
        let contributions = self
            .contributions
            .read()
            .map(|c| c.clone())
            .map_err(|e| CeremonyError::Internal(e.to_string()))?;

        let srs_guard = self
            .srs
            .read()
            .map_err(|e| CeremonyError::Internal(e.to_string()))?;
        let srs = srs_guard.as_ref().ok_or(CeremonyError::NotStarted)?;

        Ok(CeremonyTranscript {
            config: self.config.clone(),
            final_transcript_hash: srs.transcript_hash,
            contribution_count: contributions.len(),
            contributions,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::circuit::RollupCircuit;
    use crate::setup::contribute;

    /// Helper: create a test ceremony server with small degree.
    fn test_server(min_participants: usize, max_participants: usize) -> CeremonyServer {
        let config = CeremonyConfig {
            min_participants,
            max_participants,
            ceremony_id: 1,
            degree: 8, // Small degree for fast tests
        };
        CeremonyServer::new(config)
    }

    #[test]
    fn test_ceremony_server_lifecycle() {
        let server = test_server(3, 10);

        // Start the ceremony
        server.start().expect("start failed");
        assert_eq!(server.phase(), CeremonyPhase::AcceptingContributions);

        // Make 3 contributions
        for i in 0u8..3 {
            let (transcript, tau_size) = server.get_srs_state().expect("get state failed");
            let mut seed = [0u8; 32];
            seed[0] = i;
            let contribution =
                contribute(&transcript, tau_size, Some(seed)).expect("contribute failed");
            let receipt = server
                .accept_contribution(contribution)
                .expect("accept failed");
            assert_eq!(receipt.contribution_index, i as usize);
        }

        assert_eq!(server.contribution_count(), 3);

        // Finalize
        let circuit = RollupCircuit::empty();
        let key_pair = server.finalize(&circuit).expect("finalize failed");
        assert!(!key_pair.proving_key.is_empty());
        assert!(!key_pair.verifying_key.is_empty());
        assert_eq!(key_pair.tau_contributions, 3);

        assert_eq!(server.phase(), CeremonyPhase::Finalized);

        // Export transcript
        let transcript = server.export_transcript().expect("export failed");
        assert_eq!(transcript.contribution_count, 3);
        assert_eq!(transcript.contributions.len(), 3);
    }

    #[test]
    fn test_ceremony_rejects_before_start() {
        let server = test_server(1, 10);

        // Trying to contribute before start should fail
        let (transcript, tau_size) = {
            // We can't get_srs_state before start, so create a fake contribution
            let srs = PowersOfTau::new(8).unwrap();
            (srs.to_transcript(), srs.g1_powers.len())
        };
        let contribution =
            contribute(&transcript, tau_size, Some([1u8; 32])).expect("contribute failed");
        let result = server.accept_contribution(contribution);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CeremonyError::NotAcceptingContributions
        ));
    }

    #[test]
    fn test_ceremony_insufficient_participants() {
        let server = test_server(3, 10);
        server.start().expect("start failed");

        // Only 2 contributions
        for i in 0u8..2 {
            let (transcript, tau_size) = server.get_srs_state().expect("get state failed");
            let mut seed = [0u8; 32];
            seed[0] = i;
            let contribution =
                contribute(&transcript, tau_size, Some(seed)).expect("contribute failed");
            server
                .accept_contribution(contribution)
                .expect("accept failed");
        }

        // Finalize with only 2 contributors (need 3)
        let circuit = RollupCircuit::empty();
        let result = server.finalize(&circuit);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CeremonyError::InsufficientParticipants {
                current: 2,
                required: 3
            }
        ));
    }

    #[test]
    fn test_ceremony_max_participants() {
        let server = test_server(1, 2);
        server.start().expect("start failed");

        // Make 2 contributions (the max)
        for i in 0u8..2 {
            let (transcript, tau_size) = server.get_srs_state().expect("get state failed");
            let mut seed = [0u8; 32];
            seed[0] = i;
            let contribution =
                contribute(&transcript, tau_size, Some(seed)).expect("contribute failed");
            server
                .accept_contribution(contribution)
                .expect("accept failed");
        }

        // 3rd contribution should be rejected (max reached)
        let (transcript, tau_size) = server.get_srs_state().expect("get state failed");
        let contribution =
            contribute(&transcript, tau_size, Some([2u8; 32])).expect("contribute failed");
        let result = server.accept_contribution(contribution);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CeremonyError::AlreadyFinalized
        ));
    }

    #[test]
    fn test_ceremony_double_start() {
        let server = test_server(1, 10);
        server.start().expect("first start failed");
        let result = server.start();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CeremonyError::AlreadyFinalized
        ));
    }

    #[test]
    fn test_ceremony_get_srs_state_before_start() {
        let server = test_server(1, 10);
        let result = server.get_srs_state();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CeremonyError::NotStarted));
    }

    #[test]
    fn test_ceremony_export_transcript_before_start() {
        let server = test_server(1, 10);
        let result = server.export_transcript();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), CeremonyError::NotStarted));
    }

    #[test]
    fn test_ceremony_receipt_contains_valid_proof() {
        let server = test_server(1, 10);
        server.start().expect("start failed");

        let (transcript, tau_size) = server.get_srs_state().expect("get state failed");
        let contribution =
            contribute(&transcript, tau_size, Some([42u8; 32])).expect("contribute failed");
        let receipt = server
            .accept_contribution(contribution)
            .expect("accept failed");

        assert_eq!(receipt.contribution_index, 0);
        assert_ne!(receipt.transcript_hash, [0u8; 32]);
        assert!(!receipt.proof.commitment.is_empty());
        assert!(!receipt.proof.challenge.is_empty());
        assert!(!receipt.proof.response.is_empty());
        assert!(!receipt.proof.public_key.is_empty());
    }

    #[test]
    fn test_ceremony_transcript_can_be_independently_verified() {
        let server = test_server(2, 10);
        server.start().expect("start failed");

        // Make contributions
        for i in 0u8..2 {
            let (transcript, tau_size) = server.get_srs_state().expect("get state failed");
            let mut seed = [0u8; 32];
            seed[0] = i;
            let contribution =
                contribute(&transcript, tau_size, Some(seed)).expect("contribute failed");
            server
                .accept_contribution(contribution)
                .expect("accept failed");
        }

        let transcript = server.export_transcript().expect("export failed");

        // Verify each contribution independently by replaying through the SRS.
        // We use apply_contribution() which internally calls verify_contribution(),
        // rather than manually tracking transcripts, because the contribution
        // transcript only contains G1 elements but the SRS contains G1+G2.
        let mut replay_srs = PowersOfTau::new(8).unwrap();
        for (i, contribution) in transcript.contributions.iter().enumerate() {
            replay_srs
                .apply_contribution(contribution)
                .unwrap_or_else(|e| panic!("Contribution {} failed verification: {}", i, e));
        }

        // Verify the final transcript hash matches
        assert_eq!(replay_srs.transcript_hash, transcript.final_transcript_hash);
    }

    #[test]
    fn test_ceremony_config_default() {
        let config = CeremonyConfig::default();
        assert_eq!(config.min_participants, 3);
        assert_eq!(config.max_participants, 100);
        assert_eq!(config.ceremony_id, 1);
        assert_eq!(config.degree, 65536);
    }

    #[test]
    fn test_ceremony_error_from_setup_error() {
        let setup_err = SetupError::InvalidContribution("bad".to_string());
        let ceremony_err: CeremonyError = setup_err.into();
        assert!(matches!(ceremony_err, CeremonyError::Internal(_)));
    }
}
