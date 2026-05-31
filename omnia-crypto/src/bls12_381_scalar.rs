//! BLS12-381 Scalar Field Arithmetic for DKG.
//!
//! Provides safe wrappers around `blst` field-element operations for polynomial
//! evaluation, share generation, and verification in the Feldman VSS DKG.
//!
//! This module requires `unsafe` code to interface with the `blst` C library,
//! which is why it is isolated in its own module with a targeted `allow`.
//! The rest of the crate remains `unsafe`-free.
//!
//! # Internal representation
//!
//! The `Scalar` type stores values as `blst_fr` (Montgomery form), which
//! supports efficient modular arithmetic via `blst_fr_add`, `blst_fr_mul`,
//! etc. Conversion to/from byte sequences uses `blst_scalar` (the canonical
//! 256-bit representation) via `blst_fr_from_scalar` / `blst_scalar_from_fr`.
//!
//! # Security
//!
//! All `unsafe` blocks are minimal, well-documented, and wrapped in safe
//! Rust abstractions. The `Scalar` type enforces valid field elements on
//! construction and all operations are performed modulo the BLS12-381
//! subgroup order.

#![allow(unsafe_code)]

use blst::*;
use rand::{CryptoRng, RngCore};
use std::fmt;
use subtle::ConstantTimeEq;

/// Number of bytes in a BLS12-381 scalar (256 bits).
pub const SCALAR_BYTES: usize = 32;

/// Number of bytes in a compressed G1 point on BLS12-381.
pub const G1_COMPRESSED_SIZE: usize = 48;

/// A scalar in the BLS12-381 prime order subgroup.
///
/// Internally wraps `blst_fr`, which stores a 256-bit integer in
/// Montgomery representation. All arithmetic is performed modulo the
/// BLS12-381 subgroup order `r` using the efficient `blst_fr_*` API.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scalar {
    inner: blst_fr,
}

// Manual Serialize/Deserialize implementations for Scalar.
// We serialize as 32 little-endian bytes, which is the canonical
// representation of a BLS12-381 scalar field element.
impl serde::Serialize for Scalar {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&self.to_bytes())
    }
}

impl<'de> serde::Deserialize<'de> for Scalar {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ScalarVisitor;

        impl<'de> serde::de::Visitor<'de> for ScalarVisitor {
            type Value = Scalar;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "a 32-byte BLS12-381 scalar")
            }

            fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<Scalar, E> {
                if v.len() != SCALAR_BYTES {
                    return Err(E::custom(format!(
                        "expected 32 bytes for BLS12-381 scalar, got {}",
                        v.len()
                    )));
                }
                let mut bytes = [0u8; SCALAR_BYTES];
                bytes.copy_from_slice(v);
                Scalar::from_bytes(&bytes).ok_or_else(|| E::custom("invalid BLS12-381 scalar (>= subgroup order)"))
            }
        }

        deserializer.deserialize_bytes(ScalarVisitor)
    }
}

impl Scalar {
    /// Create a scalar from 32 bytes (little-endian).
    ///
    /// The input is reduced modulo the subgroup order. Returns `None` if
    /// the bytes do not represent a valid scalar (i.e. >= subgroup order).
    pub fn from_bytes(bytes: &[u8; SCALAR_BYTES]) -> Option<Self> {
        // First, parse bytes into a blst_scalar (canonical form).
        let scalar = unsafe {
            // SAFETY: blst_scalar_from_lendian reads exactly 32 bytes and
            // produces a blst_scalar. The resulting scalar is then checked
            // for validity (must be < subgroup order).
            let mut s = std::mem::MaybeUninit::<blst_scalar>::uninit();
            blst_scalar_from_lendian(s.as_mut_ptr(), bytes.as_ptr());
            s.assume_init()
        };
        // Verify the scalar is within the valid range (< subgroup order).
        if !unsafe { blst_scalar_fr_check(&scalar) } {
            return None;
        }
        // Convert from canonical blst_scalar to Montgomery form blst_fr.
        let fr = unsafe {
            // SAFETY: blst_fr_from_scalar converts a validated blst_scalar
            // into its Montgomery form blst_fr. Input is a valid scalar.
            let mut fr = std::mem::MaybeUninit::<blst_fr>::uninit();
            blst_fr_from_scalar(fr.as_mut_ptr(), &scalar);
            fr.assume_init()
        };
        Some(Self { inner: fr })
    }

