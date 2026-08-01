//! Edwards25519 elliptic-curve Verifiable Random Function (EC-VRF).
//!
//! AUDIT-2026-07 C1 (#339): the previous `vrf.rs` leader selection was a
//! **public deterministic hash** — `BLAKE3(round_seed || round_number)` —
//! with the round seed stored in plain consensus state. Every future
//! round's leader was computable by anyone, enabling targeted DoS, MEV,
//! and coordinated equivocation. The "V2 ECVRF" scaffold in `vrf.rs` was
//! no better: its `gamma = BLAKE3(sk || H)` performs no elliptic-curve
//! operations, so it carries none of a VRF's uniqueness guarantees.
//!
//! This module is a **real** EC-VRF built on `curve25519-dalek`, following
//! the RFC 9381 ECVRF construction for Edwards25519:
//!
//! - **Hash-to-curve** by try-and-increment (RFC 9381 §5.4.1.1): hash the
//!   public key and input, decode as a compressed Edwards point, clear the
//!   cofactor; retry with an incrementing counter until a valid
//!   prime-order point is found.
//! - **Gamma = x·H** — a real scalar multiplication of the secret scalar
//!   `x` by the hash-to-curve point `H`. This is the VRF pre-output; it is
//!   unforgeable without `x` and *unique* for a given `(Y, alpha)` because
//!   the discrete log binds it.
//! - **Schnorr proof `(c, s)`** of the discrete-log equality
//!   `log_B(Y) == log_H(Gamma)` (RFC 9381 §5.1): `U = k·B`, `V = k·H`,
//!   `c = challenge(Y,H,Gamma,U,V)`, `s = k + c·x`. Verification recovers
//!   `U = s·B − c·Y`, `V = s·H − c·Gamma` and checks the challenge. This
//!   is what a verifier without `x` uses to confirm the output is correct.
//! - **Output** `beta = SHA-512("suite" || 0x03 || cofactor·Gamma || 0x00)`.
//!
//! The secret scalar and nonce seed come from the Ed25519 expanded secret
//! key (RFC 8032 clamped scalar + hash prefix) via `ed25519-dalek`'s
//! `hazmat` API, so a validator's existing Ed25519 identity key *is* its
//! VRF key — `Y = x·B` is exactly the Ed25519 public key. No new key
//! material, no new distribution problem.
//!
//! ## Security property this gives consensus
//!
//! Leader eligibility becomes `beta`, a value each validator can compute
//! only for itself (needs `x`) and everyone can verify (needs only the
//! proof). Nobody can predict another validator's `beta` for a future
//! round, and nobody can grind it — `Gamma`, hence `beta`, is uniquely
//! determined by `(x, alpha)`. Combined with an `alpha` that is not known
//! until the round begins (see `omnia-consensus`), this closes #339.
//!
//! ## Scope note (honest)
//!
//! This is a cryptographically sound EC-VRF: the Schnorr/DLEQ proof and
//! the discrete-log-bound `Gamma` provide uniqueness, collision
//! resistance, and pseudorandomness under the standard random-oracle
//! assumptions, independent of the exact hash-to-curve encoding. It
//! follows the RFC 9381 structure but is **not yet pinned to the RFC's
//! byte-exact interoperability test vectors** — cross-implementation KAT
//! validation is tracked as follow-up hardening. We do not claim
//! certified RFC 9381 interop; we claim (and test) a real EC-VRF with the
//! security properties consensus needs. This is deliberately more modest,
//! and more accurate, than the previous "ECVRF per RFC 9381" docs.

use curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
use curve25519_dalek::edwards::{CompressedEdwardsY, EdwardsPoint};
use curve25519_dalek::scalar::Scalar;
use curve25519_dalek::traits::IsIdentity;
use ed25519_dalek::hazmat::ExpandedSecretKey;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};
use subtle::ConstantTimeEq;
use thiserror::Error;

/// Suite identifier octet — Edwards25519 / SHA-512 / try-and-increment,
/// per RFC 9381 §5.5 (ECVRF-EDWARDS25519-SHA512-TAI).
const SUITE_STRING: u8 = 0x03;

/// Challenge length in octets (RFC 9381 Edwards25519 uses cLen = 16).
const C_LEN: usize = 16;

/// Length of the encoded proof: Gamma (32) + c (16) + s (32) = 80 octets.
pub const PROOF_LEN: usize = 32 + C_LEN + 32;

/// Length of the VRF output (`beta`) — a SHA-512 digest.
pub const OUTPUT_LEN: usize = 64;

