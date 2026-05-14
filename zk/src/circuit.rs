//! ZK circuit for proving valid L2 state transitions.
//!
//! This module defines [`RollupCircuit`], an R1CS circuit that proves a batch
//! of events was applied correctly to transition the L2 state from an old
//! state root to a new state root. The circuit is L1-agnostic: the proof can
//! be verified on any L1 that supports Groth16 verification.
//!
//! ## Circuit Constraints
//!
//! The current skeleton enforces a single constraint:
//! `new_state_root == expected_new_state_root`
//!
//! Future iterations will add Merkle path verification and per-event
//! state transition constraints.

use ark_bn254::Fr;
use ark_ff::{PrimeField, Zero};
use ark_r1cs_std::alloc::AllocVar;
use ark_r1cs_std::eq::EqGadget;
use ark_r1cs_std::fields::fp::FpVar;
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};

/// R1CS circuit for proving valid L2 state transitions.
///
/// The circuit takes an old state root, a new state root, and an event count
/// as witnesses, and enforces that `new_state_root == expected_new_state_root`
/// where `expected_new_state_root` is a public input.
///
/// # Witnesses (private)
///
/// - `old_state_root` — the state root before the batch was applied
/// - `new_state_root` — the state root after the batch was applied
/// - `event_count` — the number of events in the batch
///
/// # Public inputs
///
/// - `expected_new_state_root` — the state root that the verifier expects
pub struct RollupCircuit {
    /// The state root before the batch was applied (witness).
    old_state_root: Option<Fr>,
    /// The state root after the batch was applied (witness).
    new_state_root: Option<Fr>,
    /// The number of events in the batch (witness).
    event_count: Option<Fr>,
    /// The expected new state root (public input).
    expected_new_state_root: Option<Fr>,
}

impl RollupCircuit {
    /// Create a new rollup circuit from raw byte state roots.
    ///
    /// Converts 32-byte state roots to [`Fr`] field elements using
    /// big-endian modular reduction, and converts the event count
    /// to a field element.
    ///
    /// # Arguments
    ///
    /// * `old` — 32-byte old state root
    /// * `new` — 32-byte new state root
    /// * `event_count` — number of events in the batch
    pub fn from_state_roots(old: [u8; 32], new: [u8; 32], event_count: u64) -> Self {
        Self {
            old_state_root: Some(Fr::from_be_bytes_mod_order(&old)),
            new_state_root: Some(Fr::from_be_bytes_mod_order(&new)),
            event_count: Some(Fr::from(event_count)),
            expected_new_state_root: Some(Fr::from_be_bytes_mod_order(&new)),
        }
    }

    /// Returns the public input for this circuit instance.
    ///
    /// The public input is the expected new state root. This is used
    /// by the verifier to check the proof without knowing the witnesses.
    ///
    /// # Errors
    ///
    /// Returns [`SynthesisError::AssignmentMissing`] if the expected
    /// new state root has not been assigned.
    pub fn public_input(&self) -> Result<Vec<Fr>, SynthesisError> {
        self.expected_new_state_root
            .map(|v| vec![v])
            .ok_or(SynthesisError::AssignmentMissing)
    }

    /// Create a circuit with no assignments (for trusted setup).
    ///
    /// The structure (number of witnesses, public inputs, and constraints)
    /// is the same regardless of the actual values. This method produces
    /// a circuit suitable for generating the trusted setup keys.
    pub fn empty() -> Self {
        Self {
            old_state_root: Some(Fr::zero()),
            new_state_root: Some(Fr::zero()),
            event_count: Some(Fr::zero()),
            expected_new_state_root: Some(Fr::zero()),
        }
    }
}

