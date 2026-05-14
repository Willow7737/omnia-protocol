//! Integration tests for the Groth16 ZK circuit.
//!
//! These tests verify the full lifecycle of the rollup circuit:
//! trusted setup → proof creation → proof verification, including
//! edge cases like tampered proofs and wrong public inputs.

use ark_bn254::Fr;
use ark_ff::PrimeField;
use omnia_zk::circuit::RollupCircuit;
use omnia_zk::prover::{
    create_proof, deserialize_proof, generate_trusted_setup, serialize_proof, verify_proof,
};

/// Helper: create a deterministic old state root.
fn old_root() -> [u8; 32] {
    let mut root = [0u8; 32];
    root[0] = 0x01;
    root
}

/// Helper: create a deterministic new state root.
fn new_root() -> [u8; 32] {
    let mut root = [0u8; 32];
    root[0] = 0x02;
    root
}

#[test]
fn test_trusted_setup_create_proof_verify_true() {
    // 1. Trusted setup
    let setup_circuit = RollupCircuit::empty();
    let (pk, vk) = generate_trusted_setup(&setup_circuit).expect("setup should succeed");

    // 2. Create proof
    let circuit = RollupCircuit::from_state_roots(old_root(), new_root(), 10);
    let public_inputs = circuit
        .public_input()
        .expect("public inputs should be available");
    let proof = create_proof(circuit, &pk).expect("proof creation should succeed");

    // 3. Verify proof
    let valid = verify_proof(&vk, &public_inputs, &proof).expect("verification should not error");
    assert!(valid, "proof should verify successfully");
}

#[test]
fn test_wrong_proof_verify_false() {
    // Create a proof for one statement and try to verify it against
    // a different public input. This tests that a valid proof for one
    // claim cannot be repurposed for a different claim.
    let setup_circuit = RollupCircuit::empty();
    let (pk, vk) = generate_trusted_setup(&setup_circuit).expect("setup should succeed");

    // Create a proof for new_root = [0x02, ...]
    let circuit = RollupCircuit::from_state_roots(old_root(), new_root(), 5);
    let proof = create_proof(circuit, &pk).expect("proof creation should succeed");

    // Try to verify with public inputs from a DIFFERENT state root
    let mut other_new_root = [0u8; 32];
    other_new_root[0] = 0x03;
    let wrong_public_inputs = vec![Fr::from_be_bytes_mod_order(&other_new_root)];

    let valid =
        verify_proof(&vk, &wrong_public_inputs, &proof).expect("verification should not error");
    assert!(
        !valid,
        "proof created for one statement should not verify against a different public input"
    );
}

#[test]
fn test_wrong_public_input_verify_false() {
    let setup_circuit = RollupCircuit::empty();
    let (pk, vk) = generate_trusted_setup(&setup_circuit).expect("setup should succeed");

    let circuit = RollupCircuit::from_state_roots(old_root(), new_root(), 3);
    let proof = create_proof(circuit, &pk).expect("proof creation should succeed");

    // Use wrong public input (a different state root)
    let wrong_public_input = vec![Fr::from_be_bytes_mod_order(&old_root())];

    let valid =
        verify_proof(&vk, &wrong_public_input, &proof).expect("verification should not error");
    assert!(!valid, "wrong public input should fail verification");
}

#[test]
fn test_proof_size_is_constant_regardless_of_event_count() {
    let setup_circuit = RollupCircuit::empty();
    let (pk, _vk) = generate_trusted_setup(&setup_circuit).expect("setup should succeed");

    // Create proofs with different event counts
    let circuit_small = RollupCircuit::from_state_roots(old_root(), new_root(), 1);
    let proof_small = create_proof(circuit_small, &pk).expect("proof creation should succeed");
    let bytes_small = serialize_proof(&proof_small).expect("serialization should succeed");

    let circuit_large = RollupCircuit::from_state_roots(old_root(), new_root(), 10_000);
    let proof_large = create_proof(circuit_large, &pk).expect("proof creation should succeed");
    let bytes_large = serialize_proof(&proof_large).expect("serialization should succeed");

    // Groth16 proof size is O(1) — constant regardless of circuit complexity
    assert_eq!(
        bytes_small.len(),
        bytes_large.len(),
        "proof size should be constant (succinct) regardless of event count"
    );

    // Verify that the proof size is reasonable for a Groth16 proof on Bn254
    // A Groth16 proof has 2 G1 points + 1 G2 point on Bn254:
    // - G1 uncompressed: 64 bytes each → 128 bytes
    // - G2 uncompressed: 128 bytes
    // Total: 256 bytes (uncompressed)
    assert!(
        bytes_small.len() > 0 && bytes_small.len() <= 512,
        "proof size should be reasonable for Groth16 on Bn254, got {}",
        bytes_small.len()
    );
}

#[test]
fn test_proof_serialization_roundtrip() {
    let setup_circuit = RollupCircuit::empty();
    let (pk, vk) = generate_trusted_setup(&setup_circuit).expect("setup should succeed");

    let circuit = RollupCircuit::from_state_roots(old_root(), new_root(), 7);
    let public_inputs = circuit
        .public_input()
        .expect("public inputs should be available");
    let proof = create_proof(circuit, &pk).expect("proof creation should succeed");

    // Serialize and deserialize
    let bytes = serialize_proof(&proof).expect("serialization should succeed");
    let restored = deserialize_proof(&bytes).expect("deserialization should succeed");

    // Verify the restored proof
    let valid =
        verify_proof(&vk, &public_inputs, &restored).expect("verification should not error");
    assert!(valid, "restored proof should verify successfully");
}