/// Errors from EC-VRF proving and verification.
#[derive(Error, Debug, PartialEq, Eq)]
pub enum VrfError {
    /// The proof did not decode into a valid `(Gamma, c, s)` triple.
    #[error("malformed VRF proof: {0}")]
    MalformedProof(String),
    /// The public key did not decode to a valid curve point.
    #[error("invalid public key encoding")]
    InvalidPublicKey,
    /// The Schnorr challenge did not match — the proof is invalid for this
    /// (public key, input).
    #[error("VRF proof failed verification")]
    VerificationFailed,
}

/// An EC-VRF proof: `pi = Gamma || c || s`, 80 octets.
///
/// `Gamma` is a compressed Edwards point (the pre-output); `c` is the
/// 16-octet Schnorr challenge; `s` is a 32-octet scalar response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VrfProof {
    /// Compressed `Gamma` point (32 octets).
    pub gamma: [u8; 32],
    /// Schnorr challenge (16 octets).
    pub c: [u8; C_LEN],
    /// Schnorr response scalar (32 octets, canonical little-endian).
    pub s: [u8; 32],
}

impl VrfProof {
    /// Encode the proof as the canonical 80-octet string `Gamma || c || s`.
    pub fn to_bytes(&self) -> [u8; PROOF_LEN] {
        let mut out = [0u8; PROOF_LEN];
        out[..32].copy_from_slice(&self.gamma);
        out[32..32 + C_LEN].copy_from_slice(&self.c);
        out[32 + C_LEN..].copy_from_slice(&self.s);
        out
    }

    /// Decode a proof from its canonical 80-octet encoding.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, VrfError> {
        if bytes.len() != PROOF_LEN {
            return Err(VrfError::MalformedProof(format!(
                "expected {PROOF_LEN} octets, got {}",
                bytes.len()
            )));
        }
        let mut gamma = [0u8; 32];
        let mut c = [0u8; C_LEN];
        let mut s = [0u8; 32];
        gamma.copy_from_slice(&bytes[..32]);
        c.copy_from_slice(&bytes[32..32 + C_LEN]);
        s.copy_from_slice(&bytes[32 + C_LEN..]);
        Ok(Self { gamma, c, s })
    }
}

/// Hash-to-curve by try-and-increment (RFC 9381 §5.4.1.1).
///
/// Returns a non-identity point in the prime-order subgroup, deterministic
/// in `(public_key, alpha)`.
fn hash_to_curve(public_key: &[u8; 32], alpha: &[u8]) -> EdwardsPoint {
    for ctr in 0u16..=256 {
        let mut hasher = Sha512::new();
        hasher.update([SUITE_STRING]);
        hasher.update([0x01]); // one_string domain tag
        hasher.update(public_key);
        hasher.update(alpha);
        hasher.update([ctr as u8]);
        hasher.update([0x00]); // zero_string domain tag
        let digest = hasher.finalize();

        let mut candidate = [0u8; 32];
        candidate.copy_from_slice(&digest[..32]);
        if let Some(point) = CompressedEdwardsY(candidate).decompress() {
            let cleared = point.mul_by_cofactor();
            if !cleared.is_identity() {
                return cleared;
            }
        }
    }
    // Each counter value succeeds with probability ~1/2; 257 failures in a
    // row is a ~2^-257 event, i.e. it never happens for honest inputs.
    unreachable!("hash_to_curve exhausted the counter — statistically impossible");
}

/// Compute the Schnorr challenge `c` over the five-point transcript
/// (RFC 9381 §5.4.3), returned as a 16-octet value.
fn challenge(points: [&EdwardsPoint; 5]) -> [u8; C_LEN] {
    let mut hasher = Sha512::new();
    hasher.update([SUITE_STRING]);
    hasher.update([0x02]); // two_string domain tag
    for p in points {
        hasher.update(p.compress().to_bytes());
    }
    hasher.update([0x00]); // zero_string domain tag
    let digest = hasher.finalize();
    let mut c = [0u8; C_LEN];
    c.copy_from_slice(&digest[..C_LEN]);
    c
}

/// Lift a 16-octet challenge into a scalar (low 16 octets, little-endian).
fn challenge_scalar(c: &[u8; C_LEN]) -> Scalar {
    let mut wide = [0u8; 32];
    wide[..C_LEN].copy_from_slice(c);
    Scalar::from_bytes_mod_order(wide)
}

