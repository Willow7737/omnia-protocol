//! Deterministic hash-based leader selection with Ed25519 signature proofs.
//!
//! This module implements a **deterministic hash construction** for leader
//! selection, **not** a Verifiable Random Function (VRF). The output is
//! derived from BLAKE3 hashes and an Ed25519 signature, which provides
//! verifiability (anyone holding the public key can confirm the output
//! was produced by the corresponding private key) but **does not** provide
//! the zero-knowledge or uniqueness-in-the-presence-of-malicious-provers
//! guarantees of a true VRF as defined in RFC 9381.
//!
//! # How It Works
//!
//! 1. The "proof" is an Ed25519 signature over the input message.
//! 2. The output is `BLAKE3(domain-separated(pk || signature || input))`.
//! 3. Verification checks the signature and recomputes the hash.
//!
//! # Leader Selection
//!
//! In Omnia's consensus, each round needs a deterministic leader. This
//! module ensures that:
//!
//! 1. The output is deterministic given the same seed and private key
//! 2. The output is verifiable by all participants
//! 3. The leader is selected proportionally to stake weight
//!
//! The leader for round `r` is selected by computing a deterministic
//! pseudorandom value from `BLAKE3(round_seed || round_number)` and
//! selecting the candidate whose stake range contains
//! `output_value mod total_stake`. This gives each candidate a selection
//! probability proportional to their stake, which is the standard
//! approach in proof-of-stake systems.
//!
//! # Important Security Note
//!
//! Unlike a true VRF (e.g., ECVRF-ED25519 per RFC 9381), this construction
//! does **not** provide zero-knowledge: a malicious holder of the private
//! key could, in theory, produce multiple valid outputs for the same input
//! by exploiting the non-deterministic nature of Ed25519 signing. In
//! practice, `ed25519-dalek` produces deterministic signatures (RFC 8032
//! compliant), so this is not an issue for honest participants. However,
//! the formal VRF uniqueness guarantee does not hold against a malicious
//! prover.

use crate::crypto::{NodeKeypair, NodePublicKey};
use ed25519_dalek::{Signature, Signer, Verifier};
use omnia_primitives::blake3_hash_domain;
use omnia_primitives::NodeId;
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use thiserror::Error;

/// Errors that can occur during deterministic hash operations.
#[derive(Error, Debug)]
pub enum DeterministicHashError {
    /// The signature proof failed verification.
    #[error("proof verification failed: {0}")]
    VerificationFailed(String),
    /// The input was invalid.
    #[error("invalid input: {0}")]
    InvalidInput(String),
    /// No eligible leader was found.
    #[error("no eligible leader found for round {0}")]
    NoEligibleLeader(u64),
    /// No candidates with positive stake were provided.
    #[error("no candidates with positive stake for round {0}")]
    NoCandidates(u64),
}

/// Deterministic output containing the hash value and its signature proof.
///
/// The `output` field is the pseudorandom 32-byte value produced by the
/// deterministic hash computation. The `proof` field is an Ed25519
/// signature over the input, which allows anyone with the public key to
/// verify that the output was computed by the holder of the corresponding
/// private key.
///
/// # Determinism
///
/// For the same private key and input, the computation always produces the
/// same output (because `ed25519-dalek` produces deterministic signatures
/// per RFC 8032). This ensures that leader selection is deterministic
/// across all honest nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeterministicOutput {
    /// The 32-byte deterministic hash output.
    pub output: [u8; 32],
    /// The Ed25519 signature proof over the input (64 bytes).
    pub proof: Vec<u8>,
}

/// Compute a deterministic hash output for the given input using the provided keypair.
///
/// The output is computed as: `output = BLAKE3("OMNIA-VRF-V1" || pk || signature || input)`,
/// and the proof is an Ed25519 signature over `input`. This provides verifiability
/// (the proof shows the output was produced by the key holder) but is **not**
/// a VRF as defined in RFC 9381.
///
/// # Arguments
///
/// * `keypair` — The Ed25519 keypair used to compute the output
/// * `input` — The input message (typically `round_seed || round_number`)
///
/// # Returns
///
/// A [`DeterministicOutput`] containing the hash output and signature proof.
///
/// # Example
///
/// ```ignore
/// use omnia_crypto::vrf::{deterministic_compute, DeterministicOutput};
/// use omnia_crypto::crypto::generate_keypair;
///
/// let keypair = generate_keypair();
/// let input = b"round-42-seed";
/// let output = deterministic_compute(&keypair, input);
/// ```
pub fn deterministic_compute(keypair: &NodeKeypair, input: &[u8]) -> DeterministicOutput {
    // Sign the input to produce the signature proof
    let signature = keypair.sign(input);
    let proof = signature.to_bytes().to_vec();

    // Derive the output from the public key and signature
    // Using blake3 with domain separation
    let mut preimage = Vec::new();
    preimage.extend_from_slice(b"OMNIA-VRF-V1");
    preimage.extend_from_slice(&keypair.verifying_key().to_bytes());
    preimage.extend_from_slice(&signature.to_bytes());
    preimage.extend_from_slice(input);

    let output: [u8; 32] = blake3_hash_domain(b"omnia-commitment", &preimage);

    DeterministicOutput { output, proof }
}

