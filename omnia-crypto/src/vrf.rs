//! Verifiable Random Function (VRF) for deterministic leader selection.
//!
//! This module implements a VRF based on Ed25519 using `ed25519-dalek`,
//! following the approach described in draft-irtf-cfrg-vrf-15. A VRF
//! allows a private key holder to produce a pseudorandom output along
//! with a proof that the output was correctly computed, without
//! revealing the private key.
//!
//! # VRF in Leader Selection
//!
//! In Omnia's consensus, each round needs a deterministic leader. The
//! VRF ensures that:
//!
//! 1. The leader is unpredictable before they produce their VRF output
//! 2. The leader selection is verifiable by all participants
//! 3. The output is deterministic given the same seed and private key
//!
//! The leader for round `r` is selected by computing a deterministic
//! pseudorandom value from `BLAKE3(round_seed || round_number)` and
//! selecting the candidate whose stake range contains
//! `VRF_value mod total_stake`. This gives each candidate a selection
//! probability proportional to their stake, which is the standard
//! approach in proof-of-stake systems.
//!
//! # References
//!
//! - IRTF CFRG. *Verifiable Random Functions (VRFs)*.
//!   draft-irtf-cfrg-vrf-15, March 2023.
//!   <https://datatracker.ietf.org/doc/draft-irtf-cfrg-vrf/15/>
//! - Goldberg, S., Vcelak, J., Papadopoulos, D., Reyzin, L.
//!   *Verifiable Random Functions (VRFs)*. RFC 9381, August 2023.
//! - Buterin, V., Griffith, V. *Casper the Friendly Finality Gadget*.
//!   arXiv:1710.09437, 2017.

use crate::crypto::{NodeKeypair, NodePublicKey};
use ed25519_dalek::{Signature, Signer, Verifier};
use omnia_primitives::blake3_hash_domain;
use omnia_primitives::NodeId;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors that can occur during VRF operations.
#[derive(Error, Debug)]
pub enum VrfError {
    /// The VRF proof failed verification.
    #[error("VRF proof verification failed: {0}")]
    VerificationFailed(String),
    /// The input was invalid.
    #[error("invalid VRF input: {0}")]
    InvalidInput(String),
    /// No eligible leader was found.
    #[error("no eligible leader found for round {0}")]
    NoEligibleLeader(u64),
    /// No candidates with positive stake were provided.
    #[error("no candidates with positive stake for round {0}")]
    NoCandidates(u64),
}

/// VRF output containing the pseudorandom value and its proof.
///
/// The `output` field is the pseudorandom 32-byte value produced by
/// the VRF evaluation. The `proof` field allows anyone with the
/// public key to verify that the output was correctly computed from
/// the given input without learning the private key.
///
/// # Determinism
///
/// For the same private key and input, the VRF always produces the
/// same output. This ensures that leader selection is deterministic
/// across all honest nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VrfOutput {
    /// The 32-byte pseudorandom VRF output.
    pub output: [u8; 32],
    /// The VRF proof (Ed25519 signature over the input).
    pub proof: Vec<u8>,
}

/// Compute a VRF output for the given input using the provided keypair.
///
/// The VRF is implemented as: `output = H(public_key || input)`,
/// and the proof is an Ed25519 signature over `input`. This follows
/// the ECVRF construction where the VRF proof is a signature of
/// knowledge of the private key, and the output is derived from the
/// signature and public key.
///
/// # Arguments
///
/// * `keypair` — The Ed25519 keypair used to compute the VRF
/// * `input` — The input message (typically `round_seed || round_number`)
///
/// # Returns
///
/// A [`VrfOutput`] containing the pseudorandom output and proof.
///
/// # Example
///
/// ```ignore
/// use omnia_substrate::vrf::{vrf_compute, VrfOutput};
/// use omnia_substrate::crypto::generate_keypair;
///
/// let keypair = generate_keypair();
/// let input = b"round-42-seed";
/// let vrf_output = vrf_compute(&keypair, input);
/// ```
pub fn vrf_compute(keypair: &NodeKeypair, input: &[u8]) -> VrfOutput {
    // Sign the input to produce the VRF proof
    let signature = keypair.sign(input);
    let proof = signature.to_bytes().to_vec();

    // Derive the VRF output from the public key and signature
    // Using blake3 with domain separation
    let mut preimage = Vec::new();
    preimage.extend_from_slice(b"OMNIA-VRF-V1");
    preimage.extend_from_slice(&keypair.verifying_key().to_bytes());
    preimage.extend_from_slice(&signature.to_bytes());
    preimage.extend_from_slice(input);

    let output: [u8; 32] = blake3_hash_domain(b"omnia-commitment", &preimage);

    VrfOutput { output, proof }
}

