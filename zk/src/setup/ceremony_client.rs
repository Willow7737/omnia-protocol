//! Multi-party trusted setup ceremony client.
//!
//! Provides a client for contributing to a ceremony server and
//! independently verifying ceremony transcripts.
//!
//! # Usage
//!
//! ```ignore
//! use omnia_zk::setup::ceremony_client::CeremonyClient;
//! use omnia_zk::setup::ceremony_server::CeremonyTranscript;
//!
//! // Generate a contribution from the current SRS state
//! let (contribution, receipt_proof) = CeremonyClient::generate_contribution(
//!     &srs_transcript,
//!     tau_size,
//!     None,  // random seed (None = system entropy)
//! )?;
//!
//! // Independently verify a full ceremony transcript
//! let transcript: CeremonyTranscript = /* ... */;
//! let valid = CeremonyClient::verify_transcript(&transcript, degree)?;
//! ```
//!
//! # Security
//!
//! The client generates contributions locally — the secret scalar never
//! leaves the client machine. Only the resulting `Contribution` (which
//! includes the updated G1 transcript and a Proof of Knowledge) is
//! shared with the ceremony server.

use thiserror::Error;

use super::contribution::{contribute, Contribution, ContributionProof};
use super::powers_of_tau::PowersOfTau;
use super::ceremony_server::CeremonyTranscript;
use super::SetupError;

/// Errors that can occur during ceremony client operations.
#[derive(Debug, Error)]
pub enum CeremonyClientError {
    /// Failed to connect to the ceremony server.
    #[error("connection failed: {0}")]
    ConnectionFailed(String),
    /// Contribution was rejected by the server.
    #[error("contribution rejected: {0}")]
    ContributionRejected(String),
    /// Transcript verification failed.
    #[error("transcript verification failed: {0}")]
    VerificationFailed(String),
    /// The ceremony is not in a valid state.
    #[error("invalid ceremony state: {0}")]
    InvalidState(String),
    /// Internal error.
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<SetupError> for CeremonyClientError {
    fn from(e: SetupError) -> Self {
        CeremonyClientError::Internal(e.to_string())
    }
}

/// Client for participating in a multi-party trusted setup ceremony.
///
/// This is a stateless helper that wraps the underlying `contribute()`
/// and `verify_contribution()` functions for ceremony-specific workflows.
pub struct CeremonyClient;

impl CeremonyClient {
    /// Generate a contribution for the ceremony.
    ///
    /// This creates a random secret scalar and applies it to the
    /// current SRS transcript, producing a [`Contribution`] with an
    /// embedded Proof of Knowledge. The secret scalar is destroyed
    /// after the contribution is generated.
    ///
    /// # Arguments
    ///
    /// * `previous_transcript` — The current SRS transcript bytes
    /// * `tau_size` — The number of G1 powers in the ceremony
    /// * `participant_seed` — Optional seed for deterministic contributions (testing only)
    ///
    /// # Returns
    ///
    /// A tuple of `(Contribution, ContributionProof)`. The `Contribution`
    /// contains the updated transcript and the `ContributionProof` is the
    /// Proof of Knowledge (also embedded in the `Contribution`).
    pub fn generate_contribution(
        previous_transcript: &[u8],
        tau_size: usize,
        participant_seed: Option<[u8; 32]>,
    ) -> Result<(Contribution, ContributionProof), CeremonyClientError> {
        let contribution = contribute(previous_transcript, tau_size, participant_seed)
            .map_err(|e| CeremonyClientError::ContributionRejected(e.to_string()))?;
        let proof = contribution.proof.clone();
        Ok((contribution, proof))
    }

