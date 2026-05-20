//! Crypto Agility — Scheme Versioning for Post-Quantum Migration.
//!
//! This module provides a versioning system for all cryptographic primitives
//! used in the Omnia Protocol. Each primitive (signatures, hashes, VRFs, ZK
//! proofs) is identified by a scheme version, enabling smooth migration when
//! a primitive is compromised or when quantum computers threaten classical
//! assumptions.
//!
//! # Motivation
//!
//! Cryptographic primitives have a limited lifespan. Advances in cryptanalysis
//! or the advent of large-scale quantum computers could break currently secure
//! algorithms. This module ensures that the protocol can migrate to new schemes
//! without requiring a hard fork — old and new schemes coexist during the
//! transition period, and consensus rules gradually phase out deprecated schemes.
//!
//! # Migration Playbook
//!
//! See `docs/CRYPTO_MIGRATION.md` for the step-by-step migration procedure
//! for each primitive compromise scenario and the Q-Day procedure.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Version tag for a cryptographic scheme.
///
/// Schemes are versioned to support in-protocol migration. When a scheme is
/// deprecated, a new version is added, and the protocol enters a transition
/// period where both old and new versions are accepted. After the transition,
/// the old version is rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SchemeVersion(pub u8);

/// Well-known scheme version constants.
impl SchemeVersion {
    /// First version — the default scheme at protocol launch.
    pub const V1: Self = SchemeVersion(1);
    /// Second version — introduced during a migration.
    pub const V2: Self = SchemeVersion(2);
    /// Third version — future use.
    pub const V3: Self = SchemeVersion(3);
}

impl fmt::Display for SchemeVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{self.0}")
    }
}

impl PartialOrd for SchemeVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SchemeVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

/// Digital signature schemes supported by the protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SignatureScheme {
    /// Ed25519 — fast, compact, widely deployed. Not quantum-safe.
    Ed25519V1,
    /// Hybrid Ed25519 + Dilithium3 — classical + post-quantum, for transition.
    HybridV1,
    /// Pure CRYSTALS-Dilithium3 — NIST PQC standard, quantum-safe.
    DilithiumV2,
}

impl SignatureScheme {
    /// Returns the scheme version for this signature scheme.
    pub fn version(&self) -> SchemeVersion {
        match self {
            SignatureScheme::Ed25519V1 => SchemeVersion::V1,
            SignatureScheme::HybridV1 => SchemeVersion::V1,
            SignatureScheme::DilithiumV2 => SchemeVersion::V2,
        }
    }

    /// Returns `true` if this scheme provides post-quantum security.
    pub fn is_quantum_safe(&self) -> bool {
        matches!(
            self,
            SignatureScheme::HybridV1 | SignatureScheme::DilithiumV2
        )
    }

    /// Returns the expected public key size in bytes for this scheme.
    pub fn public_key_size(&self) -> usize {
        match self {
            SignatureScheme::Ed25519V1 => 32,
            SignatureScheme::HybridV1 => 32 + 1952,
            SignatureScheme::DilithiumV2 => 1952,
        }
    }

    /// Returns the expected signature size in bytes for this scheme.
    pub fn signature_size(&self) -> usize {
        match self {
            SignatureScheme::Ed25519V1 => 64,
            SignatureScheme::HybridV1 => 64 + 3293,
            SignatureScheme::DilithiumV2 => 3293,
        }
    }
}

impl fmt::Display for SignatureScheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SignatureScheme::Ed25519V1 => write!(f, "Ed25519/v1"),
            SignatureScheme::HybridV1 => write!(f, "Hybrid-Ed25519-Dilithium3/v1"),
            SignatureScheme::DilithiumV2 => write!(f, "Dilithium3/v2"),
        }
    }
}

/// Hash function schemes supported by the protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HashScheme {
    /// BLAKE3-256 — default, fast, secure.
    Blake3V1,
    /// SHA3-256 — NIST standard, slower but conservative.
    Sha3V2,
    /// BLAKE3-512 — extended output for higher collision resistance.
    Blake3bV3,
}

impl HashScheme {
    /// Returns the scheme version.
    pub fn version(&self) -> SchemeVersion {
        match self {
            HashScheme::Blake3V1 => SchemeVersion::V1,
            HashScheme::Sha3V2 => SchemeVersion::V2,
            HashScheme::Blake3bV3 => SchemeVersion::V3,
        }
    }

    /// Returns the output digest size in bytes.
    pub fn digest_size(&self) -> usize {
        match self {
            HashScheme::Blake3V1 => 32,
            HashScheme::Sha3V2 => 32,
            HashScheme::Blake3bV3 => 64,
        }
    }
}

