//! AUDIT-2026-07 C9 (#347) regression tests: the ZK verifying key must
//! come from the node's VK registry, never from caller-supplied bytes.
//!
//! The attack these tests lock out: an attacker builds a trivial circuit
//! whose only "statement" is a value they already know (the public
//! binding hash is derived from public identifiers), runs their own
//! trusted setup, and submits their own (vk, proof) pair. Under the old
//! wire format the shard deserialized the attacker's VK from the proof
//! bytes and Groth16 verification passed — a full privacy bypass on the
//! biological shard. With the registry, the same submission dies on
//! "unknown circuit" because only node-operator-registered VKs verify.
//!
//! These tests require the `real_verification` feature (CI runs them in
//! the --all-features matrix):
//!     cargo test -p omnia-shards --features real_verification --test zk_vk_registry
#![cfg(feature = "real_verification")]
#![allow(clippy::unwrap_used)]

use ark_bn254::{Bn254, Fr};
use ark_groth16::Groth16;
use ark_relations::lc;
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError, Variable};
use ark_serialize::CanonicalSerialize;
use ark_snark::SNARK;
use ark_std::rand::rngs::StdRng;
use ark_std::rand::SeedableRng;

use omnia_shards::zk::{self, groth16 as zkg};
use omnia_shards::{BiologicalOp, BiologicalState, ComputationalOp, ComputationalState};
use omnia_substrate::VectorClock;

/// A minimal circuit with one public input `x` and one witness `w`,
/// enforcing `w * 1 = x`. This is deliberately the *weakest possible*
/// statement: anyone who knows the public input (which is derived from
/// public identifiers) can produce a valid proof. It stands in for the
/// attacker's trivial circuit in the bypass tests — and, once its VK is
/// registered, doubles as the "canonical" circuit for happy-path tests.
#[derive(Clone)]
struct TrivialBindingCircuit {
    value: Option<Fr>,
}

impl ConstraintSynthesizer<Fr> for TrivialBindingCircuit {
    fn generate_constraints(self, cs: ConstraintSystemRef<Fr>) -> Result<(), SynthesisError> {
        let x = cs.new_input_variable(|| self.value.ok_or(SynthesisError::AssignmentMissing))?;
        let w = cs.new_witness_variable(|| self.value.ok_or(SynthesisError::AssignmentMissing))?;
        cs.enforce_constraint(lc!() + w, lc!() + Variable::One, lc!() + x)?;
        Ok(())
    }
}

/// Run a circuit-specific trusted setup and return (vk_bytes, prove_fn).
fn setup(seed: u64) -> (Vec<u8>, ark_groth16::ProvingKey<Bn254>) {
    let mut rng = StdRng::seed_from_u64(seed);
    let (pk, vk) = Groth16::<Bn254>::circuit_specific_setup(TrivialBindingCircuit { value: None }, &mut rng).unwrap();
    let mut vk_bytes = Vec::new();
    vk.serialize_uncompressed(&mut vk_bytes).unwrap();
    (vk_bytes, pk)
}

/// Prove knowledge of `value` for the trivial circuit and return the
/// uncompressed proof bytes.
fn prove(pk: &ark_groth16::ProvingKey<Bn254>, value: Fr, seed: u64) -> Vec<u8> {
    let mut rng = StdRng::seed_from_u64(seed);
    let proof = Groth16::<Bn254>::prove(pk, TrivialBindingCircuit { value: Some(value) }, &mut rng).unwrap();
    let mut proof_bytes = Vec::new();
    proof.serialize_uncompressed(&mut proof_bytes).unwrap();
    proof_bytes
}

/// Build a `[circuit_id || proof]` submission.
fn submission(circuit_id: &[u8; 32], proof_bytes: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(32 + proof_bytes.len());
    bytes.extend_from_slice(circuit_id);
    bytes.extend_from_slice(proof_bytes);
    bytes
}

fn test_vc() -> VectorClock {
    VectorClock::with_node([1u8; 32], 1)
}

/// Grant (subject → consumer) consent so QueryWithZkProof reaches the
/// proof-verification step.
fn biological_state_with_consent(subject: [u8; 32], consumer: [u8; 32]) -> BiologicalState {
    let mut state = BiologicalState::new();
    state
        .apply(
            &BiologicalOp::GrantAccess {
                subject,
                consumer,
                scope: "lab-results".into(),
                expires_at: 0,
            },
            &test_vc(),
            Some(&subject),
        )
        .unwrap();
    state
}

