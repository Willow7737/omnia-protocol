//! Shared ZK proof verification utilities and the circuit VK registry.
//!
//! AUDIT-2026-07 C9 (#347): proofs used to embed their own
//! `VerifyingKey` in the caller-supplied proof bytes. An attacker could
//! craft a `(vk, proof)` pair for a trivial circuit of their choosing and
//! `Groth16::verify` would happily return `Ok(true)` — for the attacker's
//! VK. The verifying key is *the statement being proved*; it must come
//! from the verifier's trusted state, never from the prover.
//!
//! The registry below holds the canonical VK for every accepted circuit,
//! keyed by a self-authenticating circuit ID (the BLAKE3 hash of the
//! canonical uncompressed VK bytes). Proof submissions now carry
//! `[32-byte circuit ID || proof]`; the shard looks the VK up and rejects
//! unknown circuit IDs outright (fail-closed).
//!
//! CONSENSUS-CRITICAL: every validator must register the identical
//! circuit set — a node that accepts a proof another node rejects will
//! diverge. Registration is a node-operator/genesis action (config or
//! startup code); it is deliberately NOT reachable from consensus
//! payloads, so no network participant can inject a circuit. On-chain
//! governance-gated registration is the tracked follow-up in #347.

use std::collections::BTreeMap;
use std::sync::{LazyLock, RwLock};

use crate::shard::ShardError;

/// Self-authenticating circuit identifier: BLAKE3 of the canonical
/// uncompressed `VerifyingKey` bytes.
pub type CircuitId = [u8; 32];

/// Length of a serialized [`CircuitId`] on the wire.
pub const CIRCUIT_ID_LEN: usize = 32;

/// Conservative lower bound on a serialized Groth16 proof. An
/// uncompressed BN254 proof (2 × G1 + 1 × G2) is 256 bytes; 128 leaves
/// room for compressed encodings while still rejecting obvious garbage.
pub const MIN_GROTH16_PROOF_LEN: usize = 128;

/// Process-wide registry of accepted circuits: circuit ID → canonical
/// uncompressed VK bytes. Populated at node startup / test setup via
/// [`register_circuit_vk`]; read on every proof verification.
static VK_REGISTRY: LazyLock<RwLock<BTreeMap<CircuitId, Vec<u8>>>> = LazyLock::new(|| RwLock::new(BTreeMap::new()));

/// Derive the circuit ID for a canonical serialized verifying key.
pub fn circuit_id_for_vk(vk_bytes: &[u8]) -> CircuitId {
    *blake3::hash(vk_bytes).as_bytes()
}

/// Register a circuit's canonical verifying key and return its circuit ID.
///
/// Node-operator/genesis action only — never expose this through a
/// consensus payload. With `real_verification` enabled the bytes must
/// deserialize as a BN254 Groth16 `VerifyingKey`; without the feature the
/// bytes are stored unvalidated (verification always rejects in that
/// configuration anyway). Registration is idempotent: the ID is the hash
/// of the bytes, so re-registering the same VK is a no-op.
pub fn register_circuit_vk(vk_bytes: &[u8]) -> Result<CircuitId, ShardError> {
    #[cfg(feature = "real_verification")]
    {
        use ark_serialize::CanonicalDeserialize;
        ark_groth16::VerifyingKey::<ark_bn254::Bn254>::deserialize_uncompressed(vk_bytes)
            .map_err(|e| ShardError::ValidationFailed(format!("refusing to register malformed verifying key: {e}")))?;
    }

    let id = circuit_id_for_vk(vk_bytes);
    let mut registry = VK_REGISTRY.write().unwrap_or_else(|poisoned| poisoned.into_inner());
    registry.insert(id, vk_bytes.to_vec());
    Ok(id)
}

/// Look up the canonical VK bytes for a circuit ID.
pub fn lookup_circuit_vk(circuit_id: &CircuitId) -> Option<Vec<u8>> {
    let registry = VK_REGISTRY.read().unwrap_or_else(|poisoned| poisoned.into_inner());
    registry.get(circuit_id).cloned()
}

/// Number of registered circuits (diagnostics / startup logging).
pub fn registered_circuit_count() -> usize {
    let registry = VK_REGISTRY.read().unwrap_or_else(|poisoned| poisoned.into_inner());
    registry.len()
}

/// Split a `[circuit_id || proof]` submission into its parts.
///
/// Rejects payloads too short to contain a circuit ID plus a plausible
/// proof. Note that a legacy `[vk_len || vk || proof]` payload parses
/// into a garbage circuit ID here and is then rejected by the registry
/// lookup — old-format submissions fail closed.
pub fn split_circuit_proof(bytes: &[u8]) -> Result<(CircuitId, &[u8]), ShardError> {
    if bytes.len() < CIRCUIT_ID_LEN + MIN_GROTH16_PROOF_LEN {
        return Err(ShardError::ValidationFailed(format!(
            "ZK submission too short ({} bytes) — expected 32-byte circuit ID followed by a proof of at least {} bytes",
            bytes.len(),
            MIN_GROTH16_PROOF_LEN
        )));
    }
    let mut circuit_id = [0u8; CIRCUIT_ID_LEN];
    circuit_id.copy_from_slice(&bytes[..CIRCUIT_ID_LEN]);
    Ok((circuit_id, &bytes[CIRCUIT_ID_LEN..]))
}