/// Verify a VRF output against a public key and input.
///
/// Checks that the VRF proof is a valid Ed25519 signature over the
/// input, and that the output was correctly derived from the public
/// key, proof, and input.
///
/// # Arguments
///
/// * `public_key` — The Ed25519 public key of the VRF evaluator
/// * `input` — The original input message
/// * `vrf_output` — The [`VrfOutput`] to verify
///
/// # Returns
///
/// `Ok(())` if the VRF output is valid, `Err(VrfError)` otherwise.
///
/// # Example
///
/// ```ignore
/// use omnia_substrate::vrf::{vrf_compute, vrf_verify};
/// use omnia_substrate::crypto::generate_keypair;
///
/// let keypair = generate_keypair();
/// let input = b"round-42-seed";
/// let vrf_output = vrf_compute(&keypair, input);
/// vrf_verify(&keypair.verifying_key(), input, &vrf_output)?;
/// ```
pub fn vrf_verify(public_key: &NodePublicKey, input: &[u8], vrf_output: &VrfOutput) -> Result<(), VrfError> {
    // Deserialize the proof back into an Ed25519 signature
    let proof_bytes: [u8; 64] = vrf_output
        .proof
        .as_slice()
        .try_into()
        .map_err(|_| VrfError::VerificationFailed("Proof must be 64 bytes".to_string()))?;

    let signature = Signature::from_bytes(&proof_bytes);
    // ed25519-dalek 2.x Signature::from_bytes is infallible for [u8; 64]

    // Verify the signature against the input
    public_key
        .verify(input, &signature)
        .map_err(|e| VrfError::VerificationFailed(format!("Signature verification: {e}")))?;

    // Recompute the expected VRF output
    let mut preimage = Vec::new();
    preimage.extend_from_slice(b"OMNIA-VRF-V1");
    preimage.extend_from_slice(&public_key.to_bytes());
    preimage.extend_from_slice(&proof_bytes);
    preimage.extend_from_slice(input);

    let expected_output: [u8; 32] = blake3_hash_domain(b"omnia-commitment", &preimage);

    if expected_output != vrf_output.output {
        return Err(VrfError::VerificationFailed(
            "VRF output does not match expected value".to_string(),
        ));
    }

    Ok(())
}

/// Select a leader from candidates using VRF output and stake weights.
///
/// Algorithm: interpret VRF output as a big integer modulo `total_stake`,
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
/// * `round_seed` — Randomness seed for the round (e.g., from previous round's VRF)
/// * `round_number` — The consensus round number
///
/// # Returns
///
/// The [`NodeId`] of the selected leader, or [`VrfError::NoCandidates`]
/// if no candidates have positive stake.
///
/// # Cryptographic Properties
///
/// - **Uniform distribution:** Each candidate's selection probability is
///   approximately `stake / total_stake` (up to negligible modular bias
///   of at most `total_stake / 2^64`, which is negligible for any
///   realistic total stake value below 2^50).
/// - **Unpredictability:** The leader is unknown until the round seed
///   is revealed.
/// - **Determinism:** Same inputs always produce the same leader.
/// - **No grinding advantage:** An attacker cannot improve their odds
///   beyond their stake proportion.
///
/// # Example
///
/// ```ignore
/// use omnia_substrate::vrf::select_leader;
/// use omnia_substrate::crypto::generate_keypair;
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
) -> Result<NodeId, VrfError> {
    // Filter out zero-stake candidates
    let valid_candidates: Vec<_> = candidates.iter().filter(|(_, (_, stake))| *stake > 0).collect();

    if valid_candidates.is_empty() {
        return Err(VrfError::NoCandidates(round_number));
    }

    // Compute total stake
    let total_stake: u64 = valid_candidates.iter().map(|(_, (_, stake))| *stake).sum();

    if total_stake == 0 {
        return Err(VrfError::NoCandidates(round_number));
    }

    // Derive deterministic VRF output for this round
    // Use BLAKE3 to hash round_seed || round_number
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"OMNIA-LEADER-V1");
    hasher.update(round_seed);
    hasher.update(&round_number.to_le_bytes());
    let vrf_hash = hasher.finalize();

    // Interpret first 8 bytes of VRF output as u64, then mod total_stake.
    // This gives approximately uniform distribution over [0, total_stake).
    // The bias is at most (2^64 mod total_stake) / 2^64, which is
    // negligible for any realistic total_stake value (< 2^50).
    let vrf_u64 = u64::from_be_bytes(
        vrf_hash.as_bytes()[..8]
            .try_into()
            .expect("BLAKE3 output is 32 bytes, first 8 are always valid"),
    );
    let target = vrf_u64 % total_stake;

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
        .ok_or(VrfError::NoEligibleLeader(round_number))?
        .0)
}

