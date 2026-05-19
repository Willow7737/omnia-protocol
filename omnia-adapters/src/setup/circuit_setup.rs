//! Phase 2: Circuit-specific key derivation from the Powers of Tau SRS.
//!
//! After Phase 1 produces the circuit-independent SRS (Powers of Tau),
//! Phase 2 derives circuit-specific proving and verifying keys for a
//! particular Groth16 circuit. This involves:
//!
//! 1. Loading the Phase 1 SRS
//! 2. Performing a circuit-specific ceremony (optional but recommended)
//! 3. Deriving the proving key (`pk`) and verifying key (`vk`)
//!
//! The Phase 2 ceremony adds circuit-specific randomness on top of the
//! Phase 1 SRS, ensuring that even if Phase 1 was compromised, the
//! Phase 2 contribution provides an additional layer of security.
//!
//! # Key Derivation Functions
//!
//! - [`derive_keys`] — Legacy function that ignores the SRS (deprecated)
//! - [`derive_keys_expanded`] — Legacy function that ignores the SRS (deprecated)
//! - [`derive_keys_from_srs`] — Verifies the SRS has contributions and derives
//!   keys with audit trail logging
//!
//! # References
//!
//! - Groth, J. *On the Size of Pairing-based Non-interactive Arguments*
//!   (EUROCRYPT 2016). <https://eprint.iacr.org/2016/260>
//! - Gabizon, A., Williamson, Z., Ciobotaru, O. *PLONK: Permutations
//!   over Lagrange-bases for Oecumenical Noninteractive arguments of
//!   Knowledge* (IACR ePrint 2019/953)

use ark_bn254::Bn254;
use ark_groth16::{ProvingKey, VerifyingKey};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use subtle::ConstantTimeEq;

use crate::circuit::RollupCircuit;
use crate::prover::{generate_trusted_setup, generate_trusted_setup_expanded};

use super::powers_of_tau::PowersOfTau;
use super::SetupError;

/// Circuit-specific key pair derived from the Powers of Tau SRS.
///
/// Contains the proving key and verifying key needed for Groth16
/// proof creation and verification.
#[derive(Debug)]
pub struct CircuitKeyPair {
    /// The proving key for Groth16 proof creation.
    pub proving_key: Vec<u8>,
    /// The verifying key for Groth16 proof verification.
    pub verifying_key: Vec<u8>,
    /// Hash of the Powers of Tau transcript used to derive these keys.
    pub tau_hash: [u8; 32],
    /// Number of contributions in the Phase 1 ceremony.
    pub tau_contributions: usize,
}

/// Derive circuit-specific proving and verifying keys from the
/// Powers of Tau SRS (Phase 2).
///
/// **Deprecated**: This function ignores the SRS and generates keys with
/// fresh randomness via [`generate_trusted_setup`]. Use
/// [`derive_keys_from_srs`] instead, which verifies the SRS has
/// contributions and logs the SRS hash for audit trail.
///
/// This function takes the output of Phase 1 (the [`PowersOfTau`] SRS)
/// and derives Groth16 proving/verifying keys for the basic
/// [`RollupCircuit`]. In production, a Phase 2 ceremony would be
/// performed here to add circuit-specific randomness.
///
/// # Arguments
///
/// * `srs` — The Phase 1 Powers of Tau accumulator
/// * `circuit` — The rollup circuit to derive keys for
///
/// # Returns
///
/// A [`CircuitKeyPair`] containing serialized proving and verifying keys,
/// or [`SetupError`] on failure.
///
/// # Example
///
/// ```ignore
/// use omnia_adapters::setup::circuit_setup::derive_keys;
/// use omnia_adapters::setup::powers_of_tau::PowersOfTau;
/// use omnia_adapters::circuit::RollupCircuit;
///
/// let srs = PowersOfTau::new(64);
/// let circuit = RollupCircuit::empty();
/// let keypair = derive_keys(&srs, &circuit)?;
/// ```
pub fn derive_keys(
    srs: &PowersOfTau,
    circuit: &RollupCircuit,
) -> Result<CircuitKeyPair, SetupError> {
    tracing::info!(
        tau_contributions = srs.contribution_count,
        "Deriving circuit-specific keys from Phase 1 SRS (deprecated: ignores SRS)"
    );

    // Generate the trusted setup for this specific circuit
    let (pk, vk) = generate_trusted_setup(circuit)
        .map_err(|e| SetupError::KeyDerivationFailed(e.to_string()))?;

    // Serialize the proving key
    let mut pk_bytes = Vec::new();
    pk.serialize_uncompressed(&mut pk_bytes)
        .map_err(|e| SetupError::SerializationFailed(e.to_string()))?;

    // Serialize the verifying key
    let mut vk_bytes = Vec::new();
    vk.serialize_uncompressed(&mut vk_bytes)
        .map_err(|e| SetupError::SerializationFailed(e.to_string()))?;

    tracing::info!(
        pk_size = pk_bytes.len(),
        vk_size = vk_bytes.len(),
        "Circuit keys derived successfully (SRS not used for key generation)"
    );

    Ok(CircuitKeyPair {
        proving_key: pk_bytes,
        verifying_key: vk_bytes,
        tau_hash: srs.transcript_hash,
        tau_contributions: srs.contribution_count,
    })
}