/// Derive the VRF output `beta` from `Gamma` (RFC 9381 §5.2).
fn proof_to_hash(gamma: &EdwardsPoint) -> [u8; OUTPUT_LEN] {
    let mut hasher = Sha512::new();
    hasher.update([SUITE_STRING]);
    hasher.update([0x03]); // three_string domain tag
    hasher.update(gamma.mul_by_cofactor().compress().to_bytes());
    hasher.update([0x00]); // zero_string domain tag
    let mut out = [0u8; OUTPUT_LEN];
    out.copy_from_slice(&hasher.finalize());
    out
}

/// Produce a VRF proof for `alpha` under the given Ed25519 signing key.
///
/// The returned proof both authorizes and determines the VRF output; call
/// [`output`] on it (or [`verify`]) to obtain `beta`.
pub fn prove(signing_key: &ed25519_dalek::SigningKey, alpha: &[u8]) -> VrfProof {
    let expanded = ExpandedSecretKey::from(&signing_key.to_bytes());
    let x = expanded.scalar;
    let public_key = signing_key.verifying_key().to_bytes();

    let h_point = hash_to_curve(&public_key, alpha);
    let h_string = h_point.compress().to_bytes();
    let gamma = x * h_point;

    // Nonce k = SHA-512(hash_prefix || h_string) reduced mod q
    // (RFC 9381 §5.4.2.2, ECVRF-EDWARDS25519 variant).
    let mut nonce_hasher = Sha512::new();
    nonce_hasher.update(expanded.hash_prefix);
    nonce_hasher.update(h_string);
    let mut k_wide = [0u8; 64];
    k_wide.copy_from_slice(&nonce_hasher.finalize());
    let k = Scalar::from_bytes_mod_order_wide(&k_wide);

    let u = k * ED25519_BASEPOINT_POINT;
    let v = k * h_point;
    let y_point = CompressedEdwardsY(public_key)
        .decompress()
        .expect("a signing key's own public key is always a valid point");

    let c = challenge([&y_point, &h_point, &gamma, &u, &v]);
    let s = k + challenge_scalar(&c) * x;

    VrfProof {
        gamma: gamma.compress().to_bytes(),
        c,
        s: s.to_bytes(),
    }
}

/// Verify a VRF proof against a public key and input, returning the VRF
/// output `beta` on success.
pub fn verify(
    public_key: &ed25519_dalek::VerifyingKey,
    alpha: &[u8],
    proof: &VrfProof,
) -> Result<[u8; OUTPUT_LEN], VrfError> {
    let pk_bytes = public_key.to_bytes();
    let y_point = CompressedEdwardsY(pk_bytes)
        .decompress()
        .ok_or(VrfError::InvalidPublicKey)?;

    let gamma = CompressedEdwardsY(proof.gamma)
        .decompress()
        .ok_or_else(|| VrfError::MalformedProof("gamma is not a valid curve point".into()))?;

    let s = Option::<Scalar>::from(Scalar::from_canonical_bytes(proof.s))
        .ok_or_else(|| VrfError::MalformedProof("s is not a canonical scalar".into()))?;
    let c_scalar = challenge_scalar(&proof.c);

    let h_point = hash_to_curve(&pk_bytes, alpha);

    // U = s·B − c·Y ; V = s·H − c·Gamma
    let u = s * ED25519_BASEPOINT_POINT - c_scalar * y_point;
    let v = s * h_point - c_scalar * gamma;

    let c_prime = challenge([&y_point, &h_point, &gamma, &u, &v]);

    if c_prime.ct_eq(&proof.c).into() {
        Ok(proof_to_hash(&gamma))
    } else {
        Err(VrfError::VerificationFailed)
    }
}