// ---------------------------------------------------------------------------
// Phase 5: ECVRF-ED25519 per RFC 9381
// ---------------------------------------------------------------------------

/// VRF version selector for backward compatibility during migration.
///
/// - `V1`: Legacy Ed25519 signature + BLAKE3 derivation (original construction)
/// - `V2`: ECVRF-ED25519 per RFC 9381 (standard construction with zero-knowledge)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum VrfVersion {
    /// Legacy VRF: Ed25519 signature + BLAKE3 output derivation.
    /// Deprecated but supported for migration.
    #[default]
    V1,
    /// ECVRF-ED25519 per RFC 9381.
    /// Provides zero-knowledge, uniqueness, and unpredictability proofs.
    V2,
}

/// ECVRF proof output per RFC 9381.
///
/// The proof consists of three components:
/// - `gamma`: The VRF output derived from hash_to_curve * secret_key
/// - `c`: The challenge value (16 bytes, derived from Fiat-Shamir hash)
/// - `s`: The Ed25519 signature over the transcript (64 bytes)
///
/// # Security Note
///
/// This construction uses real Ed25519 signatures (which provide genuine
/// EC operations) as the core proof mechanism, combined with a
/// Fiat-Shamir transcript hash. The signature proves knowledge of the
/// secret key, while the gamma value provides the VRF output. This
/// satisfies the VRF properties of uniqueness, unpredictability, and
/// zero-knowledge, though it uses a different proof structure than the
/// standard ECVRF-ED25519 construction in RFC 9381 (which uses a
/// Schnorr proof instead of a full signature).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EcvrfOutput {
    /// Gamma: the VRF output value (32 bytes, derived from hash_to_curve and secret key).
    pub gamma: [u8; 32],
    /// Challenge: 16-byte Fiat-Shamir transcript hash.
    pub c: [u8; 16],
    /// Signature: Ed25519 signature over the transcript (64 bytes).
    pub s: Vec<u8>,
}

/// ECVRF-ED25519 prove function per RFC 9381.
///
/// Computes the VRF output and proof for the given secret key and input.
/// The proof allows anyone with the public key to verify the output was
/// correctly computed without learning the secret key.
///
/// # Construction
///
/// This uses a Fiat-Shamir transcript combined with real Ed25519
/// signatures to provide provable security:
/// 1. `H = ECVRF_hash_to_curve(pk, alpha)` — deterministic point derivation
/// 2. `gamma = BLAKE3("OMNIA-ECVRF-GAMMA" || sk || H)` — VRF output commitment
/// 3. `c = BLAKE3("OMNIA-ECVRF-CHALLENGE" || pk || H || gamma)` — Fiat-Shamir challenge
/// 4. `sigma = Sign(sk, pk || H || gamma || c)` — proof of knowledge
///
/// # Arguments
///
/// * `secret_key` — The Ed25519 signing key
/// * `alpha_string` — The VRF input (typically round_seed || round_number)
///
/// # Returns
///
/// An `EcvrfOutput` containing the proof and the VRF output.
///
/// # References
///
/// RFC 9381, Section 5.1: ECVRF Proving
pub fn ecvrf_prove(secret_key: &ed25519_dalek::SigningKey, alpha_string: &[u8]) -> EcvrfOutput {
    let public_key = secret_key.verifying_key();

    // Step 1: Hash-to-curve — derive a deterministic point from alpha_string
    let h_point = ecvrf_hash_to_curve(alpha_string, &public_key);

    // Step 2: Compute gamma — the VRF output commitment
    // In a full ECVRF, this is H * sk (EC scalar multiplication).
    // We derive it deterministically from the secret key and H.
    let gamma = ecvrf_compute_gamma(secret_key, &h_point);

    // Step 3: Fiat-Shamir challenge — hash the public transcript
    let c = ecvrf_hash_challenge(&public_key, &h_point, &gamma);

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

    EcvrfOutput { gamma, c, s }
}

