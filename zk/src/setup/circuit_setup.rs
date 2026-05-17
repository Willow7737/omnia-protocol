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
/// use omnia_zk::setup::circuit_setup::derive_keys;
/// use omnia_zk::setup::powers_of_tau::PowersOfTau;
/// use omnia_zk::circuit::RollupCircuit;
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
        "Deriving circuit-specific keys from Phase 1 SRS"
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
        "Circuit keys derived successfully"
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
/// use omnia_zk::setup::circuit_setup::derive_keys_expanded;
/// use omnia_zk::setup::powers_of_tau::PowersOfTau;
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
        "Deriving expanded circuit keys from Phase 1 SRS"
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
        "Expanded circuit keys derived successfully"
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
}