// ---------------------------------------------------------------------------
// The audit's attack scenario (red-checkable against the old code)
// ---------------------------------------------------------------------------

/// THE C9 regression: an attacker-generated (vk, proof) pair for a
/// trivial circuit, submitted with the attacker's own circuit ID, must
/// be rejected — the attacker's circuit is not in the registry. Under
/// the old embedded-VK wire format this exact proof passed verification.
#[test]
fn attacker_supplied_vk_is_rejected() {
    let subject = [0xA1; 32];
    let consumer = [0xB2; 32];
    let mut state = biological_state_with_consent(subject, consumer);

    // Attacker runs their own setup (a seed nobody registered) and
    // proves the trivial statement for the correct public binding —
    // which they CAN do, because the binding hash is public knowledge.
    let (attacker_vk, attacker_pk) = setup(0xBAD5EED);
    let binding = zkg::biological_public_input(&subject, &consumer);
    let proof_bytes = prove(&attacker_pk, binding, 7);
    let attacker_circuit_id = zk::circuit_id_for_vk(&attacker_vk);

    let result = state.apply(
        &BiologicalOp::QueryWithZkProof {
            subject,
            consumer,
            zk_proof: submission(&attacker_circuit_id, &proof_bytes),
            query: "is my trivial circuit accepted?".into(),
        },
        &test_vc(),
        Some(&consumer),
    );

    let err = result.expect_err("attacker-chosen circuit must not verify");
    assert!(
        err.to_string().contains("unknown circuit"),
        "rejection must be the registry lookup, got: {err}"
    );
}

/// Legacy `[vk_len || vk || proof]` submissions (the old embedded-VK
/// format) must fail closed: the first 32 bytes parse as a garbage
/// circuit ID that is never registered.
#[test]
fn legacy_embedded_vk_format_is_rejected() {
    let subject = [0xA3; 32];
    let consumer = [0xB4; 32];
    let mut state = biological_state_with_consent(subject, consumer);

    let (attacker_vk, attacker_pk) = setup(0xDEAD);
    let binding = zkg::biological_public_input(&subject, &consumer);
    let proof_bytes = prove(&attacker_pk, binding, 11);

    // Old wire format: [vk_len u32 LE || vk || proof].
    let mut legacy = (attacker_vk.len() as u32).to_le_bytes().to_vec();
    legacy.extend_from_slice(&attacker_vk);
    legacy.extend_from_slice(&proof_bytes);

    let result = state.apply(
        &BiologicalOp::QueryWithZkProof {
            subject,
            consumer,
            zk_proof: legacy,
            query: "legacy format".into(),
        },
        &test_vc(),
        Some(&consumer),
    );
    assert!(result.is_err(), "legacy embedded-VK submissions must be rejected");
}

// ---------------------------------------------------------------------------
// Registry behavior
// ---------------------------------------------------------------------------

#[test]
fn registration_rejects_malformed_vk() {
    let err = zk::register_circuit_vk(&[0x00; 64]).expect_err("garbage bytes are not a verifying key");
    assert!(err.to_string().contains("malformed verifying key"));
}

#[test]
fn registration_is_idempotent_and_content_addressed() {
    let (vk_bytes, _) = setup(42);
    let id1 = zk::register_circuit_vk(&vk_bytes).unwrap();
    let id2 = zk::register_circuit_vk(&vk_bytes).unwrap();
    assert_eq!(id1, id2);
    assert_eq!(id1, zk::circuit_id_for_vk(&vk_bytes));
    assert_eq!(zk::lookup_circuit_vk(&id1).unwrap(), vk_bytes);
}

// ---------------------------------------------------------------------------
// Happy paths: registered circuits verify, wrong bindings do not
// ---------------------------------------------------------------------------

/// With the circuit registered, a proof bound to the right
/// (subject, consumer) pair verifies on the biological shard.
#[test]
fn registered_circuit_with_correct_binding_verifies_biological() {
    let subject = [0xA5; 32];
    let consumer = [0xB6; 32];
    let mut state = biological_state_with_consent(subject, consumer);

    let (vk_bytes, pk) = setup(1001);
    let circuit_id = zk::register_circuit_vk(&vk_bytes).unwrap();
    let binding = zkg::biological_public_input(&subject, &consumer);
    let proof_bytes = prove(&pk, binding, 13);

    state
        .apply(
            &BiologicalOp::QueryWithZkProof {
                subject,
                consumer,
                zk_proof: submission(&circuit_id, &proof_bytes),
                query: "registered circuit".into(),
            },
            &test_vc(),
            Some(&consumer),
        )
        .expect("registered circuit + correct binding must verify");
}

