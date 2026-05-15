//! Integration tests for the expanded Groth16 ZK circuit.
//!
//! These tests verify the full lifecycle of the expanded rollup circuit:
//! trusted setup -> proof creation -> proof verification, including
//! edge cases like tampered events, wrong intermediate roots,
//! single-event batches, empty batches, and proof size consistency.

use ark_bn254::Fr;
use ark_ff::Zero;
use omnia_zk::circuit::ExpandedRollupCircuit;
use omnia_zk::merkle::{fr_to_hash, poseidon_hash_to_fr, MerkleProof};
use omnia_zk::prover::{
    create_expanded_proof, generate_trusted_setup_expanded, serialize_proof, verify_proof,
};

/// Fixed Merkle proof depth used in tests.
const MERKLE_DEPTH: usize = 3;

/// Build a Poseidon-based Merkle tree from event hashes.
///
/// Constructs a binary Merkle tree where each internal node is computed as
/// `Poseidon(left_child, right_child)` using [`poseidon_hash_to_fr`]. Leaves
/// are padded with `Fr::zero()` to fill `2^depth` slots.
///
/// # Returns
///
/// A tuple of `(root, proofs)` where `root` is the Merkle root as an `Fr`
/// element, and `proofs` contains an inclusion proof for each event hash.
fn build_poseidon_merkle_tree(event_hashes: &[Fr], depth: usize) -> (Fr, Vec<MerkleProof>) {
    let num_leaves = 1usize << depth;
    assert!(
        event_hashes.len() <= num_leaves,
        "too many events for merkle depth"
    );

    // Initialize leaves, padding with zeros
    let mut current_level: Vec<Fr> = event_hashes.to_vec();
    current_level.resize(num_leaves, Fr::zero());

    // Build the tree bottom-up, storing each level for proof generation
    let mut levels: Vec<Vec<Fr>> = vec![current_level.clone()];
    while current_level.len() > 1 {
        let mut next_level = Vec::new();
        for i in (0..current_level.len()).step_by(2) {
            let left = current_level[i];
            let right = current_level[i + 1];
            next_level.push(poseidon_hash_to_fr(left, right));
        }
        current_level = next_level.clone();
        levels.push(next_level);
    }

    let root = current_level[0];

    // Generate proofs for each event
    let mut proofs = Vec::new();
    for (idx, _) in event_hashes.iter().enumerate() {
        let mut siblings = Vec::new();
        let mut directions = Vec::new();
        let mut pos = idx;

        for level in 0..depth {
            let sibling_pos = if pos % 2 == 0 { pos + 1 } else { pos - 1 };
            let sibling = levels[level][sibling_pos];
            siblings.push(fr_to_hash(&sibling));
            // go_left = true means "sibling is on the left".
            // If current position is odd (right child), sibling is on the left.
            directions.push(pos % 2 == 1);
            pos /= 2;
        }

        proofs.push(MerkleProof {
            siblings,
            directions,
        });
    }

    (root, proofs)
}

/// Helper: build a consistent test batch for `num_events` events.
///
/// Creates event hashes, Merkle proofs, intermediate roots, and a commitment
/// such that all circuit constraints are satisfied. The Merkle proofs are
/// constructed using Poseidon hash (matching the on-circuit hash function),
/// and intermediate roots are computed using Poseidon state transitions.
///
/// # Returns
///
/// A tuple of `(circuit, public_inputs)`.
fn build_valid_batch(num_events: usize) -> (ExpandedRollupCircuit, Vec<Fr>) {
    if num_events == 0 {
        let old_root = Fr::from(42u64);
        let new_root = old_root; // Empty batch: old_root == new_root
        let event_commitment = Fr::zero();

        let circuit = ExpandedRollupCircuit::from_batch(
            old_root,
            new_root,
            vec![],
            event_commitment,
            vec![],
            vec![],
        );
        let public_inputs = circuit
            .public_input()
            .expect("public inputs should be available");
        return (circuit, public_inputs);
    }

    // Generate distinct event hashes (small non-zero values)
    let event_hashes: Vec<Fr> = (0..num_events).map(|i| Fr::from((i as u64) + 10)).collect();

    // Build Poseidon-based Merkle tree — the root is the event_commitment
    let (event_commitment, merkle_proofs) = build_poseidon_merkle_tree(&event_hashes, MERKLE_DEPTH);

    // Compute intermediate roots using Poseidon hash:
    //   intermediate_roots[0] = old_root
    //   intermediate_roots[i+1] = Poseidon(intermediate_roots[i], event_hash[i])
    //   new_root = intermediate_roots[num_events]
    let old_root = Fr::from(42u64);
    let mut intermediate_roots = vec![old_root];
    let mut current_root = old_root;
    for event_hash in &event_hashes {
        current_root = poseidon_hash_to_fr(current_root, *event_hash);
        intermediate_roots.push(current_root);
    }
    let new_root = intermediate_roots[num_events];

    let circuit = ExpandedRollupCircuit::from_batch(
        old_root,
        new_root,
        event_hashes,
        event_commitment,
        merkle_proofs,
        intermediate_roots,
    );
    let public_inputs = circuit
        .public_input()
        .expect("public inputs should be available");
    (circuit, public_inputs)
}