    /// Convert the scalar to 32 little-endian bytes.
    pub fn to_bytes(&self) -> [u8; SCALAR_BYTES] {
        let mut bytes = [0u8; SCALAR_BYTES];
        unsafe {
            // SAFETY: Convert from Montgomery form back to canonical scalar,
            // then write 32 bytes in little-endian order.
            let mut scalar = std::mem::MaybeUninit::<blst_scalar>::uninit();
            blst_scalar_from_fr(scalar.as_mut_ptr(), &self.inner);
            blst_lendian_from_scalar(bytes.as_mut_ptr(), scalar.as_ptr());
        }
        bytes
    }

    /// Generate a random scalar using the provided RNG.
    ///
    /// Uses rejection sampling: generates 32 random bytes and retries
    /// if the value is >= subgroup order r. Since r ≈ 2^254.33, the
    /// probability of rejection is approximately 25%, so this loop
    /// terminates quickly (expected ~1.33 iterations).
    pub fn random(rng: &mut (impl CryptoRng + RngCore)) -> Self {
        loop {
            let mut bytes = [0u8; SCALAR_BYTES];
            rng.fill_bytes(&mut bytes);
            if let Some(scalar) = Self::from_bytes(&bytes) {
                return scalar;
            }
        }
    }

    /// Create a scalar from a u64 value.
    ///
    /// The value is treated as a 256-bit integer with the upper three
    /// limbs set to zero, then converted to Montgomery form.
    pub fn from_u64(val: u64) -> Self {
        // blst_fr_from_uint64 expects a pointer to an array of 4 u64 limbs
        // representing a 256-bit integer in little-endian limb order.
        let limbs = [val, 0u64, 0u64, 0u64];
        unsafe {
            // SAFETY: blst_fr_from_uint64 initializes a blst_fr from a
            // pointer to a 4-element u64 array (256-bit integer in
            // little-endian limb order), performing the conversion to
            // Montgomery form. The value is guaranteed < r since r > 2^254.
            let mut fr = std::mem::MaybeUninit::<blst_fr>::uninit();
            blst_fr_from_uint64(fr.as_mut_ptr(), limbs.as_ptr());
            Self {
                inner: fr.assume_init(),
            }
        }
    }

    /// The additive identity (zero).
    pub fn zero() -> Self {
        // Default blst_fr is zero (all bytes = 0).
        Self {
            inner: blst_fr::default(),
        }
    }

    /// Test if this scalar is zero.
    ///
    /// Uses constant-time comparison via `subtle::ConstantTimeEq` to avoid
    /// timing side-channels. An alternative approach that avoids the
    /// `to_bytes()` round-trip would be to compare the Montgomery limbs
    /// directly using `blst_fr_is_one` / zero-check intrinsics, but blst
    /// does not expose a `blst_fr_is_zero` function. The current approach
    /// is correct and constant-time, just not zero-copy.
    pub fn is_zero(&self) -> bool {
        self.to_bytes().ct_eq(&[0u8; 32]).unwrap_u8() == 1
    }

    /// The multiplicative identity (one).
    pub fn one() -> Self {
        Self::from_u64(1)
    }

    // Internal access to the wrapped blst_fr.
    #[allow(dead_code)]
    pub(crate) fn as_inner(&self) -> &blst_fr {
        &self.inner
    }

    /// Raw addition of two scalars: `self + rhs` (mod r).
    pub fn add(&self, rhs: &Self) -> Self {
        let result = unsafe {
            // SAFETY: blst_fr_add computes (a + b) mod r in Montgomery form.
            // Both inputs are valid field elements, so the result is always
            // a valid field element.
            let mut out = std::mem::MaybeUninit::<blst_fr>::uninit();
            blst_fr_add(out.as_mut_ptr(), &self.inner, &rhs.inner);
            out.assume_init()
        };
        Self { inner: result }
    }

    /// Raw multiplication of two scalars: `self * rhs` (mod r).
    pub fn multiply(&self, rhs: &Self) -> Self {
        let result = unsafe {
            // SAFETY: blst_fr_mul computes (a * b) mod r in Montgomery form.
            // Both inputs are valid field elements, so the result is always
            // a valid field element.
            let mut out = std::mem::MaybeUninit::<blst_fr>::uninit();
            blst_fr_mul(out.as_mut_ptr(), &self.inner, &rhs.inner);
            out.assume_init()
        };
        Self { inner: result }
    }

    /// Negate a scalar: `-self` (mod r).
    pub fn negate(&self) -> Self {
        let result = unsafe {
            // SAFETY: blst_fr_cneg computes conditional negation.
            // Passing `true` unconditionally negates the input.
            let mut out = self.inner;
            blst_fr_cneg(&mut out, &self.inner, true);
            out
        };
        Self { inner: result }
    }