/// A valid proof for one (subject, consumer) pair must not verify when
/// replayed against a different pair — the public-input binding holds.
#[test]
fn proof_bound_to_other_pair_is_rejected() {
    let subject = [0xA7; 32];
    let consumer = [0xB8; 32];
    let other_consumer = [0xC9; 32];
    let mut state = biological_state_with_consent(subject, consumer);

    let (vk_bytes, pk) = setup(2002);
    let circuit_id = zk::register_circuit_vk(&vk_bytes).unwrap();
    // Proof bound to (subject, other_consumer), replayed against
    // (subject, consumer).
    let wrong_binding = zkg::biological_public_input(&subject, &other_consumer);
    let proof_bytes = prove(&pk, wrong_binding, 17);

    let result = state.apply(
        &BiologicalOp::QueryWithZkProof {
            subject,
            consumer,
            zk_proof: submission(&circuit_id, &proof_bytes),
            query: "replayed proof".into(),
        },
        &test_vc(),
        Some(&consumer),
    );
    let err = result.expect_err("proof bound to a different pair must not verify");
    assert!(
        err.to_string().contains("proof is invalid"),
        "rejection must be the verification equation, got: {err}"
    );
}

/// End-to-end computational flow: SubmitTask → SubmitProof (with a
/// registry submission bound to the task) → VerifyProof reaches
/// Verified. This path was previously non-functional — the hardcoded
/// empty public-input list rejected every submission.
#[test]
fn computational_verify_reaches_verified_with_registered_circuit() {
    let mut state = ComputationalState::new();
    let vc = test_vc();
    let task_id = [0x11; 32];
    let spec = b"matmul:2048x2048:fp16".to_vec();

    state
        .apply(
            &ComputationalOp::SubmitTask {
                task_id,
                spec: spec.clone(),
                reward: 500,
            },
            &vc,
        )
        .unwrap();

    let (vk_bytes, pk) = setup(3003);
    let circuit_id = zk::register_circuit_vk(&vk_bytes).unwrap();
    let binding = zkg::computational_public_input(&task_id, &spec);
    let proof_bytes = prove(&pk, binding, 19);

    state
        .apply(
            &ComputationalOp::SubmitProof {
                task_id,
                proof: submission(&circuit_id, &proof_bytes),
            },
            &vc,
        )
        .unwrap();

    state
        .apply(&ComputationalOp::VerifyProof { task_id }, &vc)
        .expect("verification must succeed for a registered circuit bound to this task");

    let task = state.tasks.get(&task_id).unwrap();
    assert_eq!(task.status, omnia_shards::TaskStatus::Verified);
}

/// A computational proof bound to task A must not verify for task B.
#[test]
fn computational_proof_is_bound_to_its_task() {
    let mut state = ComputationalState::new();
    let vc = test_vc();
    let task_a = [0x21; 32];
    let task_b = [0x22; 32];
    let spec = b"shared-spec".to_vec();

    for task_id in [task_a, task_b] {
        state
            .apply(
                &ComputationalOp::SubmitTask {
                    task_id,
                    spec: spec.clone(),
                    reward: 100,
                },
                &vc,
            )
            .unwrap();
    }

    let (vk_bytes, pk) = setup(4004);
    let circuit_id = zk::register_circuit_vk(&vk_bytes).unwrap();
    // Proof bound to task A, submitted for task B.
    let binding_a = zkg::computational_public_input(&task_a, &spec);
    let proof_bytes = prove(&pk, binding_a, 23);

    state
        .apply(
            &ComputationalOp::SubmitProof {
                task_id: task_b,
                proof: submission(&circuit_id, &proof_bytes),
            },
            &vc,
        )
        .unwrap();

    let result = state.apply(&ComputationalOp::VerifyProof { task_id: task_b }, &vc);
    assert!(result.is_err(), "proof bound to task A must not verify for task B");
    let task = state.tasks.get(&task_b).unwrap();
    assert_eq!(task.status, omnia_shards::TaskStatus::Failed);
}
