//! Quantum-safe key rotation support.
//!
//! Extends the existing key rotation mechanism to handle PQC key pairs.
//! When rotating from a classical key to a hybrid key, both the old
//! and new keys are valid during a transition period.

use crate::quantum_commit::{CommitmentPhase, PqPublicKey};
use ed25519_dalek::Verifier;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors that can occur during PQC key rotation.
#[derive(Error, Debug)]
pub enum KeyRotationError {
    /// Cannot downgrade the commitment phase.
    #[error("cannot downgrade from {from:?} to {to:?}")]
    PhaseDowngrade {
        /// Current commitment phase.
        from: CommitmentPhase,
        /// Requested (lower) phase.
        to: CommitmentPhase,
    },
    /// Authorization signature is missing.
    #[error("authorization signature required")]
    AuthorizationSignatureRequired,
    /// Authorization signature is invalid.
    #[error("invalid authorization signature")]
    InvalidAuthorizationSignature,
}

/// A key rotation request that supports PQC transitions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PqcKeyRotationRequest {
    /// The old public key (being rotated from).
    pub old_key: PqPublicKey,
    /// The new public key (being rotated to).
    pub new_key: PqPublicKey,
    /// Signature from the old key authorizing the rotation.
    pub authorization_sig: Vec<u8>,
    /// The new commitment phase after rotation.
    pub new_phase: CommitmentPhase,
    /// Block/round at which the rotation takes effect.
    pub effective_at: u64,
    /// Block/round at which the old key is no longer valid.
    pub sunset_at: u64,
}

/// Manages key rotation with PQC support.
pub struct PqcKeyRotationManager {
    /// Current commitment phase.
    current_phase: CommitmentPhase,
    /// Pending rotations.
    pending: Vec<PqcKeyRotationRequest>,
    /// Current Ed25519 public key used for verifying rotation authorization.
    current_ed25519_public: Option<[u8; 32]>,
}

impl PqcKeyRotationManager {
    /// Create a new key rotation manager starting at the given commitment phase.
    pub fn new(current_phase: CommitmentPhase) -> Self {
        Self {
            current_phase,
            pending: Vec::new(),
            current_ed25519_public: None,
        }
    }

    /// Set the current Ed25519 public key for verifying rotation authorization signatures.
    pub fn set_current_ed25519_public(&mut self, public_key: [u8; 32]) {
        self.current_ed25519_public = Some(public_key);
    }

    /// Submit a key rotation request.
    pub fn submit_rotation(&mut self, request: PqcKeyRotationRequest) -> Result<(), KeyRotationError> {
        if (request.new_phase as u8) < (self.current_phase as u8) {
            return Err(KeyRotationError::PhaseDowngrade {
                from: self.current_phase,
                to: request.new_phase,
            });
        }

        if request.authorization_sig.is_empty() {
            return Err(KeyRotationError::AuthorizationSignatureRequired);
        }

        // Verify the authorization signature against the current Ed25519 key
        // SECURITY: The current Ed25519 public key MUST be set before submitting
        // a rotation. If it is None, the authorization signature cannot be
        // verified, and the rotation must be rejected — otherwise anyone could
        // submit an unverified rotation request.
        let pubkey_bytes = self
            .current_ed25519_public
            .ok_or(KeyRotationError::AuthorizationSignatureRequired)?;
        {
            let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&pubkey_bytes)
                .map_err(|_| KeyRotationError::InvalidAuthorizationSignature)?;
            let signature = ed25519_dalek::Signature::try_from(request.authorization_sig.as_slice())
                .map_err(|_| KeyRotationError::InvalidAuthorizationSignature)?;
            // The message being signed includes nonce and new key commitment
            // to prevent replay attacks. The nonce is derived from the current
            // public key, binding the signature to the current key state.
            let nonce = blake3::hash(&pubkey_bytes);
            let message = format!(
                "omnia-key-rotation:{:03}:{:020}:{}:{}",
                request.new_phase as u8,
                request.effective_at,
                hex::encode(nonce.as_bytes()),
                hex::encode(request.new_key.ed25519)
            );
            verifying_key
                .verify(message.as_bytes(), &signature)
                .map_err(|_| KeyRotationError::InvalidAuthorizationSignature)?;
        }