impl ConstraintSynthesizer<Fr> for RollupCircuit {
    fn generate_constraints(self, cs: ConstraintSystemRef<Fr>) -> Result<(), SynthesisError> {
        // Allocate witnesses (private inputs)
        let old_state_root =
            FpVar::<Fr>::new_witness(ark_relations::ns!(cs, "old_state_root"), || {
                self.old_state_root.ok_or(SynthesisError::AssignmentMissing)
            })?;

        let new_state_root =
            FpVar::<Fr>::new_witness(ark_relations::ns!(cs, "new_state_root"), || {
                self.new_state_root.ok_or(SynthesisError::AssignmentMissing)
            })?;

        let event_count = FpVar::<Fr>::new_witness(ark_relations::ns!(cs, "event_count"), || {
            self.event_count.ok_or(SynthesisError::AssignmentMissing)
        })?;

        // Allocate public input
        let expected_new_state_root =
            FpVar::<Fr>::new_input(ark_relations::ns!(cs, "expected_new_state_root"), || {
                self.expected_new_state_root
                    .ok_or(SynthesisError::AssignmentMissing)
            })?;

        // Constraint 1: new_state_root == expected_new_state_root
        new_state_root.enforce_equal(&expected_new_state_root)?;

        // old_state_root and event_count are witnesses that participate in the
        // circuit structure. They are not constrained further in this skeleton,
        // but will be used when Merkle path verification is added.
        let _ = (old_state_root, event_count);

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Legacy stub circuit (test-only)
// ---------------------------------------------------------------------------

/// Legacy hash-chain circuit for backward-compatible testing.
///
/// This is the Phase 0 stub that used a BLAKE3 hash chain instead of
/// real ZK proofs. It is kept for test compatibility only.
#[cfg(test)]
pub struct RollupCircuitLegacy {
    /// The state root before the batch was applied.
    pub old_state_root: [u8; 32],
    /// The state root after the batch was applied.
    pub new_state_root: [u8; 32],
    /// The events in the batch.
    pub events: Vec<omnia_substrate::Event>,
}

#[cfg(test)]
impl RollupCircuitLegacy {
    /// Create a new legacy circuit with the given state roots and events.
    pub fn new(
        old_state_root: [u8; 32],
        new_state_root: [u8; 32],
        events: Vec<omnia_substrate::Event>,
    ) -> Self {
        Self {
            old_state_root,
            new_state_root,
            events,
        }
    }

    /// Phase 0 stub: computes a hash chain instead of a real ZK proof.
    pub fn prove_stub(&self) -> Vec<u8> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.old_state_root);
        for event in &self.events {
            hasher.update(&event.to_bytes());
        }
        hasher.update(&self.new_state_root);
        hasher.finalize().as_bytes().to_vec()
    }

    /// Phase 0 stub: verifies the hash chain.
    pub fn verify_stub(&self, proof: &[u8]) -> bool {
        let expected = self.prove_stub();
        proof == expected.as_slice()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omnia_substrate::crypto::generate_keypair;

    fn test_node(id: u8) -> [u8; 32] {
        let mut node = [0u8; 32];
        node[0] = id;
        node
    }

    #[test]
    fn test_circuit_from_state_roots() {
        let old = [1u8; 32];
        let new = [2u8; 32];
        let circuit = RollupCircuit::from_state_roots(old, new, 5);

        let public_input = circuit
            .public_input()
            .expect("public input should be available");
        assert_eq!(public_input.len(), 1);
        assert_eq!(public_input[0], Fr::from_be_bytes_mod_order(&new));
    }

    #[test]
    fn test_circuit_empty_public_input() {
        let circuit = RollupCircuit::empty();
        let public_input = circuit
            .public_input()
            .expect("public input should be available");
        assert_eq!(public_input.len(), 1);
        assert_eq!(public_input[0], Fr::zero());
    }

    #[test]
    fn test_legacy_circuit_prove_and_verify() {
        let creator = test_node(1);
        let keypair = generate_keypair();
        let mut event = omnia_substrate::Event::genesis(creator, vec![1, 2, 3]);
        event.sign_with_keypair(&keypair);

        let circuit = RollupCircuitLegacy::new([0u8; 32], [1u8; 32], vec![event]);
        let proof = circuit.prove_stub();

        assert!(circuit.verify_stub(&proof));
        assert!(!circuit.verify_stub(&[0u8; 32])); // Wrong proof
    }

    #[test]
    fn test_legacy_circuit_empty_events() {
        let circuit = RollupCircuitLegacy::new([0u8; 32], [0u8; 32], vec![]);
        let proof = circuit.prove_stub();

        assert!(circuit.verify_stub(&proof));
    }
}
