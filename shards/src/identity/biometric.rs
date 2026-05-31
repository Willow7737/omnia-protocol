//! Privacy-preserving biometric anchors
//!
//! Biometric anchors store only a salted cryptographic commitment of a
//! biometric template — never the raw template itself. This ensures that
//! even if the on-chain data is compromised, the original biometric data
//! cannot be recovered.
//!
//! The commitment is computed as BLAKE3(salt || template), providing a
//! one-way binding that can be verified against fresh templates without
//! ever persisting the raw biometric data.

use omnia_substrate::VectorClock;
use rand::Rng;
use serde::{Deserialize, Serialize};

/// A privacy-preserving biometric commitment.
///
/// Stores only a hash-based commitment and a random salt. The raw
/// biometric template is never stored on-chain or in memory longer
/// than necessary for enrollment or verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BiometricAnchor {
    /// Hash of (salt || raw_template) — never the raw template itself.
    pub commitment: [u8; 32],
    /// Random salt unique per enrollment.
    pub salt: [u8; 32],
    /// Algorithm identifier: "fingerprint_v2", "iris_v3", "face_v1", etc.
    pub algorithm: String,
    /// Enrollment timestamp as a vector clock.
    pub enrolled_at: VectorClock,
}

impl BiometricAnchor {
    /// Create a biometric anchor from a raw biometric template.
    ///
    /// The template is NOT stored — only the commitment and salt persist.
    /// A fresh random salt is generated for each enrollment, ensuring
    /// that identical templates produce different commitments across
    /// enrollments (rainbow-table resistance).
    pub fn enroll(template: &[u8], algorithm: &str) -> Self {
        let mut salt = [0u8; 32];
        rand::thread_rng().fill(&mut salt);

        let mut hasher = blake3::Hasher::new();
        hasher.update(&salt);
        hasher.update(template);

        Self {
            commitment: *hasher.finalize().as_bytes(),
            salt,
            algorithm: algorithm.to_string(),
            enrolled_at: VectorClock::new(),
        }
    }

    /// Verify a fresh template against the stored commitment.
    ///
    /// **IMPORTANT**: This performs exact hash comparison, which is suitable for
    /// template hash verification but NOT for real biometric data that has natural
    /// variance between readings. For production biometric verification, implement
    /// fuzzy matching (e.g., Hamming distance threshold for iris, cosine similarity
    /// for face embeddings) instead of exact hash comparison.
    ///
    /// Returns `true` if the fresh template matches the enrolled template
    /// (i.e., BLAKE3(salt || fresh_template) == commitment), `false` otherwise.
    pub fn verify(&self, fresh_template: &[u8]) -> bool {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.salt);
        hasher.update(fresh_template);
        hasher.finalize().as_bytes() == &self.commitment
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_enroll_and_verify_matching() {
        let template = b"fingerprint_template_bytes";
        let anchor = BiometricAnchor::enroll(template, "fingerprint_v2");

        // Same template should verify successfully
        assert!(anchor.verify(template));
    }

    #[test]
    fn test_enroll_and_verify_different() {
        let template = b"fingerprint_template_bytes";
        let anchor = BiometricAnchor::enroll(template, "fingerprint_v2");

        // Different template should fail verification
        assert!(!anchor.verify(b"wrong_template"));
    }

    #[test]
    fn test_different_enrollments_produce_different_commitments() {
        let template = b"same_template";

        // Two enrollments of the same template should produce different
        // commitments (due to different salts)
        let anchor1 = BiometricAnchor::enroll(template, "fingerprint_v2");
        let anchor2 = BiometricAnchor::enroll(template, "fingerprint_v2");

        assert_ne!(anchor1.commitment, anchor2.commitment);
        assert_ne!(anchor1.salt, anchor2.salt);

        // Both should verify against the original template
        assert!(anchor1.verify(template));
        assert!(anchor2.verify(template));
    }

    #[test]
    fn test_commitment_is_not_the_template() {
        let template = b"my_biometric_data";
        let anchor = BiometricAnchor::enroll(template, "iris_v3");

        // The commitment should not be equal to the template
        assert_ne!(&anchor.commitment[..], template);
        // The salt should not be equal to the template
        assert_ne!(&anchor.salt[..], template);
    }
}
