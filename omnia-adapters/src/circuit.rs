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
//!   - Each event's operation type is in the valid range `[0, MAX_OPERATION_TYPE]`
//!     (via bit decomposition constraints)
//!   - Each event's payload hash is bound to the event hash and operation type:
//!     `payload_hash == Poseidon(event_hash, operation_type)`
//!
//! ## Hash Function
//!
//! Both Merkle path verification and state transition constraints use the
//! **Poseidon** SNARK-friendly hash function (Grassi et al. 2019,
//! <https://eprint.iacr.org/2019/458>). This replaces the previous field-addition
//! placeholder with a cryptographically sound hash that is efficient inside
//! R1CS (~243 multiplication constraints per hash).
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
use ark_r1cs_std::fields::FieldVar;
use ark_r1cs_std::select::CondSelectGadget;
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};

use crate::merkle::{self, MerkleProof, Poseidon};

// ---------------------------------------------------------------------------
// Operation type definitions
// ---------------------------------------------------------------------------

/// Maximum valid operation type value.
pub const MAX_OPERATION_TYPE: u8 = 7;
/// Number of bits needed to represent operation types (ceil(log2(8))).
pub const OP_TYPE_BITS: usize = 3;

/// Operation types for rollup events.
///
/// Each event in a batch must have a valid operation type. The circuit
/// enforces that operation types are in the range `[0, MAX_OPERATION_TYPE]`
/// via bit decomposition constraints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum OperationType {
    /// Token transfer between accounts.
    Transfer = 0,
    /// Stake tokens for validation.
    Stake = 1,
    /// Unstake previously staked tokens.
    Unstake = 2,
    /// Delegate stake to another validator.
    Delegate = 3,
    /// Slash a misbehaving validator.
    Slash = 4,
    /// Vote on a governance proposal.
    GovernanceVote = 5,
    /// Send a message to another shard.
    CrossShardMessage = 6,
    /// Update identity information.
    IdentityUpdate = 7,
}

impl OperationType {
    /// Convert to field element.
    pub fn to_fr(&self) -> Fr {
        Fr::from(*self as u64)
    }