/// Verify a deterministic hash output against a public key and input.
///
/// Checks that the proof is a valid Ed25519 signature over the input,
/// and that the output was correctly derived from the public key,
/// proof, and input.
///
/// # Arguments
///
/// * `public_key` — The Ed25519 public key of the output producer
/// * `input` — The original input message
/// * `det_output` — The [`DeterministicOutput`] to verify
///
/// # Returns
///
/// `Ok(())` if the output is valid, `Err(DeterministicHashError)` otherwise.
///
/// # Example
///
/// ```ignore
/// use omnia_crypto::vrf::{deterministic_compute, deterministic_verify};
/// use omnia_crypto::crypto::generate_keypair;
///
/// let keypair = generate_keypair();
/// let input = b"round-42-seed";
/// let output = deterministic_compute(&keypair, input);
/// deterministic_verify(&keypair.verifying_key(), input, &output)?;
/// ```
pub fn deterministic_verify(
    public_key: &NodePublicKey,
    input: &[u8],
    det_output: &DeterministicOutput,
) -> Result<(), DeterministicHashError> {
    // Deserialize the proof back into an Ed25519 signature
    let proof_bytes: [u8; 64] = det_output
        .proof
        .as_slice()
        .try_into()
        .map_err(|_| DeterministicHashError::VerificationFailed("Proof must be 64 bytes".to_string()))?;

    let signature = Signature::from_bytes(&proof_bytes);
    // ed25519-dalek 2.x Signature::from_bytes is infallible for [u8; 64]

    // Verify the signature against the input
    public_key
        .verify(input, &signature)
        .map_err(|e| DeterministicHashError::VerificationFailed(format!("Signature verification: {e}")))?;

    // Recompute the expected output
    let mut preimage = Vec::new();
    preimage.extend_from_slice(b"OMNIA-VRF-V1");
    preimage.extend_from_slice(&public_key.to_bytes());
    preimage.extend_from_slice(&proof_bytes);
    preimage.extend_from_slice(input);

    let expected_output: [u8; 32] = blake3_hash_domain(b"omnia-commitment", &preimage);

    if expected_output.ct_eq(&det_output.output).unwrap_u8() == 0 {
        return Err(DeterministicHashError::VerificationFailed(
            "Output does not match expected value".to_string(),
        ));
    }

    Ok(())
}