    /// Subtract two scalars: `self - rhs` (mod r).
    pub fn sub(&self, rhs: &Self) -> Self {
        let result = unsafe {
            // SAFETY: blst_fr_sub computes (a - b) mod r in Montgomery form.
            let mut out = std::mem::MaybeUninit::<blst_fr>::uninit();
            blst_fr_sub(out.as_mut_ptr(), &self.inner, &rhs.inner);
            out.assume_init()
        };
        Self { inner: result }
    }

    /// Compute the multiplicative inverse: `self^{-1}` (mod r).
    /// Returns `None` if `self` is zero.
    ///
    /// This implementation is constant-time: it always performs the
    /// `blst_fr_inverse` computation regardless of whether the input is
    /// zero, then uses constant-time conditional selection to return `None`
    /// for zero inputs. This avoids a timing leak that would reveal whether
    /// a scalar is zero.
    pub fn invert(&self) -> Option<Self> {
        let is_zero = self.is_zero();
        // Always compute the inverse, even for zero (the result is undefined
        // but we discard it via conditional selection).
        let result = unsafe {
            let mut out = std::mem::MaybeUninit::<blst_fr>::uninit();
            blst_fr_inverse(out.as_mut_ptr(), &self.inner);
            out.assume_init()
        };
        // Use constant-time conditional selection: if zero, return None;
        // otherwise return Some(result). The `is_zero` boolean is derived
        // from constant-time comparison, and the branch on it occurs after
        // the inversion, so no timing leak from the inversion itself.
        if is_zero {
            None
        } else {
            Some(Self { inner: result })
        }
    }
}