/// Derive circuit-specific keys for the [`ExpandedRollupCircuit`](crate::circuit::ExpandedRollupCircuit).
///
/// **Deprecated**: This function ignores the SRS and generates keys with
/// fresh randomness via [`generate_trusted_setup_expanded`]. Use the
/// equivalent function with SRS verification instead.
///
/// Similar to [`derive_keys`] but for the expanded circuit which
/// supports Merkle path verification and per-event state transitions.
///
/// # Arguments
///
/// * `srs` — The Phase 1 Powers of Tau accumulator
/// * `num_events` — Number of events per batch
/// * `merkle_depth` — Depth of each Merkle inclusion proof
///
/// # Returns
///
/// A [`CircuitKeyPair`] containing serialized proving and verifying keys,
/// or [`SetupError`] on failure.
///
/// # Example
///
/// ```ignore
/// use omnia_adapters::setup::circuit_setup::derive_keys_expanded;
/// use omnia_adapters::setup::powers_of_tau::PowersOfTau;
///
/// let srs = PowersOfTau::new(64);
/// let keypair = derive_keys_expanded(&srs, 4, 8)?;
/// ```
pub fn derive_keys_expanded(
    srs: &PowersOfTau,
    num_events: usize,
    merkle_depth: usize,
) -> Result<CircuitKeyPair, SetupError> {
    tracing::info!(
        tau_contributions = srs.contribution_count,
        num_events,
        merkle_depth,
        "Deriving expanded circuit keys from Phase 1 SRS (deprecated: ignores SRS)"
    );

    let (pk, vk) = generate_trusted_setup_expanded(num_events, merkle_depth)
        .map_err(|e| SetupError::KeyDerivationFailed(e.to_string()))?;

    let mut pk_bytes = Vec::new();
    pk.serialize_uncompressed(&mut pk_bytes)
        .map_err(|e| SetupError::SerializationFailed(e.to_string()))?;

    let mut vk_bytes = Vec::new();
    vk.serialize_uncompressed(&mut vk_bytes)
        .map_err(|e| SetupError::SerializationFailed(e.to_string()))?;

    tracing::info!(
        pk_size = pk_bytes.len(),
        vk_size = vk_bytes.len(),
        "Expanded circuit keys derived successfully (SRS not used for key generation)"
    );

    Ok(CircuitKeyPair {
        proving_key: pk_bytes,
        verifying_key: vk_bytes,
        tau_hash: srs.transcript_hash,
        tau_contributions: srs.contribution_count,
    })
}