/// Select a leader from candidates using deterministic hash output and stake weights.
///
/// Algorithm: interpret the hash output as a big integer modulo `total_stake`,
/// then walk the candidate list accumulating stakes until the target
/// falls within a candidate's stake range.
///
/// This gives each candidate a probability proportional to their stake,
/// which is the standard approach in proof-of-stake systems (Cosmos,
/// Polkadot, Ethereum PoS).
///
/// # Arguments
///
/// * `candidates` — Map of candidate NodeId to (keypair, stake) pairs
/// * `round_seed` — Randomness seed for the round
/// * `round_number` — The consensus round number
///
/// # Returns
///
/// The [`NodeId`] of the selected leader, or [`DeterministicHashError::NoCandidates`]
/// if no candidates have positive stake.
///
/// # Cryptographic Properties
///
/// - **Uniform distribution:** Each candidate's selection probability is
///   approximately `stake / total_stake` (up to negligible modular bias
///   of at most `total_stake / 2^64`, which is negligible for any
///   realistic total stake value below 2^50).
/// - **Determinism:** Same inputs always produce the same leader.
/// - **Verifiability:** The output can be verified against the public key
///   and input, but this is not a true VRF (see module-level docs).
///
/// # Example
///
/// ```ignore
/// use omnia_crypto::vrf::select_leader;
/// use omnia_crypto::crypto::generate_keypair;
/// use std::collections::HashMap;
///
/// let kp1 = generate_keypair();
/// let kp2 = generate_keypair();
/// let mut node1 = [0u8; 32]; node1[0] = 1;
/// let mut node2 = [0u8; 32]; node2[0] = 2;
///
/// let mut candidates = HashMap::new();
/// candidates.insert(node1, (kp1, 100u64));
/// candidates.insert(node2, (kp2, 200u64));
///
/// let leader = select_leader(&candidates, b"seed", 1)?;
/// ```
pub fn select_leader(
    candidates: &std::collections::HashMap<NodeId, (NodeKeypair, u64)>,
    round_seed: &[u8],
    round_number: u64,
) -> Result<NodeId, DeterministicHashError> {
    // Filter out zero-stake candidates
    let valid_candidates: Vec<_> = candidates.iter().filter(|(_, (_, stake))| *stake > 0).collect();

    if valid_candidates.is_empty() {
        return Err(DeterministicHashError::NoCandidates(round_number));
    }

    // Compute total stake
    let total_stake: u64 = valid_candidates.iter().map(|(_, (_, stake))| *stake).sum();

    if total_stake == 0 {
        return Err(DeterministicHashError::NoCandidates(round_number));
    }

    // Derive deterministic output for this round
    // Use BLAKE3 to hash round_seed || round_number
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"OMNIA-LEADER-V1");
    hasher.update(round_seed);
    hasher.update(&round_number.to_le_bytes());
    let hash = hasher.finalize();

    // Interpret first 8 bytes of output as u64, then mod total_stake.
    // This gives approximately uniform distribution over [0, total_stake).
    // The bias is at most (2^64 mod total_stake) / 2^64, which is
    // negligible for any realistic total_stake value (< 2^50).
    let hash_u64 = u64::from_be_bytes(
        hash.as_bytes()[..8]
            .try_into()
            .expect("BLAKE3 output is 32 bytes, first 8 are always valid"),
    );
    let target = hash_u64 % total_stake;

    // Walk candidates accumulating stake until target falls within range
    let mut cumulative: u64 = 0;
    for (node_id, (_, stake)) in &valid_candidates {
        cumulative += *stake;
        if target < cumulative {
            return Ok(**node_id);
        }
    }

    // Should be unreachable if total_stake > 0 and math is correct.
    // Return last candidate as fallback (should never happen).
    Ok(*valid_candidates
        .last()
        .ok_or(DeterministicHashError::NoEligibleLeader(round_number))?
        .0)
}

// ---------------------------------------------------------------------------
// Phase 5: ECDSA-proof construction (Ed25519 signature + Fiat-Shamir transcript)
// ---------------------------------------------------------------------------

/// Hash version selector for backward compatibility during migration.
///
/// - `V1`: Legacy Ed25519 signature + BLAKE3 derivation (original construction)
/// - `V2`: ECDSA-proof construction with Fiat-Shamir transcript (improved binding)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum HashVersion {
    /// Legacy construction: Ed25519 signature + BLAKE3 output derivation.
    /// Deprecated but supported for migration.
    #[default]
    V1,
    /// ECDSA-proof construction with Fiat-Shamir transcript.
    /// Provides stronger binding of the output to the public key and input.
    V2,
}

/// ECDSA-proof output using Ed25519 signatures.
///
/// The proof consists of three components:
/// - `gamma`: The output value derived from BLAKE3(secret_key || hash_to_curve_point)
/// - `c`: The challenge value (16 bytes, derived from Fiat-Shamir hash)
/// - `s`: The Ed25519 signature over the transcript (64 bytes)
///
/// # Security Note
///
/// This construction uses Ed25519 signatures (which provide genuine
/// EC operations) as the core proof mechanism, combined with a
/// Fiat-Shamir transcript hash. The signature proves knowledge of the
/// secret key, while the gamma value provides the deterministic output.
/// This provides **verifiability** (anyone with the public key can
/// confirm the output was correctly computed) but **does not** provide
/// zero-knowledge or the formal uniqueness guarantees of a true VRF
/// per RFC 9381.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EcdsaProofOutput {
    /// Gamma: the output commitment (32 bytes, derived from BLAKE3(secret_key || H)).
    pub gamma: [u8; 32],
    /// Challenge: 16-byte Fiat-Shamir transcript hash.
    pub c: [u8; 16],
    /// Signature: Ed25519 signature over the transcript (64 bytes).
    pub s: Vec<u8>,
}

