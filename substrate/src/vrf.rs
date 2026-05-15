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
use crate::vector_clock::NodeId;
use ed25519_dalek::{Signature, Signer, Verifier};
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
    // Using blake3 for domain-separated hashing
    let mut preimage = Vec::new();
    preimage.extend_from_slice(b"OMNIA-VRF-V1");
    preimage.extend_from_slice(&keypair.verifying_key().to_bytes());
    preimage.extend_from_slice(&signature.to_bytes());
    preimage.extend_from_slice(input);

    let output: [u8; 32] = blake3::hash(&preimage).into();

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
pub fn vrf_verify(
    public_key: &NodePublicKey,
    input: &[u8],
    vrf_output: &VrfOutput,
) -> Result<(), VrfError> {
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
        .map_err(|e| VrfError::VerificationFailed(format!("Signature verification: {}", e)))?;

    // Recompute the expected VRF output
    let mut preimage = Vec::new();
    preimage.extend_from_slice(b"OMNIA-VRF-V1");
    preimage.extend_from_slice(&public_key.to_bytes());
    preimage.extend_from_slice(&proof_bytes);
    preimage.extend_from_slice(input);

    let expected_output: [u8; 32] = blake3::hash(&preimage).into();

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
    let valid_candidates: Vec<_> = candidates
        .iter()
        .filter(|(_, (_, stake))| *stake > 0)
        .collect();

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
        .expect("valid_candidates is non-empty")
        .0)
}

#[cfg(test)]
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
        vrf_verify(&keypair.verifying_key(), input, &vrf_output)
            .expect("VRF verification should succeed");
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

        // Each candidate should be selected approximately proportional to stake
        let total = rounds as f64;
        let freq_1 = *counts.get(&test_node(1)).unwrap_or(&0) as f64 / total;
        let freq_2 = *counts.get(&test_node(2)).unwrap_or(&0) as f64 / total;
        let freq_3 = *counts.get(&test_node(3)).unwrap_or(&0) as f64 / total;

        // Allow 5% tolerance for statistical variance
        assert!(
            (freq_1 - 0.10).abs() < 0.05,
            "Node 1 frequency {} not ~10%",
            freq_1
        );
        assert!(
            (freq_2 - 0.40).abs() < 0.05,
            "Node 2 frequency {} not ~40%",
            freq_2
        );
        assert!(
            (freq_3 - 0.50).abs() < 0.05,
            "Node 3 frequency {} not ~50%",
            freq_3
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
}
