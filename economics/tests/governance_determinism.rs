#![allow(clippy::unwrap_used)]
//! Governance determinism regression tests
//!
//! These tests verify that governance calculations produce bit-for-bit
//! identical results across repeated calls, ensuring no floating-point
//! drift or platform-dependent behavior exists.

use omnia_economics::fixed_point::{isqrt, BASIS_PPM};
use omnia_economics::{DecayRate, GovernanceState};

/// Test that effective_weight produces the EXACT same result when called
/// 10,000 times. This would fail with f64 arithmetic due to potential
/// accumulation errors or platform-dependent precision.
#[test]
fn test_effective_weight_determinism_10k() {
    let mut gov = GovernanceState::new(DecayRate::ten_percent());
    gov.set_weight("did:omnia:alice", 100, 0); // base weight = 10

    let first = gov.effective_weight("did:omnia:alice", 5);
    for _ in 0..10_000 {
        assert_eq!(
            gov.effective_weight("did:omnia:alice", 5),
            first,
            "effective_weight is not deterministic across 10,000 calls"
        );
    }
}

/// Test edge cases for effective_weight
#[test]
fn test_effective_weight_edge_cases() {
    let mut gov = GovernanceState::new(DecayRate::ten_percent());

    // Zero base weight (unregistered DID)
    assert_eq!(gov.effective_weight("unknown", 0), 0);

    // Zero inactive epochs
    gov.set_weight("did:omnia:alice", 100, 0); // base weight = 10
    assert_eq!(gov.effective_weight("did:omnia:alice", 0), 10);

    // Decay rate = 0 PPM (no decay ever)
    let mut gov_no_decay = GovernanceState::new(DecayRate::new(0));
    gov_no_decay.set_weight("did:omnia:alice", 100, 0);
    assert_eq!(gov_no_decay.effective_weight("did:omnia:alice", 1000), 10);

    // Decay rate = BASIS_PPM (100% decay, instant zero)
    let mut gov_full_decay = GovernanceState::new(DecayRate::new(BASIS_PPM));
    gov_full_decay.set_weight("did:omnia:alice", 100, 0);
    assert_eq!(gov_full_decay.effective_weight("did:omnia:alice", 1), 0);
}

/// Test quadratic voting: set_weight uses integer square root
#[test]
fn test_quadratic_voting_isqrt() {
    let mut gov = GovernanceState::new(DecayRate::ten_percent());

    // 100 stake → isqrt(100) = 10
    gov.set_weight("did:omnia:alice", 100, 0);
    assert_eq!(gov.voting_weights.get("did:omnia:alice"), Some(&10));

    // 0 stake → minimum weight of 1
    gov.set_weight("did:omnia:zero", 0, 0);
    assert_eq!(gov.voting_weights.get("did:omnia:zero"), Some(&1));

    // Large value: u64::MAX → isqrt(u64::MAX) = 4294967295
    gov.set_weight("did:omnia:whale", u64::MAX, 0);
    assert_eq!(gov.voting_weights.get("did:omnia:whale"), Some(&4294967295));
}

/// Test that isqrt produces correct results for key values
#[test]
fn test_isqrt_key_values() {
    assert_eq!(isqrt(0), 0);
    assert_eq!(isqrt(1), 1);
    assert_eq!(isqrt(4), 2);
    assert_eq!(isqrt(100), 10);
    assert_eq!(isqrt(u64::MAX), 4294967295);

    // Property: isqrt(n)^2 <= n < (isqrt(n)+1)^2
    // Use u128 arithmetic to avoid overflow at boundary values.
    for n in [0u64, 1, 2, 3, 4, 100, 10000, 1_000_000, u64::MAX] {
        let r = isqrt(n);
        let r_sq: u128 = (r as u128) * (r as u128);
        let n128: u128 = n as u128;
        assert!(r_sq <= n128, "isqrt({n}) = {r}, but {r}^2 > {n}");
        if r < u64::MAX {
            let r_plus_1_sq: u128 = ((r + 1) as u128) * ((r + 1) as u128);
            assert!(n128 < r_plus_1_sq, "isqrt({n}) = {r}, but {n} >= ({r}+1)^2");
        }
    }
}