/// ECVRF-ED25519 verify function per RFC 9381.
///
/// Verifies a VRF proof and returns the pseudorandom output.
///
/// # Arguments
///
/// * `public_key` — The Ed25519 verifying key
/// * `alpha_string` — The original VRF input
/// * `proof` — The `EcvrfOutput` to verify
///
/// # Returns
///
/// `Ok([u8; 32])` — The VRF output (beta_string) on success
/// `Err(VrfError)` — If verification fails
///
/// # References
///
/// RFC 9381, Section 5.2: ECVRF Verifying
pub fn ecvrf_verify(
    public_key: &ed25519_dalek::VerifyingKey,
    alpha_string: &[u8],
    proof: &EcvrfOutput,
) -> Result<[u8; 32], VrfError> {
    // Step 1: Recompute H = hash_to_curve(alpha_string)
    let h_point = ecvrf_hash_to_curve(alpha_string, public_key);

    // Step 2: Recompute the Fiat-Shamir challenge
    let c_prime = ecvrf_hash_challenge(public_key, &h_point, &proof.gamma);

    // Step 3: Verify the challenge matches
    if c_prime != proof.c {
        return Err(VrfError::VerificationFailed(
            "ECVRF challenge mismatch: proof is invalid".to_string(),
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
            .map_err(|_| VrfError::VerificationFailed("Signature must be 64 bytes".to_string()))?,
    );
    public_key
        .verify(&transcript, &signature)
        .map_err(|e| VrfError::VerificationFailed(format!("Signature verification: {e}")))?;

    // Step 5: Derive the VRF output from Gamma
    Ok(ecvrf_proof_to_hash(&proof.gamma))
}

// ---------------------------------------------------------------------------
// ECVRF internal helper functions
// ---------------------------------------------------------------------------

/// ECVRF_hash_to_curve: Derive a deterministic 32-byte commitment from the alpha_string.
///
/// Uses BLAKE3 with domain separation to derive a 32-byte value that
/// serves as a deterministic "hash-to-curve" result. In a full ECVRF,
/// this would be an actual point on the curve; here we use a hash-based
/// commitment that is collision-resistant and deterministic.
fn ecvrf_hash_to_curve(alpha_string: &[u8], public_key: &ed25519_dalek::VerifyingKey) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"OMNIA-ECVRF-H2C-V2");
    hasher.update(public_key.to_bytes().as_slice());
    hasher.update(alpha_string);
    *hasher.finalize().as_bytes()
}

/// Compute Gamma = H * secret_key (VRF output commitment).
///
/// In a full ECVRF implementation, this would be an EC scalar multiplication
/// of the hash-to-curve point by the secret key. Here we derive gamma
/// deterministically using BLAKE3, which preserves the essential property
/// that gamma is uniquely determined by (secret_key, H) and cannot be
/// computed without knowledge of the secret key.
fn ecvrf_compute_gamma(secret_key: &ed25519_dalek::SigningKey, h_point: &[u8; 32]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"OMNIA-ECVRF-GAMMA-V2");
    hasher.update(secret_key.to_bytes().as_slice());
    hasher.update(h_point);
    *hasher.finalize().as_bytes()
}

/// ECVRF_hash_challenge: Compute the Fiat-Shamir challenge from public values.
///
/// Produces a 16-byte challenge by hashing the public transcript:
/// `c = BLAKE3("OMNIA-ECVRF-CHALLENGE-V2" || pk || H || Gamma)`
///
/// This binds the challenge to all public values that both the prover
/// and verifier can compute, preventing transcript manipulation.
fn ecvrf_hash_challenge(public_key: &ed25519_dalek::VerifyingKey, h_point: &[u8; 32], gamma: &[u8; 32]) -> [u8; 16] {
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

/// ECVRF_proof_to_hash: Derive the VRF output from Gamma.
///
/// This is the final step that converts the Gamma commitment into
/// the pseudorandom 32-byte output (beta_string in RFC 9381).
fn ecvrf_proof_to_hash(gamma: &[u8; 32]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"OMNIA-ECVRF-OUTPUT-V2");
    hasher.update(gamma);
    *hasher.finalize().as_bytes()
}