    /// Verify an entire ceremony transcript independently.
    ///
    /// Replays all contributions from scratch by applying each one
    /// to a fresh SRS (which verifies the PoK internally), then
    /// checks the final transcript hash matches. This ensures that
    /// the ceremony was conducted honestly — a single honest verifier
    /// can detect any malicious contribution.
    ///
    /// # Arguments
    ///
    /// * `transcript` — The [`CeremonyTranscript`] to verify
    /// * `degree` — The degree of the Powers of Tau SRS (for initializing the initial state)
    ///
    /// # Returns
    ///
    /// `Ok(true)` if all contributions verify and the final transcript
    /// hash matches. `Err(CeremonyClientError)` on the first invalid
    /// contribution or a hash mismatch.
    pub fn verify_transcript(
        transcript: &CeremonyTranscript,
        degree: usize,
    ) -> Result<bool, CeremonyClientError> {
        // Initialize the SRS from scratch and replay all contributions.
        // apply_contribution() internally calls verify_contribution(),
        // so each contribution's PoK is verified as part of the replay.
        let mut replay_srs = PowersOfTau::new(degree)
            .map_err(|e| CeremonyClientError::InvalidState(e.to_string()))?;

        for (i, contribution) in transcript.contributions.iter().enumerate() {
            replay_srs
                .apply_contribution(contribution)
                .map_err(|e| {
                    CeremonyClientError::VerificationFailed(format!(
                        "Contribution {} failed verification: {}",
                        i, e
                    ))
                })?;
        }

        // Verify the final transcript hash matches
        if replay_srs.transcript_hash != transcript.final_transcript_hash {
            return Err(CeremonyClientError::VerificationFailed(
                "Final transcript hash mismatch".to_string(),
            ));
        }

        Ok(true)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::setup::ceremony_server::{CeremonyConfig, CeremonyServer};

    #[test]
    fn test_client_generate_contribution() {
        let srs = PowersOfTau::new(8).unwrap();
        let transcript = srs.to_transcript();
        let tau_size = srs.g1_powers.len();

        let (contribution, proof) =
            CeremonyClient::generate_contribution(&transcript, tau_size, Some([1u8; 32]))
                .expect("generate_contribution failed");

        assert!(!contribution.transcript.is_empty());
        assert!(!contribution.proof.commitment.is_empty());
        assert!(!proof.commitment.is_empty());
        assert_eq!(contribution.proof.commitment, proof.commitment);
    }

    #[test]
    fn test_client_verify_transcript() {
        let config = CeremonyConfig {
            min_participants: 2,
            max_participants: 10,
            ceremony_id: 42,
            degree: 8,
        };
        let server = CeremonyServer::new(config);
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
        let result = CeremonyClient::verify_transcript(&transcript, 8);
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_client_verify_transcript_tampered() {
        let config = CeremonyConfig {
            min_participants: 1,
            max_participants: 10,
            ceremony_id: 43,
            degree: 8,
        };
        let server = CeremonyServer::new(config);
        server.start().expect("start failed");

        let (transcript, tau_size) = server.get_srs_state().expect("get state failed");
        let contribution =
            contribute(&transcript, tau_size, Some([1u8; 32])).expect("contribute failed");
        server
            .accept_contribution(contribution)
            .expect("accept failed");

        let mut transcript = server.export_transcript().expect("export failed");

        // Tamper with the first contribution's transcript
        if !transcript.contributions[0].transcript.is_empty() {
            transcript.contributions[0].transcript[0] ^= 0xFF;
        }

        let result = CeremonyClient::verify_transcript(&transcript, 8);
        assert!(result.is_err());
    }

    #[test]
    fn test_client_verify_transcript_hash_mismatch() {
        let config = CeremonyConfig {
            min_participants: 1,
            max_participants: 10,
            ceremony_id: 44,
            degree: 8,
        };
        let server = CeremonyServer::new(config);
        server.start().expect("start failed");

        let (transcript, tau_size) = server.get_srs_state().expect("get state failed");
        let contribution =
            contribute(&transcript, tau_size, Some([1u8; 32])).expect("contribute failed");
        server
            .accept_contribution(contribution)
            .expect("accept failed");

        let mut transcript = server.export_transcript().expect("export failed");

        // Tamper with the final transcript hash
        transcript.final_transcript_hash[0] ^= 0xFF;

        let result = CeremonyClient::verify_transcript(&transcript, 8);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("hash mismatch"));
    }

    #[test]
    fn test_client_error_from_setup_error() {
        let setup_err = SetupError::InvalidContribution("test".to_string());
        let client_err: CeremonyClientError = setup_err.into();
        assert!(matches!(client_err, CeremonyClientError::Internal(_)));
    }
}