/// Helper: build a valid batch with explicit event hashes.
///
/// Similar to [`build_valid_batch`] but allows specifying the event hashes
/// directly. Useful for testing that different event data produces
/// non-interchangeable proofs.
fn build_batch_with_hashes(event_hashes: Vec<Fr>) -> (ExpandedRollupCircuit, Vec<Fr>) {
    let num_events = event_hashes.len();
    if num_events == 0 {
        return build_valid_batch(0);
    }

    let (event_commitment, merkle_proofs) = build_poseidon_merkle_tree(&event_hashes, MERKLE_DEPTH);

    let old_root = Fr::from(42u64);
    let mut intermediate_roots = vec![old_root];
    let mut current_root = old_root;
    for event_hash in &event_hashes {
        current_root = poseidon_hash_to_fr(current_root, *event_hash);
        intermediate_roots.push(current_root);
    }
    let new_root = intermediate_roots[num_events];

    let circuit = ExpandedRollupCircuit::from_batch(
        old_root,
        new_root,
        event_hashes,
        event_commitment,
        merkle_proofs,
        intermediate_roots,
    );
    let public_inputs = circuit
        .public_input()
        .expect("public inputs should be available");
    (circuit, public_inputs)
}

// ---------------------------------------------------------------------------
// Test 1: Valid batch -> proof verifies
// ---------------------------------------------------------------------------

#[test]
fn test_valid_batch_proof_verifies() {
    let num_events = 3;
    let (pk, vk) = generate_trusted_setup_expanded(num_events, MERKLE_DEPTH)
        .expect("expanded trusted setup should succeed");

    let (circuit, public_inputs) = build_valid_batch(num_events);
    let proof = create_expanded_proof(circuit, &pk).expect("proof creation should succeed");

    let valid = verify_proof(&vk, &public_inputs, &proof).expect("verification should not error");
    assert!(valid, "valid batch proof should verify successfully");
}

// ---------------------------------------------------------------------------
// Test 2: Tampered event -> proof fails (different event hash)
// ---------------------------------------------------------------------------

#[test]
fn test_tampered_event_proof_fails() {
    let num_events = 3;
    let (pk, vk) = generate_trusted_setup_expanded(num_events, MERKLE_DEPTH)
        .expect("expanded trusted setup should succeed");

    // Build a valid batch with original event hashes
    let original_hashes = vec![Fr::from(10u64), Fr::from(11u64), Fr::from(12u64)];
    let (circuit_original, _public_inputs_original) = build_batch_with_hashes(original_hashes);

    // Build a valid batch with a different second event hash
    let tampered_hashes = vec![Fr::from(10u64), Fr::from(99u64), Fr::from(12u64)];
    let (_circuit_tampered, public_inputs_tampered) = build_batch_with_hashes(tampered_hashes);

    // Generate proof for the original batch
    let proof =
        create_expanded_proof(circuit_original, &pk).expect("proof creation should succeed");

    // The two batches have different new_roots and different event commitments
    // (because the Merkle paths depend on the event hashes). Verifying the
    // original proof against the tampered batch's public inputs should fail.
    let valid =
        verify_proof(&vk, &public_inputs_tampered, &proof).expect("verification should not error");
    assert!(
        !valid,
        "proof for one batch should not verify against a different batch's public inputs"
    );
}