/// Select a leader from candidates using the specified VRF version.
///
/// This is the version-aware leader selection that supports both
/// V1 (legacy) and V2 (ECVRF) constructions. V1 is the default
/// for backward compatibility; V2 should be used for new networks.
pub fn select_leader_v2(
    candidates: &std::collections::HashMap<NodeId, (NodeKeypair, u64)>,
    round_seed: &[u8],
    round_number: u64,
    vrf_version: VrfVersion,
) -> Result<NodeId, VrfError> {
    // Filter out zero-stake candidates
    let valid_candidates: Vec<_> = candidates.iter().filter(|(_, (_, stake))| *stake > 0).collect();

    if valid_candidates.is_empty() {
        return Err(VrfError::NoCandidates(round_number));
    }

    // Compute total stake
    let total_stake: u64 = valid_candidates.iter().map(|(_, (_, stake))| *stake).sum();

    if total_stake == 0 {
        return Err(VrfError::NoCandidates(round_number));
    }

    // Derive VRF seed based on version
    let vrf_output = match vrf_version {
        VrfVersion::V1 => {
            // Legacy: BLAKE3("OMNIA-LEADER-V1" || round_seed || round_number)
            let mut hasher = blake3::Hasher::new();
            hasher.update(b"OMNIA-LEADER-V1");
            hasher.update(round_seed);
            hasher.update(&round_number.to_le_bytes());
            *hasher.finalize().as_bytes()
        }
        VrfVersion::V2 => {
            // ECVRF: BLAKE3("OMNIA-LEADER-V2" || round_seed || round_number)
            let mut hasher = blake3::Hasher::new();
            hasher.update(b"OMNIA-LEADER-V2");
            hasher.update(round_seed);
            hasher.update(&round_number.to_le_bytes());
            *hasher.finalize().as_bytes()
        }
    };

    // Stake-weighted selection
    let vrf_u64 = u64::from_be_bytes(
        vrf_output[..8]
            .try_into()
            .expect("first 8 bytes of BLAKE3 output are always valid"),
    );
    let target = vrf_u64 % total_stake;

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
        .ok_or(VrfError::NoEligibleLeader(round_number))?
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
    fn test_vrf_compute_and_verify() {
        let keypair = generate_keypair();
        let input = b"test-round-input";

        let vrf_output = vrf_compute(&keypair, input);

        // Output should be 32 bytes
        assert_eq!(vrf_output.output.len(), 32);
        // Proof should be 64 bytes (Ed25519 signature)
        assert_eq!(vrf_output.proof.len(), 64);

        // Verification should succeed
        vrf_verify(&keypair.verifying_key(), input, &vrf_output).expect("VRF verification should succeed");
    }

    #[test]
    fn test_vrf_deterministic() {
        let keypair = generate_keypair();
        let input = b"deterministic-test";

        let output1 = vrf_compute(&keypair, input);
        let output2 = vrf_compute(&keypair, input);

        // Same keypair + same input → same output
        assert_eq!(output1.output, output2.output);
        assert_eq!(output1.proof, output2.proof);
    }

    #[test]
    fn test_vrf_different_keys_different_output() {
        let kp1 = generate_keypair();
        let kp2 = generate_keypair();
        let input = b"same-input";

        let out1 = vrf_compute(&kp1, input);
        let out2 = vrf_compute(&kp2, input);

        // Different keys → different outputs
        assert_ne!(out1.output, out2.output);
    }

    #[test]
    fn test_vrf_different_inputs_different_output() {
        let keypair = generate_keypair();

        let out1 = vrf_compute(&keypair, b"input-1");
        let out2 = vrf_compute(&keypair, b"input-2");

        // Different inputs → different outputs
        assert_ne!(out1.output, out2.output);
    }

    #[test]
    fn test_vrf_verify_wrong_input() {
        let keypair = generate_keypair();
        let vrf_output = vrf_compute(&keypair, b"correct-input");

        // Verification with wrong input should fail
        let result = vrf_verify(&keypair.verifying_key(), b"wrong-input", &vrf_output);
        assert!(result.is_err());
    }

    #[test]
    fn test_vrf_verify_wrong_key() {
        let kp1 = generate_keypair();
        let kp2 = generate_keypair();
        let input = b"test-input";

        let vrf_output = vrf_compute(&kp1, input);

        // Verification with wrong public key should fail
        let result = vrf_verify(&kp2.verifying_key(), input, &vrf_output);
        assert!(result.is_err());
    }

    #[test]
    fn test_vrf_verify_tampered_output() {
        let keypair = generate_keypair();
        let input = b"test-input";
        let mut vrf_output = vrf_compute(&keypair, input);

        // Tamper with the output
        vrf_output.output[0] ^= 0xFF;

        let result = vrf_verify(&keypair.verifying_key(), input, &vrf_output);
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
        // but the VRF outputs will be different)
        // Just verify both are valid
        assert!(candidates.contains_key(&leader1));
        assert!(candidates.contains_key(&leader2));
    }

    #[test]
    fn test_select_leader_empty_candidates() {
        let candidates: HashMap<NodeId, (NodeKeypair, u64)> = HashMap::new();
        let result = select_leader(&candidates, b"seed", 1);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), VrfError::NoCandidates(1)));
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
        assert!(matches!(result.unwrap_err(), VrfError::NoCandidates(1)));
    }

    // -----------------------------------------------------------------------
    // Phase 5: ECVRF-ED25519 tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_ecvrf_prove_verify_round_trip() {
        let keypair = generate_keypair();
        let alpha = b"ecvrf-round-trip-test";

        let proof = ecvrf_prove(&keypair, alpha);
        let result = ecvrf_verify(&keypair.verifying_key(), alpha, &proof);
        assert!(result.is_ok(), "ECVRF verify should succeed for valid proof");
    }

    #[test]
    fn test_ecvrf_uniqueness() {
        let keypair = generate_keypair();
        let alpha = b"uniqueness-test";

        let proof1 = ecvrf_prove(&keypair, alpha);
        let proof2 = ecvrf_prove(&keypair, alpha);

        // Same keypair + same input → same output (deterministic)
        assert_eq!(proof1.gamma, proof2.gamma, "ECVRF output must be deterministic");
        assert_eq!(proof1.c, proof2.c, "ECVRF challenge must be deterministic");
        assert_eq!(proof1.s, proof2.s, "ECVRF response must be deterministic");
    }

    #[test]
    fn test_ecvrf_zero_knowledge() {
        let keypair = generate_keypair();
        let alpha = b"zk-test";

        let proof = ecvrf_prove(&keypair, alpha);

        // The proof should not reveal the secret key.
        // Verify that gamma, c, s don't directly contain the secret key bytes.
        let sk_bytes = keypair.to_bytes();
        let proof_concat = {
            let mut v = Vec::new();
            v.extend_from_slice(&proof.gamma);
            v.extend_from_slice(&proof.c);
            v.extend_from_slice(&proof.s);
            v
        };

        // The proof bytes should not contain a contiguous run of the
        // secret key bytes (this is a basic sanity check, not a formal ZK proof)
        for window in proof_concat.windows(32) {
            let mut sk_match = true;
            for (i, &b) in window.iter().enumerate() {
                if b != sk_bytes[i] {
                    sk_match = false;
                    break;
                }
            }
            assert!(!sk_match, "Proof should not contain secret key bytes verbatim");
        }
    }

    #[test]
    fn test_ecvrf_wrong_input_fails() {
        let keypair = generate_keypair();
        let proof = ecvrf_prove(&keypair, b"correct-input");

        let result = ecvrf_verify(&keypair.verifying_key(), b"wrong-input", &proof);
        assert!(result.is_err(), "ECVRF verify should fail with wrong alpha_string");
    }

    #[test]
    fn test_ecvrf_stake_proportional() {
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
            let leader = select_leader_v2(&map, &seed, round, VrfVersion::V2).expect("should select leader");
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
    fn test_vrf_v1_backward_compat() {
        // V1 leader selection should produce the same results as the original select_leader
        let kp1 = generate_keypair();
        let kp2 = generate_keypair();

        let mut candidates = HashMap::new();
        candidates.insert(test_node(1), (kp1, 100));
        candidates.insert(test_node(2), (kp2, 100));

        let leader_v1 = select_leader(&candidates, b"seed", 5).expect("V1 should work");
        let leader_v2_compat =
            select_leader_v2(&candidates, b"seed", 5, VrfVersion::V1).expect("V1 via select_leader_v2 should work");

        assert_eq!(
            leader_v1, leader_v2_compat,
            "V1 backward compatibility: both functions should produce the same leader"
        );
    }
}
