#![allow(clippy::unwrap_used)]
#![cfg(feature = "pqc")]
//! Real cryptographic verification tests for quantum commitments.
//!
//! These tests exercise the Ed25519 and Dilithium signing and verification
//! paths with real cryptographic operations — no stubs or shortcuts.
//!
//! This test file is only compiled when the `pqc` feature is enabled,
//! since it depends on `pqc_dilithium` for Dilithium keypair generation.

use omnia_binding::{BindingError, CommitmentPhase, PqPublicKey, QuantumCommitment};
use omnia_substrate::{generate_keypair, NodeKeypair};

/// Helper: generate an Ed25519 keypair and corresponding PqPublicKey.
fn ed_keypair_and_pk() -> (NodeKeypair, PqPublicKey) {
    let kp = generate_keypair();
    let pk = PqPublicKey {
        ed25519: kp.verifying_key().to_bytes(),
        dilithium: Vec::new(),
    };
    (kp, pk)
}

/// Helper: generate a Dilithium keypair and corresponding PqPublicKey
/// (with a dummy Ed25519 key since PostQuantum doesn't use it).
fn dilithium_keypair_and_pk() -> (pqc_dilithium::Keypair, PqPublicKey) {
    let kp = pqc_dilithium::Keypair::generate();
    let pk = PqPublicKey {
        ed25519: [0u8; 32],
        dilithium: kp.public.to_vec(),
    };
    (kp, pk)
}

/// Helper: generate both Ed25519 and Dilithium keypairs and a combined PqPublicKey.
fn hybrid_keypairs_and_pk() -> (NodeKeypair, pqc_dilithium::Keypair, PqPublicKey) {
    let ed_kp = generate_keypair();
    let dilithium_kp = pqc_dilithium::Keypair::generate();
    let pk = PqPublicKey {
        ed25519: ed_kp.verifying_key().to_bytes(),
        dilithium: dilithium_kp.public.to_vec(),
    };
    (ed_kp, dilithium_kp, pk)
}

// ---------------------------------------------------------------------------
// Test 1: sign_classical → verify(ClassicalOnly) returns true
// ---------------------------------------------------------------------------

#[test]
fn test_sign_classical_verifies_classical_only() {
    let (kp, pk) = ed_keypair_and_pk();
    let data = b"test data for classical signing";
    let commitment = QuantumCommitment::sign_classical(data, &kp).unwrap();
    assert!(
        commitment.verify(&pk, data, CommitmentPhase::ClassicalOnly),
        "ClassicalOnly verification should succeed with correct key and data"
    );
}

// ---------------------------------------------------------------------------
// Test 2: sign with Ed25519 → verify(Hybrid) returns false (missing Dilithium)
// ---------------------------------------------------------------------------

#[test]
fn test_sign_classical_hybrid_fails_missing_dilithium() {
    let (kp, pk) = ed_keypair_and_pk();
    let data = b"test data for hybrid check";
    let commitment = QuantumCommitment::sign_classical(data, &kp).unwrap();
    assert!(
        !commitment.verify(&pk, data, CommitmentPhase::Hybrid),
        "Hybrid verification should fail: classical-only commitment lacks Dilithium signature"
    );
}

// ---------------------------------------------------------------------------
// Test 3: sign with both → verify(Hybrid) returns true
// ---------------------------------------------------------------------------

#[test]
fn test_sign_hybrid_verifies_hybrid() {
    let (ed_kp, dilithium_kp, pk) = hybrid_keypairs_and_pk();
    let data = b"hybrid signed data";
    let commitment = QuantumCommitment::sign_hybrid(data, &ed_kp, &dilithium_kp).unwrap();
    assert!(
        commitment.verify(&pk, data, CommitmentPhase::Hybrid),
        "Hybrid verification should succeed with both signatures present"
    );
    // Also verify ClassicalOnly still works
    assert!(
        commitment.verify(&pk, data, CommitmentPhase::ClassicalOnly),
        "ClassicalOnly should also pass for a hybrid-signed commitment"
    );
}

// ---------------------------------------------------------------------------
// Test 4: tamper with data → verify() returns false
// ---------------------------------------------------------------------------

#[test]
fn test_tampered_data_fails_verification() {
    let (kp, pk) = ed_keypair_and_pk();
    let original_data = b"original data";
    let tampered_data = b"tampered data";
    let commitment = QuantumCommitment::sign_classical(original_data, &kp).unwrap();
    assert!(
        !commitment.verify(&pk, tampered_data, CommitmentPhase::ClassicalOnly),
        "Verification should fail when data is tampered"
    );
}

// ---------------------------------------------------------------------------
// Test 5: use wrong public key → verify() returns false
// ---------------------------------------------------------------------------

