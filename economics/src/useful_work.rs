//! Proof-of-Useful-Work verification
//!
//! Excess compute capacity can be contributed to useful work in exchange
//! for additional UBC rewards. The three supported work types are:
//!
//! - **AI Training**: Verifiable model training (future: ZK proofs)
//! - **Scientific Simulation**: Distributed computation
//! - **Distributed Storage**: Data hosting with duration commitments
//!
//! Each work submission must include a proof that is verified by a
//! designated verifier node. In the current scaffold, verification is
//! a stub — real ZK proof or VDF verification will be added later.

use serde::{Deserialize, Serialize};

use crate::error::EconomicsError;

/// The type of useful work that was performed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UsefulWorkType {
    /// AI model training with verifiable computation.
    AiTraining {
        /// Hash of the model architecture.
        model_hash: [u8; 32],
        /// Hash of the training dataset.
        training_data_hash: [u8; 32],
    },
    /// Scientific simulation with parameterized computation.
    ScientificSimulation {
        /// Identifier for the simulation.
        simulation_id: String,
        /// Hash of the simulation parameters.
        params_hash: [u8; 32],
    },
    /// Distributed storage with a duration commitment.
    DistributedStorage {
        /// Hash of the stored data.
        data_hash: [u8; 32],
        /// Duration of the storage commitment in milliseconds.
        storage_duration: u64,
    },
}

impl UsefulWorkType {
    /// Create an AI training work type.
    pub fn ai_training(model_hash: [u8; 32], training_data_hash: [u8; 32]) -> Self {
        Self::AiTraining {
            model_hash,
            training_data_hash,
        }
    }

    /// Create a scientific simulation work type.
    pub fn scientific_simulation(simulation_id: String, params_hash: [u8; 32]) -> Self {
        Self::ScientificSimulation {
            simulation_id,
            params_hash,
        }
    }

    /// Create a distributed storage work type.
    pub fn distributed_storage(data_hash: [u8; 32], storage_duration: u64) -> Self {
        Self::DistributedStorage {
            data_hash,
            storage_duration,
        }
    }
}

/// A proof that useful work was performed.
///
/// Contains the work type, result hash, compute units consumed, and
/// a signature from the verifier node that attests to the work's
/// validity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsefulWorkProof {
    /// The type of useful work that was performed.
    pub work_type: UsefulWorkType,
    /// Hash of the computation result (model weights, simulation output, etc.).
    pub result_hash: [u8; 32],
    /// Number of compute units consumed by this work.
    pub compute_units_consumed: u64,
    /// Signature from the verifier node attesting to the work's validity.
    pub verifier_signature: Vec<u8>,
}

impl UsefulWorkProof {
    /// Create a new useful work proof.
    ///
    /// Validates the proof on construction, ensuring compute units are
    /// non-zero and the result hash is not entirely zeros.
    pub fn new(
        work_type: UsefulWorkType,
        result_hash: [u8; 32],
        compute_units_consumed: u64,
        verifier_signature: Vec<u8>,
    ) -> Result<Self, EconomicsError> {
        let proof = Self {
            work_type,
            result_hash,
            compute_units_consumed,
            verifier_signature,
        };
        proof.validate()?; // Validate on construction
        Ok(proof)
    }

