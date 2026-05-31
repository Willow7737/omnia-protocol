//! Physical Anchor — The Unified Binding Type
//!
//! A `PhysicalAnchor` combines all three pillars of the Binding Layer:
//!
//! 1. **RF Fingerprint** — Proves the physical device's identity
//! 2. **Quantum Commitment** — Ensures long-term cryptographic integrity
//! 3. **Provenance Log** — Records the complete chain of custody
//!
//! This is the core type that replaces trusted oracle attestations with
//! cryptographic self-proof. When a physical claim needs to be verified
//! (e.g., "this diamond is real", "this shipment arrived"), the verifier
//! checks the `PhysicalAnchor` rather than trusting a third party.

use crate::provenance::ProvenanceLog;
use crate::quantum_commit::{CommitmentPhase, PqPublicKey, QuantumCommitment};
use crate::rf_fingerprint::RfFingerprint;

/// The unified physical anchor that binds a real-world item to the
/// digital causal graph.
///
/// A `PhysicalAnchor` is created when an item is first registered on-chain
/// and is updated with each provenance event (transfer, verification, etc.).
/// It can be verified at any time by checking:
///
/// 1. The RF fingerprint matches the current measurement
/// 2. The quantum commitment is valid
/// 3. The provenance chain is intact
#[derive(Debug, Clone)]
pub struct PhysicalAnchor {
    /// The item's RF fingerprint (physical identity proof).
    pub rf_fingerprint: RfFingerprint,
    /// The quantum-resistant commitment (long-term integrity proof).
    pub quantum_commitment: QuantumCommitment,
    /// The provenance log (chain of custody).
    pub provenance_log: ProvenanceLog,
    /// The commitment phase used when the anchor was created/last updated.
    /// This is stored alongside the commitment so that `verify()` uses the
    /// correct phase instead of hardcoding `ClassicalOnly`, which would
    /// cause verification to fail for anchors created during Hybrid or
    /// PostQuantum phases.
    pub commitment_phase: CommitmentPhase,
}

impl PhysicalAnchor {
    /// Create a new PhysicalAnchor for a freshly anchored item.
    ///
    /// # Arguments
    ///
    /// * `rf_fingerprint` — The item's initial RF fingerprint
    /// * `quantum_commitment` — The initial quantum commitment
    /// * `provenance_log` — The provenance log (should have at least a
    ///   `Created` event)
    pub fn new(
        rf_fingerprint: RfFingerprint,
        quantum_commitment: QuantumCommitment,
        provenance_log: ProvenanceLog,
        commitment_phase: CommitmentPhase,
    ) -> Self {
        Self {
            rf_fingerprint,
            quantum_commitment,
            provenance_log,
            commitment_phase,
        }
    }

    /// Verify the physical anchor against a current RF measurement and
    /// a public key.
    ///
    /// This performs all three verification checks:
    ///
    /// 1. **RF verification**: The current RF measurement matches the
    ///    stored fingerprint.
    /// 2. **Quantum commitment verification**: The commitment is
    ///    cryptographically valid.
    /// 3. **Provenance chain verification**: The chain of custody is
    ///    intact (every link is valid).
    ///
    /// # Arguments
    ///
    /// * `current_rf` — The freshly measured 32-byte RF spectral hash
    /// * `public_key` — The signer's public key for commitment verification
    ///
    /// # Returns
    ///
    /// `true` if all three checks pass.
    pub fn verify(&self, current_rf: &[u8; 32], public_key: &PqPublicKey) -> bool {
        // 1. Verify RF fingerprint
        if !self.rf_fingerprint.verify(current_rf) {
            return false;
        }

        // 2. Verify quantum commitment
        let provenance_bytes = match self.provenance_log.to_bytes() {
            Ok(b) => b,
            Err(_) => return false,
        };
        if !self
            .quantum_commitment
            .verify(public_key, &provenance_bytes, self.commitment_phase)
        {
            return false;
        }

        // 3. Verify provenance chain integrity
        if !self.provenance_log.verify_chain() {
            return false;
        }

        true
    }

    /// Verify only the provenance chain integrity (without RF measurement).
    ///
    /// Useful for offline verification or when RF measurement hardware
    /// is unavailable.
    pub fn verify_chain_only(&self) -> bool {
        self.provenance_log.verify_chain()
    }

    /// Get the current holder of the item from the provenance log.
    pub fn current_holder(&self) -> &str {
        &self.provenance_log.current_holder
    }

