//! RF Fingerprinting — Physical Device Identity (Stub)
//!
//! Every physical device has a unique radio frequency signature caused by
//! manufacturing imperfections in oscillators, amplifiers, and antennas.
//! These imperfections create an unclonable "fingerprint" — a Physically
//! Unclonable Function (PUF) in the RF domain.
//!
//! # Research Foundations
//!
//! - **PUF (Physically Unclonable Functions)**: SRAM PUF, Arbiter PUF, Ring
//!   Oscillator PUF — silicon-level uniqueness from manufacturing variation.
//! - **RF-DNA**: DARPA's Radio Frequency Distinct Native Attribute
//!   fingerprinting — extracts device-specific features from RF emissions.
//! - **IEEE 802.11 fingerprinting**: WiFi device identification via clock
//!   skew and transmitter imperfections.
//!
//! # Current Status
//!
//! This is a **stub implementation** using Hamming distance for spectral hash
//! comparison. Real RF capture requires hardware access (SDR, spectrum
//! analyzer) and would produce spectral feature vectors rather than simple
//! byte arrays.

use omnia_substrate::VectorClock;
use serde::{Deserialize, Serialize};

/// A device's unique RF signature captured at a point in time.
///
/// The `spectral_hash` is a 32-byte digest derived from the device's RF
/// emission features (carrier frequency offset, phase noise, I/Q imbalance,
/// etc.). Two measurements from the same device should produce hashes with
/// very small Hamming distance; measurements from different devices should
/// produce hashes with large Hamming distance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RfFingerprint {
    /// Device's unique RF signature (spectral features hashed to 32 bytes).
    pub spectral_hash: [u8; 32],
    /// Measurement timestamp (causal, not wall-clock).
    pub measured_at: VectorClock,
    /// Device's claimed identity (must match DID).
    pub device_did: String,
    /// Confidence threshold in parts per million (PPM).
    /// 800_000 = 80% similarity required.
    /// Higher means stricter matching threshold.
    /// A confidence_ppm of 950_000 means the Hamming similarity must
    /// exceed 95% for the fingerprint to be considered a match.
    pub confidence_ppm: u64,
}

impl RfFingerprint {
    /// Create a new RF fingerprint.
    ///
    /// # Arguments
    ///
    /// * `spectral_hash` — 32-byte hash of the device's RF spectral features
    /// * `measured_at` — Vector clock at measurement time
    /// * `device_did` — DID string of the device being fingerprinted
    /// * `confidence_ppm` — Matching threshold in PPM (0–1_000_000); typical values: 900_000–990_000
    pub fn new(spectral_hash: [u8; 32], measured_at: VectorClock, device_did: String, confidence_ppm: u64) -> Self {
        Self {
            spectral_hash,
            measured_at,
            device_did,
            confidence_ppm: confidence_ppm.min(1_000_000), // Cap at 100%
        }
    }

    /// Verify that a device's current RF signature matches its registered
    /// fingerprint.
    ///
    /// Uses Hamming distance between the stored `spectral_hash` and the
    /// `current_measurement`. The similarity is computed using integer
    /// PPM arithmetic: `(256 - distance) * 1_000_000 / 256`, and the match
    /// is accepted if similarity exceeds the stored `confidence_ppm` threshold.
    ///
    /// # Arguments
    ///
    /// * `current_measurement` — The freshly measured 32-byte spectral hash
    ///
    /// # Returns
    ///
    /// `true` if the similarity exceeds the confidence threshold.
    pub fn verify(&self, current_measurement: &[u8; 32]) -> bool {
        let distance = hamming_distance(&self.spectral_hash, current_measurement);
        // Integer PPM-based similarity: (256 - distance) * 1_000_000 / 256
        let similarity_ppm = (256u64 - distance as u64) * 1_000_000 / 256;
        similarity_ppm >= self.confidence_ppm
    }

    /// Create a dummy/stub RF fingerprint for testing.
    ///
    /// In a real deployment, the spectral hash would be computed from
    /// actual RF measurements via feature extraction and hashing. This
    /// stub simply uses the provided bytes directly.
    #[cfg(test)]
    pub fn stub(device_did: &str, hash_bytes: [u8; 32]) -> Self {
        Self {
            spectral_hash: hash_bytes,
            measured_at: VectorClock::new(),
            device_did: device_did.to_string(),
            confidence_ppm: 950_000, // 95% in PPM
        }
    }
}

/// Compute the Hamming distance between two 32-byte arrays.
///
/// The Hamming distance counts the number of differing bits between
/// the two arrays. For identical arrays, the distance is 0. For
/// maximally different arrays, the distance is 256 (32 bytes × 8 bits).
pub fn hamming_distance(a: &[u8; 32], b: &[u8; 32]) -> u32 {
    a.iter()
        .zip(b.iter())
        .map(|(byte_a, byte_b)| (byte_a ^ byte_b).count_ones())
        .sum()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_identical_fingerprints_match() {
        let hash = [0xAB_u8; 32];
        let fp = RfFingerprint::stub("did:omnia:device1", hash);
        assert!(fp.verify(&hash));
    }

    #[test]
    fn test_completely_different_fingerprints_do_not_match() {
        let hash_a = [0x00_u8; 32];
        let hash_b = [0xFF_u8; 32];
        let fp = RfFingerprint::stub("did:omnia:device1", hash_a);
        // Hamming distance = 256, similarity_ppm = 0, which is NOT > 950_000
        assert!(!fp.verify(&hash_b));
    }

    #[test]
    fn test_slightly_different_fingerprints_match() {
        let hash_a = [0xAB_u8; 32];
        let mut hash_b = [0xAB_u8; 32];
        // Flip 1 bit — similarity_ppm = 255 * 1_000_000 / 256 = 996_093 > 950_000
        hash_b[0] ^= 0x01;
        let fp = RfFingerprint::stub("did:omnia:device1", hash_a);
        assert!(fp.verify(&hash_b));
    }

    #[test]
    fn test_hamming_distance_identical() {
        let a = [0x55_u8; 32];
        assert_eq!(hamming_distance(&a, &a), 0);
    }

    #[test]
    fn test_hamming_distance_max() {
        let a = [0x00_u8; 32];
        let b = [0xFF_u8; 32];
        assert_eq!(hamming_distance(&a, &b), 256);
    }

    #[test]
    fn test_confidence_clamping() {
        let fp = RfFingerprint::new(
            [0u8; 32],
            VectorClock::new(),
            "did:omnia:test".to_string(),
            1_500_000, // Should be clamped to 1_000_000
        );
        // With confidence_ppm 1_000_000 (clamped from 1_500_000), similarity_ppm
        // for identical hashes is 1_000_000 which IS >= 1_000_000, so they match.
        // This is correct: confidence_ppm=1_000_000 means "require 100% similarity",
        // and identical fingerprints have 100% similarity.
        assert!(fp.verify(&[0u8; 32]));
    }
}
