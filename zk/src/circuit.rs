//! ZK circuit for proving valid L2 state transitions.
//!
//! This module defines two circuits:
//!
//! - [`RollupCircuit`] — The original skeleton circuit that enforces a single
//!   constraint: `new_state_root == expected_new_state_root`. Retained for
//!   backward compatibility.
//!
//! - [`ExpandedRollupCircuit`] — An expanded circuit with Merkle path
//!   verification and per-event state transition constraints. This circuit
//!   enforces:
//!   - Each event is included in the event commitment (via Merkle path)
//!   - Each event causes a valid state transition (intermediate root updates)
//!   - The first intermediate root equals the old state root
//!   - The last intermediate root equals the new state root
//!
//! ## Public Inputs (ExpandedRollupCircuit)
//!
//! - `old_state_root` — The state root before the batch
//! - `new_state_root` — The state root after the batch
//! - `event_commitment` — The Merkle root of all events in the batch
//!
//! ## Witnesses (ExpandedRollupCircuit)
//!
//! - `events` — Individual event data (hash, operation type, payload hash)
//! - `merkle_proofs` — Merkle inclusion proofs for each event
//! - `intermediate_roots` — State roots after each event application

use ark_bn254::Fr;
use ark_ff::{PrimeField, Zero};
use ark_r1cs_std::alloc::AllocVar;
use ark_r1cs_std::boolean::Boolean;
use ark_r1cs_std::eq::EqGadget;
use ark_r1cs_std::fields::fp::FpVar;
use ark_r1cs_std::select::CondSelectGadget;
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};

use crate::merkle::{self, MerkleProof};

// ---------------------------------------------------------------------------
// Original RollupCircuit (unchanged)
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// ExpandedRollupCircuit
// ---------------------------------------------------------------------------

/// An expanded rollup circuit with Merkle path verification and per-event
/// state transition constraints.
///
/// This circuit enforces:
/// - Each event is included in the event commitment (via Merkle path)
/// - Each event causes a valid state transition (intermediate root updates)
/// - The first intermediate root equals the old state root
/// - The last intermediate root equals the new state root
///
/// # Public Inputs
///
/// - `old_state_root` — The state root before the batch
/// - `new_state_root` — The state root after the batch
/// - `event_commitment` — The Merkle root of all events in the batch
///
/// # Witnesses
///
/// - `events` — Individual event data (hash, operation type, payload hash)
/// - `merkle_proofs` — Merkle inclusion proofs for each event
/// - `intermediate_roots` — State roots after each event application
#[derive(Clone)]
pub struct ExpandedRollupCircuit {
    /// Public input: state root before the batch.
    pub old_state_root: Option<Fr>,
    /// Public input: state root after the batch.
    pub new_state_root: Option<Fr>,
    /// Public input: Merkle root of all events in the batch.
    pub event_commitment: Option<Fr>,

    /// Witness: individual event data.
    pub events: Vec<Option<EventWitness>>,
    /// Witness: Merkle inclusion proofs for each event.
    pub merkle_proofs: Vec<Option<MerklePathWitness>>,
    /// Witness: intermediate state roots (len = events.len() + 1).
    pub intermediate_roots: Vec<Option<Fr>>,
}

/// Witness data for a single event in the batch.
#[derive(Clone, Debug)]
pub struct EventWitness {
    /// Hash of the event (BLAKE3).
    pub event_hash: Option<Fr>,
    /// Operation type encoded as a field element.
    pub operation_type: Option<Fr>,
    /// Hash of the event payload.
    pub payload_hash: Option<Fr>,
}

/// Witness data for a Merkle path (siblings + directions).
#[derive(Clone, Debug)]
pub struct MerklePathWitness {
    /// Sibling hashes at each level.
    pub siblings: Vec<Option<Fr>>,
    /// Direction bits: `true` means the sibling is on the left.
    pub directions: Vec<Option<bool>>,
}