/// Real Groth16 verification against the VK registry.
#[cfg(feature = "real_verification")]
pub mod groth16 {
    use ark_bn254::{Bn254, Fr};
    use ark_ff::PrimeField;
    use ark_groth16::Groth16;
    use ark_serialize::CanonicalDeserialize;
    use ark_snark::SNARK;

    use super::{lookup_circuit_vk, split_circuit_proof};
    use crate::shard::ShardError;

    /// Public input binding a biological query proof to the specific
    /// consent record being queried: BLAKE3(subject || consumer) reduced
    /// to a BN254 scalar. A proof made for one (subject, consumer) pair
    /// cannot be replayed against another.
    pub fn biological_public_input(subject: &[u8; 32], consumer: &[u8; 32]) -> Fr {
        let mut preimage = [0u8; 64];
        preimage[..32].copy_from_slice(subject);
        preimage[32..].copy_from_slice(consumer);
        Fr::from_le_bytes_mod_order(blake3::hash(&preimage).as_bytes())
    }

    /// Public input binding a computational proof to the specific task:
    /// BLAKE3(task_id || spec) reduced to a BN254 scalar. A proof made
    /// for one task cannot be replayed against another.
    pub fn computational_public_input(task_id: &[u8; 32], spec: &[u8]) -> Fr {
        let mut preimage = Vec::with_capacity(32 + spec.len());
        preimage.extend_from_slice(task_id);
        preimage.extend_from_slice(spec);
        Fr::from_le_bytes_mod_order(blake3::hash(&preimage).as_bytes())
    }

    /// Verify a `[circuit_id || proof]` submission against the registry.
    ///
    /// The VK comes exclusively from the registry — an unknown circuit ID
    /// is rejected before any deserialization of prover-controlled bytes
    /// beyond the proof itself.
    pub fn verify_with_registry(submission: &[u8], public_inputs: &[Fr], context: &str) -> Result<(), ShardError> {
        // A proof with no public inputs binds to no statement; nothing
        // legitimate ever verifies against an empty input list.
        if public_inputs.is_empty() {
            return Err(ShardError::ValidationFailed(format!(
                "{context}: empty public inputs are not accepted"
            )));
        }

        let (circuit_id, proof_bytes) = split_circuit_proof(submission)?;

        let vk_bytes = lookup_circuit_vk(&circuit_id).ok_or_else(|| {
            ShardError::ValidationFailed(format!(
                "{context}: unknown circuit {} — circuit is not registered on this node",
                hex::encode(&circuit_id[..8])
            ))
        })?;

        // Registry bytes are validated at registration; a failure here
        // means the registry itself is corrupt, not that the caller is
        // malicious — but we still fail closed.
        let vk = ark_groth16::VerifyingKey::<Bn254>::deserialize_uncompressed(vk_bytes.as_slice()).map_err(|e| {
            ShardError::ValidationFailed(format!("{context}: registered verifying key is corrupt: {e}"))
        })?;

        let proof = ark_groth16::Proof::<Bn254>::deserialize_uncompressed(proof_bytes)
            .map_err(|e| ShardError::ValidationFailed(format!("{context}: invalid proof encoding: {e}")))?;

        match Groth16::<Bn254>::verify(&vk, public_inputs, &proof) {
            Ok(true) => Ok(()),
            Ok(false) => Err(ShardError::ValidationFailed(format!(
                "{context}: proof is invalid for this circuit and statement"
            ))),
            Err(e) => Err(ShardError::ValidationFailed(format!(
                "{context}: proof verification error: {e}"
            ))),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn circuit_id_is_blake3_of_vk_bytes() {
        let bytes = b"canonical vk bytes";
        assert_eq!(circuit_id_for_vk(bytes), *blake3::hash(bytes).as_bytes());
        // Deterministic across calls.
        assert_eq!(circuit_id_for_vk(bytes), circuit_id_for_vk(bytes));
    }

    #[test]
    fn lookup_of_unregistered_circuit_is_none() {
        assert!(lookup_circuit_vk(&[0xEE; 32]).is_none());
    }

    #[test]
    fn split_rejects_short_submissions() {
        assert!(split_circuit_proof(&[]).is_err());
        assert!(split_circuit_proof(&[0u8; CIRCUIT_ID_LEN]).is_err());
        assert!(split_circuit_proof(&[0u8; CIRCUIT_ID_LEN + MIN_GROTH16_PROOF_LEN - 1]).is_err());
    }

    #[test]
    fn split_extracts_circuit_id_and_proof() {
        let mut bytes = vec![0xABu8; CIRCUIT_ID_LEN];
        bytes.extend_from_slice(&[0xCD; MIN_GROTH16_PROOF_LEN]);
        let (id, proof) = split_circuit_proof(&bytes).unwrap();
        assert_eq!(id, [0xAB; 32]);
        assert_eq!(proof, &[0xCD; MIN_GROTH16_PROOF_LEN][..]);
    }
}