impl fmt::Display for HashScheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HashScheme::Blake3V1 => write!(f, "BLAKE3-256/v1"),
            HashScheme::Sha3V2 => write!(f, "SHA3-256/v2"),
            HashScheme::Blake3bV3 => write!(f, "BLAKE3-512/v3"),
        }
    }
}

/// VRF (Verifiable Random Function) schemes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VrfScheme {
    /// Ed25519-VRF — based on Ed25519, fast, not quantum-safe.
    Ed25519VrfV1,
    /// BLS-VRF — BLS-based VRF for threshold settings.
    BlsVrfV2,
    /// Dilithium-VRF — post-quantum VRF based on Dilithium.
    DilithiumVrfV2,
}

impl VrfScheme {
    /// Returns the scheme version.
    pub fn version(&self) -> SchemeVersion {
        match self {
            VrfScheme::Ed25519VrfV1 => SchemeVersion::V1,
            VrfScheme::BlsVrfV2 => SchemeVersion::V2,
            VrfScheme::DilithiumVrfV2 => SchemeVersion::V2,
        }
    }

    /// Returns `true` if this scheme provides post-quantum security.
    pub fn is_quantum_safe(&self) -> bool {
        matches!(self, VrfScheme::DilithiumVrfV2)
    }
}

impl fmt::Display for VrfScheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VrfScheme::Ed25519VrfV1 => write!(f, "Ed25519-VRF/v1"),
            VrfScheme::BlsVrfV2 => write!(f, "BLS-VRF/v2"),
            VrfScheme::DilithiumVrfV2 => write!(f, "Dilithium-VRF/v2"),
        }
    }
}

/// Zero-knowledge proof schemes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ZkScheme {
    /// Groth16 on BN254 — compact proofs, trusted setup required.
    Groth16Bn254V1,
    /// Groth16 on BLS12-381 — stronger curve, universal setup.
    Groth16Bls12381V2,
    /// PLONK — universal trusted setup, transparent, future-proof.
    PlonkV3,
}

impl ZkScheme {
    /// Returns the scheme version.
    pub fn version(&self) -> SchemeVersion {
        match self {
            ZkScheme::Groth16Bn254V1 => SchemeVersion::V1,
            ZkScheme::Groth16Bls12381V2 => SchemeVersion::V2,
            ZkScheme::PlonkV3 => SchemeVersion::V3,
        }
    }
}

impl fmt::Display for ZkScheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ZkScheme::Groth16Bn254V1 => write!(f, "Groth16/BN254/v1"),
            ZkScheme::Groth16Bls12381V2 => write!(f, "Groth16/BLS12-381/v2"),
            ZkScheme::PlonkV3 => write!(f, "PLONK/v3"),
        }
    }
}

/// A complete profile of all cryptographic schemes used by a node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CryptoProfile {
    /// Active signature scheme (used for producing new signatures).
    pub signature: SignatureScheme,
    /// Accepted signature schemes (used for verifying existing signatures).
    pub accepted_signatures: Vec<SignatureScheme>,
    /// Active hash scheme.
    pub hash: HashScheme,
    /// Accepted hash schemes.
    pub accepted_hashes: Vec<HashScheme>,
    /// Active VRF scheme.
    pub vrf: VrfScheme,
    /// Accepted VRF schemes.
    pub accepted_vrfs: Vec<VrfScheme>,
    /// Active ZK scheme.
    pub zk: ZkScheme,
    /// Accepted ZK schemes.
    pub accepted_zks: Vec<ZkScheme>,
}

impl Default for CryptoProfile {
    fn default() -> Self {
        Self {
            signature: SignatureScheme::Ed25519V1,
            accepted_signatures: vec![SignatureScheme::Ed25519V1],
            hash: HashScheme::Blake3V1,
            accepted_hashes: vec![HashScheme::Blake3V1],
            vrf: VrfScheme::Ed25519VrfV1,
            accepted_vrfs: vec![VrfScheme::Ed25519VrfV1],
            zk: ZkScheme::Groth16Bn254V1,
            accepted_zks: vec![ZkScheme::Groth16Bn254V1],
        }
    }
}

impl CryptoProfile {
    /// Create a new crypto profile with the default (V1) schemes.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a hybrid profile that accepts both V1 and V2 signature schemes.
    pub fn hybrid_migration() -> Self {
        Self {
            signature: SignatureScheme::HybridV1,
            accepted_signatures: vec![
                SignatureScheme::Ed25519V1,
                SignatureScheme::HybridV1,
                SignatureScheme::DilithiumV2,
            ],
            hash: HashScheme::Blake3V1,
            accepted_hashes: vec![HashScheme::Blake3V1, HashScheme::Sha3V2],
            vrf: VrfScheme::Ed25519VrfV1,
            accepted_vrfs: vec![VrfScheme::Ed25519VrfV1, VrfScheme::BlsVrfV2],
            zk: ZkScheme::Groth16Bn254V1,
            accepted_zks: vec![ZkScheme::Groth16Bn254V1, ZkScheme::Groth16Bls12381V2],
        }
    }