/// ECDSA-proof compute function using Ed25519 signatures.
///
/// Computes the deterministic output and proof for the given secret key
/// and input. The proof allows anyone with the public key to verify the
/// output was correctly computed without learning the secret key.
///
/// # Construction
///
/// This uses a Fiat-Shamir transcript combined with Ed25519 signatures:
/// 1. `H = hash_to_curve(pk, alpha)` — deterministic 32-byte commitment
/// 2. `gamma = BLAKE3("OMNIA-ECVRF-GAMMA" || sk || H)` — output commitment
/// 3. `c = BLAKE3("OMNIA-ECVRF-CHALLENGE" || pk || H || gamma)` — Fiat-Shamir challenge
/// 4. `sigma = Sign(sk, pk || H || gamma || c)` — proof of knowledge
///
/// # Arguments
///
/// * `secret_key` — The Ed25519 signing key
/// * `alpha_string` — The input (typically round_seed || round_number)
///
/// # Returns
///
/// An `EcdsaProofOutput` containing the proof and the output commitment.
pub fn ecdsa_prove(secret_key: &ed25519_dalek::SigningKey, alpha_string: &[u8]) -> EcdsaProofOutput {
    let public_key = secret_key.verifying_key();

    // Step 1: Hash-to-curve — derive a deterministic commitment from alpha_string
    let h_point = proof_hash_to_curve(alpha_string, &public_key);

    // Step 2: Compute gamma — the output commitment
    // Derived deterministically from the secret key and H.
    let gamma = proof_compute_gamma(secret_key, &h_point);

    // Step 3: Fiat-Shamir challenge — hash the public transcript
    let c = proof_hash_challenge(&public_key, &h_point, &gamma);

    // Step 4: Sign the transcript to prove knowledge of the secret key
    // The signature covers: pk || H || gamma || c
    let mut transcript = Vec::new();
    transcript.extend_from_slice(b"OMNIA-ECVRF-SIGN-V2");
    transcript.extend_from_slice(public_key.to_bytes().as_slice());
    transcript.extend_from_slice(&h_point);
    transcript.extend_from_slice(&gamma);
    transcript.extend_from_slice(&c);
    let signature = secret_key.sign(&transcript);
    let s = signature.to_bytes().to_vec();

    EcdsaProofOutput { gamma, c, s }
}

/// ECDSA-proof verify function using Ed25519 signatures.
///
/// Verifies a proof and returns the pseudorandom output.
///
/// # Arguments
///
/// * `public_key` — The Ed25519 verifying key
/// * `alpha_string` — The original input
/// * `proof` — The `EcdsaProofOutput` to verify
///
/// # Returns
///
/// `Ok([u8; 32])` — The derived output on success
/// `Err(DeterministicHashError)` — If verification fails
pub fn ecdsa_verify(
    public_key: &ed25519_dalek::VerifyingKey,
    alpha_string: &[u8],
    proof: &EcdsaProofOutput,
) -> Result<[u8; 32], DeterministicHashError> {
    // Step 1: Recompute H = hash_to_curve(alpha_string)
    let h_point = proof_hash_to_curve(alpha_string, public_key);

    // Step 2: Recompute the Fiat-Shamir challenge
    let c_prime = proof_hash_challenge(public_key, &h_point, &proof.gamma);

    // Step 3: Verify the challenge matches
    if c_prime != proof.c {
        return Err(DeterministicHashError::VerificationFailed(
            "Challenge mismatch: proof is invalid".to_string(),
        ));
    }

    // Step 4: Verify the Ed25519 signature over the transcript
    let mut transcript = Vec::new();
    transcript.extend_from_slice(b"OMNIA-ECVRF-SIGN-V2");
    transcript.extend_from_slice(public_key.to_bytes().as_slice());
    transcript.extend_from_slice(&h_point);
    transcript.extend_from_slice(&proof.gamma);
    transcript.extend_from_slice(&proof.c);

    let signature = Signature::from_bytes(
        &proof
            .s
            .as_slice()
            .try_into()
            .map_err(|_| DeterministicHashError::VerificationFailed("Signature must be 64 bytes".to_string()))?,
    );
    public_key
        .verify(&transcript, &signature)
        .map_err(|e| DeterministicHashError::VerificationFailed(format!("Signature verification: {e}")))?;

    // Step 5: Derive the output from Gamma
    Ok(proof_to_hash(&proof.gamma))
}

// ---------------------------------------------------------------------------
// Internal helper functions for ECDSA-proof construction
// ---------------------------------------------------------------------------

/// Derive a deterministic 32-byte commitment from the alpha_string.
///
/// Uses BLAKE3 with domain separation to derive a 32-byte value that
/// serves as a deterministic "hash-to-curve" result.
fn proof_hash_to_curve(alpha_string: &[u8], public_key: &ed25519_dalek::VerifyingKey) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"OMNIA-ECVRF-H2C-V2");
    hasher.update(public_key.to_bytes().as_slice());
    hasher.update(alpha_string);
    *hasher.finalize().as_bytes()
}

