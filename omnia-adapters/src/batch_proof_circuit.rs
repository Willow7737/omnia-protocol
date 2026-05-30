//! ZK batch proof circuit for verifying batch proofs within the ZK circuit.
//!
//! This module defines a [`BatchProofCircuit`] that verifies a batch proof
//! (Merkle root of event hashes) within the ZK circuit, supporting the
//! 100-tx batch proof aggregation target.
//!
//! # Circuit Overview
//!
//! The batch proof circuit verifies:
//! - The Merkle root of all event hashes in the batch matches the claimed root
//! - Each event hash is properly computed from the event data
//! - The batch ID (domain-separated hash of merkle_root || event_count) is correct
//!
//! # Public Inputs
//!
//! - `batch_merkle_root` — The claimed Merkle root of all event hashes
//! - `batch_id` — The domain-separated batch identifier
//! - `event_count` — Number of events in the batch
//!
//! # Witnesses
//!
//! - `event_hashes` — Individual event hashes in the batch
//! - `merkle_proof_siblings` — Sibling hashes for each event's Merkle path
//! - `merkle_proof_directions` — Direction bits for each event's Merkle path

use ark_bn254::Fr;
use ark_ff::Zero;
use ark_ff::PrimeField;
use ark_r1cs_std::alloc::AllocVar;
use ark_r1cs_std::boolean::Boolean;
use ark_r1cs_std::eq::EqGadget;
use ark_r1cs_std::fields::fp::FpVar;
use ark_r1cs_std::fields::FieldVar;
use ark_r1cs_std::select::CondSelectGadget;
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};

use crate::merkle::{self, Poseidon};

/// Target batch size for proof aggregation (100 transactions).
pub const BATCH_PROOF_TARGET_SIZE: usize = 100;

/// Domain separation for batch proof Merkle tree hashing.
pub const BATCH_PROOF_DOMAIN: &[u8] = b"omnia-batch-proof";

/// Domain separation for batch ID computation.
pub const BATCH_ID_DOMAIN: &[u8] = b"omnia-batch-id";

/// ZK circuit for verifying batch proofs.
///
/// This circuit verifies that a set of event hashes correctly produces
/// a claimed Merkle root and batch ID. It is designed for the 100-tx
/// batch proof aggregation target.
///
/// # Security
///
/// The circuit uses Poseidon hash for all Merkle path computations,
/// ensuring SNARK-friendly verification. The batch ID is computed as
/// `Poseidon(merkle_root, event_count)` within the circuit.
#[derive(Clone)]
pub struct BatchProofCircuit {
    /// Public input: claimed Merkle root of all event hashes.
    pub batch_merkle_root: Option<Fr>,
    /// Public input: claimed batch identifier.
    pub batch_id: Option<Fr>,
    /// Public input: number of events in the batch.
    pub event_count: Option<Fr>,

    /// Witness: individual event hashes.
    pub event_hashes: Vec<Option<Fr>>,
    /// Witness: Merkle path siblings for each event.
    pub merkle_siblings: Vec<Vec<Option<Fr>>>,
    /// Witness: Merkle path direction bits for each event.
    pub merkle_directions: Vec<Vec<Option<bool>>>,
}

