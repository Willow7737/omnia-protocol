//! Cryptographic primitives for Omnia Protocol Layer 1
//!
//! Provides Ed25519 signing and verification for consensus-critical events.
//! All operations use constant-time implementations from `ed25519-dalek`.

pub use ed25519_dalek::{Signature, SignatureError, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;

/// A node keypair for signing events.
pub type NodeKeypair = ed25519_dalek::SigningKey;

/// A node public key for verifying events.
pub type NodePublicKey = ed25519_dalek::VerifyingKey;

/// Generate a new random Ed25519 keypair using OS RNG.
pub fn generate_keypair() -> NodeKeypair {
    NodeKeypair::generate(&mut OsRng)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keypair_generation() {
        let kp = generate_keypair();
        let pubkey = kp.verifying_key();
        assert_eq!(pubkey.to_bytes().len(), 32);
    }

    #[test]
    fn test_sign_and_verify() {
        let kp = generate_keypair();
        let message = b"test message for omnia";
        let signature = kp.sign(message);
        assert!(kp.verifying_key().verify(message, &signature).is_ok());
    }
}