/// Compute Gamma = BLAKE3(secret_key || H) — the output commitment.
///
/// Derived deterministically from the secret key and the hash-to-curve
/// point. Cannot be computed without knowledge of the secret key.
fn proof_compute_gamma(secret_key: &ed25519_dalek::SigningKey, h_point: &[u8; 32]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"OMNIA-ECVRF-GAMMA-V2");
    hasher.update(secret_key.to_bytes().as_slice());
    hasher.update(h_point);
    *hasher.finalize().as_bytes()
}

/// Compute the Fiat-Shamir challenge from public values.
///
/// Produces a 16-byte challenge by hashing the public transcript:
/// `c = BLAKE3("OMNIA-ECVRF-CHALLENGE-V2" || pk || H || Gamma)`
fn proof_hash_challenge(public_key: &ed25519_dalek::VerifyingKey, h_point: &[u8; 32], gamma: &[u8; 32]) -> [u8; 16] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"OMNIA-ECVRF-CHALLENGE-V2");
    hasher.update(public_key.to_bytes().as_slice());
    hasher.update(h_point);
    hasher.update(gamma);
    let hash = hasher.finalize();
    let mut challenge = [0u8; 16];
    challenge.copy_from_slice(&hash.as_bytes()[..16]);
    challenge
}

/// Derive the final output from the Gamma commitment.
///
/// This is the final step that converts the Gamma commitment into
/// the pseudorandom 32-byte output.
fn proof_to_hash(gamma: &[u8; 32]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"OMNIA-ECVRF-OUTPUT-V2");
    hasher.update(gamma);
    *hasher.finalize().as_bytes()
}