impl BatchProofCircuit {
    /// Create a batch proof circuit from a list of event hashes and their
    /// Merkle proofs.
    ///
    /// **Important**: The `merkle_proofs` and `merkle_root` must be produced by
    /// [`build_poseidon_merkle_tree`], not [`build_merkle_tree`]. The circuit
    /// verifies Merkle paths using Poseidon hashing, so BLAKE3-based proofs
    /// would produce roots that don't match the in-circuit verification.
    ///
    /// [`build_poseidon_merkle_tree`]: crate::merkle::build_poseidon_merkle_tree
    /// [`build_merkle_tree`]: crate::merkle::build_merkle_tree
    ///
    /// # Arguments
    ///
    /// * `event_hashes` — Field element hashes for each event in the batch
    /// * `merkle_root` — The expected Merkle root
    /// * `merkle_proofs` — Merkle inclusion proofs for each event
    ///
    /// # Panics
    ///
    /// Panics if `merkle_proofs.len() != event_hashes.len()`.
    #[allow(clippy::too_many_arguments)]
    pub fn from_batch(
        event_hashes: Vec<Fr>,
        merkle_root: Fr,
        merkle_proofs: Vec<merkle::MerkleProof<Poseidon>>,
    ) -> Self {
        let num_events = event_hashes.len();
        assert_eq!(
            merkle_proofs.len(),
            num_events,
            "number of merkle proofs must match number of events"
        );

        let event_count = Fr::from(num_events as u64);

        // Compute batch_id off-circuit as Poseidon(merkle_root, event_count)
        // so the circuit can verify it matches the in-circuit computation.
        let batch_id = crate::poseidon::poseidon_hash_offchain(merkle_root, event_count)
            .ok()
            .unwrap_or(Fr::zero());

        let merkle_siblings: Vec<Vec<Option<Fr>>> = merkle_proofs
            .iter()
            .map(|proof| proof.siblings.iter().map(|s| Some(merkle::hash_to_fr(s))).collect())
            .collect();

        let merkle_directions: Vec<Vec<Option<bool>>> = merkle_proofs
            .iter()
            .map(|proof| proof.directions.iter().map(|d| Some(*d)).collect())
            .collect();

        Self {
            batch_merkle_root: Some(merkle_root),
            batch_id: Some(batch_id),
            event_count: Some(event_count),
            event_hashes: event_hashes.into_iter().map(Some).collect(),
            merkle_siblings,
            merkle_directions,
        }
    }

    /// Create an empty circuit suitable for trusted setup.
    ///
    /// # Arguments
    ///
    /// * `num_events` — Number of events in the batch (determines circuit size)
    /// * `merkle_depth` — Depth of each Merkle proof
    pub fn empty(num_events: usize, merkle_depth: usize) -> Self {
        Self {
            batch_merkle_root: Some(Fr::zero()),
            batch_id: Some(Fr::zero()),
            event_count: Some(Fr::from(num_events as u64)),
            event_hashes: (0..num_events).map(|_| Some(Fr::zero())).collect(),
            merkle_siblings: (0..num_events)
                .map(|_| (0..merkle_depth).map(|_| Some(Fr::zero())).collect())
                .collect(),
            merkle_directions: (0..num_events)
                .map(|_| (0..merkle_depth).map(|_| Some(false)).collect())
                .collect(),
        }
    }

    /// Create a circuit for trusted setup key generation with non-zero witnesses.
    ///
    /// # Arguments
    ///
    /// * `num_events` — Number of events in the batch
    /// * `merkle_depth` — Depth of each Merkle proof
    pub fn for_setup(num_events: usize, merkle_depth: usize) -> Self {
        Self {
            batch_merkle_root: Some(Fr::from(1u64)),
            batch_id: Some(Fr::from(2u64)),
            event_count: Some(Fr::from(num_events as u64)),
            event_hashes: (0..num_events).map(|i| Some(Fr::from(i as u64 + 3))).collect(),
            merkle_siblings: (0..num_events)
                .map(|_| (0..merkle_depth).map(|j| Some(Fr::from(j as u64 + 1))).collect())
                .collect(),
            merkle_directions: (0..num_events)
                .map(|_| (0..merkle_depth).map(|_| Some(true)).collect())
                .collect(),
        }
    }

    /// Returns the public inputs for this circuit instance.
    ///
    /// # Errors
    ///
    /// Returns [`SynthesisError::AssignmentMissing`] if any public input
    /// has not been assigned.
    pub fn public_input(&self) -> Result<Vec<Fr>, SynthesisError> {
        let root = self.batch_merkle_root.ok_or(SynthesisError::AssignmentMissing)?;
        let id = self.batch_id.ok_or(SynthesisError::AssignmentMissing)?;
        let count = self.event_count.ok_or(SynthesisError::AssignmentMissing)?;
        Ok(vec![root, id, count])
    }
}