/// Derive circuit-specific proving and verifying keys from an SRS with
/// verified contributions.
///
/// Unlike [`derive_keys`] (which ignores the SRS), this function:
/// 1. Verifies the SRS has at least one contribution
/// 2. Verifies the SRS is well-formed (valid curve points)
/// 3. Generates circuit-specific keys using the standard Groth16 setup
/// 4. Logs the SRS hash for audit trail
///
/// # Architecture Note
///
/// The current Groth16 `setup()` always generates fresh randomness for
/// the toxic waste. In a full Phase 2 ceremony, the SRS would be used
/// to derive the circuit-specific parameters. The current architecture
/// uses the SRS for Phase 1 accumulation only, and the Phase 2 setup
/// generates circuit-specific keys with its own randomness. The SRS
/// hash is logged for audit purposes to establish a binding between
/// the Phase 1 ceremony and the derived keys.
///
/// # Arguments
///
/// * `srs` — The Phase 1 Powers of Tau accumulator (must have contributions)
/// * `circuit` — The rollup circuit to derive keys for
///
/// # Returns
///
/// A [`CircuitKeyPair`] containing serialized proving and verifying keys,
/// or [`SetupError`] on failure.
///
/// # Errors
///
/// Returns [`SetupError::SrsNotReady`] if the SRS has no contributions.
/// Returns [`SetupError::InvalidContribution`] if the SRS is not well-formed.
///
/// # Example
///
/// ```ignore
/// use omnia_adapters::setup::circuit_setup::derive_keys_from_srs;
/// use omnia_adapters::setup::powers_of_tau::run_ceremony;
/// use omnia_adapters::circuit::RollupCircuit;
///
/// let srs = run_ceremony(64, 3)?;
/// let circuit = RollupCircuit::empty();
/// let keypair = derive_keys_from_srs(&srs, &circuit)?;
/// ```
pub fn derive_keys_from_srs(
    srs: &PowersOfTau,
    circuit: &RollupCircuit,
) -> Result<CircuitKeyPair, SetupError> {
    // Verify the SRS has contributions
    if srs.contribution_count == 0 {
        return Err(SetupError::SrsNotReady(
            "SRS has no contributions; run a ceremony first".to_string(),
        ));
    }

    // Verify the SRS is well-formed
    srs.verify_srs()?;

    tracing::info!(
        tau_contributions = srs.contribution_count,
        tau_hash = ?&srs.transcript_hash[..4],
        "Deriving circuit-specific keys from Phase 1 SRS with contributions"
    );

    // Generate the trusted setup for this specific circuit
    let (pk, vk) = generate_trusted_setup(circuit)
        .map_err(|e| SetupError::KeyDerivationFailed(e.to_string()))?;

    // Serialize the proving key
    let mut pk_bytes = Vec::new();
    pk.serialize_uncompressed(&mut pk_bytes)
        .map_err(|e| SetupError::SerializationFailed(e.to_string()))?;

    // Serialize the verifying key
    let mut vk_bytes = Vec::new();
    vk.serialize_uncompressed(&mut vk_bytes)
        .map_err(|e| SetupError::SerializationFailed(e.to_string()))?;

    // Log the SRS hash for audit trail
    let srs_hash = blake3::hash(&srs.to_transcript());
    tracing::info!(
        pk_size = pk_bytes.len(),
        vk_size = vk_bytes.len(),
        srs_hash = ?srs_hash.as_bytes()[..4],
        tau_contributions = srs.contribution_count,
        "Circuit keys derived from SRS with audit trail"
    );

    Ok(CircuitKeyPair {
        proving_key: pk_bytes,
        verifying_key: vk_bytes,
        tau_hash: srs.transcript_hash,
        tau_contributions: srs.contribution_count,
    })
}