    /// Create a fully post-quantum profile (Q-Day configuration).
    pub fn post_quantum() -> Self {
        Self {
            signature: SignatureScheme::DilithiumV2,
            accepted_signatures: vec![SignatureScheme::HybridV1, SignatureScheme::DilithiumV2],
            hash: HashScheme::Blake3bV3,
            accepted_hashes: vec![HashScheme::Sha3V2, HashScheme::Blake3bV3],
            vrf: VrfScheme::DilithiumVrfV2,
            accepted_vrfs: vec![VrfScheme::BlsVrfV2, VrfScheme::DilithiumVrfV2],
            zk: ZkScheme::PlonkV3,
            accepted_zks: vec![ZkScheme::Groth16Bls12381V2, ZkScheme::PlonkV3],
        }
    }

    /// Check whether a given signature scheme is accepted by this profile.
    pub fn accepts_signature(&self, scheme: SignatureScheme) -> bool {
        self.accepted_signatures.contains(&scheme)
    }

    /// Check whether a given hash scheme is accepted by this profile.
    pub fn accepts_hash(&self, scheme: HashScheme) -> bool {
        self.accepted_hashes.contains(&scheme)
    }

    /// Check whether a given VRF scheme is accepted by this profile.
    pub fn accepts_vrf(&self, scheme: VrfScheme) -> bool {
        self.accepted_vrfs.contains(&scheme)
    }

    /// Check whether a given ZK scheme is accepted by this profile.
    pub fn accepts_zk(&self, scheme: ZkScheme) -> bool {
        self.accepted_zks.contains(&scheme)
    }

    /// Returns `true` if all active schemes are quantum-safe.
    pub fn is_fully_quantum_safe(&self) -> bool {
        self.signature.is_quantum_safe() && self.vrf.is_quantum_safe()
    }