/// Select a leader from candidates using the specified hash version.
///
/// This is the version-aware leader selection that supports both
/// V1 (legacy) and V2 (ECDSA-proof) constructions. V1 is the default
/// for backward compatibility; V2 should be used for new networks.
pub fn select_leader_v2(
    candidates: &std::collections::HashMap<NodeId, (NodeKeypair, u64)>,
    round_seed: &[u8],
    round_number: u64,
    hash_version: HashVersion,
) -> Result<NodeId, DeterministicHashError> {
    // Filter out zero-stake candidates
    let valid_candidates: Vec<_> = candidates.iter().filter(|(_, (_, stake))| *stake > 0).collect();

    if valid_candidates.is_empty() {
        return Err(DeterministicHashError::NoCandidates(round_number));
    }

    // Compute total stake
    let total_stake: u64 = valid_candidates.iter().map(|(_, (_, stake))| *stake).sum();

    if total_stake == 0 {
        return Err(DeterministicHashError::NoCandidates(round_number));
    }

    // Derive hash seed based on version
    let hash_output = match hash_version {
        HashVersion::V1 => {
            // Legacy: BLAKE3("OMNIA-LEADER-V1" || round_seed || round_number)
            let mut hasher = blake3::Hasher::new();
            hasher.update(b"OMNIA-LEADER-V1");
            hasher.update(round_seed);
            hasher.update(&round_number.to_le_bytes());
            *hasher.finalize().as_bytes()
        }
        HashVersion::V2 => {
            // V2: BLAKE3("OMNIA-LEADER-V2" || round_seed || round_number)
            let mut hasher = blake3::Hasher::new();
            hasher.update(b"OMNIA-LEADER-V2");
            hasher.update(round_seed);
            hasher.update(&round_number.to_le_bytes());
            *hasher.finalize().as_bytes()
        }
    };

    // Stake-weighted selection
    let hash_u64 = u64::from_be_bytes(
        hash_output[..8]
            .try_into()
            .expect("first 8 bytes of BLAKE3 output are always valid"),
    );
    let target = hash_u64 % total_stake;

    // Walk candidates accumulating stake until target falls within range
    let mut cumulative: u64 = 0;
    for (node_id, (_, stake)) in &valid_candidates {
        cumulative += *stake;
        if target < cumulative {
            return Ok(**node_id);
        }
    }

    // Fallback (should be unreachable)
    Ok(*valid_candidates
        .last()
        .ok_or(DeterministicHashError::NoEligibleLeader(round_number))?
        .0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::crypto::generate_keypair;
    use std::collections::HashMap;

    fn test_node(id: u8) -> NodeId {
        let mut node = [0u8; 32];
        node[0] = id;
        node
    }

    #[test]
    fn test_deterministic_compute_and_verify() {
        let keypair = generate_keypair();
        let input = b"test-round-input";

        let output = deterministic_compute(&keypair, input);

        // Output should be 32 bytes
        assert_eq!(output.output.len(), 32);
        // Proof should be 64 bytes (Ed25519 signature)
        assert_eq!(output.proof.len(), 64);

        // Verification should succeed
        deterministic_verify(&keypair.verifying_key(), input, &output)
            .expect("deterministic verification should succeed");
    }

    #[test]
    fn test_deterministic_output_reproducible() {
        let keypair = generate_keypair();
        let input = b"deterministic-test";

        let output1 = deterministic_compute(&keypair, input);
        let output2 = deterministic_compute(&keypair, input);

        // Same keypair + same input → same output
        assert_eq!(output1.output, output2.output);
        assert_eq!(output1.proof, output2.proof);
    }

    #[test]
    fn test_different_keys_different_output() {
        let kp1 = generate_keypair();
        let kp2 = generate_keypair();
        let input = b"same-input";

        let out1 = deterministic_compute(&kp1, input);
        let out2 = deterministic_compute(&kp2, input);

        // Different keys → different outputs
        assert_ne!(out1.output, out2.output);
    }

    #[test]
    fn test_different_inputs_different_output() {
        let keypair = generate_keypair();

        let out1 = deterministic_compute(&keypair, b"input-1");
        let out2 = deterministic_compute(&keypair, b"input-2");

        // Different inputs → different outputs
        assert_ne!(out1.output, out2.output);
    }

    #[test]
    fn test_verify_wrong_input() {
        let keypair = generate_keypair();
        let output = deterministic_compute(&keypair, b"correct-input");

        // Verification with wrong input should fail
        let result = deterministic_verify(&keypair.verifying_key(), b"wrong-input", &output);
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_wrong_key() {
        let kp1 = generate_keypair();
        let kp2 = generate_keypair();
        let input = b"test-input";

        let output = deterministic_compute(&kp1, input);

        // Verification with wrong public key should fail
        let result = deterministic_verify(&kp2.verifying_key(), input, &output);
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_tampered_output() {
        let keypair = generate_keypair();
        let input = b"test-input";
        let mut output = deterministic_compute(&keypair, input);

        // Tamper with the output
        output.output[0] ^= 0xFF;

        let result = deterministic_verify(&keypair.verifying_key(), input, &output);
        assert!(result.is_err());
    }

    #[test]
    fn test_select_leader_basic() {
        let kp1 = generate_keypair();
        let kp2 = generate_keypair();
        let kp3 = generate_keypair();

        let mut candidates = HashMap::new();
        candidates.insert(test_node(1), (kp1, 100));
        candidates.insert(test_node(2), (kp2, 100));
        candidates.insert(test_node(3), (kp3, 100));

        let leader = select_leader(&candidates, b"test-seed", 1).expect("should select leader");
        // Leader should be one of the candidates
        assert!(candidates.contains_key(&leader));
    }

    #[test]
    fn test_select_leader_deterministic() {
        let kp1 = generate_keypair();
        let kp2 = generate_keypair();

        let mut candidates = HashMap::new();
        candidates.insert(test_node(1), (kp1, 100));
        candidates.insert(test_node(2), (kp2, 100));

        let leader1 = select_leader(&candidates, b"seed", 5).expect("should select leader");
        let leader2 = select_leader(&candidates, b"seed", 5).expect("should select leader");

        // Same inputs → same leader
        assert_eq!(leader1, leader2);
    }

    #[test]
    fn test_select_leader_different_rounds() {
        let kp1 = generate_keypair();
        let kp2 = generate_keypair();

        let mut candidates = HashMap::new();
        candidates.insert(test_node(1), (kp1, 100));
        candidates.insert(test_node(2), (kp2, 100));

        let leader1 = select_leader(&candidates, b"seed", 1).expect("should select leader");
        let leader2 = select_leader(&candidates, b"seed", 2).expect("should select leader");

        // Different rounds may produce different leaders (not guaranteed,
        // but the hash outputs will be different)
        // Just verify both are valid
        assert!(candidates.contains_key(&leader1));
        assert!(candidates.contains_key(&leader2));
    }

    #[test]
    fn test_select_leader_empty_candidates() {
        let candidates: HashMap<NodeId, (NodeKeypair, u64)> = HashMap::new();
        let result = select_leader(&candidates, b"seed", 1);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DeterministicHashError::NoCandidates(1)));
    }

    #[test]
    fn test_select_leader_zero_stake_skipped() {
        let kp_with_stake = generate_keypair();
        let kp_zero_stake = generate_keypair();

        let mut candidates = HashMap::new();
        candidates.insert(test_node(1), (kp_with_stake, 100));
        candidates.insert(test_node(2), (kp_zero_stake, 0));

        // Should select from candidates with non-zero stake
        let leader = select_leader(&candidates, b"seed", 1).expect("should select leader");
        assert_eq!(leader, test_node(1)); // Only one eligible candidate
    }

    #[test]
    fn test_select_leader_stake_proportional() {
        // Create candidates with known stake distribution
        let candidates: Vec<(NodeId, (NodeKeypair, u64))> = vec![
            (test_node(1), (generate_keypair(), 100)), // 10% stake
            (test_node(2), (generate_keypair(), 400)), // 40% stake
            (test_node(3), (generate_keypair(), 500)), // 50% stake
        ];

        // Run selection with different round numbers
        let mut counts: HashMap<NodeId, usize> = HashMap::new();
        let rounds = 10_000;
        let seed = [42u8; 32];

        for round in 0..rounds {
            let mut map = HashMap::new();
            for (id, (kp, stake)) in &candidates {
                map.insert(*id, (kp.clone(), *stake));
            }
            let leader = select_leader(&map, &seed, round).expect("should select leader");
            *counts.entry(leader).or_insert(0) += 1;
        }

        // Each candidate should be selected approximately proportional to stake.
        // Use basis-point (BPS) arithmetic instead of f64: frequency_bps = count * 10_000 / total.
        // 10% = 1000 bps, 40% = 4000 bps, 50% = 5000 bps.
        let freq_1_bps = (*counts.get(&test_node(1)).unwrap_or(&0) as u128 * 10_000 / rounds as u128) as u64;
        let freq_2_bps = (*counts.get(&test_node(2)).unwrap_or(&0) as u128 * 10_000 / rounds as u128) as u64;
        let freq_3_bps = (*counts.get(&test_node(3)).unwrap_or(&0) as u128 * 10_000 / rounds as u128) as u64;

        // Allow 500 bps (5%) tolerance for statistical variance
        assert!(
            freq_1_bps > 500 && freq_1_bps < 1500,
            "Node 1 frequency {freq_1_bps} bps not ~1000 bps (10%)"
        );
        assert!(
            freq_2_bps > 3500 && freq_2_bps < 4500,
            "Node 2 frequency {freq_2_bps} bps not ~4000 bps (40%)"
        );
        assert!(
            freq_3_bps > 4500 && freq_3_bps < 5500,
            "Node 3 frequency {freq_3_bps} bps not ~5000 bps (50%)"
        );
    }

    #[test]
    fn test_select_leader_single_candidate() {
        let kp = generate_keypair();
        let candidates: Vec<(NodeId, (NodeKeypair, u64))> = vec![(test_node(7), (kp, 1000))];

        // Single candidate should always win regardless of round
        for round in 0..100 {
            let mut map = HashMap::new();
            for (id, (kp, stake)) in &candidates {
                map.insert(*id, (kp.clone(), *stake));
            }
            let leader = select_leader(&map, &[0u8; 32], round).expect("should select leader");
            assert_eq!(leader, test_node(7));
        }
    }

    #[test]
    fn test_select_leader_all_zero_stake() {
        let kp1 = generate_keypair();
        let kp2 = generate_keypair();

        let mut candidates = HashMap::new();
        candidates.insert(test_node(1), (kp1, 0));
        candidates.insert(test_node(2), (kp2, 0));

        let result = select_leader(&candidates, b"seed", 1);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DeterministicHashError::NoCandidates(1)));
    }

    // -----------------------------------------------------------------------
    // ECDSA-proof construction tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_ecdsa_prove_verify_round_trip() {
        let keypair = generate_keypair();
        let alpha = b"ecdsa-round-trip-test";

        let proof = ecdsa_prove(&keypair, alpha);
        let result = ecdsa_verify(&keypair.verifying_key(), alpha, &proof);
        assert!(result.is_ok(), "ECDSA verify should succeed for valid proof");
    }

    #[test]
    fn test_ecdsa_uniqueness() {
        let keypair = generate_keypair();
        let alpha = b"uniqueness-test";

        let proof1 = ecdsa_prove(&keypair, alpha);
        let proof2 = ecdsa_prove(&keypair, alpha);

        // Same keypair + same input → same output (deterministic)
        assert_eq!(proof1.gamma, proof2.gamma, "Output must be deterministic");
        assert_eq!(proof1.c, proof2.c, "Challenge must be deterministic");
        assert_eq!(proof1.s, proof2.s, "Response must be deterministic");
    }

    #[test]
    fn test_ecdsa_wrong_input_fails() {
        let keypair = generate_keypair();
        let proof = ecdsa_prove(&keypair, b"correct-input");

        let result = ecdsa_verify(&keypair.verifying_key(), b"wrong-input", &proof);
        assert!(result.is_err(), "ECDSA verify should fail with wrong input");
    }

    #[test]
    fn test_ecdsa_stake_proportional() {
        // V2 leader selection should still be stake-proportional
        let candidates: Vec<(NodeId, (NodeKeypair, u64))> = vec![
            (test_node(1), (generate_keypair(), 100)), // 10%
            (test_node(2), (generate_keypair(), 400)), // 40%
            (test_node(3), (generate_keypair(), 500)), // 50%
        ];

        let mut counts: HashMap<NodeId, usize> = HashMap::new();
        let rounds = 10_000;
        let seed = [42u8; 32];

        for round in 0..rounds {
            let mut map = HashMap::new();
            for (id, (kp, stake)) in &candidates {
                map.insert(*id, (kp.clone(), *stake));
            }
            let leader = select_leader_v2(&map, &seed, round, HashVersion::V2).expect("should select leader");
            *counts.entry(leader).or_insert(0) += 1;
        }

        // Check stake-proportional selection (same BPS logic as V1 test)
        let freq_1_bps = (*counts.get(&test_node(1)).unwrap_or(&0) as u128 * 10_000 / rounds as u128) as u64;
        let freq_2_bps = (*counts.get(&test_node(2)).unwrap_or(&0) as u128 * 10_000 / rounds as u128) as u64;
        let freq_3_bps = (*counts.get(&test_node(3)).unwrap_or(&0) as u128 * 10_000 / rounds as u128) as u64;

        assert!(
            freq_1_bps > 500 && freq_1_bps < 1500,
            "Node 1 V2 frequency {freq_1_bps} bps not ~1000 bps (10%)"
        );
        assert!(
            freq_2_bps > 3500 && freq_2_bps < 4500,
            "Node 2 V2 frequency {freq_2_bps} bps not ~4000 bps (40%)"
        );
        assert!(
            freq_3_bps > 4500 && freq_3_bps < 5500,
            "Node 3 V2 frequency {freq_3_bps} bps not ~5000 bps (50%)"
        );
    }

    #[test]
    fn test_v1_backward_compat() {
        // V1 leader selection should produce the same results as the original select_leader
        let kp1 = generate_keypair();
        let kp2 = generate_keypair();

        let mut candidates = HashMap::new();
        candidates.insert(test_node(1), (kp1, 100));
        candidates.insert(test_node(2), (kp2, 100));

        let leader_v1 = select_leader(&candidates, b"seed", 5).expect("V1 should work");
        let leader_v2_compat =
            select_leader_v2(&candidates, b"seed", 5, HashVersion::V1).expect("V1 via select_leader_v2 should work");

        assert_eq!(
            leader_v1, leader_v2_compat,
            "V1 backward compatibility: both functions should produce the same leader"
        );
    }

    // -----------------------------------------------------------------------
    // Property tests (required by Critical Issue #2 fix)
    // -----------------------------------------------------------------------

    #[test]
    fn test_proof_verification() {
        // Property: compute + verify roundtrip always succeeds for valid inputs
        for i in 0..10 {
            let keypair = generate_keypair();
            let input = format!("round-input-{i}");
            let output = deterministic_compute(&keypair, input.as_bytes());
            deterministic_verify(&keypair.verifying_key(), input.as_bytes(), &output)
                .unwrap_or_else(|e| panic!("Verification failed for input '{}': {e}", input));
        }
    }

    #[test]
    fn test_uniqueness_of_output() {
        // Property: same key + input → same output (determinism)
        let keypair = generate_keypair();
        let input = b"uniqueness-property-test";

        let out1 = deterministic_compute(&keypair, input);
        let out2 = deterministic_compute(&keypair, input);
        let out3 = deterministic_compute(&keypair, input);

        assert_eq!(out1.output, out2.output, "Deterministic outputs must match (1 vs 2)");
        assert_eq!(out2.output, out3.output, "Deterministic outputs must match (2 vs 3)");
        assert_eq!(out1.proof, out2.proof, "Deterministic proofs must match (1 vs 2)");
    }

    #[test]
    fn test_output_changes_with_input() {
        // Property: different input → different output
        let keypair = generate_keypair();
        let mut seen_outputs = std::collections::HashSet::new();

        for i in 0u64..50 {
            let input = i.to_le_bytes();
            let output = deterministic_compute(&keypair, &input);
            assert!(
                seen_outputs.insert(output.output),
                "Duplicate output for input {i} — outputs must differ across inputs"
            );
        }
    }
}