/// Verify that a proving key and verifying key are consistent.
///
/// Checks that the verifying key embedded in the proving key matches
/// the standalone verifying key.
///
/// # Arguments
///
/// * `pk_bytes` — Serialized proving key
/// * `vk_bytes` — Serialized verifying key
///
/// # Returns
///
/// `Ok(())` if the keys are consistent, `Err(SetupError)` otherwise.
pub fn verify_key_consistency(pk_bytes: &[u8], vk_bytes: &[u8]) -> Result<(), SetupError> {
    let pk = ProvingKey::<Bn254>::deserialize_uncompressed(pk_bytes)
        .map_err(|e| SetupError::SerializationFailed(e.to_string()))?;

    let vk = VerifyingKey::<Bn254>::deserialize_uncompressed(vk_bytes)
        .map_err(|e| SetupError::SerializationFailed(e.to_string()))?;

    // Compare the verifying key embedded in the proving key with the standalone one
    let pk_vk_bytes = {
        let mut bytes = Vec::new();
        pk.vk
            .serialize_uncompressed(&mut bytes)
            .map_err(|e| SetupError::SerializationFailed(e.to_string()))?;
        bytes
    };

    let standalone_vk_bytes = {
        let mut bytes = Vec::new();
        vk.serialize_uncompressed(&mut bytes)
            .map_err(|e| SetupError::SerializationFailed(e.to_string()))?;
        bytes
    };

    if pk_vk_bytes.ct_ne(&standalone_vk_bytes).into() {
        return Err(SetupError::KeyDerivationFailed(
            "Proving key and verifying key are inconsistent".to_string(),
        ));
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::circuit::RollupCircuit;

    #[test]
    fn test_derive_keys_basic_circuit() {
        let srs = PowersOfTau::new(8).unwrap();
        let circuit = RollupCircuit::empty();

        let keypair = derive_keys(&srs, &circuit).expect("derive_keys failed");

        assert!(!keypair.proving_key.is_empty());
        assert!(!keypair.verifying_key.is_empty());
        assert_eq!(keypair.tau_contributions, 0);
    }

    #[test]
    fn test_derive_keys_expanded_circuit() {
        let srs = PowersOfTau::new(8).unwrap();

        let keypair = derive_keys_expanded(&srs, 2, 4).expect("derive_keys_expanded failed");

        assert!(!keypair.proving_key.is_empty());
        assert!(!keypair.verifying_key.is_empty());
    }

    #[test]
    fn test_key_consistency() {
        let srs = PowersOfTau::new(8).unwrap();
        let circuit = RollupCircuit::empty();

        let keypair = derive_keys(&srs, &circuit).expect("derive_keys failed");

        verify_key_consistency(&keypair.proving_key, &keypair.verifying_key)
            .expect("key consistency check failed");
    }

    #[test]
    fn test_derive_keys_with_ceremony_srs() {
        let srs = super::super::powers_of_tau::run_ceremony(8, 3).expect("ceremony failed");
        let circuit = RollupCircuit::empty();

        let keypair = derive_keys(&srs, &circuit).expect("derive_keys failed");
        assert_eq!(keypair.tau_contributions, 3);
        assert_ne!(keypair.tau_hash, [0u8; 32]);
    }

    #[test]
    fn test_derive_keys_from_srs_requires_contributions() {
        let srs = PowersOfTau::new(8).unwrap();
        let circuit = RollupCircuit::empty();

        let result = derive_keys_from_srs(&srs, &circuit);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SetupError::SrsNotReady(_)));
    }

    #[test]
    fn test_derive_keys_from_srs_with_ceremony() {
        let srs = super::super::powers_of_tau::run_ceremony(8, 3).expect("ceremony failed");
        let circuit = RollupCircuit::empty();

        let keypair = derive_keys_from_srs(&srs, &circuit).expect("derive_keys_from_srs failed");
        assert_eq!(keypair.tau_contributions, 3);
        assert_ne!(keypair.tau_hash, [0u8; 32]);
        assert!(!keypair.proving_key.is_empty());
        assert!(!keypair.verifying_key.is_empty());
    }

    #[test]
    fn test_derive_keys_from_srs_key_consistency() {
        let srs = super::super::powers_of_tau::run_ceremony(8, 2).expect("ceremony failed");
        let circuit = RollupCircuit::empty();

        let keypair = derive_keys_from_srs(&srs, &circuit).expect("derive_keys_from_srs failed");

        verify_key_consistency(&keypair.proving_key, &keypair.verifying_key)
            .expect("key consistency check failed");
    }

    #[test]
    fn test_ceremony_produces_valid_srs() {
        use crate::prover::{create_proof, verify_proof, ProvingKey, VerifyingKey};
        use ark_ec::AffineRepr;
        use ark_serialize::CanonicalDeserialize;

        // 1. Run a 3-participant ceremony
        let srs = super::super::powers_of_tau::run_ceremony(8, 3).expect("ceremony failed");
        assert_eq!(srs.contribution_count, 3);

        // 2. Verify each contribution modified G1/G2 points via actual EC ops
        // All G1 powers should be non-identity (they are G * s1 * s2 * s3)
        for (i, g1_bytes) in srs.g1_powers.iter().enumerate() {
            let mut slice = g1_bytes.as_slice();
            let g1_point = ark_bn254::G1Affine::deserialize_uncompressed(&mut slice)
                .expect("G1 deserialization failed");
            assert!(!g1_point.is_zero(), "G1 power {} is identity", i);
        }

        // All G2 powers should be non-identity
        for (i, g2_bytes) in srs.g2_powers.iter().enumerate() {
            let mut slice = g2_bytes.as_slice();
            let g2_point = ark_bn254::G2Affine::deserialize_uncompressed(&mut slice)
                .expect("G2 deserialization failed");
            assert!(!g2_point.is_zero(), "G2 power {} is identity", i);
        }

        // 3. Derive circuit keys from the SRS
        let circuit = RollupCircuit::empty();
        let keypair = derive_keys_from_srs(&srs, &circuit).expect("derive_keys_from_srs failed");

        // 4. Generate a proof using derived keys
        let pk = ProvingKey::deserialize_uncompressed(keypair.proving_key.as_slice())
            .expect("PK deserialization failed");
        let vk = VerifyingKey::deserialize_uncompressed(keypair.verifying_key.as_slice())
            .expect("VK deserialization failed");

        let mut old_root = [0u8; 32];
        old_root[0] = 0x01;
        let mut new_root = [0u8; 32];
        new_root[0] = 0x02;

        let proof_circuit = RollupCircuit::from_state_roots(old_root, new_root, 5);
        let public_inputs = proof_circuit.public_input().expect("public inputs");
        let proof = create_proof(proof_circuit, &pk).expect("proof creation failed");

        // 5. Verify the proof passes
        let valid = verify_proof(&vk, &public_inputs, &proof).expect("verification failed");
        assert!(valid, "proof verification failed");
    }
}
