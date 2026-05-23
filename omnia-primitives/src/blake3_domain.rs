//! Domain-separated BLAKE3 hashing.
//!
//! Every hash context in the Omnia protocol uses a unique domain prefix
//! to prevent cross-context hash collisions. For example, a hash of a
//! public key in the creator-identity context cannot be confused with a
//! hash of the same bytes in the state-root context.
//!
//! # Domain Prefixes
//!
//! | Prefix                | Usage                                    |
//! |-----------------------|------------------------------------------|
//! | `omnia-creator`      | Creator ID derivation from pubkey        |
//! | `omnia-state-root`   | Merkle tree leaf hashing in `state_root` |
//! | `omnia-nonce`        | Nonce / rate-limiter key derivation      |
//! | `omnia-commitment`   | Commitment and message-ID schemes        |
//!
//! # Example
//!
//! ```ignore
//! use omnia_substrate::blake3_domain::blake3_hash_domain;
//!
//! let creator_id = blake3_hash_domain(b"omnia-creator", &pubkey_bytes);
//! ```

/// Compute a domain-separated BLAKE3 hash.
///
/// Prepends `domain` to `data` so that the same input bytes produce
/// different hashes when used in different protocol contexts. This
/// prevents accidental collisions between, e.g., a creator-identity
/// hash and a Merkle leaf hash that happen to have the same raw input.
///
/// # Arguments
///
/// * `domain` — A unique byte prefix identifying the hashing context
///   (e.g., `b"omnia-creator"`).
/// * `data` — The input data to hash.
///
/// # Returns
///
/// A 32-byte BLAKE3 digest.
pub fn blake3_hash_domain(domain: &[u8], data: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(domain);
    hasher.update(data);
    *hasher.finalize().as_bytes()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_separation_produces_different_hashes() {
        let data = b"same-input-data";
        let hash_a = blake3_hash_domain(b"omnia-creator", data);
        let hash_b = blake3_hash_domain(b"omnia-state-root", data);
        assert_ne!(
            hash_a, hash_b,
            "Different domains must produce different hashes for the same data"
        );
    }

    #[test]
    fn test_same_domain_same_data_deterministic() {
        let data = b"test-data";
        let hash1 = blake3_hash_domain(b"omnia-creator", data);
        let hash2 = blake3_hash_domain(b"omnia-creator", data);
        assert_eq!(hash1, hash2, "Same domain + data must be deterministic");
    }

    #[test]
    fn test_domain_separation_differs_from_raw_blake3() {
        let data = b"test-data";
        let raw_hash = *blake3::hash(data).as_bytes();
        let domain_hash = blake3_hash_domain(b"omnia-creator", data);
        assert_ne!(
            raw_hash, domain_hash,
            "Domain-separated hash must differ from raw blake3::hash"
        );
    }
}