impl ExpandedRollupCircuit {
    /// Construct an `ExpandedRollupCircuit` from a batch of events with their
    /// Merkle proofs and intermediate state roots.
    ///
    /// This is the primary constructor for production use. It converts
    /// byte-based Merkle proofs (from the [`merkle`] module) into the
    /// field-element representation used by the circuit.
    ///
    /// # Arguments
    ///
    /// * `old_root` — Field element representing the old state root
    /// * `new_root` — Field element representing the new state root
    /// * `event_hashes` — Field element hashes for each event in the batch
    /// * `event_commitment` — Field element for the Merkle root of all events
    /// * `merkle_proofs` — Byte-based Merkle inclusion proofs for each event
    /// * `intermediate_roots` — Field element state roots after each event
    ///   (length must be `event_hashes.len() + 1`)
    ///
    /// # Panics
    ///
    /// Panics if `merkle_proofs.len() != event_hashes.len()` or
    /// `intermediate_roots.len() != event_hashes.len() + 1` (when events is
    /// non-empty).
    pub fn from_batch(
        old_root: Fr,
        new_root: Fr,
        event_hashes: Vec<Fr>,
        event_commitment: Fr,
        merkle_proofs: Vec<MerkleProof>,
        intermediate_roots: Vec<Fr>,
    ) -> Self {
        let num_events = event_hashes.len();
        assert_eq!(
            merkle_proofs.len(),
            num_events,
            "number of merkle proofs must match number of events"
        );
        if num_events > 0 {
            assert_eq!(
                intermediate_roots.len(),
                num_events + 1,
                "intermediate_roots must have len = events + 1"
            );
        }

        let events: Vec<Option<EventWitness>> = event_hashes
            .into_iter()
            .map(|h| {
                Some(EventWitness {
                    event_hash: Some(h),
                    operation_type: Some(Fr::zero()),
                    payload_hash: Some(Fr::zero()),
                })
            })
            .collect();

        let merkle_path_witnesses: Vec<Option<MerklePathWitness>> = merkle_proofs
            .into_iter()
            .map(|proof| {
                Some(MerklePathWitness {
                    siblings: proof
                        .siblings
                        .iter()
                        .map(|s| Some(merkle::hash_to_fr(s)))
                        .collect(),
                    directions: proof.directions.iter().map(|d| Some(*d)).collect(),
                })
            })
            .collect();

        let intermediate_roots_opt: Vec<Option<Fr>> =
            intermediate_roots.into_iter().map(Some).collect();

        Self {
            old_state_root: Some(old_root),
            new_state_root: Some(new_root),
            event_commitment: Some(event_commitment),
            events,
            merkle_proofs: merkle_path_witnesses,
            intermediate_roots: intermediate_roots_opt,
        }
    }

    /// Create an empty circuit suitable for trusted setup.
    ///
    /// The circuit structure depends on the number of events and the Merkle
    /// proof depth. This method creates a circuit with dummy values that has
    /// the same structure as a real circuit with the given parameters.
    ///
    /// # Arguments
    ///
    /// * `num_events` — Number of events in the batch (determines circuit size)
    /// * `merkle_depth` — Depth of each Merkle proof (number of siblings)
    ///
    /// # Returns
    ///
    /// A circuit with zero-valued assignments suitable for generating
    /// trusted setup keys.
    pub fn empty(num_events: usize, merkle_depth: usize) -> Self {
        let events: Vec<Option<EventWitness>> = (0..num_events)
            .map(|_| {
                Some(EventWitness {
                    event_hash: Some(Fr::zero()),
                    operation_type: Some(Fr::zero()),
                    payload_hash: Some(Fr::zero()),
                })
            })
            .collect();

        let merkle_proofs: Vec<Option<MerklePathWitness>> = (0..num_events)
            .map(|_| {
                Some(MerklePathWitness {
                    siblings: (0..merkle_depth).map(|_| Some(Fr::zero())).collect(),
                    directions: (0..merkle_depth).map(|_| Some(false)).collect(),
                })
            })
            .collect();

        let intermediate_roots: Vec<Option<Fr>> =
            (0..=num_events).map(|_| Some(Fr::zero())).collect();

        Self {
            old_state_root: Some(Fr::zero()),
            new_state_root: Some(Fr::zero()),
            event_commitment: Some(Fr::zero()),
            events,
            merkle_proofs,
            intermediate_roots,
        }
    }

    /// Returns the public inputs for this circuit instance.
    ///
    /// The public inputs are `[old_state_root, new_state_root, event_commitment]`.
    /// These are used by the verifier to check the proof without knowing
    /// the witnesses.
    ///
    /// # Errors
    ///
    /// Returns [`SynthesisError::AssignmentMissing`] if any public input
    /// has not been assigned.
    pub fn public_input(&self) -> Result<Vec<Fr>, SynthesisError> {
        let old = self
            .old_state_root
            .ok_or(SynthesisError::AssignmentMissing)?;
        let new = self
            .new_state_root
            .ok_or(SynthesisError::AssignmentMissing)?;
        let commitment = self
            .event_commitment
            .ok_or(SynthesisError::AssignmentMissing)?;
        Ok(vec![old, new, commitment])
    }
}