    /// Verify that the proof is valid and the work was actually done.
    ///
    /// **Stub implementation**: Currently only checks that the result
    /// hash is non-zero, compute units were consumed, and the verifier
    /// signature is non-empty. In production, this will verify a ZK proof
    /// or check a Verifiable Delay Function (VDF) result against the
    /// verifier's public key.
    ///
    /// The method is named `verify` (not `verify_stub`) because the
    /// public API should be stable — the implementation will be upgraded
    /// to real cryptographic verification without changing the call site.
    ///
    /// C-9 fix (audit v0.1.68): Implements Ed25519 signature verification
    /// against the verifier's public key. The verifier signs the
    /// `result_hash || compute_units_consumed` tuple, attesting that the
    /// work was verified. This replaces the previous stub that only checked
    /// `result_hash != [0;32]` (non-cryptographic).
    ///
    /// F-6 caveat (architecture audit 2026-06-29): The verifier signature
    /// proves that a trusted verifier ATTESTED to the work — it does NOT
    /// prove that the work was actually done. Real PoUW verification
    /// (zkML for AI training, folding schemes for scientific computation)
    /// is not yet implemented. In production, PoUW reward minting should
    /// be gated behind `OMNIA_ALLOW_UNVERIFIED_POUW=1` until real
    /// verification lands.
    ///
    /// In production mode, real Ed25519 verification is always performed.
    /// In non-production mode, if the verifier_signature is empty, a
    /// warning is logged and the proof is accepted (testing mode). If
    /// the signature is non-empty, it is verified cryptographically.
    pub fn verify(&self, verifier_pubkey: &[u8; 32]) -> bool {
        // F-6 fix: in production, require explicit opt-in for PoUW rewards
        // since real work verification is not yet implemented.
        #[cfg(feature = "production")]
        {
            let allow_unverified = std::env::var("OMNIA_ALLOW_UNVERIFIED_POUW")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            if !allow_unverified {
                tracing::error!(
                    "PoUW proof rejected — set OMNIA_ALLOW_UNVERIFIED_POUW=1 to accept \
                     attested-but-unverified work proofs in production. Real PoUW verification \
                     (zkML/folding) is not yet implemented."
                );
                return false;
            }
        }

        // Construct the message that the verifier should have signed:
        // result_hash (32 bytes) || compute_units_consumed (8 bytes LE)
        let mut message = Vec::with_capacity(40);
        message.extend_from_slice(&self.result_hash);
        message.extend_from_slice(&self.compute_units_consumed.to_le_bytes());

        if self.verifier_signature.is_empty() {
            // No signature provided
            #[cfg(feature = "production")]
            {
                tracing::error!("Production mode: work proof rejected — no verifier signature");
                return false;
            }
            #[cfg(not(feature = "production"))]
            {
                tracing::warn!("Work proof accepted without verifier signature — testing mode only");
                return self.result_hash.iter().any(|&b| b != 0) && self.compute_units_consumed > 0;
            }
        }

        // Verify the Ed25519 signature against the verifier's public key
        use ed25519_dalek::{Signature, Verifier, VerifyingKey};

        let Ok(pubkey) = VerifyingKey::from_bytes(verifier_pubkey) else {
            tracing::error!("Invalid verifier public key format");
            return false;
        };

        let Ok(sig) = Signature::from_slice(&self.verifier_signature) else {
            tracing::error!(
                "Invalid verifier signature format (len={})",
                self.verifier_signature.len()
            );
            return false;
        };

        match pubkey.verify(&message, &sig) {
            Ok(()) => {
                tracing::debug!("Work proof verifier signature valid");
                true
            }
            Err(e) => {
                tracing::warn!("Work proof verifier signature invalid: {e}");
                false
            }
        }
    }

    /// Validate the internal consistency of the proof.
    ///
    /// Checks that compute units are non-zero and that the result
    /// hash is not entirely zeros. This is a lighter check than
    /// `verify` — it doesn't check the signature.
    pub fn validate(&self) -> Result<(), EconomicsError> {
        if self.compute_units_consumed == 0 {
            return Err(EconomicsError::WorkProofInvalid);
        }
        if self.result_hash.iter().all(|&b| b == 0) {
            return Err(EconomicsError::WorkProofInvalid);
        }
        Ok(())
    }

    /// Maximum UBC reward per work proof
    pub const MAX_REWARD_PER_PROOF: u64 = 1_000_000;

    /// Calculate the UBC reward for this work.
    ///
    /// The reward is proportional to the compute units consumed,
    /// at a 1:1 ratio (1 UBC per compute unit), capped at
    /// [`UsefulWorkProof::MAX_REWARD_PER_PROOF`]. This ratio may be governed by a
    /// future on-chain parameter.
    pub fn reward_amount(&self) -> u64 {
        self.compute_units_consumed.min(Self::MAX_REWARD_PER_PROOF)
    }
}