    /// Returns the minimum required crypto profile for a given date.
    ///
    /// This is used to enforce migration deadlines — after a certain date,
    /// nodes must use at least the specified scheme versions. The returned
    /// profile describes the *minimum* acceptable schemes; nodes may use
    /// stronger schemes.
    ///
    /// # Arguments
    ///
    /// * `date` — The date as a UNIX timestamp (seconds since epoch).
    ///
    /// # Migration Schedule
    ///
    /// | Date | Minimum Profile |
    /// |------|-----------------|
    /// | Before 2026-01-01 | Default (V1) |
    /// | 2026-01-01 to 2027-12-31 | Hybrid migration |
    /// | 2028-01-01 onwards | Post-quantum |
    pub fn minimum_for_date(date: u64) -> Self {
        // Post-2028: require post-quantum signatures and VRF
        // 2028-01-01 00:00:00 UTC = 1830297600
        if date >= 1_830_297_600 {
            Self::post_quantum()
        }
        // Post-2026: require hybrid migration profile
        // 2026-01-01 00:00:00 UTC = 1767225600
        else if date >= 1_767_225_600 {
            Self::hybrid_migration()
        }
        // Default: V1 schemes acceptable
        else {
            Self::default()
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_scheme_version_ordering() {
        assert!(SchemeVersion::V2 > SchemeVersion::V1);
        assert!(SchemeVersion::V3 > SchemeVersion::V2);
        assert!(SchemeVersion::V3 > SchemeVersion::V1);
        assert!(SchemeVersion(5) > SchemeVersion(3));
    }

    #[test]
    fn test_scheme_version_display() {
        assert_eq!(format!("{}", SchemeVersion::V1), "v1");
        assert_eq!(format!("{}", SchemeVersion::V2), "v2");
        assert_eq!(format!("{}", SchemeVersion::V3), "v3");
        assert_eq!(format!("{}", SchemeVersion(42)), "v42");
    }

    #[test]
    fn test_scheme_version_equality() {
        assert_eq!(SchemeVersion::V1, SchemeVersion(1));
        assert_eq!(SchemeVersion::V2, SchemeVersion(2));
        assert_eq!(SchemeVersion::V3, SchemeVersion(3));
    }

    #[test]
    fn test_signature_scheme_properties() {
        assert!(!SignatureScheme::Ed25519V1.is_quantum_safe());
        assert!(SignatureScheme::HybridV1.is_quantum_safe());
        assert!(SignatureScheme::DilithiumV2.is_quantum_safe());
        assert_eq!(SignatureScheme::Ed25519V1.public_key_size(), 32);
        assert_eq!(SignatureScheme::Ed25519V1.signature_size(), 64);
    }

    #[test]
    fn test_signature_scheme_display() {
        assert_eq!(format!("{}", SignatureScheme::Ed25519V1), "Ed25519/v1");
    }

    #[test]
    fn test_hash_scheme_digest_sizes() {
        assert_eq!(HashScheme::Blake3V1.digest_size(), 32);
        assert_eq!(HashScheme::Sha3V2.digest_size(), 32);
        assert_eq!(HashScheme::Blake3bV3.digest_size(), 64);
    }

    #[test]
    fn test_vrf_scheme_quantum_safety() {
        assert!(!VrfScheme::Ed25519VrfV1.is_quantum_safe());
        assert!(!VrfScheme::BlsVrfV2.is_quantum_safe());
        assert!(VrfScheme::DilithiumVrfV2.is_quantum_safe());
    }

    #[test]
    fn test_vrf_scheme_versions() {
        assert_eq!(VrfScheme::Ed25519VrfV1.version(), SchemeVersion::V1);
        assert_eq!(VrfScheme::BlsVrfV2.version(), SchemeVersion::V2);
        assert_eq!(VrfScheme::DilithiumVrfV2.version(), SchemeVersion::V2);
    }

    #[test]
    fn test_zk_scheme_variants() {
        assert_eq!(ZkScheme::Groth16Bn254V1.version(), SchemeVersion::V1);
        assert_eq!(ZkScheme::Groth16Bls12381V2.version(), SchemeVersion::V2);
        assert_eq!(ZkScheme::PlonkV3.version(), SchemeVersion::V3);
    }

    #[test]
    fn test_zk_scheme_display() {
        assert_eq!(format!("{}", ZkScheme::PlonkV3), "PLONK/v3");
    }

    #[test]
    fn test_default_crypto_profile() {
        let profile = CryptoProfile::default();
        assert_eq!(profile.signature, SignatureScheme::Ed25519V1);
        assert_eq!(profile.hash, HashScheme::Blake3V1);
        assert_eq!(profile.vrf, VrfScheme::Ed25519VrfV1);
        assert_eq!(profile.zk, ZkScheme::Groth16Bn254V1);
        assert!(!profile.is_fully_quantum_safe());
    }

    #[test]
    fn test_hybrid_migration_profile() {
        let profile = CryptoProfile::hybrid_migration();
        assert!(profile.accepts_signature(SignatureScheme::Ed25519V1));
        assert!(profile.accepts_signature(SignatureScheme::HybridV1));
        assert!(profile.accepts_signature(SignatureScheme::DilithiumV2));
        assert!(profile.accepts_vrf(VrfScheme::BlsVrfV2));
        assert!(!profile.is_fully_quantum_safe());
    }

    #[test]
    fn test_post_quantum_profile() {
        let profile = CryptoProfile::post_quantum();
        assert!(profile.is_fully_quantum_safe());
        assert_eq!(profile.signature, SignatureScheme::DilithiumV2);
        assert_eq!(profile.vrf, VrfScheme::DilithiumVrfV2);
        assert_eq!(profile.zk, ZkScheme::PlonkV3);
    }

    #[test]
    fn test_accepts_methods() {
        let profile = CryptoProfile::default();
        assert!(profile.accepts_signature(SignatureScheme::Ed25519V1));
        assert!(!profile.accepts_signature(SignatureScheme::DilithiumV2));
        assert!(profile.accepts_hash(HashScheme::Blake3V1));
        assert!(!profile.accepts_hash(HashScheme::Sha3V2));
    }

    #[test]
    fn test_minimum_for_date_before_2026() {
        // 2024-01-01
        let profile = CryptoProfile::minimum_for_date(1_704_067_200);
        assert_eq!(profile.signature, SignatureScheme::Ed25519V1);
        assert_eq!(profile.vrf, VrfScheme::Ed25519VrfV1);
    }

    #[test]
    fn test_minimum_for_date_2026() {
        // 2026-06-01
        let profile = CryptoProfile::minimum_for_date(1_778_496_000);
        assert_eq!(profile.signature, SignatureScheme::HybridV1);
        assert!(profile.accepts_signature(SignatureScheme::Ed25519V1));
        assert!(profile.accepts_vrf(VrfScheme::BlsVrfV2));
    }

    #[test]
    fn test_minimum_for_date_2028() {
        // 2028-01-01
        let profile = CryptoProfile::minimum_for_date(1_830_297_600);
        assert_eq!(profile.signature, SignatureScheme::DilithiumV2);
        assert!(profile.is_fully_quantum_safe());
    }
}