// ---------------------------------------------------------------------------
// Test 3: Wrong intermediate root -> proof fails
// ---------------------------------------------------------------------------

#[test]
fn test_wrong_intermediate_root_proof_fails() {
    let num_events = 3;
    let (pk, vk) = generate_trusted_setup_expanded(num_events, MERKLE_DEPTH)
        .expect("expanded trusted setup should succeed");

    // Build a valid batch
    let (circuit, public_inputs) = build_valid_batch(num_events);

    // Create wrong public inputs with a different new_root. This simulates
    // the scenario where an attacker claims a different state transition
    // (i.e., different intermediate/final roots) than what actually occurred.
    let mut wrong_public_inputs = public_inputs.clone();
    wrong_public_inputs[1] = wrong_public_inputs[1] + Fr::from(777u64); // Corrupt new_root

    // Generate proof for the valid batch
    let proof = create_expanded_proof(circuit, &pk).expect("proof creation should succeed");

    // The proof should not verify against the wrong public inputs
    let valid =
        verify_proof(&vk, &wrong_public_inputs, &proof).expect("verification should not error");
    assert!(
        !valid,
        "proof should not verify against public inputs with a wrong new_root (state transition)"
    );
}

// ---------------------------------------------------------------------------
// Test 3b: Circuit rejects inconsistent intermediate roots
// ---------------------------------------------------------------------------

#[test]
fn test_circuit_rejects_inconsistent_intermediate_roots() {
    // This test verifies that the circuit enforces the relationship between
    // intermediate roots and event hashes. When the constraint system is
    // given inconsistent intermediate roots, proof generation should fail
    // (because the constraints are not satisfied).
    //
    // We use std::panic::catch_unwind to verify the prover panics, which
    // demonstrates that the constraint system is enforced.
    let num_events = 2;

    let event_hashes: Vec<Fr> = vec![Fr::from(10u64), Fr::from(11u64)];

    // Build proper Poseidon-based Merkle proofs
    let (event_commitment, merkle_proofs) = build_poseidon_merkle_tree(&event_hashes, MERKLE_DEPTH);

    let old_root = Fr::from(42u64);

    // Compute CORRECT intermediate roots using Poseidon hash
    let mut intermediate_roots = vec![old_root];
    let mut current_root = old_root;
    for event_hash in &event_hashes {
        current_root = poseidon_hash_to_fr(current_root, *event_hash);
        intermediate_roots.push(current_root);
    }
    let new_root = intermediate_roots[num_events];

    // CORRUPT an intermediate root (index 2)
    let mut wrong_roots = intermediate_roots.clone();
    wrong_roots[2] = wrong_roots[2] + Fr::from(777u64);

    let bad_circuit = ExpandedRollupCircuit::from_batch(
        old_root,
        new_root,
        event_hashes,
        event_commitment,
        merkle_proofs,
        wrong_roots,
    );

    let (pk, _vk) =
        generate_trusted_setup_expanded(num_events, MERKLE_DEPTH).expect("setup should succeed");

    // The prover should panic because the constraints are not satisfied.
    // This demonstrates that the circuit enforces intermediate root consistency.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = create_expanded_proof(bad_circuit, &pk);
    }));
    assert!(
        result.is_err(),
        "prover should panic when intermediate roots are inconsistent (constraints violated)"
    );
}

// ---------------------------------------------------------------------------
// Test 4: Single-event batch -> proof verifies
// ---------------------------------------------------------------------------

#[test]
fn test_single_event_batch_proof_verifies() {
    let num_events = 1;
    let (pk, vk) = generate_trusted_setup_expanded(num_events, MERKLE_DEPTH)
        .expect("expanded trusted setup should succeed");

    let (circuit, public_inputs) = build_valid_batch(num_events);
    let proof = create_expanded_proof(circuit, &pk).expect("proof creation should succeed");

    let valid = verify_proof(&vk, &public_inputs, &proof).expect("verification should not error");
    assert!(valid, "single-event batch proof should verify successfully");
}

// ---------------------------------------------------------------------------
// Test 5: Empty batch -> old_root == new_root -> proof verifies
// ---------------------------------------------------------------------------

