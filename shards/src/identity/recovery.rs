//! Shamir's Secret Sharing for social recovery
//!
//! Splits a secret (e.g., a DID's private key) into N shares using
//! Shamir's Secret Sharing over GF(2^8). Any K shares can reconstruct
//! the secret; K-1 shares reveal nothing about it.
//!
//! Each byte of the secret is treated as an independent polynomial's
//! constant term. This avoids big-integer arithmetic and works naturally
//! with byte arrays. GF(256) arithmetic uses the AES irreducible
//! polynomial x^8 + x^4 + x^3 + x + 1 (0x11B) for reduction.

use rand::Rng;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors that can occur during Shamir secret sharing operations.
#[derive(Debug, Error)]
pub enum ShamirError {
    /// Attempted to invert zero in GF(256).
    #[error("cannot invert zero in GF(256)")]
    ZeroInverse,
    /// Invalid share data provided.
    #[error("invalid share: {0}")]
    InvalidShare(String),
    /// Invalid threshold parameter.
    #[error("invalid threshold: {0}")]
    InvalidThreshold(u8),
    /// Invalid secret data provided.
    #[error("invalid secret: {0}")]
    InvalidSecret(String),
}

/// A single share in a Shamir secret sharing scheme.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryShare {
    /// Share index (1-based, non-zero for Lagrange interpolation).
    pub index: u8,
    /// Share bytes (same length as the original secret).
    pub value: Vec<u8>,
}

/// Shamir's Secret Sharing implementation over GF(256).
pub struct ShamirRecovery;

impl ShamirRecovery {
    /// Split a secret into `total` shares, any `threshold` of which can
    /// reconstruct it.
    ///
    /// # Errors
    ///
    /// Returns [`ShamirError::InvalidThreshold`] if `threshold < 2` or
    /// `threshold > total`.
    /// Returns [`ShamirError::InvalidSecret`] if `secret` is empty.
    pub fn split(secret: &[u8], threshold: u8, total: u8) -> Result<Vec<RecoveryShare>, ShamirError> {
        if threshold < 2 {
            return Err(ShamirError::InvalidThreshold(threshold));
        }
        if threshold > total {
            return Err(ShamirError::InvalidThreshold(threshold));
        }
        if secret.is_empty() {
            return Err(ShamirError::InvalidSecret("secret cannot be empty".into()));
        }

        let mut rng = rand::thread_rng();

        // For each byte position in the secret, create ONE polynomial of
        // degree (threshold - 1) with the secret byte as the constant term.
        // The random coefficients must be the same for all shares of the
        // same byte position — only the evaluation point (x) varies.
        let mut byte_polynomials: Vec<Vec<u8>> = Vec::with_capacity(secret.len());
        for &secret_byte in secret {
            // Polynomial: f(x) = a_0 + a_1*x + a_2*x^2 + ... + a_{k-1}*x^{k-1}
            // where a_0 = secret_byte, and a_1...a_{k-1} are random
            let mut coefficients = vec![secret_byte];
            for _ in 1..threshold {
                coefficients.push(rng.gen());
            }
            byte_polynomials.push(coefficients);
        }

        // Evaluate each byte's polynomial at x = 1, 2, ..., total
        let mut shares: Vec<RecoveryShare> = Vec::with_capacity(total as usize);
        for share_idx in 1..=total {
            let mut share_value = Vec::with_capacity(secret.len());
            for coeffs in &byte_polynomials {
                let y = Self::eval_polynomial(coeffs, share_idx);
                share_value.push(y);
            }
            shares.push(RecoveryShare {
                index: share_idx,
                value: share_value,
            });
        }
        Ok(shares)
    }

    /// Reconstruct the secret from at least `threshold` shares.
    ///
    /// Returns an error if the shares list is empty, shares have
    /// inconsistent lengths, duplicate indices are present, or
    /// interpolation fails.
    pub fn reconstruct(shares: &[RecoveryShare]) -> Result<Vec<u8>, ShamirError> {
        if shares.is_empty() {
            return Err(ShamirError::InvalidShare("no shares provided".into()));
        }
        let secret_len = shares[0].value.len();

        // Validate share consistency: all shares must have the same value length
        for (i, share) in shares.iter().enumerate().skip(1) {
            if share.value.len() != secret_len {
                return Err(ShamirError::InvalidShare(format!(
                    "inconsistent share lengths: share {} has {} bytes, expected {}",
                    i,
                    share.value.len(),
                    secret_len
                )));
            }
        }

        // Check for duplicate indices
        let mut seen_indices = std::collections::HashSet::new();
        for share in shares {
            if !seen_indices.insert(share.index) {
                return Err(ShamirError::InvalidShare(format!(
                    "duplicate share index: {}",
                    share.index
                )));
            }
        }

        let mut secret = Vec::with_capacity(secret_len);

        for byte_pos in 0..secret_len {
            // Lagrange interpolation at x = 0 to recover the constant term
            let points: Vec<(u8, u8)> = shares.iter().map(|s| (s.index, s.value[byte_pos])).collect();
            let byte = Self::lagrange_interpolate(&points, 0)?;
            secret.push(byte);
        }
        Ok(secret)
    }

    /// Evaluate a polynomial at x in GF(256).
    fn eval_polynomial(coeffs: &[u8], x: u8) -> u8 {
        let mut result = 0u8;
        let mut x_power = 1u8;
        for &coeff in coeffs {
            result ^= Self::gf_mul(coeff, x_power);
            x_power = Self::gf_mul(x_power, x);
        }
        result
    }