impl ConstraintSynthesizer<Fr> for ExpandedRollupCircuit {
    fn generate_constraints(self, cs: ConstraintSystemRef<Fr>) -> Result<(), SynthesisError> {
        let num_events = self.events.len();

        // Allocate public inputs
        let old_root = FpVar::<Fr>::new_input(ark_relations::ns!(cs, "old_state_root"), || {
            self.old_state_root.ok_or(SynthesisError::AssignmentMissing)
        })?;
        let new_root = FpVar::<Fr>::new_input(ark_relations::ns!(cs, "new_state_root"), || {
            self.new_state_root.ok_or(SynthesisError::AssignmentMissing)
        })?;
        let event_commitment =
            FpVar::<Fr>::new_input(ark_relations::ns!(cs, "event_commitment"), || {
                self.event_commitment
                    .ok_or(SynthesisError::AssignmentMissing)
            })?;

        // Allocate all intermediate root witnesses upfront so they are shared
        // across boundary constraints and state-transition constraints.
        let intermediate_root_vars: Vec<FpVar<Fr>> = self
            .intermediate_roots
            .iter()
            .map(|root| {
                FpVar::<Fr>::new_witness(cs.clone(), || {
                    root.ok_or(SynthesisError::AssignmentMissing)
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        // First intermediate root must equal old_state_root
        if num_events > 0 {
            intermediate_root_vars[0].enforce_equal(&old_root)?;
        }

        // For each event: verify Merkle inclusion and state transition
        for i in 0..num_events {
            // Allocate event hash witness
            let event_hash = FpVar::<Fr>::new_witness(cs.clone(), || {
                self.events[i]
                    .as_ref()
                    .and_then(|e| e.event_hash)
                    .ok_or(SynthesisError::AssignmentMissing)
            })?;

            // Allocate Merkle path witnesses and verify inclusion
            let proof = &self.merkle_proofs[i];
            let mut current = event_hash.clone();
            if let Some(ref path_witness) = proof {
                for j in 0..path_witness.siblings.len() {
                    let sibling = FpVar::<Fr>::new_witness(cs.clone(), || {
                        path_witness.siblings[j].ok_or(SynthesisError::AssignmentMissing)
                    })?;
                    let go_left = Boolean::new_witness(cs.clone(), || {
                        path_witness.directions[j].ok_or(SynthesisError::AssignmentMissing)
                    })?;

                    // Conditional swap based on direction.
                    // If go_left (sibling is on the left): left = sibling, right = current
                    // If !go_left (current is on the left): left = current, right = sibling
                    let left = <FpVar<Fr> as CondSelectGadget<Fr>>::conditionally_select(
                        &go_left, &sibling, &current,
                    )?;
                    let right = <FpVar<Fr> as CondSelectGadget<Fr>>::conditionally_select(
                        &go_left, &current, &sibling,
                    )?;

                    // Simplified hash: use field addition as a commitment.
                    // A real implementation would use a Pedersen or Poseidon hash gadget.
                    current = left + right;
                }
            }

            // Enforce that the computed root equals the event commitment
            current.enforce_equal(&event_commitment)?;

            // State transition constraint:
            // intermediate_root[i+1] = intermediate_root[i] + event_hash[i]
            //
            // This is a simplified state transition function. A real
            // implementation would use a proper state transition function
            // (e.g., sparse Merkle tree update).
            let expected_next = intermediate_root_vars[i].clone() + event_hash.clone();
            intermediate_root_vars[i + 1].enforce_equal(&expected_next)?;
        }

        // Last intermediate root must equal new_state_root
        if num_events > 0 {
            intermediate_root_vars[num_events].enforce_equal(&new_root)?;
        }

        // Edge case: empty batch (no events) — old_root must equal new_root
        if num_events == 0 {
            old_root.enforce_equal(&new_root)?;
        }

        Ok(())
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

    #[test]
    fn test_expanded_circuit_public_input() {
        let circuit = ExpandedRollupCircuit::empty(2, 3);
        let public_input = circuit
            .public_input()
            .expect("public input should be available");
        assert_eq!(public_input.len(), 3);
        assert_eq!(public_input[0], Fr::zero());
        assert_eq!(public_input[1], Fr::zero());
        assert_eq!(public_input[2], Fr::zero());
    }

    #[test]
    fn test_expanded_circuit_empty_batch_public_input() {
        let circuit = ExpandedRollupCircuit::empty(0, 0);
        let public_input = circuit
            .public_input()
            .expect("public input should be available");
        assert_eq!(public_input.len(), 3);
    }
}