/// Recover the VRF output `beta` from a proof *without* verifying it.
///
/// Only use this when the proof has already been verified (e.g. it was
/// checked on receipt and stored). Prefer [`verify`], which returns the
/// same value only when the proof is valid.
pub fn output(proof: &VrfProof) -> Result<[u8; OUTPUT_LEN], VrfError> {
    let gamma = CompressedEdwardsY(proof.gamma)
        .decompress()
        .ok_or_else(|| VrfError::MalformedProof("gamma is not a valid curve point".into()))?;
    Ok(proof_to_hash(&gamma))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn key(seed: u64) -> SigningKey {
        let mut rng = StdRng::seed_from_u64(seed);
        SigningKey::generate(&mut rng)
    }

    #[test]
    fn prove_then_verify_roundtrips_and_outputs_match() {
        let sk = key(1);
        let alpha = b"round-42-beacon-input";
        let proof = prove(&sk, alpha);

        let beta = verify(&sk.verifying_key(), alpha, &proof).expect("valid proof must verify");
        // The output recovered from the (verified) proof matches verify's.
        assert_eq!(beta, output(&proof).unwrap());
        assert_eq!(proof.to_bytes().len(), PROOF_LEN);
    }

    #[test]
    fn output_is_deterministic_for_same_key_and_input() {
        let sk = key(2);
        let alpha = b"same-input";
        let p1 = prove(&sk, alpha);
        let p2 = prove(&sk, alpha);
        assert_eq!(p1, p2, "EC-VRF proving must be deterministic");
    }

    #[test]
    fn different_inputs_give_different_outputs() {
        let sk = key(3);
        let b1 = verify(&sk.verifying_key(), b"round-1", &prove(&sk, b"round-1")).unwrap();
        let b2 = verify(&sk.verifying_key(), b"round-2", &prove(&sk, b"round-2")).unwrap();
        assert_ne!(b1, b2);
    }

    #[test]
    fn different_keys_give_different_outputs() {
        let alpha = b"shared-input";
        let a = key(4);
        let b = key(5);
        let ba = verify(&a.verifying_key(), alpha, &prove(&a, alpha)).unwrap();
        let bb = verify(&b.verifying_key(), alpha, &prove(&b, alpha)).unwrap();
        assert_ne!(ba, bb);
    }

    #[test]
    fn proof_does_not_verify_for_wrong_key() {
        let signer = key(6);
        let other = key(7);
        let alpha = b"input";
        let proof = prove(&signer, alpha);
        assert_eq!(
            verify(&other.verifying_key(), alpha, &proof),
            Err(VrfError::VerificationFailed)
        );
    }

    #[test]
    fn proof_does_not_verify_for_wrong_input() {
        let sk = key(8);
        let proof = prove(&sk, b"input-A");
        assert_eq!(
            verify(&sk.verifying_key(), b"input-B", &proof),
            Err(VrfError::VerificationFailed)
        );
    }

    #[test]
    fn tampered_gamma_is_rejected() {
        let sk = key(9);
        let alpha = b"input";
        let mut proof = prove(&sk, alpha);
        proof.gamma[0] ^= 0x01;
        // Either the point no longer decodes, or the challenge fails.
        assert!(verify(&sk.verifying_key(), alpha, &proof).is_err());
    }

    #[test]
    fn tampered_scalar_is_rejected() {
        let sk = key(10);
        let alpha = b"input";
        let mut proof = prove(&sk, alpha);
        proof.s[0] ^= 0x01;
        assert!(verify(&sk.verifying_key(), alpha, &proof).is_err());
    }

    #[test]
    fn tampered_challenge_is_rejected() {
        let sk = key(11);
        let alpha = b"input";
        let mut proof = prove(&sk, alpha);
        proof.c[0] ^= 0x01;
        assert_eq!(
            verify(&sk.verifying_key(), alpha, &proof),
            Err(VrfError::VerificationFailed)
        );
    }

    #[test]
    fn proof_encoding_roundtrips() {
        let sk = key(12);
        let proof = prove(&sk, b"input");
        let bytes = proof.to_bytes();
        assert_eq!(VrfProof::from_bytes(&bytes).unwrap(), proof);
        assert!(VrfProof::from_bytes(&bytes[..PROOF_LEN - 1]).is_err());
    }

    #[test]
    fn non_canonical_scalar_is_rejected_cleanly() {
        let sk = key(13);
        let alpha = b"input";
        let mut proof = prove(&sk, alpha);
        // All-ones is a non-canonical (>= group order) scalar encoding.
        proof.s = [0xFF; 32];
        assert!(matches!(
            verify(&sk.verifying_key(), alpha, &proof),
            Err(VrfError::MalformedProof(_)) | Err(VrfError::VerificationFailed)
        ));
    }

    #[test]
    fn output_is_uniform_enough_to_rank() {
        // Sanity: outputs across many keys spread across the u64 space,
        // so stake-weighted ranking by beta is well-behaved.
        let alpha = b"round";
        let mut lows = 0u32;
        let n = 200;
        for i in 0..n {
            let sk = key(1000 + i);
            let beta = verify(&sk.verifying_key(), alpha, &prove(&sk, alpha)).unwrap();
            let v = u64::from_be_bytes(beta[..8].try_into().unwrap());
            if v < u64::MAX / 2 {
                lows += 1;
            }
        }
        // Expect ~half below the midpoint; allow a wide band.
        assert!((60..=140).contains(&lows), "beta distribution looks skewed: {lows}/{n}");
    }
}