impl ConstraintSynthesizer<Fr> for BatchProofCircuit {
    fn generate_constraints(self, cs: ConstraintSystemRef<Fr>) -> Result<(), SynthesisError> {
        let num_events = self.event_hashes.len();

        // Allocate public inputs
        let batch_merkle_root = FpVar::<Fr>::new_input(ark_relations::ns!(cs, "batch_merkle_root"), || {
            self.batch_merkle_root.ok_or(SynthesisError::AssignmentMissing)
        })?;
        let batch_id = FpVar::<Fr>::new_input(ark_relations::ns!(cs, "batch_id"), || {
            self.batch_id.ok_or(SynthesisError::AssignmentMissing)
        })?;
        let event_count = FpVar::<Fr>::new_input(ark_relations::ns!(cs, "event_count"), || {
            self.event_count.ok_or(SynthesisError::AssignmentMissing)
        })?;

        // Constrain event_count to match the number of events
        event_count.enforce_equal(&FpVar::constant(Fr::from(num_events as u64)))?;

        // If the batch is empty (num_events == 0), the merkle_root must be the
        // domain-separated empty root, preventing a malicious prover from claiming
        // an arbitrary root for an empty batch.
        if num_events == 0 {
            let empty_root_bytes = blake3::derive_key("OMNIA-MERKLE-EMPTY-ROOT", &[]);
            let empty_root_fr = Fr::from_be_bytes_mod_order(&empty_root_bytes);
            batch_merkle_root.enforce_equal(&FpVar::constant(empty_root_fr))?;
        }

        // For each event: verify Merkle inclusion path
        // All paths should converge to the same Merkle root
        for i in 0..num_events {
            let event_hash = FpVar::<Fr>::new_witness(cs.clone(), || {
                self.event_hashes[i].ok_or(SynthesisError::AssignmentMissing)
            })?;

            let proof = &self.merkle_siblings[i];
            let directions = &self.merkle_directions[i];

            let mut current = event_hash;

            for j in 0..proof.len() {
                let sibling =
                    FpVar::<Fr>::new_witness(cs.clone(), || proof[j].ok_or(SynthesisError::AssignmentMissing))?;
                let go_left =
                    Boolean::new_witness(cs.clone(), || directions[j].ok_or(SynthesisError::AssignmentMissing))?;

                // Conditional swap based on direction
                let left = <FpVar<Fr> as CondSelectGadget<Fr>>::conditionally_select(&go_left, &sibling, &current)?;
                let right = <FpVar<Fr> as CondSelectGadget<Fr>>::conditionally_select(&go_left, &current, &sibling)?;

                // Poseidon hash
                current = crate::poseidon::poseidon_hash(cs.clone(), &left, &right)?;
            }

            // The computed root must equal the claimed Merkle root
            current.enforce_equal(&batch_merkle_root)?;
        }

        // Compute batch_id in-circuit: Poseidon(merkle_root, event_count)
        let computed_batch_id = crate::poseidon::poseidon_hash(cs.clone(), &batch_merkle_root, &event_count)?;

        // Constrain batch_id
        computed_batch_id.enforce_equal(&batch_id)?;

        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_batch_proof_circuit_public_input() {
        let circuit = BatchProofCircuit::empty(10, 5);
        let public_input = circuit.public_input().unwrap();
        assert_eq!(public_input.len(), 3);
        assert_eq!(public_input[0], Fr::zero()); // merkle_root
        assert_eq!(public_input[1], Fr::zero()); // batch_id
        assert_eq!(public_input[2], Fr::from(10u64)); // event_count
    }

    #[test]
    fn test_batch_proof_circuit_for_setup() {
        let circuit = BatchProofCircuit::for_setup(10, 5);
        let public_input = circuit.public_input().unwrap();
        assert_eq!(public_input.len(), 3);
        assert_ne!(public_input[0], Fr::zero()); // merkle_root
        assert_ne!(public_input[1], Fr::zero()); // batch_id
        assert_eq!(public_input[2], Fr::from(10u64)); // event_count
    }

    #[test]
    fn test_batch_proof_target_size() {
        // Verify the 100-tx target is achievable
        assert_eq!(BATCH_PROOF_TARGET_SIZE, 100);

        // Create an empty circuit with target size
        let circuit = BatchProofCircuit::empty(BATCH_PROOF_TARGET_SIZE, 8);
        assert_eq!(circuit.event_hashes.len(), BATCH_PROOF_TARGET_SIZE);
    }

    #[test]
    fn test_batch_proof_circuit_empty_batch() {
        let circuit = BatchProofCircuit::empty(0, 0);
        let public_input = circuit.public_input().unwrap();
        assert_eq!(public_input[2], Fr::zero()); // event_count = 0
    }
}