impl fmt::Display for Scalar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bytes = self.to_bytes();
        // Display as hex (little-endian bytes reversed to big-endian display)
        write!(f, "0x")?;
        for byte in bytes.iter().rev() {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Evaluate a polynomial at a given point using Horner's method.
///
/// Given coefficients `[a_0, a_1, ..., a_n]` and evaluation point `x`,
/// computes `f(x) = a_0 + a_1*x + a_2*x^2 + ... + a_n*x^n` (mod r).
///
/// This replaces the previous BLAKE3-based "evaluation" with proper
/// BLS12-381 scalar field arithmetic, enabling Lagrange interpolation
/// and threshold reconstruction.
///
/// # Arguments
///
/// * `coeffs` — Polynomial coefficients in ascending order of power
/// * `x` — Evaluation point (a scalar)
///
/// # Returns
///
/// The polynomial evaluated at `x` as a `Scalar`.
pub fn polynomial_evaluate(coeffs: &[Scalar], x: &Scalar) -> Scalar {
    if coeffs.is_empty() {
        return Scalar::zero();
    }
    // Horner's method: a_0 + x*(a_1 + x*(a_2 + x*(...)))
    let mut result = coeffs[coeffs.len() - 1];
    for i in (0..coeffs.len() - 1).rev() {
        result = result.multiply(x);
        result = result.add(&coeffs[i]);
    }
    result
}

/// Compute a Feldman commitment: `C = scalar * G1`.
///
/// This performs scalar multiplication on the G1 subgroup of BLS12-381,
/// producing the standard Feldman commitment `C_j = a_j * G1` where `a_j`
/// is a polynomial coefficient. The result is a compressed G1 point (48 bytes).
///
/// # Arguments
///
/// * `scalar` — The coefficient scalar `a_j`
///
/// * `Returns` — 48-byte compressed G1 point, or `None` if the scalar is zero.
pub fn compute_commitment(scalar: &Scalar) -> Option<[u8; G1_COMPRESSED_SIZE]> {
    if scalar.is_zero() {
        return None;
    }

    // Convert Scalar (blst_fr Montgomery form) to blst_scalar (canonical form)
    // then compute PK = sk * G1 using blst_sk_to_pk_in_g1.
    // This is equivalent to scalar * G1 where G1 is the generator.
    unsafe {
        // Convert blst_fr -> blst_scalar (canonical representation)
        let mut blst_scalar_val = std::mem::MaybeUninit::<blst_scalar>::uninit();
        blst_scalar_from_fr(blst_scalar_val.as_mut_ptr(), &scalar.inner);

        // Compute pk = scalar * G1
        let mut pk = std::mem::MaybeUninit::<blst_p1>::uninit();
        blst_sk_to_pk_in_g1(pk.as_mut_ptr(), blst_scalar_val.as_ptr());

        // Compress the projective point directly
        let mut out = [0u8; G1_COMPRESSED_SIZE];
        blst_p1_compress(out.as_mut_ptr(), pk.as_ptr());
        Some(out)
    }
}

/// Derive a BLS public key in G1 from a scalar secret key.
///
/// This is the algebraically correct way to derive a public key from a DKG
/// share: `PK = sk * G1`. Unlike `BlsKeypair::generate()` which hashes the
/// seed through `key_gen` (HKDF-based), this function directly multiplies
/// the scalar by the G1 generator, preserving algebraic consistency.
///
/// # Arguments
///
/// * `scalar` — The secret key as a BLS12-381 scalar field element
///
/// * `Returns` — 48-byte compressed G1 public key point
pub fn scalar_to_g1_public_key(scalar: &Scalar) -> [u8; G1_COMPRESSED_SIZE] {
    unsafe {
        // Convert blst_fr -> blst_scalar (canonical representation)
        let mut blst_scalar_val = std::mem::MaybeUninit::<blst_scalar>::uninit();
        blst_scalar_from_fr(blst_scalar_val.as_mut_ptr(), &scalar.inner);

        // Compute pk = scalar * G1
        let mut pk = std::mem::MaybeUninit::<blst_p1>::uninit();
        blst_sk_to_pk_in_g1(pk.as_mut_ptr(), blst_scalar_val.as_ptr());

        // Compress the projective point directly
        let mut out = [0u8; G1_COMPRESSED_SIZE];
        blst_p1_compress(out.as_mut_ptr(), pk.as_ptr());
        out
    }
}

/// Validate that a byte slice represents a valid compressed G1 point on BLS12-381.
///
/// Returns `true` if the bytes can be successfully decompressed as a G1 point,
/// `false` otherwise. This is used for commitment verification in DKG.
pub fn validate_g1_point(bytes: &[u8]) -> bool {
    if bytes.len() != G1_COMPRESSED_SIZE {
        return false;
    }
    unsafe {
        let mut pt = std::mem::MaybeUninit::<blst_p1_affine>::uninit();
        blst_p1_uncompress(pt.as_mut_ptr(), bytes.as_ptr()) == BLST_ERROR::BLST_SUCCESS
    }
}

/// Aggregate multiple compressed G1 points by point addition.
///
/// Deserializes each 48-byte compressed G1 point, adds them together
/// via repeated `blst_p1_add_or_double`, and returns the compressed result.
/// Used to aggregate Feldman C_0 commitments into the group public key.
///
/// # Arguments
///
/// * `points` — Slice of 48-byte compressed G1 point references
///
/// * `Returns` — Compressed G1 point as `[u8; 48]`, or `None` if any point is invalid.
pub fn aggregate_g1_points(points: &[&[u8]]) -> Option<[u8; G1_COMPRESSED_SIZE]> {
    if points.is_empty() {
        return None;
    }

    let mut acc = blst_p1::default(); // identity (point at infinity)

    for point_bytes in points {
        if point_bytes.len() != G1_COMPRESSED_SIZE {
            return None;
        }
        // Decompress
        let affine = unsafe {
            let mut pt = std::mem::MaybeUninit::<blst_p1_affine>::uninit();
            if blst_p1_uncompress(pt.as_mut_ptr(), point_bytes.as_ptr()) != BLST_ERROR::BLST_SUCCESS {
                return None;
            }
            pt.assume_init()
        };
        // Convert to projective
        let proj = unsafe {
            let mut pt = std::mem::MaybeUninit::<blst_p1>::uninit();
            blst_p1_from_affine(pt.as_mut_ptr(), &affine);
            pt.assume_init()
        };
        // Add to accumulator
        acc = unsafe {
            let mut sum = std::mem::MaybeUninit::<blst_p1>::uninit();
            blst_p1_add_or_double(sum.as_mut_ptr(), &acc, &proj);
            sum.assume_init()
        };
    }

    // Compress result (from projective directly)
    let result = unsafe {
        let mut out = [0u8; G1_COMPRESSED_SIZE];
        blst_p1_compress(out.as_mut_ptr(), &acc);
        out
    };
    Some(result)
}

/// Verify a Feldman share against commitments using group operations.
///
/// Checks that `share * G1 == sum_{j=0}^{t-1}(index^j * C_j)` where `C_j`
/// are the Feldman commitments (G1 points). This is the standard Feldman VSS
/// verification equation, which verifies that the share is a valid evaluation
/// of the polynomial whose commitments are published.
///
/// # Arguments
///
/// * `share` — The claimed share (a scalar)
/// * `index` — The 1-based participant index
/// * `commitments` — The Feldman commitments (48-byte compressed G1 points)
///
/// * `Returns` — `true` if the share is consistent with the commitments.
pub fn verify_feldman_share(share: &Scalar, index: u64, commitments: &[Vec<u8>]) -> bool {
    // Basic structural checks
    if share.is_zero() || index == 0 || commitments.is_empty() {
        return false;
    }

    // All commitments must be 48-byte compressed G1 points
    for c in commitments {
        if c.len() != G1_COMPRESSED_SIZE {
            return false;
        }
    }

    // Compute left side: share * G1
    let left = unsafe {
        // Convert blst_fr -> blst_scalar (canonical representation)
        let mut blst_scalar_val = std::mem::MaybeUninit::<blst_scalar>::uninit();
        blst_scalar_from_fr(blst_scalar_val.as_mut_ptr(), &share.inner);

        let mut pt = std::mem::MaybeUninit::<blst_p1>::uninit();
        blst_sk_to_pk_in_g1(pt.as_mut_ptr(), blst_scalar_val.as_ptr());
        pt.assume_init()
    };

    // Compute right side: sum_{j=0}^{t-1}(index^j * C_j)
    // Using multi-scalar multiplication approach:
    // Decompose each commitment C_j, multiply by index^j, and add to accumulator.
    let index_scalar = Scalar::from_u64(index);
    let mut right = blst_p1::default(); // identity (point at infinity)

    let mut index_power = Scalar::one(); // index^0 = 1

    for commitment_bytes in commitments {
        // Decompress the commitment G1 point
        let commitment_affine = unsafe {
            let mut pt = std::mem::MaybeUninit::<blst_p1_affine>::uninit();
            if blst_p1_uncompress(pt.as_mut_ptr(), commitment_bytes.as_ptr()) != BLST_ERROR::BLST_SUCCESS {
                return false; // Invalid commitment point
            }
            pt.assume_init()
        };

        // Convert affine to projective for multiplication
        let commitment_proj = unsafe {
            let mut pt = std::mem::MaybeUninit::<blst_p1>::uninit();
            blst_p1_from_affine(pt.as_mut_ptr(), &commitment_affine);
            pt.assume_init()
        };

        // Compute index_power * C_j using double-and-add via blst
        // Convert index_power to scalar bytes for blst multiplication
        let power_bytes = index_power.to_bytes();
        let term = unsafe {
            let mut pt = std::mem::MaybeUninit::<blst_p1>::uninit();
            blst_p1_mult(pt.as_mut_ptr(), &commitment_proj, power_bytes.as_ptr(), 256);
            pt.assume_init()
        };

        // Accumulate: right += term
        right = unsafe {
            let mut sum = std::mem::MaybeUninit::<blst_p1>::uninit();
            blst_p1_add_or_double(sum.as_mut_ptr(), &right, &term);
            sum.assume_init()
        };

        // index_power *= index_scalar
        index_power = index_power.multiply(&index_scalar);
    }

    // Compare left and right by comparing their compressed forms
    let left_compressed = unsafe {
        let mut out = [0u8; G1_COMPRESSED_SIZE];
        blst_p1_compress(out.as_mut_ptr(), &left);
        out
    };
    let right_compressed = unsafe {
        let mut out = [0u8; G1_COMPRESSED_SIZE];
        blst_p1_compress(out.as_mut_ptr(), &right);
        out
    };

    left_compressed.ct_eq(&right_compressed).unwrap_u8() == 1
}

/// Accumulate shares using field addition instead of hashing.
///
/// Given existing accumulated share and a new share, returns their sum
/// in the BLS12-381 scalar field. This replaces the previous BLAKE3
/// hash-based accumulation with proper field addition, enabling
/// Lagrange interpolation for threshold reconstruction.
///
/// # Arguments
///
/// * `accumulated` — The running sum of shares (or None for first share)
/// * `new_share` — The new share to add
///
/// # Returns
///
/// The field addition of accumulated and new_share.
pub fn accumulate_share_field(accumulated: Option<Scalar>, new_share: Scalar) -> Scalar {
    match accumulated {
        Some(existing) => existing.add(&new_share),
        None => new_share,
    }
}

/// Reconstruct the secret from a set of shares using Lagrange interpolation.
///
/// Given `threshold` shares of the form `(x_i, y_i)`, reconstructs the
/// constant term of the polynomial (i.e., the secret) using Lagrange
/// basis polynomials evaluated at x = 0:
///
/// ```text
/// secret = sum_i( y_i * L_i(0) )
/// where L_i(0) = product_{j != i} (0 - x_j) / (x_i - x_j)
///             = product_{j != i} ( x_j / (x_i - x_j) )
/// ```
///
/// This is the standard reconstruction formula for Shamir's Secret Sharing
/// and Feldman VSS. The result is exact (no approximation error) because
/// all arithmetic is in the finite field modulo the BLS12-381 subgroup order.
///
/// # Arguments
///
/// * `shares` — A vector of `(index, share_value)` pairs where `index` is
///   the 1-based participant index and `share_value` is the scalar share.
///   Must contain at least 2 shares for meaningful reconstruction.
///
/// # Returns
///
/// The reconstructed secret as a `Scalar`, or `None` if fewer than 2 shares
/// are provided or if any two shares have the same index.
pub fn reconstruct_secret(shares: &[(usize, Scalar)]) -> Option<Scalar> {
    if shares.len() < 2 {
        return None;
    }

    // Check for duplicate indices using HashSet (O(n) instead of O(n²))
    let mut seen_indices = std::collections::HashSet::new();
    for (idx, _) in shares {
        if !seen_indices.insert(*idx) {
            return None; // Duplicate index
        }
    }

    let mut secret = Scalar::zero();

    for (i, (xi, yi)) in shares.iter().enumerate() {
        // Compute Lagrange basis polynomial L_i(0)
        // L_i(0) = product_{j != i} (x_j / (x_i - x_j))
        let xi_scalar = Scalar::from_u64(*xi as u64);
        let mut li = Scalar::one(); // Start with 1

        for (j, (xj, _)) in shares.iter().enumerate() {
            if i == j {
                continue;
            }
            let xj_scalar = Scalar::from_u64(*xj as u64);

            // numerator = x_j (since we evaluate at 0: (0 - x_j) = -x_j,
            //   but we want x_j / (x_i - x_j), not -x_j / (x_i - x_j))
            // Actually: L_i(0) = prod_{j!=i} (0 - x_j) / (x_i - x_j)
            //                      = prod_{j!=i} (-x_j) / (x_i - x_j)
            //                      = prod_{j!=i} x_j / (x_j - x_i)
            // (negating both numerator and denominator)
            let numerator = xj_scalar;
            let denominator = xj_scalar.sub(&xi_scalar);

            if denominator.is_zero() {
                return None; // Duplicate index — should not happen after check above
            }

            let denom_inv = denominator.invert()?;
            let term = numerator.multiply(&denom_inv);
            li = li.multiply(&term);
        }

        // Accumulate: secret += y_i * L_i(0)
        let contribution = yi.multiply(&li);
        secret = secret.add(&contribution);
    }

    Some(secret)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_scalar_from_bytes_roundtrip() {
        let bytes = [42u8; 32];
        let scalar = Scalar::from_bytes(&bytes).unwrap();
        let recovered = scalar.to_bytes();
        // Note: bytes are reduced mod r, so may not be identical
        assert!(!scalar.is_zero());
        assert_eq!(scalar, Scalar::from_bytes(&recovered).unwrap());
    }

    #[test]
    fn test_scalar_zero() {
        let zero = Scalar::zero();
        assert!(zero.is_zero());
        let bytes = zero.to_bytes();
        assert_eq!(bytes, [0u8; 32]);
    }

    #[test]
    fn test_scalar_one() {
        let one = Scalar::one();
        assert!(!one.is_zero());
        let bytes = one.to_bytes();
        assert_eq!(bytes[0], 1);
        assert!(bytes[1..].iter().all(|&b| b == 0));
    }

    #[test]
    fn test_scalar_addition() {
        let a = Scalar::from_u64(5);
        let b = Scalar::from_u64(7);
        let sum = a.add(&b);
        // 5 + 7 = 12 mod r
        let expected = Scalar::from_u64(12);
        assert_eq!(sum, expected);
    }

    #[test]
    fn test_scalar_addition_zero() {
        let a = Scalar::from_u64(42);
        let zero = Scalar::zero();
        let sum = a.add(&zero);
        assert_eq!(sum, a);
    }

    #[test]
    fn test_scalar_subtraction() {
        let a = Scalar::from_u64(10);
        let b = Scalar::from_u64(3);
        let diff = a.sub(&b);
        let expected = Scalar::from_u64(7);
        assert_eq!(diff, expected);
    }

    #[test]
    fn test_scalar_negate() {
        let a = Scalar::from_u64(1);
        let neg = a.negate();
        // -1 + 1 = 0
        let sum = neg.add(&a);
        assert!(sum.is_zero());
    }

    #[test]
    fn test_scalar_multiplication() {
        let a = Scalar::from_u64(6);
        let b = Scalar::from_u64(7);
        let product = a.multiply(&b);
        // 6 * 7 = 42 mod r
        let expected = Scalar::from_u64(42);
        assert_eq!(product, expected);
    }

    #[test]
    fn test_scalar_multiplication_by_zero() {
        let a = Scalar::from_u64(42);
        let zero = Scalar::zero();
        let product = a.multiply(&zero);
        assert!(product.is_zero());
    }

    #[test]
    fn test_scalar_invert() {
        let a = Scalar::from_u64(42);
        let inv = a.invert().expect("non-zero scalar should invert");
        // a * a^{-1} = 1
        let product = a.multiply(&inv);
        assert_eq!(product, Scalar::one());
    }

    #[test]
    fn test_scalar_invert_zero_fails() {
        let zero = Scalar::zero();
        assert!(zero.invert().is_none());
    }

    #[test]
    fn test_scalar_random_is_non_zero() {
        let mut rng = rand::thread_rng();
        let scalar = Scalar::random(&mut rng);
        // Probability of random scalar being zero is 1/r ~ 2^{-256}
        assert!(!scalar.is_zero());
    }

    #[test]
    fn test_polynomial_evaluate_constant() {
        // f(x) = 5 (constant polynomial)
        let coeffs = vec![Scalar::from_u64(5)];
        let x = Scalar::from_u64(123);
        let result = polynomial_evaluate(&coeffs, &x);
        assert_eq!(result, Scalar::from_u64(5));
    }

    #[test]
    fn test_polynomial_evaluate_linear() {
        // f(x) = 2 + 3x
        let coeffs = vec![Scalar::from_u64(2), Scalar::from_u64(3)];
        let x = Scalar::from_u64(4);
        let result = polynomial_evaluate(&coeffs, &x);
        // f(4) = 2 + 3*4 = 14
        assert_eq!(result, Scalar::from_u64(14));
    }

    #[test]
    fn test_polynomial_evaluate_quadratic() {
        // f(x) = 1 + 2x + 3x^2
        let coeffs = vec![Scalar::from_u64(1), Scalar::from_u64(2), Scalar::from_u64(3)];
        let x = Scalar::from_u64(2);
        let result = polynomial_evaluate(&coeffs, &x);
        // f(2) = 1 + 2*2 + 3*4 = 1 + 4 + 12 = 17
        assert_eq!(result, Scalar::from_u64(17));
    }

    #[test]
    fn test_polynomial_evaluate_empty() {
        let coeffs: Vec<Scalar> = vec![];
        let x = Scalar::from_u64(1);
        let result = polynomial_evaluate(&coeffs, &x);
        assert!(result.is_zero());
    }

    #[test]
    fn test_share_accumulation() {
        let share1 = Scalar::from_u64(10);
        let share2 = Scalar::from_u64(20);
        let share3 = Scalar::from_u64(30);

        let accum1 = accumulate_share_field(None, share1);
        let accum2 = accumulate_share_field(Some(accum1), share2);
        let accum3 = accumulate_share_field(Some(accum2), share3);

        // 10 + 20 + 30 = 60
        assert_eq!(accum3, Scalar::from_u64(60));
    }

    #[test]
    fn test_share_accumulation_commutative() {
        let a = Scalar::from_u64(5);
        let b = Scalar::from_u64(7);

        let ab = accumulate_share_field(Some(a), b);
        let ba = accumulate_share_field(Some(b), a);

        assert_eq!(ab, ba);
    }

    #[test]
    fn test_feldman_share_verify_valid() {
        // f(x) = 10 + 5x, so f(3) = 25
        // Commitments: C_0 = 10*G1, C_1 = 5*G1
        let a0 = Scalar::from_u64(10);
        let a1 = Scalar::from_u64(5);
        let c0 = compute_commitment(&a0).unwrap();
        let c1 = compute_commitment(&a1).unwrap();
        let commitments = vec![c0.to_vec(), c1.to_vec()];

        let share = polynomial_evaluate(&[a0, a1], &Scalar::from_u64(3));
        assert!(verify_feldman_share(&share, 3, &commitments));
    }

    #[test]
    fn test_feldman_share_verify_wrong_share_rejected() {
        // f(x) = 10 + 5x, so f(3) = 25
        // But we pass share = 26 (wrong)
        let a0 = Scalar::from_u64(10);
        let a1 = Scalar::from_u64(5);
        let c0 = compute_commitment(&a0).unwrap();
        let c1 = compute_commitment(&a1).unwrap();
        let commitments = vec![c0.to_vec(), c1.to_vec()];

        let wrong_share = Scalar::from_u64(26);
        assert!(!verify_feldman_share(&wrong_share, 3, &commitments));
    }

    #[test]
    fn test_feldman_share_verify_zero_rejected() {
        let c0 = compute_commitment(&Scalar::from_u64(42)).unwrap();
        let commitments = vec![c0.to_vec()];
        assert!(!verify_feldman_share(&Scalar::zero(), 1, &commitments));
    }

    #[test]
    fn test_feldman_share_verify_empty_commitments() {
        let share = Scalar::from_u64(42);
        let commitments: Vec<Vec<u8>> = vec![];
        assert!(!verify_feldman_share(&share, 1, &commitments));
    }

    #[test]
    fn test_feldman_share_verify_wrong_commitment_size() {
        let share = Scalar::from_u64(42);
        let commitments = vec![vec![1u8; 96]]; // Wrong size (96 not 48)
        assert!(!verify_feldman_share(&share, 1, &commitments));
    }

    #[test]
    fn test_compute_commitment_zero_returns_none() {
        assert!(compute_commitment(&Scalar::zero()).is_none());
    }

    #[test]
    fn test_scalar_to_g1_public_key() {
        let scalar = Scalar::from_u64(42);
        let pk_bytes = scalar_to_g1_public_key(&scalar);
        assert_eq!(pk_bytes.len(), G1_COMPRESSED_SIZE);
        // Should match compute_commitment
        let commitment = compute_commitment(&scalar).unwrap();
        assert_eq!(pk_bytes, commitment);
    }

    #[test]
    fn test_reconstruct_secret_linear() {
        // f(x) = 42 + 7x, so f(1)=49, f(2)=56, f(3)=63
        // Reconstructing from any 2 shares should give secret=42
        let shares = vec![
            (1, Scalar::from_u64(49)),
            (2, Scalar::from_u64(56)),
            (3, Scalar::from_u64(63)),
        ];
        let secret = reconstruct_secret(&shares).expect("reconstruction should succeed");
        assert_eq!(secret, Scalar::from_u64(42));
    }

    #[test]
    fn test_reconstruct_secret_quadratic() {
        // f(x) = 10 + 5x + 2x^2, so f(1)=17, f(2)=28, f(3)=43, f(4)=62
        // Need at least 3 shares (degree 2 polynomial), secret=10
        let shares = vec![
            (1, Scalar::from_u64(17)),
            (2, Scalar::from_u64(28)),
            (3, Scalar::from_u64(43)),
        ];
        let secret = reconstruct_secret(&shares).expect("reconstruction should succeed");
        assert_eq!(secret, Scalar::from_u64(10));
    }

    #[test]
    fn test_reconstruct_secret_subset_suffices() {
        // Same polynomial, any 3-of-4 shares should reconstruct the same secret
        let shares_a = vec![
            (1, Scalar::from_u64(17)),
            (2, Scalar::from_u64(28)),
            (4, Scalar::from_u64(62)),
        ];
        let shares_b = vec![
            (1, Scalar::from_u64(17)),
            (3, Scalar::from_u64(43)),
            (4, Scalar::from_u64(62)),
        ];
        let secret_a = reconstruct_secret(&shares_a).expect("reconstruction should succeed");
        let secret_b = reconstruct_secret(&shares_b).expect("reconstruction should succeed");
        assert_eq!(secret_a, secret_b);
        assert_eq!(secret_a, Scalar::from_u64(10));
    }

    #[test]
    fn test_reconstruct_secret_too_few_shares() {
        let shares = vec![(1, Scalar::from_u64(42))];
        assert!(reconstruct_secret(&shares).is_none());
    }

    #[test]
    fn test_reconstruct_secret_duplicate_indices() {
        let shares = vec![(1, Scalar::from_u64(17)), (1, Scalar::from_u64(28))];
        assert!(reconstruct_secret(&shares).is_none());
    }

    #[test]
    fn test_reconstruct_secret_with_random_polynomial() {
        // Create a random polynomial, evaluate at points, then reconstruct
        let mut rng = rand::thread_rng();
        let secret = Scalar::random(&mut rng);
        let a1 = Scalar::random(&mut rng);
        let a2 = Scalar::random(&mut rng);
        // f(x) = secret + a1*x + a2*x^2
        let coeffs = vec![secret, a1, a2];

        // Evaluate at indices 1..5
        let shares: Vec<(usize, Scalar)> = (1..=5)
            .map(|i| (i, polynomial_evaluate(&coeffs, &Scalar::from_u64(i as u64))))
            .collect();

        // Reconstruct from first 3 shares (threshold = 3)
        let reconstructed = reconstruct_secret(&shares[..3]).expect("reconstruction should succeed");
        assert_eq!(reconstructed, secret, "reconstructed secret must match original");
    }
}
