//! Fixed-point arithmetic for consensus-critical governance calculations
//!
//! Floating-point arithmetic (`f64`/`f32`) is non-deterministic across platforms:
//! x86 uses 80-bit extended precision while ARM uses strict 64-bit. Two nodes
//! computing the same governance calculation can get different results, causing
//! consensus divergence.
//!
//! This module replaces all floating-point arithmetic with integer fixed-point
//! using **parts-per-million (PPM)**:
//!
//! - `BASIS_PPM = 1_000_000` represents 1.0
//! - 10% decay = `100_000` PPM
//! - Decay formula: `effective = (base_weight * (BASIS - rate)^n / BASIS^n)` computed iteratively
//!
//! All operations use `u64` integer arithmetic — no `f64` or `f32` anywhere.

use serde::{Deserialize, Serialize};

/// One million — the fixed-point basis representing 1.0 in PPM.
///
/// Using PPM (parts per million) gives 6 decimal places of precision,
/// which is more than sufficient for governance decay rates while
/// remaining entirely in integer arithmetic.
///
/// # Examples
///
/// ```
/// use omnia_economics::fixed_point::BASIS_PPM;
/// assert_eq!(BASIS_PPM, 1_000_000);
/// ```
pub const BASIS_PPM: u64 = 1_000_000;

/// A fixed-point decay rate in parts-per-million.
///
/// Encodes a rate between 0.0 and 1.0 as an integer where
/// `BASIS_PPM` = 1.0. For example, a 10% decay rate is `100_000` PPM.
///
/// # Invariants
///
/// - `ppm` is always in `[0, BASIS_PPM]`
/// - Serializes as a `u64` PPM value for deterministic cross-platform behavior
///
/// # Examples
///
/// ```
/// use omnia_economics::fixed_point::DecayRate;
/// let rate = DecayRate::ten_percent();
/// assert_eq!(rate.ppm(), 100_000);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DecayRate {
    /// The PPM value of this decay rate.
    ppm: u64,
}

impl DecayRate {
    /// Create a new decay rate from a PPM value.
    ///
    /// Clamps the value to `[0, BASIS_PPM]` to ensure the rate is valid.
    /// A rate of 0 means no decay; a rate of `BASIS_PPM` means 100% decay
    /// per epoch (all weight is lost immediately).
    ///
    /// # Arguments
    ///
    /// * `ppm` — The decay rate in parts-per-million (0 = 0%, 1_000_000 = 100%)
    ///
    /// # Examples
    ///
    /// ```
    /// use omnia_economics::fixed_point::{DecayRate, BASIS_PPM};
    /// let rate = DecayRate::new(50_000); // 5% decay
    /// assert_eq!(rate.ppm(), 50_000);
    ///
    /// let clamped = DecayRate::new(BASIS_PPM + 100);
    /// assert_eq!(clamped.ppm(), BASIS_PPM); // Clamped to 100%
    /// ```
    pub fn new(ppm: u64) -> Self {
        if ppm > BASIS_PPM {
            tracing::warn!(
                "DecayRate::new({}) clamped to maximum {}", ppm, BASIS_PPM
            );
        }
        Self { ppm: ppm.min(BASIS_PPM) }
    }

    /// Convenience constructor for the standard 10% decay rate.
    ///
    /// # Examples
    ///
    /// ```
    /// use omnia_economics::fixed_point::DecayRate;
    /// let rate = DecayRate::ten_percent();
    /// assert_eq!(rate.ppm(), 100_000);
    /// ```
    pub fn ten_percent() -> Self {
        Self::new(100_000)
    }

    /// Create a decay rate from a percentage (0-100).
    ///
    /// # Arguments
    ///
    /// * `percent` — The decay percentage (0 = 0%, 100 = 100%)
    ///
    /// # Examples
    ///
    /// ```
    /// use omnia_economics::fixed_point::DecayRate;
    /// let rate = DecayRate::from_percent(10); // 10% decay
    /// assert_eq!(rate.ppm(), 100_000);
    /// ```
    pub fn from_percent(percent: u64) -> Self {
        Self::new(percent.saturating_mul(10_000))
    }