    /// Check if the item has been destroyed.
    pub fn is_destroyed(&self) -> bool {
        self.provenance_log.is_destroyed()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::provenance::ProvenanceLog;
    use crate::quantum_commit::QuantumCommitment;
    use crate::rf_fingerprint::RfFingerprint;
    use omnia_substrate::{generate_keypair, NodeKeypair, VectorClock};

    fn test_rf(did: &str, hash: [u8; 32]) -> RfFingerprint {
        RfFingerprint::stub(did, hash)
    }

    fn test_keypair_and_pk() -> (NodeKeypair, PqPublicKey) {
        let kp = generate_keypair();
        let pk = PqPublicKey {
            ed25519: kp.verifying_key().to_bytes(),
            dilithium: Vec::new(),
        };
        (kp, pk)
    }

    fn test_commitment_signed(data: &[u8], kp: &NodeKeypair) -> QuantumCommitment {
        QuantumCommitment::sign_classical(data, kp).unwrap()
    }

    fn test_commitment_with_vc(data: &[u8], kp: &NodeKeypair, _vc: &VectorClock) -> QuantumCommitment {
        QuantumCommitment::sign_classical(data, kp).unwrap()
    }

    fn test_anchor_id() -> [u8; 32] {
        [0xEFu8; 32]
    }

    #[test]
    fn test_create_and_verify_anchor() {
        let (kp, pk) = test_keypair_and_pk();
        let rf_hash = [0x55u8; 32];
        let rf = test_rf("did:omnia:factory", rf_hash);
        let vc = VectorClock::new();

        // Create provenance log first so we can commit to its bytes
        let provenance = ProvenanceLog::new(
            test_anchor_id(),
            "did:omnia:factory".to_string(),
            test_rf("did:omnia:factory", rf_hash),
            test_commitment_with_vc(b"creation", &kp, &vc),
            [0xCDu8; 32],
        );

        // Create commitment over the provenance log bytes (as verify() expects)
        let commitment = test_commitment_signed(&provenance.to_bytes().unwrap(), &kp);

        let anchor = PhysicalAnchor::new(rf, commitment, provenance, CommitmentPhase::ClassicalOnly);

        // Verify with matching RF and correct public key
        assert!(anchor.verify(&rf_hash, &pk));
    }

    #[test]
    fn test_verify_fails_with_wrong_rf() {
        let (kp, pk) = test_keypair_and_pk();
        let rf_hash = [0x55u8; 32];
        let wrong_rf = [0xFFu8; 32];
        let rf = test_rf("did:omnia:factory", rf_hash);
        let vc = VectorClock::new();

        let provenance = ProvenanceLog::new(
            test_anchor_id(),
            "did:omnia:factory".to_string(),
            test_rf("did:omnia:factory", rf_hash),
            test_commitment_with_vc(b"creation", &kp, &vc),
            [0xCDu8; 32],
        );
        let commitment = test_commitment_signed(&provenance.to_bytes().unwrap(), &kp);

        let anchor = PhysicalAnchor::new(rf, commitment, provenance, CommitmentPhase::ClassicalOnly);

        // Verify with wrong RF should fail
        assert!(!anchor.verify(&wrong_rf, &pk));
    }

    #[test]
    fn test_verify_chain_only() {
        let (kp, _) = test_keypair_and_pk();
        let rf_hash = [0x55u8; 32];
        let rf = test_rf("did:omnia:factory", rf_hash);
        let vc = VectorClock::new();

        let provenance = ProvenanceLog::new(
            test_anchor_id(),
            "did:omnia:factory".to_string(),
            test_rf("did:omnia:factory", rf_hash),
            test_commitment_with_vc(b"creation", &kp, &vc),
            [0xCDu8; 32],
        );
        let commitment = test_commitment_signed(&provenance.to_bytes().unwrap(), &kp);

        let anchor = PhysicalAnchor::new(rf, commitment, provenance, CommitmentPhase::ClassicalOnly);
        assert!(anchor.verify_chain_only());
    }

    #[test]
    fn test_current_holder() {
        let (kp, _) = test_keypair_and_pk();
        let rf_hash = [0x55u8; 32];
        let rf = test_rf("did:omnia:alice", rf_hash);
        let vc = VectorClock::new();

        let mut provenance = ProvenanceLog::new(
            test_anchor_id(),
            "did:omnia:alice".to_string(),
            test_rf("did:omnia:alice", rf_hash),
            test_commitment_with_vc(b"creation", &kp, &vc),
            [0xCDu8; 32],
        );

        provenance
            .transfer(
                "did:omnia:bob".to_string(),
                test_rf("did:omnia:bob", [0x66u8; 32]),
                test_commitment_signed(b"transfer1", &kp),
            )
            .unwrap();

        let commitment = test_commitment_signed(&provenance.to_bytes().unwrap(), &kp);
        let anchor = PhysicalAnchor::new(rf, commitment, provenance, CommitmentPhase::ClassicalOnly);
        assert_eq!(anchor.current_holder(), "did:omnia:bob");
    }
}
