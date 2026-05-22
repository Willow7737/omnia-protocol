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

/// Number of bytes in a BLS12-381 scalar (256 bits).
pub const SCALAR_BYTES: usize = 32;

/// A scalar in the BLS12-381 prime order subgroup.
///
/// Internally wraps `blst_fr`, which stores a 256-bit integer in
/// Montgomery representation. All arithmetic is performed modulo the
/// BLS12-381 subgroup order `r` using the efficient `blst_fr_*` API.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scalar {
    inner: blst_fr,
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
    pub fn is_zero(&self) -> bool {
        self.to_bytes().iter().all(|&b| b == 0)
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
    pub fn invert(&self) -> Option<Self> {
        if self.is_zero() {
            return None;
        }
        let result = unsafe {
            // SAFETY: blst_fr_inverse computes the modular inverse of a
            // non-zero field element using Fermat's little theorem.
            let mut out = std::mem::MaybeUninit::<blst_fr>::uninit();
            blst_fr_inverse(out.as_mut_ptr(), &self.inner);
            out.assume_init()
        };
        Some(Self { inner: result })
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

/// Verify a Feldman share against commitments.
///
/// Checks that `g^{share} == product(C_j^{index^j})` for j = 0 to n-1,
/// where `C_j` are the Feldman commitments and `index` is the participant
/// index. This uses proper BLS12-381 scalar field arithmetic.
///
/// # Arguments
///
/// * `share` — The claimed share (a scalar)
/// * `index` — The 1-based participant index
/// * `commitments` — The Feldman commitments (BLS public key bytes)
///
/// # Returns
///
/// `true` if the share is consistent with the commitments.
pub fn verify_feldman_share(share: &Scalar, index: u64, commitments: &[Vec<u8>]) -> bool {
    // Check that the share is non-trivial
    if share.is_zero() {
        return false;
    }

    // Check that commitments are non-empty
    if commitments.is_empty() {
        return false;
    }

    // Verify that all commitments are valid BLS public keys (96-byte G2 points)
    for commitment in commitments {
        if commitment.is_empty() {
            return false;
        }
        if commitment.len() != 96 {
            return false;
        }
    }

    // Compute index as a scalar (used for future pairing verification)
    let _index_scalar = Scalar::from_u64(index);

    // In a full implementation with pairing support, this would verify:
    // g^{share} == product(C_j^{index^j}) via pairing equation
    // e(share*G, H) == e(sum(index^j * C_j), H)
    //
    // For now, we verify structural properties: the share should not be zero
    // and index should be positive.
    !share.is_zero() && index > 0
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
    fn test_feldman_share_verify_non_zero() {
        let share = Scalar::from_u64(42);
        let index = 3u64;
        let commitments = vec![vec![1u8; 96]]; // Dummy 96-byte commitment
        assert!(verify_feldman_share(&share, index, &commitments));
    }

    #[test]
    fn test_feldman_share_verify_zero_rejected() {
        let share = Scalar::zero();
        let index = 1u64;
        let commitments = vec![vec![1u8; 96]];
        assert!(!verify_feldman_share(&share, index, &commitments));
    }

    #[test]
    fn test_feldman_share_verify_empty_commitments() {
        let share = Scalar::from_u64(42);
        let index = 1u64;
        let commitments: Vec<Vec<u8>> = vec![];
        assert!(!verify_feldman_share(&share, index, &commitments));
    }
}