    /// Return the PPM value of this decay rate.
    ///
    /// # Examples
    ///
    /// ```
    /// use omnia_economics::fixed_point::DecayRate;
    /// assert_eq!(DecayRate::ten_percent().ppm(), 100_000);
    /// ```
    pub fn ppm(&self) -> u64 {
        self.ppm
    }

    /// Compute the remaining weight fraction (in PPM) after `epochs` of inactivity.
    ///
    /// The formula is: `remaining = (BASIS - rate)^epochs / BASIS^epochs`
    ///
    /// Computed iteratively to avoid overflow: each step multiplies by
    /// `(BASIS - rate)` and divides by `BASIS`, keeping the intermediate
    /// result in PPM space.
    ///
    /// # Arguments
    ///
    /// * `epochs` — Number of inactive epochs
    ///
    /// # Returns
    ///
    /// The remaining weight fraction in PPM (0 = fully decayed, 1_000_000 = no decay)
    ///
    /// # Examples
    ///
    /// ```
    /// use omnia_economics::fixed_point::{DecayRate, BASIS_PPM};
    /// let rate = DecayRate::ten_percent();
    ///
    /// // 0 epochs inactive → full weight
    /// assert_eq!(rate.remaining_ppm_after(0), BASIS_PPM);
    ///
    /// // 1 epoch inactive → 90% remaining
    /// assert_eq!(rate.remaining_ppm_after(1), 900_000);
    /// ```
    pub fn remaining_ppm_after(&self, epochs: u64) -> u64 {
        if epochs == 0 {
            return BASIS_PPM;
        }
        if self.ppm == 0 {
            return BASIS_PPM;
        }
        if self.ppm == BASIS_PPM {
            return 0; // 100% decay → zero remaining immediately
        }

        let remaining_per_epoch = BASIS_PPM - self.ppm;
        let mut result: u64 = BASIS_PPM;
        for _ in 0..epochs {
            // Multiply then divide to minimize rounding error.
            // We use checked arithmetic to prevent overflow; on overflow,
            // the result saturates.
            result = result
                .checked_mul(remaining_per_epoch)
                .and_then(|v| v.checked_div(BASIS_PPM))
                .unwrap_or(0);
            if result == 0 {
                break; // Fully decayed — no point continuing
            }
        }
        result
    }
}

impl Default for DecayRate {
    fn default() -> Self {
        Self::ten_percent()
    }
}

/// Integer square root via Newton's method.
///
/// Returns the largest integer `r` such that `r * r <= n`.
/// This is used for quadratic voting where `weight = sqrt(stake)`.
///
/// Newton's method converges quadratically — it finds the exact integer
/// square root in at most ~64 iterations for any `u64` value.
///
/// # Arguments
///
/// * `n` — A non-negative integer
///
/// # Returns
///
/// The integer square root of `n`
///
/// # Examples
///
/// ```
/// use omnia_economics::fixed_point::isqrt;
/// assert_eq!(isqrt(0), 0);
/// assert_eq!(isqrt(1), 1);
/// assert_eq!(isqrt(4), 2);
/// assert_eq!(isqrt(100), 10);
/// assert_eq!(isqrt(101), 10); // floor(sqrt(101)) = 10
/// ```
pub fn isqrt(n: u64) -> u64 {
    if n == 0 {
        return 0;
    }

    // Newton's method: x_{k+1} = (x_k + n / x_k) / 2
    // Use a good initial guess to minimize iterations.
    let bits = 64 - n.leading_zeros();
    let mut x: u64 = 1u64 << (bits.div_ceil(2));

    loop {
        let next = (x + n / x) / 2;
        if next >= x {
            // next >= x means we've converged; x is the answer or slightly below
            break;
        }
        x = next;
    }

    // Final adjustment: ensure x^2 <= n < (x+1)^2
    // Newton's method can converge to a value that's slightly too low
    // (integer division rounds down on each step).
    // We want the largest r such that r^2 <= n.
    //
    // Use u128 multiplication to avoid overflow for the boundary check.
    while x > 0 && (x as u128) * (x as u128) > (n as u128) {
        x -= 1;
    }
    while x < u64::MAX && ((x + 1) as u128) * ((x + 1) as u128) <= (n as u128) {
        x += 1;
    }

    x
}