    /// Lagrange interpolation at `at` in GF(256).
    fn lagrange_interpolate(points: &[(u8, u8)], at: u8) -> Result<u8, ShamirError> {
        let mut result = 0u8;
        for (i, &(x_i, y_i)) in points.iter().enumerate() {
            let mut numerator = 1u8;
            let mut denominator = 1u8;
            for (j, &(x_j, _)) in points.iter().enumerate() {
                if i != j {
                    numerator = Self::gf_mul(numerator, at ^ x_j);
                    denominator = Self::gf_mul(denominator, x_i ^ x_j);
                }
            }
            result ^= Self::gf_mul(y_i, Self::gf_div(numerator, denominator)?);
        }
        Ok(result)
    }

    /// GF(256) multiplication using carryless multiplication with reduction
    /// by the AES irreducible polynomial x^8 + x^4 + x^3 + x + 1.
    fn gf_mul(a: u8, b: u8) -> u8 {
        let mut result = 0u8;
        let mut a = a;
        let mut b = b;
        while b != 0 {
            if b & 1 != 0 {
                result ^= a;
            }
            let high_bit = a & 0x80;
            a <<= 1;
            if high_bit != 0 {
                a ^= 0x1B; // x^8 + x^4 + x^3 + x + 1 reduction
            }
            b >>= 1;
        }
        result
    }

    /// GF(256) division: a / b = a * b^(-1).
    fn gf_div(a: u8, b: u8) -> Result<u8, ShamirError> {
        let inv = Self::gf_inverse(b)?;
        Ok(Self::gf_mul(a, inv))
    }

    /// GF(256) multiplicative inverse using exponentiation.
    /// a^(-1) = a^(254) since GF(256)* is cyclic of order 255.
    fn gf_inverse(a: u8) -> Result<u8, ShamirError> {
        if a == 0 {
            return Err(ShamirError::ZeroInverse);
        }
        // 254 = 11111110 in binary
        let mut result = 1u8;
        let mut base = a;
        for _ in 0..7 {
            base = Self::gf_mul(base, base); // square
            result = Self::gf_mul(result, base);
        }
        Ok(result)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_gf_mul_identity() {
        // a * 1 = a
        assert_eq!(ShamirRecovery::gf_mul(0x57, 1), 0x57);
        assert_eq!(ShamirRecovery::gf_mul(1, 0x83), 0x83);
    }

    #[test]
    fn test_gf_mul_zero() {
        // a * 0 = 0
        assert_eq!(ShamirRecovery::gf_mul(0x57, 0), 0);
        assert_eq!(ShamirRecovery::gf_mul(0, 0x83), 0);
    }

    #[test]
    fn test_gf_inverse_roundtrip() {
        // a * a^(-1) = 1 for any non-zero a
        for a in [1u8, 2, 0x57, 0x83, 0xFF] {
            let inv = ShamirRecovery::gf_inverse(a).expect("test assertion failed");
            assert_eq!(ShamirRecovery::gf_mul(a, inv), 1);
        }
    }

    #[test]
    fn test_split_and_reconstruct_threshold() {
        let secret = b"my super secret key";
        let shares = ShamirRecovery::split(secret, 3, 5).expect("test assertion failed");
        assert_eq!(shares.len(), 5);

        // Reconstruct with exactly threshold shares
        let reconstructed = ShamirRecovery::reconstruct(&shares[0..3]).expect("test assertion failed");
        assert_eq!(reconstructed, secret);

        // Reconstruct with a different set of threshold shares
        let reconstructed = ShamirRecovery::reconstruct(&shares[1..4]).expect("test assertion failed");
        assert_eq!(reconstructed, secret);

        // Reconstruct with all shares
        let reconstructed = ShamirRecovery::reconstruct(&shares).expect("test assertion failed");
        assert_eq!(reconstructed, secret);
    }

    #[test]
    fn test_insufficient_shares_reveal_nothing() {
        let secret = b"my super secret key";
        let shares = ShamirRecovery::split(secret, 3, 5).expect("test assertion failed");

        // With fewer than threshold shares, reconstruction should produce
        // different data (not the secret)
        let result = ShamirRecovery::reconstruct(&shares[0..2]);
        assert!(result.is_ok());
        assert_ne!(result.expect("test assertion failed"), secret);
    }

    #[test]
    fn test_single_byte_secret() {
        let secret = b"\x42";
        let shares = ShamirRecovery::split(secret, 2, 3).expect("test assertion failed");

        let reconstructed = ShamirRecovery::reconstruct(&shares[0..2]).expect("test assertion failed");
        assert_eq!(reconstructed, secret);
    }

    #[test]
    fn test_split_invalid_threshold() {
        let secret = b"test";

        // threshold < 2
        assert!(ShamirRecovery::split(secret, 1, 3).is_err());

        // threshold > total
        assert!(ShamirRecovery::split(secret, 5, 3).is_err());
    }

    #[test]
    fn test_split_empty_secret() {
        assert!(ShamirRecovery::split(&[], 2, 3).is_err());
    }

    #[test]
    fn test_reconstruct_duplicate_indices() {
        let secret = b"test";
        let shares = ShamirRecovery::split(secret, 2, 3).expect("test assertion failed");
        // Duplicate the first share
        let dup_shares = vec![shares[0].clone(), shares[0].clone()];
        assert!(ShamirRecovery::reconstruct(&dup_shares).is_err());
    }

    #[test]
    fn test_reconstruct_empty_shares() {
        let result = ShamirRecovery::reconstruct(&[]);
        assert!(result.is_err());
    }
}