#[test]
fn test_wrong_public_key_fails_verification() {
    let (signing_kp, _) = ed_keypair_and_pk();
    let (_, wrong_pk) = ed_keypair_and_pk();
    let data = b"signed with different key";
    let commitment = QuantumCommitment::sign_classical(data, &signing_kp).unwrap();
    assert!(
        !commitment.verify(&wrong_pk, data, CommitmentPhase::ClassicalOnly),
        "Verification should fail with a wrong public key"
    );
}

// ---------------------------------------------------------------------------
// Test 6: use empty signature → verify() returns false
// ---------------------------------------------------------------------------

#[test]
fn test_empty_signature_fails_verification() {
    let (_kp, pk) = ed_keypair_and_pk();
    let data = b"data with empty sig";
    let hash = blake3::hash(data);
    let commitment = QuantumCommitment {
        dilithium_sig: Vec::new(),
        kyber_key: Vec::new(),
        classical_sig: Vec::new(),
        data_hash: *hash.as_bytes(),
        committed_at: omnia_substrate::VectorClock::new(),
        previous_hash: [0u8; 32],
    };
    assert!(
        !commitment.verify(&pk, data, CommitmentPhase::ClassicalOnly),
        "Verification should fail with an empty signature"
    );
}

// ---------------------------------------------------------------------------
// Test 7: PostQuantum signing and verification
// ---------------------------------------------------------------------------

#[test]
fn test_sign_post_quantum_verifies() {
    let (dilithium_kp, pk) = dilithium_keypair_and_pk();
    let data = b"post-quantum data";
    let commitment = QuantumCommitment::sign_post_quantum(data, &dilithium_kp).unwrap();
    assert!(
        commitment.verify(&pk, data, CommitmentPhase::PostQuantum),
        "PostQuantum verification should succeed"
    );
    // ClassicalOnly should fail because there's no Ed25519 signature
    assert!(
        !commitment.verify(&pk, data, CommitmentPhase::ClassicalOnly),
        "ClassicalOnly should fail for a post-quantum-only commitment"
    );
}

// ---------------------------------------------------------------------------
// Test 8: PostQuantum with wrong Dilithium public key fails
// ---------------------------------------------------------------------------

#[test]
fn test_post_quantum_wrong_key_fails() {
    let (dilithium_kp, _) = dilithium_keypair_and_pk();
    let (_, wrong_pk) = dilithium_keypair_and_pk();
    let data = b"post-quantum wrong key";
    let commitment = QuantumCommitment::sign_post_quantum(data, &dilithium_kp).unwrap();
    assert!(
        !commitment.verify(&wrong_pk, data, CommitmentPhase::PostQuantum),
        "PostQuantum verification should fail with wrong Dilithium public key"
    );
}

// ---------------------------------------------------------------------------
// Test 9: Hybrid with wrong Ed25519 key fails
// ---------------------------------------------------------------------------

#[test]
fn test_hybrid_wrong_ed25519_key_fails() {
    let (_ed_kp, dilithium_kp, pk) = hybrid_keypairs_and_pk();
    let (wrong_ed_kp, _) = ed_keypair_and_pk();

    // Sign with one Ed25519 key, verify with another
    let data = b"hybrid wrong ed key";
    let commitment = QuantumCommitment::sign_hybrid(data, &wrong_ed_kp, &dilithium_kp).unwrap();
    assert!(
        !commitment.verify(&pk, data, CommitmentPhase::Hybrid),
        "Hybrid verification should fail with wrong Ed25519 key"
    );
}

// ---------------------------------------------------------------------------
// Test 10: Data hash mismatch takes priority over signature check
// ---------------------------------------------------------------------------

#[test]
fn test_hash_mismatch_priority() {
    let (kp, pk) = ed_keypair_and_pk();
    let data = b"correct data";
    let wrong_data = b"wrong data";
    let commitment = QuantumCommitment::sign_classical(data, &kp).unwrap();
    // Hash mismatch should fail even if signature would be valid
    assert!(
        !commitment.verify(&pk, wrong_data, CommitmentPhase::ClassicalOnly),
        "Hash mismatch should cause verification to fail immediately"
    );
}

// ---------------------------------------------------------------------------
// Test 11: BindingError variants exist and are displayable
// ---------------------------------------------------------------------------

#[test]
fn test_binding_error_variants() {
    let err = BindingError::Dilithium("test error".to_string());
    assert!(err.to_string().contains("test error"));

    let err = BindingError::InvalidSignatureLength {
        expected: 64,
        actual: 32,
    };
    assert!(err.to_string().contains("64"));
    assert!(err.to_string().contains("32"));

    let err = BindingError::InvalidPublicKey("bad key".to_string());
    assert!(err.to_string().contains("bad key"));

    let err = BindingError::SigningFailed("sign fail".to_string());
    assert!(err.to_string().contains("sign fail"));
}