/// Extension trait for multiplying a `u64` value by a PPM fraction.
///
/// Computes `self * ppm / BASIS_PPM` using integer arithmetic,
/// which is the core operation for applying decay rates to weights.
///
/// # Examples
///
/// ```
/// use omnia_economics::fixed_point::{BasisPpmExt, BASIS_PPM};
/// let weight: u64 = 10;
/// let remaining_ppm = 900_000; // 90% remaining after 1 epoch of 10% decay
/// assert_eq!(weight.mul_ppm(remaining_ppm), 9);
/// ```
pub trait BasisPpmExt {
    /// Multiply `self` by a PPM fraction: `(self * ppm) / BASIS_PPM`.
    ///
    /// Uses saturating arithmetic to prevent overflow on large values.
    fn mul_ppm(self, ppm: u64) -> u64;
}

impl BasisPpmExt for u64 {
    fn mul_ppm(self, ppm: u64) -> u64 {
        self.saturating_mul(ppm).saturating_div(BASIS_PPM)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    // --- DecayRate tests ---

    #[test]
    fn test_ten_percent_remaining_at_epoch_0() {
        assert_eq!(DecayRate::ten_percent().remaining_ppm_after(0), BASIS_PPM);
    }

    #[test]
    fn test_ten_percent_remaining_at_epoch_1() {
        assert_eq!(DecayRate::ten_percent().remaining_ppm_after(1), 900_000);
    }

    #[test]
    fn test_ten_percent_remaining_at_epoch_10() {
        let rate = DecayRate::ten_percent();
        let result = rate.remaining_ppm_after(10);
        // Expected: (900_000)^10 / (1_000_000)^10
        // = 0.9^10 = 0.3486784401... → 348_678 PPM
        let mut expected: u64 = BASIS_PPM;
        for _ in 0..10 {
            expected = expected * 900_000 / 1_000_000;
        }
        assert_eq!(result, expected);
        // Should be roughly 348k PPM (34.87%)
        assert!(result > 300_000 && result < 400_000);
    }

    #[test]
    fn test_zero_decay_rate() {
        let rate = DecayRate::new(0);
        assert_eq!(rate.remaining_ppm_after(0), BASIS_PPM);
        assert_eq!(rate.remaining_ppm_after(100), BASIS_PPM); // Never decays
    }

    #[test]
    fn test_full_decay_rate() {
        let rate = DecayRate::new(BASIS_PPM); // 100%
        assert_eq!(rate.remaining_ppm_after(0), BASIS_PPM);
        assert_eq!(rate.remaining_ppm_after(1), 0); // Instantly gone
    }

    #[test]
    fn test_decay_rate_clamping() {
        let rate = DecayRate::new(BASIS_PPM + 1000);
        assert_eq!(rate.ppm(), BASIS_PPM); // Clamped to 100%
    }

    #[test]
    fn test_from_percent() {
        assert_eq!(DecayRate::from_percent(0).ppm(), 0);
        assert_eq!(DecayRate::from_percent(10).ppm(), 100_000);
        assert_eq!(DecayRate::from_percent(50).ppm(), 500_000);
        assert_eq!(DecayRate::from_percent(100).ppm(), 1_000_000);
    }

    #[test]
    fn test_default_decay_rate() {
        assert_eq!(DecayRate::default(), DecayRate::ten_percent());
    }

    // --- isqrt tests ---

    #[test]
    fn test_isqrt_zero() {
        assert_eq!(isqrt(0), 0);
    }

    #[test]
    fn test_isqrt_one() {
        assert_eq!(isqrt(1), 1);
    }

    #[test]
    fn test_isqrt_perfect_squares() {
        assert_eq!(isqrt(4), 2);
        assert_eq!(isqrt(9), 3);
        assert_eq!(isqrt(16), 4);
        assert_eq!(isqrt(25), 5);
        assert_eq!(isqrt(100), 10);
        assert_eq!(isqrt(10000), 100);
        assert_eq!(isqrt(1_000_000), 1000);
    }

    #[test]
    fn test_isqrt_non_perfect_squares() {
        assert_eq!(isqrt(2), 1); // floor(1.414) = 1
        assert_eq!(isqrt(3), 1); // floor(1.732) = 1
        assert_eq!(isqrt(5), 2); // floor(2.236) = 2
        assert_eq!(isqrt(8), 2); // floor(2.828) = 2
        assert_eq!(isqrt(101), 10); // floor(10.05) = 10
    }

    #[test]
    fn test_isqrt_max_u64() {
        // sqrt(u64::MAX) = 4294967295
        assert_eq!(isqrt(u64::MAX), 4294967295);
    }

    #[test]
    fn test_isqrt_property_invariant() {
        // Property: isqrt(n)^2 <= n < (isqrt(n)+1)^2
        // Use u128 arithmetic to avoid overflow at boundary values.
        let test_values: Vec<u64> = vec![
            0,
            1,
            2,
            3,
            4,
            7,
            8,
            9,
            15,
            16,
            17,
            100,
            255,
            256,
            1000,
            65535,
            65536,
            1_000_000,
            1_000_000_000,
            u64::MAX / 2,
            u64::MAX,
        ];

        for n in test_values {
            let r = isqrt(n);
            let r_sq: u128 = (r as u128) * (r as u128);
            let n128: u128 = n as u128;
            assert!(r_sq <= n128, "isqrt({n}) = {r}, but {r}^2 = {r_sq} > {n}");
            if r < u64::MAX {
                let r_plus_1_sq: u128 = ((r + 1) as u128) * ((r + 1) as u128);
                assert!(
                    n128 < r_plus_1_sq,
                    "isqrt({n}) = {r}, but {n} >= ({r}+1)^2 = {r_plus_1_sq}"
                );
            }
        }
    }

    #[test]
    fn test_isqrt_property_fuzz() {
        // Extended property test with many random values
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        for i in 0u64..1000 {
            // Generate pseudo-random values using a simple hash
            let mut hasher = DefaultHasher::new();
            i.hash(&mut hasher);
            let n = hasher.finish();

            let r = isqrt(n);
            assert!(r.saturating_mul(r) <= n, "isqrt({n}) = {r}, but {r}^2 > {n}");
            if r < u64::MAX {
                assert!(
                    n < (r + 1).saturating_mul(r + 1),
                    "isqrt({n}) = {r}, but {n} >= ({r}+1)^2"
                );
            }
        }
    }

    // --- Serialization roundtrip ---

    #[test]
    fn test_decay_rate_serialization() {
        let rate = DecayRate::ten_percent();
        let bytes = postcard::to_allocvec(&rate).expect("serialize");
        let restored: DecayRate = postcard::from_bytes(&bytes).expect("deserialize");
        assert_eq!(rate, restored);
    }

    #[test]
    fn test_decay_rate_serialized_as_u64() {
        let rate = DecayRate::ten_percent();
        let bytes = postcard::to_allocvec(&rate.ppm()).expect("serialize u64");
        let bytes2 = postcard::to_allocvec(&rate).expect("serialize DecayRate");
        // DecayRate serializes as a u64 (transparent)
        assert_eq!(bytes, bytes2);
    }
}