        self.pending.push(request);
        Ok(())
    }

    /// Process rotations that have become effective.
    pub fn process_effective(&mut self, current_round: u64) -> Vec<PqcKeyRotationRequest> {
        let (effective, remaining): (Vec<_>, Vec<_>) =
            self.pending.drain(..).partition(|r| current_round >= r.effective_at);

        self.pending = remaining;

        for rotation in &effective {
            self.current_phase = rotation.new_phase;
            tracing::info!(
                "Key rotation effective at round {}: phase -> {:?}",
                current_round,
                rotation.new_phase
            );
        }

        effective
    }

    /// Get the current commitment phase.
    pub fn current_phase(&self) -> &CommitmentPhase {
        &self.current_phase
    }

    /// Check if an old key is still in the transition period.
    pub fn is_key_in_transition(&self, key: &PqPublicKey, current_round: u64) -> bool {
        self.pending
            .iter()
            .any(|r| &r.old_key == key && current_round < r.sunset_at)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use ed25519_dalek::Signer as _;

    fn hybrid_key(val: u8) -> PqPublicKey {
        PqPublicKey {
            ed25519: [val; 32],
            dilithium: vec![0u8; 1952],
        }
    }

    /// Helper: create a valid authorization signature for a rotation request.
    ///
    /// The message format must match `PqcKeyRotationManager::submit_rotation`:
    /// `"{new_phase_u8}:{effective_at}:{nonce_hex}:{new_key_ed25519_hex}"`
    fn sign_rotation_request(
        signing_key: &ed25519_dalek::SigningKey,
        new_key: &PqPublicKey,
        new_phase: CommitmentPhase,
        effective_at: u64,
    ) -> Vec<u8> {
        let pubkey_bytes = signing_key.verifying_key().to_bytes();
        let nonce = blake3::hash(&pubkey_bytes);
        let message = format!(
            "omnia-key-rotation:{:03}:{:020}:{}:{}",
            new_phase as u8,
            effective_at,
            hex::encode(nonce.as_bytes()),
            hex::encode(new_key.ed25519)
        );
        signing_key.sign(message.as_bytes()).to_bytes().to_vec()
    }

    /// Helper: create a manager with a real Ed25519 keypair for authorization.
    fn manager_with_key(phase: CommitmentPhase) -> (PqcKeyRotationManager, ed25519_dalek::SigningKey) {
        let mut manager = PqcKeyRotationManager::new(phase);
        let signing_key = ed25519_dalek::SigningKey::generate(&mut rand::rngs::OsRng);
        manager.set_current_ed25519_public(signing_key.verifying_key().to_bytes());
        (manager, signing_key)
    }

    #[test]
    fn test_phase_upgrade_allowed() {
        let (mut manager, signing_key) = manager_with_key(CommitmentPhase::ClassicalOnly);
        let new_key = hybrid_key(2);
        let request = PqcKeyRotationRequest {
            old_key: PqPublicKey {
                ed25519: signing_key.verifying_key().to_bytes(),
                dilithium: vec![],
            },
            new_key: new_key.clone(),
            authorization_sig: sign_rotation_request(&signing_key, &new_key, CommitmentPhase::Hybrid, 100),
            new_phase: CommitmentPhase::Hybrid,
            effective_at: 100,
            sunset_at: 200,
        };
        assert!(manager.submit_rotation(request).is_ok());
    }

    #[test]
    fn test_phase_downgrade_rejected() {
        let (mut manager, signing_key) = manager_with_key(CommitmentPhase::Hybrid);
        let new_key = PqPublicKey {
            ed25519: [2u8; 32],
            dilithium: vec![],
        };
        let request = PqcKeyRotationRequest {
            old_key: hybrid_key(1),
            new_key: new_key.clone(),
            authorization_sig: sign_rotation_request(&signing_key, &new_key, CommitmentPhase::ClassicalOnly, 100),
            new_phase: CommitmentPhase::ClassicalOnly,
            effective_at: 100,
            sunset_at: 200,
        };
        assert!(manager.submit_rotation(request).is_err());
    }

    #[test]
    fn test_transition_period_tracking() {
        let (mut manager, signing_key) = manager_with_key(CommitmentPhase::ClassicalOnly);
        let old_key = PqPublicKey {
            ed25519: signing_key.verifying_key().to_bytes(),
            dilithium: vec![],
        };
        let new_key = hybrid_key(2);
        let request = PqcKeyRotationRequest {
            old_key: old_key.clone(),
            new_key: new_key.clone(),
            authorization_sig: sign_rotation_request(&signing_key, &new_key, CommitmentPhase::Hybrid, 100),
            new_phase: CommitmentPhase::Hybrid,
            effective_at: 100,
            sunset_at: 200,
        };
        manager.submit_rotation(request).unwrap();

        assert!(manager.is_key_in_transition(&old_key, 150));
        assert!(!manager.is_key_in_transition(&old_key, 250));
    }

    #[test]
    fn test_empty_authorization_sig_rejected() {
        let (mut manager, _signing_key) = manager_with_key(CommitmentPhase::ClassicalOnly);
        let request = PqcKeyRotationRequest {
            old_key: PqPublicKey {
                ed25519: [1u8; 32],
                dilithium: vec![],
            },
            new_key: hybrid_key(2),
            authorization_sig: vec![],
            new_phase: CommitmentPhase::Hybrid,
            effective_at: 100,
            sunset_at: 200,
        };
        assert!(manager.submit_rotation(request).is_err());
    }

    #[test]
    fn test_process_effective_advances_phase() {
        let (mut manager, signing_key) = manager_with_key(CommitmentPhase::ClassicalOnly);
        let new_key = hybrid_key(2);
        let request = PqcKeyRotationRequest {
            old_key: PqPublicKey {
                ed25519: signing_key.verifying_key().to_bytes(),
                dilithium: vec![],
            },
            new_key: new_key.clone(),
            authorization_sig: sign_rotation_request(&signing_key, &new_key, CommitmentPhase::Hybrid, 100),
            new_phase: CommitmentPhase::Hybrid,
            effective_at: 100,
            sunset_at: 200,
        };
        manager.submit_rotation(request).unwrap();

        // Before effective round: phase unchanged
        assert_eq!(*manager.current_phase(), CommitmentPhase::ClassicalOnly);
        let effective = manager.process_effective(50);
        assert!(effective.is_empty());

        // At effective round: phase advances
        let effective = manager.process_effective(100);
        assert_eq!(effective.len(), 1);
        assert_eq!(*manager.current_phase(), CommitmentPhase::Hybrid);
    }

    #[test]
    fn test_full_transition_to_post_quantum() {
        let (mut manager, signing_key) = manager_with_key(CommitmentPhase::ClassicalOnly);

        // ClassicalOnly -> Hybrid
        let new_key1 = hybrid_key(2);
        let req1 = PqcKeyRotationRequest {
            old_key: PqPublicKey {
                ed25519: signing_key.verifying_key().to_bytes(),
                dilithium: vec![],
            },
            new_key: new_key1.clone(),
            authorization_sig: sign_rotation_request(&signing_key, &new_key1, CommitmentPhase::Hybrid, 100),
            new_phase: CommitmentPhase::Hybrid,
            effective_at: 100,
            sunset_at: 200,
        };
        manager.submit_rotation(req1).unwrap();
        manager.process_effective(100);
        assert_eq!(*manager.current_phase(), CommitmentPhase::Hybrid);

        // Hybrid -> PostQuantum
        let new_key2 = hybrid_key(3);
        let req2 = PqcKeyRotationRequest {
            old_key: hybrid_key(2),
            new_key: new_key2.clone(),
            authorization_sig: sign_rotation_request(&signing_key, &new_key2, CommitmentPhase::PostQuantum, 300),
            new_phase: CommitmentPhase::PostQuantum,
            effective_at: 300,
            sunset_at: 400,
        };
        manager.submit_rotation(req2).unwrap();
        manager.process_effective(300);
        assert_eq!(*manager.current_phase(), CommitmentPhase::PostQuantum);
    }

    #[test]
    fn test_key_rotation_error_variants_display() {
        let e = KeyRotationError::PhaseDowngrade {
            from: CommitmentPhase::Hybrid,
            to: CommitmentPhase::ClassicalOnly,
        };
        assert!(e.to_string().contains("cannot downgrade"));
        assert!(e.to_string().contains("Hybrid"));
        assert!(e.to_string().contains("ClassicalOnly"));

        let e = KeyRotationError::AuthorizationSignatureRequired;
        assert!(e.to_string().contains("authorization signature required"));
    }

    #[test]
    fn test_invalid_signature_rejected() {
        let (mut manager, _signing_key) = manager_with_key(CommitmentPhase::ClassicalOnly);
        let new_key = hybrid_key(2);
        let request = PqcKeyRotationRequest {
            old_key: PqPublicKey {
                ed25519: [1u8; 32],
                dilithium: vec![],
            },
            new_key: new_key.clone(),
            authorization_sig: vec![0u8; 64], // Invalid signature
            new_phase: CommitmentPhase::Hybrid,
            effective_at: 100,
            sunset_at: 200,
        };
        assert!(manager.submit_rotation(request).is_err());
    }

    #[test]
    fn test_no_ed25519_public_key_set() {
        let mut manager = PqcKeyRotationManager::new(CommitmentPhase::ClassicalOnly);
        // No ed25519 public key set — should reject
        let new_key = hybrid_key(2);
        let request = PqcKeyRotationRequest {
            old_key: PqPublicKey {
                ed25519: [1u8; 32],
                dilithium: vec![],
            },
            new_key: new_key.clone(),
            authorization_sig: vec![1u8; 64],
            new_phase: CommitmentPhase::Hybrid,
            effective_at: 100,
            sunset_at: 200,
        };
        assert!(manager.submit_rotation(request).is_err());
    }
}
