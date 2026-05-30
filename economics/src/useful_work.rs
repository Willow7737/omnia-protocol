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
    pub fn new(
        work_type: UsefulWorkType,
        result_hash: [u8; 32],
        compute_units_consumed: u64,
        verifier_signature: Vec<u8>,
    ) -> Self {
        Self {
            work_type,
            result_hash,
            compute_units_consumed,
            verifier_signature,
        }
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
    pub fn verify(&self, _verifier_pubkey: &[u8; 32]) -> bool {
        // A non-zero result hash, positive compute units, and non-empty
        // signature are the minimum validity requirements for the stub.
        // Real verification will replace this with cryptographic proof checking.
        self.result_hash.iter().any(|&b| b != 0)
            && self.compute_units_consumed > 0
            && !self.verifier_signature.is_empty()
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