    /// Try to convert a `u8` to an [`OperationType`].
    ///
    /// Returns `None` if the value is greater than [`MAX_OPERATION_TYPE`].
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(OperationType::Transfer),
            1 => Some(OperationType::Stake),
            2 => Some(OperationType::Unstake),
            3 => Some(OperationType::Delegate),
            4 => Some(OperationType::Slash),
            5 => Some(OperationType::GovernanceVote),
            6 => Some(OperationType::CrossShardMessage),
            7 => Some(OperationType::IdentityUpdate),
            _ => None,
        }
    }
}

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
#[derive(Clone)]
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
    ///
    /// # Security
    ///
    /// The `expected_new_state_root` is set from the `new` parameter,
    /// meaning the circuit will enforce that the witness new state root
    /// matches the public input. A proof is only valid if the prover
    /// knows a witness that satisfies this constraint.
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
        let old_state_root = FpVar::<Fr>::new_witness(ark_relations::ns!(cs, "old_state_root"), || {
            self.old_state_root.ok_or(SynthesisError::AssignmentMissing)
        })?;

        let new_state_root = FpVar::<Fr>::new_witness(ark_relations::ns!(cs, "new_state_root"), || {
            self.new_state_root.ok_or(SynthesisError::AssignmentMissing)
        })?;

        let event_count = FpVar::<Fr>::new_witness(ark_relations::ns!(cs, "event_count"), || {
            self.event_count.ok_or(SynthesisError::AssignmentMissing)
        })?;

        // Allocate public input
        let expected_new_state_root =
            FpVar::<Fr>::new_input(ark_relations::ns!(cs, "expected_new_state_root"), || {
                self.expected_new_state_root.ok_or(SynthesisError::AssignmentMissing)
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
    pub events: Vec<omnia_primitives::Event>,
}

#[cfg(test)]
impl RollupCircuitLegacy {
    /// Create a new legacy circuit with the given state roots and events.
    pub fn new(old_state_root: [u8; 32], new_state_root: [u8; 32], events: Vec<omnia_primitives::Event>) -> Self {
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
            if let Ok(bytes) = event.to_bytes() {
                hasher.update(&bytes);
            }
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
    /// **Important**: The `merkle_proofs` and `event_commitment` must be
    /// produced by [`build_poseidon_merkle_tree`], not [`build_merkle_tree`].
    /// The circuit verifies Merkle paths using Poseidon hashing, so BLAKE3-based
    /// proofs would produce roots that don't match the in-circuit verification.
    ///
    /// [`build_poseidon_merkle_tree`]: crate::merkle::build_poseidon_merkle_tree
    /// [`build_merkle_tree`]: crate::merkle::build_merkle_tree
    ///
    /// # Arguments
    ///
    /// * `old_root` — Field element representing the old state root
    /// * `new_root` — Field element representing the new state root
    /// * `event_hashes` — Field element hashes for each event in the batch
    /// * `operation_types` — Field element operation types for each event
    ///   (must be in range `[0, MAX_OPERATION_TYPE]`; enforced by circuit)
    /// * `payload_hashes` — Field element payload hashes for each event
    ///   (must equal `Poseidon(event_hash, operation_type)`; enforced by circuit)
    /// * `event_commitment` — Field element for the Merkle root of all events
    /// * `merkle_proofs` — Byte-based Merkle inclusion proofs for each event
    /// * `intermediate_roots` — Field element state roots after each event
    ///   (length must be `event_hashes.len() + 1`)
    ///
    /// # Panics
    ///
    /// Panics if `merkle_proofs.len() != event_hashes.len()`,
    /// `operation_types.len() != event_hashes.len()`,
    /// `payload_hashes.len() != event_hashes.len()`, or
    /// `intermediate_roots.len() != event_hashes.len() + 1` (when events is
    /// non-empty).
    ///
    /// # Security
    ///
    /// The circuit enforces Merkle inclusion proofs using Poseidon hash,
    /// binding each event to the `event_commitment` public input. A valid
    /// proof guarantees that all events in the batch are committed to the
    /// Merkle root, preventing inclusion of forged events. Operation types
    /// are constrained to the valid range `[0, MAX_OPERATION_TYPE]` via bit
    /// decomposition, and payload hashes are bound to the event hash and
    /// operation type via `payload_hash == Poseidon(event_hash, operation_type)`.
    #[allow(clippy::too_many_arguments)]
    pub fn from_batch(
        old_root: Fr,
        new_root: Fr,
        event_hashes: Vec<Fr>,
        operation_types: Vec<Fr>,
        payload_hashes: Vec<Fr>,
        event_commitment: Fr,
        merkle_proofs: Vec<MerkleProof<Poseidon>>,
        intermediate_roots: Vec<Fr>,
    ) -> Self {
        let num_events = event_hashes.len();
        assert_eq!(
            merkle_proofs.len(),
            num_events,
            "number of merkle proofs must match number of events"
        );
        assert_eq!(
            operation_types.len(),
            num_events,
            "number of operation types must match number of events"
        );
        assert_eq!(
            payload_hashes.len(),
            num_events,
            "number of payload hashes must match number of events"
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
            .zip(operation_types)
            .zip(payload_hashes)
            .map(|((h, op), ph)| {
                Some(EventWitness {
                    event_hash: Some(h),
                    operation_type: Some(op),
                    payload_hash: Some(ph),
                })
            })
            .collect();

        let merkle_path_witnesses: Vec<Option<MerklePathWitness>> = merkle_proofs
            .into_iter()
            .map(|proof| {
                Some(MerklePathWitness {
                    siblings: proof.siblings.iter().map(|s| Some(merkle::hash_to_fr(s))).collect(),
                    directions: proof.directions.iter().map(|d| Some(*d)).collect(),
                })
            })
            .collect();

        let intermediate_roots_opt: Vec<Option<Fr>> = intermediate_roots.into_iter().map(Some).collect();

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
    ///
    /// # Warning
    ///
    /// Using `empty()` for trusted setup may produce keys that do not
    /// correctly constrain all circuit branches, since some constraint
    /// systems behave differently with all-zero witnesses. Use
    /// [`for_setup()`](Self::for_setup) instead for production key generation.
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

        let intermediate_roots: Vec<Option<Fr>> = (0..=num_events).map(|_| Some(Fr::zero())).collect();

        Self {
            old_state_root: Some(Fr::zero()),
            new_state_root: Some(Fr::zero()),
            event_commitment: Some(Fr::zero()),
            events,
            merkle_proofs,
            intermediate_roots,
        }
    }

    /// Create a circuit instance for trusted setup key generation.
    ///
    /// Uses `MAX_OPERATION_TYPE` as the operation_type and non-zero values
    /// for all witness fields to ensure the setup covers all constraint branches.
    /// This is critical: if `empty()` (all zeros) is used for setup, the
    /// resulting proving key may not correctly constrain all branches of the
    /// circuit, allowing invalid proofs to be accepted.
    ///
    /// # Arguments
    ///
    /// * `num_events` — Number of events in the batch (determines circuit size)
    /// * `merkle_depth` — Depth of each Merkle proof (number of siblings)
    ///
    /// # Returns
    ///
    /// A circuit with non-zero assignments suitable for generating
    /// trusted setup keys that correctly constrain all circuit branches.
    pub fn for_setup(num_events: usize, merkle_depth: usize) -> Self {
        let events: Vec<Option<EventWitness>> = (0..num_events)
            .map(|i| {
                Some(EventWitness {
                    event_hash: Some(Fr::from(i as u64 + 3)),
                    operation_type: Some(Fr::from(MAX_OPERATION_TYPE as u64)),
                    payload_hash: Some(Fr::from(i as u64 + 5)),
                })
            })
            .collect();

        let merkle_proofs: Vec<Option<MerklePathWitness>> = (0..num_events)
            .map(|_| {
                Some(MerklePathWitness {
                    siblings: (0..merkle_depth).map(|j| Some(Fr::from(j as u64 + 1))).collect(),
                    directions: (0..merkle_depth).map(|_| Some(true)).collect(),
                })
            })
            .collect();

        let intermediate_roots: Vec<Option<Fr>> = (0..=num_events).map(|i| Some(Fr::from(i as u64 + 1))).collect();

        Self {
            old_state_root: Some(Fr::from(1u64)),
            new_state_root: Some(Fr::from(2u64)),
            event_commitment: Some(Fr::from(3u64)),
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
        let old = self.old_state_root.ok_or(SynthesisError::AssignmentMissing)?;
        let new = self.new_state_root.ok_or(SynthesisError::AssignmentMissing)?;
        let commitment = self.event_commitment.ok_or(SynthesisError::AssignmentMissing)?;
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
        let event_commitment = FpVar::<Fr>::new_input(ark_relations::ns!(cs, "event_commitment"), || {
            self.event_commitment.ok_or(SynthesisError::AssignmentMissing)
        })?;

        // Allocate all intermediate root witnesses upfront so they are shared
        // across boundary constraints and state-transition constraints.
        let intermediate_root_vars: Vec<FpVar<Fr>> = self
            .intermediate_roots
            .iter()
            .map(|root| FpVar::<Fr>::new_witness(cs.clone(), || root.ok_or(SynthesisError::AssignmentMissing)))
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
                    let left = <FpVar<Fr> as CondSelectGadget<Fr>>::conditionally_select(&go_left, &sibling, &current)?;
                    let right =
                        <FpVar<Fr> as CondSelectGadget<Fr>>::conditionally_select(&go_left, &current, &sibling)?;

                    // Poseidon SNARK-friendly hash: replaces the previous field-addition
                    // placeholder with a cryptographically sound hash function.
                    //
                    // Reference: Grassi et al. (2019), "Poseidon: A New Hash Function
                    // for Zero-Knowledge Proof Systems", https://eprint.iacr.org/2019/458
                    current = crate::poseidon::poseidon_hash(cs.clone(), &left, &right)?;
                }
            }

            // Enforce that the computed root equals the event commitment
            current.enforce_equal(&event_commitment)?;

            // --- Event semantics constraints (H-1) ---

            // Allocate operation_type witness and constrain it to a valid range
            // using bit decomposition. Each bit is constrained to be 0 or 1
            // (boolean constraint), and the reconstructed value must equal
            // the allocated operation_type. This ensures operation_type is in
            // [0, 2^OP_TYPE_BITS - 1] = [0, 7].
            let operation_type = FpVar::<Fr>::new_witness(cs.clone(), || {
                self.events[i]
                    .as_ref()
                    .and_then(|e| e.operation_type)
                    .ok_or(SynthesisError::AssignmentMissing)
            })?;

            // Bit decomposition: allocate OP_TYPE_BITS boolean witnesses and
            // reconstruct the value to enforce range constraint.
            let mut reconstructed = FpVar::constant(Fr::zero());
            for j in 0..OP_TYPE_BITS {
                let bit = Boolean::new_witness(cs.clone(), || {
                    let op_val = self.events[i]
                        .as_ref()
                        .and_then(|e| e.operation_type)
                        .ok_or(SynthesisError::AssignmentMissing)?;
                    let bit_val = (op_val.into_bigint().as_ref()[0] >> j) & 1;
                    Ok(bit_val == 1)
                })?;
                // Boolean constraint (bit ∈ {0,1}) is already enforced by
                // Boolean::new_witness allocation.
                let bit_val = <FpVar<Fr> as CondSelectGadget<Fr>>::conditionally_select(
                    &bit,
                    &FpVar::constant(Fr::from(1u64 << j)),
                    &FpVar::constant(Fr::zero()),
                )?;
                reconstructed += bit_val;
            }
            // Enforce that the bit decomposition reconstructs operation_type.
            // This proves operation_type ∈ [0, MAX_OPERATION_TYPE].
            reconstructed.enforce_equal(&operation_type)?;

            // Allocate payload_hash witness and bind it to event_hash and
            // operation_type. This ensures the payload hash is not arbitrary —
            // it must equal Poseidon(event_hash, operation_type), which
            // prevents a malicious prover from submitting mismatched payload
            // hashes.
            //
            // Note: future circuit versions may incorporate payload_hash into
            // the state transition constraint directly.
            let payload_hash = FpVar::<Fr>::new_witness(cs.clone(), || {
                self.events[i]
                    .as_ref()
                    .and_then(|e| e.payload_hash)
                    .ok_or(SynthesisError::AssignmentMissing)
            })?;

            let expected_payload_hash = crate::poseidon::poseidon_hash(cs.clone(), &event_hash, &operation_type)?;
            payload_hash.enforce_equal(&expected_payload_hash)?;

            // State transition constraint:
            // intermediate_root[i+1] = Poseidon(intermediate_root[i], event_hash[i])
            //
            // Uses Poseidon hash for the state transition function, ensuring
            // the same hash function is used throughout the circuit (both for
            // Merkle path verification and state updates).
            //
            // Reference: Grassi et al. (2019), "Poseidon: A New Hash Function
            // for Zero-Knowledge Proof Systems", https://eprint.iacr.org/2019/458
            let expected_next = crate::poseidon::poseidon_hash(cs.clone(), &intermediate_root_vars[i], &event_hash)?;
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
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use omnia_crypto::generate_keypair;

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

        let public_input = circuit.public_input().expect("public input should be available");
        assert_eq!(public_input.len(), 1);
        assert_eq!(public_input[0], Fr::from_be_bytes_mod_order(&new));
    }

    #[test]
    fn test_circuit_empty_public_input() {
        let circuit = RollupCircuit::empty();
        let public_input = circuit.public_input().expect("public input should be available");
        assert_eq!(public_input.len(), 1);
        assert_eq!(public_input[0], Fr::zero());
    }

    #[test]
    fn test_legacy_circuit_prove_and_verify() {
        let creator = test_node(1);
        let keypair = generate_keypair();
        let mut event = omnia_primitives::Event::genesis(creator, vec![1, 2, 3]).expect("valid genesis event");
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
        let public_input = circuit.public_input().expect("public input should be available");
        assert_eq!(public_input.len(), 3);
        assert_eq!(public_input[0], Fr::zero());
        assert_eq!(public_input[1], Fr::zero());
        assert_eq!(public_input[2], Fr::zero());
    }

    #[test]
    fn test_expanded_circuit_empty_batch_public_input() {
        let circuit = ExpandedRollupCircuit::empty(0, 0);
        let public_input = circuit.public_input().expect("public input should be available");
        assert_eq!(public_input.len(), 3);
    }

    // --- OperationType tests ---

    #[test]
    fn test_operation_type_from_u8_valid() {
        assert_eq!(OperationType::from_u8(0), Some(OperationType::Transfer));
        assert_eq!(OperationType::from_u8(1), Some(OperationType::Stake));
        assert_eq!(OperationType::from_u8(2), Some(OperationType::Unstake));
        assert_eq!(OperationType::from_u8(3), Some(OperationType::Delegate));
        assert_eq!(OperationType::from_u8(4), Some(OperationType::Slash));
        assert_eq!(OperationType::from_u8(5), Some(OperationType::GovernanceVote));
        assert_eq!(OperationType::from_u8(6), Some(OperationType::CrossShardMessage));
        assert_eq!(OperationType::from_u8(7), Some(OperationType::IdentityUpdate));
    }

    #[test]
    fn test_operation_type_from_u8_invalid() {
        assert_eq!(OperationType::from_u8(8), None);
        assert_eq!(OperationType::from_u8(255), None);
    }

    #[test]
    fn test_operation_type_to_fr() {
        assert_eq!(OperationType::Transfer.to_fr(), Fr::from(0u64));
        assert_eq!(OperationType::Stake.to_fr(), Fr::from(1u64));
        assert_eq!(OperationType::IdentityUpdate.to_fr(), Fr::from(7u64));
    }

    #[test]
    fn test_operation_type_roundtrip() {
        for i in 0u8..=7 {
            let op = OperationType::from_u8(i).expect("valid operation type");
            assert_eq!(op as u8, i);
            assert_eq!(op.to_fr(), Fr::from(i as u64));
        }
    }

    #[test]
    fn test_max_operation_type_constant() {
        assert_eq!(MAX_OPERATION_TYPE, 7);
        assert_eq!(OP_TYPE_BITS, 3);
    }

    #[test]
    fn test_for_setup_produces_non_zero_witnesses() {
        let circuit = ExpandedRollupCircuit::for_setup(4, 3);
        // All witness fields should be non-zero
        for w in &circuit.events {
            let witness = w.as_ref().expect("event witness should be Some");
            let event_hash = witness.event_hash.expect("event_hash should be Some");
            let operation_type = witness.operation_type.expect("operation_type should be Some");
            let payload_hash = witness.payload_hash.expect("payload_hash should be Some");
            assert_ne!(event_hash, Fr::zero());
            assert_ne!(operation_type, Fr::zero());
            assert_ne!(payload_hash, Fr::zero());
        }
        assert_ne!(
            circuit.old_state_root.expect("old_state_root should be Some"),
            Fr::zero()
        );
        assert_ne!(
            circuit.new_state_root.expect("new_state_root should be Some"),
            Fr::zero()
        );
        assert_ne!(
            circuit.event_commitment.expect("event_commitment should be Some"),
            Fr::zero()
        );
    }
}