#[test]
fn test_empty_batch_proof_verifies() {
    let num_events = 0;
    let (pk, vk) = generate_trusted_setup_expanded(num_events, 0)
        .expect("expanded trusted setup should succeed");

    let (circuit, public_inputs) = build_valid_batch(num_events);
    let proof = create_expanded_proof(circuit, &pk).expect("proof creation should succeed");

    let valid = verify_proof(&vk, &public_inputs, &proof).expect("verification should not error");
    assert!(
        valid,
        "empty batch proof should verify successfully when old_root == new_root"
    );
}

// ---------------------------------------------------------------------------
// Test 6: Proof size is O(1) regardless of event count
// ---------------------------------------------------------------------------

#[test]
fn test_proof_size_is_constant_regardless_of_event_count() {
    // Use different batch sizes to verify succinctness
    let small_events = 1;
    let large_events = 5;

    let (pk_small, _vk_small) = generate_trusted_setup_expanded(small_events, MERKLE_DEPTH)
        .expect("setup for small batch should succeed");
    let (pk_large, _vk_large) = generate_trusted_setup_expanded(large_events, MERKLE_DEPTH)
        .expect("setup for large batch should succeed");

    let (circuit_small, _) = build_valid_batch(small_events);
    let (circuit_large, _) = build_valid_batch(large_events);

    let proof_small =
        create_expanded_proof(circuit_small, &pk_small).expect("small batch proof should succeed");
    let proof_large =
        create_expanded_proof(circuit_large, &pk_large).expect("large batch proof should succeed");

    let bytes_small =
        serialize_proof(&proof_small).expect("small proof serialization should succeed");
    let bytes_large =
        serialize_proof(&proof_large).expect("large proof serialization should succeed");

    // Groth16 proof size is O(1) — constant regardless of circuit complexity
    assert_eq!(
        bytes_small.len(),
        bytes_large.len(),
        "proof size should be constant (succinct) regardless of event count"
    );

    // Verify that the proof size is reasonable for a Groth16 proof on Bn254
    // A Groth16 proof has 2 G1 points + 1 G2 point on Bn254:
    // - G1 uncompressed: 64 bytes each -> 128 bytes
    // - G2 uncompressed: 128 bytes
    // Total: 256 bytes (uncompressed)
    assert!(
        bytes_small.len() > 0 && bytes_small.len() <= 512,
        "proof size should be reasonable for Groth16 on Bn254, got {}",
        bytes_small.len()
    );
}

// ---------------------------------------------------------------------------
// Test 7: Wrong public input (different new_root) -> proof fails
// ---------------------------------------------------------------------------

#[test]
fn test_wrong_public_input_proof_fails() {
    let num_events = 2;
    let (pk, vk) = generate_trusted_setup_expanded(num_events, MERKLE_DEPTH)
        .expect("expanded trusted setup should succeed");

    let (circuit, public_inputs) = build_valid_batch(num_events);
    let proof = create_expanded_proof(circuit, &pk).expect("proof creation should succeed");

    // Create wrong public inputs with a different new_root
    let mut wrong_inputs = public_inputs.clone();
    wrong_inputs[1] = wrong_inputs[1] + Fr::from(1u64); // Corrupt new_root

    let valid = verify_proof(&vk, &wrong_inputs, &proof).expect("verification should not error");
    assert!(
        !valid,
        "proof should not verify against wrong public inputs"
    );
}

// ---------------------------------------------------------------------------
// Test 8: Proof serialization roundtrip
// ---------------------------------------------------------------------------

#[test]
fn test_expanded_proof_serialization_roundtrip() {
    let num_events = 2;
    let (pk, vk) = generate_trusted_setup_expanded(num_events, MERKLE_DEPTH)
        .expect("expanded trusted setup should succeed");

    let (circuit, public_inputs) = build_valid_batch(num_events);
    let proof = create_expanded_proof(circuit, &pk).expect("proof creation should succeed");

    // Serialize and deserialize
    let bytes = serialize_proof(&proof).expect("serialization should succeed");
    let restored =
        omnia_zk::prover::deserialize_proof(&bytes).expect("deserialization should succeed");

    // Verify the restored proof
    let valid =
        verify_proof(&vk, &public_inputs, &restored).expect("verification should not error");
    assert!(valid, "restored proof should verify successfully");
}
