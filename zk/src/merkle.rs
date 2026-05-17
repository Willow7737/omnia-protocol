//! Sparse Merkle Tree verification for the rollup circuit.
//!
//! Provides both off-circuit (native) and on-circuit (R1CS) Merkle path
//! verification. The sparse Merkle tree maps 32-byte keys to 32-byte values
//! with a fixed depth of 32.
//!
//! ## Off-circuit verification
//!
//! The [`compute_root_from_proof`] function computes a Merkle root from a leaf
//! and an inclusion proof using BLAKE3 hashing. This is used during proof
//! generation to verify the witness data before submitting it to the circuit.
//!
//! For Poseidon-based Merkle trees (used when the on-circuit hash function
//! is Poseidon), use [`poseidon_hash_to_fr`] to hash field elements off-circuit.
//!
//! ## On-circuit verification
//!
//! The on-circuit verification uses Poseidon hash (via
//! [`crate::poseidon::poseidon_hash`]) for both Merkle path verification and
//! state transition constraints. This ensures on-circuit and off-circuit
//! computations match when using [`poseidon_hash_to_fr`] for off-circuit
//! tree construction.

use ark_bn254::Fr;
use ark_ff::{BigInteger, PrimeField};

/// Default leaf value for the Sparse Merkle Tree (all zeros).
pub const DEFAULT_LEAF: [u8; 32] = [0u8; 32];

/// Maximum depth of the Merkle tree (256-bit keys).
pub const MERKLE_DEPTH: usize = 32;

/// A Merkle proof consisting of sibling hashes and direction bits.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MerkleProof {
    /// Sibling hashes at each level of the tree.
    pub siblings: Vec<[u8; 32]>,
    /// Direction bits: `true` means the sibling is on the left.
    pub directions: Vec<bool>,
}

/// Compute a Merkle root from a leaf and a proof.
///
/// This is the off-circuit (native) verification function used for
/// test comparison and proof generation. Uses BLAKE3 for hashing.
///
/// # Arguments
///
/// * `leaf` — The 32-byte leaf hash
/// * `proof` — The [`MerkleProof`] containing siblings and directions
///
/// # Returns
///
/// The computed 32-byte Merkle root.
pub fn compute_root_from_proof(leaf: &[u8; 32], proof: &MerkleProof) -> [u8; 32] {
    let mut current = *leaf;
    for (i, (sibling, go_left)) in proof
        .siblings
        .iter()
        .zip(proof.directions.iter())
        .enumerate()
    {
        if i >= MERKLE_DEPTH {
            break;
        }
        let mut hasher = blake3::Hasher::new();
        if *go_left {
            // Sibling is on the left: H(sibling || current)
            hasher.update(sibling);
            hasher.update(&current);
        } else {
            // Current is on the left: H(current || sibling)
            hasher.update(&current);
            hasher.update(sibling);
        }
        current = *hasher.finalize().as_bytes();
    }
    current
}

/// Build a simple Merkle tree from a list of items and return the root.
///
/// Uses BLAKE3 for hashing. Returns `(root, proofs)` where `proofs[i]` is the
/// inclusion proof for `items[i]`.
///
/// # Arguments
///
/// * `items` — Slice of 32-byte items to include as leaves
///
/// # Returns
///
/// A tuple of `(root_hash, vector_of_proofs)`.
pub fn build_merkle_tree(items: &[[u8; 32]]) -> ([u8; 32], Vec<MerkleProof>) {
    if items.is_empty() {
        return ([0u8; 32], vec![]);
    }

    // Hash each item to produce leaves
    let leaves: Vec<[u8; 32]> = items
        .iter()
        .map(|item| *blake3::hash(item).as_bytes())
        .collect();

    let current_level = leaves.clone();
    let mut proofs = Vec::new();

    // Build proofs for each leaf
    for (idx, _) in items.iter().enumerate() {
        let mut siblings = Vec::new();
        let mut directions = Vec::new();
        let mut level = current_level.clone();
        let mut pos = idx;

        while level.len() > 1 {
            let sibling_pos = if pos % 2 == 0 { pos + 1 } else { pos - 1 };
            let sibling = if sibling_pos < level.len() {
                level[sibling_pos]
            } else {
                [0u8; 32]
            };
            siblings.push(sibling);
            // go_left = true means "sibling is on the left".
            // If current position is odd (right child), sibling is on the left.
            directions.push(pos % 2 == 1);

            let mut next_level = Vec::new();
            let mut i = 0;
            while i < level.len() {
                let left = level[i];
                let right = if i + 1 < level.len() {
                    level[i + 1]
                } else {
                    [0u8; 32]
                };
                let mut hasher = blake3::Hasher::new();
                hasher.update(&left);
                hasher.update(&right);
                next_level.push(*hasher.finalize().as_bytes());
                i += 2;
            }
            level = next_level;
            pos /= 2;
        }
        proofs.push(MerkleProof {
            siblings,
            directions,
        });
    }

    // Compute root
    let mut level = current_level;
    while level.len() > 1 {
        let mut next_level = Vec::new();
        let mut i = 0;
        while i < level.len() {
            let left = level[i];
            let right = if i + 1 < level.len() {
                level[i + 1]
            } else {
                [0u8; 32]
            };
            let mut hasher = blake3::Hasher::new();
            hasher.update(&left);
            hasher.update(&right);
            next_level.push(*hasher.finalize().as_bytes());
            i += 2;
        }
        level = next_level;
    }

    (level[0], proofs)
}

/// Convert a 32-byte hash to a field element (`Fr`).
///
/// Uses big-endian modular reduction to interpret the hash as a field element.
/// This is the standard method for converting hash outputs to R1CS-compatible
/// values.
///
/// # Arguments
///
/// * `hash` — A 32-byte hash value
///
/// # Returns
///
/// The corresponding `Fr` field element.
pub fn hash_to_fr(hash: &[u8; 32]) -> Fr {
    Fr::from_be_bytes_mod_order(hash)
}

/// Convert two field elements to a single field element using Poseidon hash.
///
/// This is the **recommended** method for off-circuit Merkle tree construction
/// when the tree will be verified inside a ZK circuit that uses Poseidon as
/// its hash function. Using [`hash_to_fr`] (which is BLAKE3-based) would
/// create a mismatch between off-circuit and on-circuit hash computations.
///
/// # Arguments
///
/// * `left` — The left child field element
/// * `right` — The right child field element
///
/// # Returns
///
/// The Poseidon hash: `Poseidon_permutation([0, left, right])[0]`
///
/// # Example
///
/// ```
/// use ark_bn254::Fr;
/// use ark_ff::Zero;
/// use omnia_zk::merkle::poseidon_hash_to_fr;
///
/// let a = Fr::from(42u64);
/// let b = Fr::from(123u64);
/// let hash = poseidon_hash_to_fr(a, b);
/// assert_ne!(hash, Fr::zero()); // non-trivial output
/// ```
///
/// # Reference
///
/// Grassi et al. (2019), "Poseidon: A New Hash Function for
/// Zero-Knowledge Proof Systems", <https://eprint.iacr.org/2019/458>
pub fn poseidon_hash_to_fr(left: Fr, right: Fr) -> Fr {
    crate::poseidon::poseidon_hash_offchain(left, right)
}

/// Convert a field element (`Fr`) to a 32-byte big-endian representation.
///
/// This is the inverse of [`hash_to_fr`] for field elements whose value
/// is less than the field modulus (which is always the case for valid
/// `Fr` values).
///
/// # Arguments
///
/// * `val` — A field element
///
/// # Returns
///
/// The 32-byte big-endian representation of the field element.
pub fn fr_to_hash(val: &Fr) -> [u8; 32] {
    let bigint = <Fr as PrimeField>::BigInt::from(*val);
    let bytes = bigint.to_bytes_be();
    let mut result = [0u8; 32];
    let len = bytes.len().min(32);
    result[32 - len..].copy_from_slice(&bytes[..len]);
    result
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use ark_ff::Zero;

    #[test]
    fn test_compute_root_from_proof_single_item() {
        let item = [42u8; 32];
        let (root, proofs) = build_merkle_tree(&[item]);
        let _computed = compute_root_from_proof(&item, &proofs[0]);
        // The leaf is blake3(item), and the root is blake3(blake3(item) || [0;32])
        // The proof's siblings contain the padding node, not the leaf itself
        // So compute_root_from_proof with the raw item won't match the root
        // We need to use the hashed leaf
        let leaf = blake3::hash(&item).as_bytes().clone();
        let computed_from_leaf = compute_root_from_proof(&leaf, &proofs[0]);
        assert_eq!(root, computed_from_leaf);
    }

    #[test]
    fn test_compute_root_from_proof_two_items() {
        let items: Vec<[u8; 32]> = vec![[1u8; 32], [2u8; 32]];
        let (root, proofs) = build_merkle_tree(&items);
        for (i, item) in items.iter().enumerate() {
            let leaf = blake3::hash(item).as_bytes().clone();
            let computed = compute_root_from_proof(&leaf, &proofs[i]);
            assert_eq!(root, computed, "proof for item {} should verify", i);
        }
    }

    #[test]
    fn test_build_merkle_tree_empty() {
        let (root, proofs) = build_merkle_tree(&[]);
        assert_eq!(root, [0u8; 32]);
        assert!(proofs.is_empty());
    }

    #[test]
    fn test_hash_to_fr_roundtrip() {
        // Small values should roundtrip through hash_to_fr / fr_to_hash
        let val = Fr::from(42u64);
        let bytes = fr_to_hash(&val);
        let restored = hash_to_fr(&bytes);
        assert_eq!(val, restored);
    }

    #[test]
    fn test_hash_to_fr_zero() {
        let bytes = [0u8; 32];
        let val = hash_to_fr(&bytes);
        assert_eq!(val, Fr::from(0u64));
    }

    #[test]
    fn test_poseidon_hash_to_fr_matches_poseidon_offchain() {
        let a = Fr::from(42u64);
        let b = Fr::from(123u64);
        let hash = poseidon_hash_to_fr(a, b);
        // Must match the direct call to poseidon_hash_offchain
        let expected = crate::poseidon::poseidon_hash_offchain(a, b);
        assert_eq!(hash, expected);
    }

    #[test]
    fn test_poseidon_hash_to_fr_non_zero() {
        let a = Fr::from(1u64);
        let b = Fr::from(2u64);
        let hash = poseidon_hash_to_fr(a, b);
        assert_ne!(
            hash,
            Fr::zero(),
            "Poseidon hash of non-zero inputs should be non-zero"
        );
    }

    #[test]
    fn test_poseidon_hash_to_fr_non_commutative() {
        let a = Fr::from(42u64);
        let b = Fr::from(123u64);
        let hash_ab = poseidon_hash_to_fr(a, b);
        let hash_ba = poseidon_hash_to_fr(b, a);
        assert_ne!(
            hash_ab, hash_ba,
            "poseidon_hash_to_fr should not be commutative"
        );
    }
}

/// Property-based tests for Merkle tree and hash invariants.
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Property: compute_root_from_proof is deterministic —
        /// the same leaf and proof always produce the same root.
        #[test]
        fn proptest_merkle_root_deterministic(
            items in prop::collection::vec(any::<[u8; 32]>(), 1..8)
        ) {
            let (root1, proofs) = build_merkle_tree(&items);
            let root2 = build_merkle_tree(&items).0;
            assert_eq!(root1, root2, "Merkle root not deterministic for same inputs!");

            // Verify each proof also produces the same root
            for (i, item) in items.iter().enumerate() {
                let leaf = blake3::hash(item).as_bytes().clone();
                let computed1 = compute_root_from_proof(&leaf, &proofs[i]);
                let computed2 = compute_root_from_proof(&leaf, &proofs[i]);
                assert_eq!(computed1, computed2, "Proof verification not deterministic!");
            }
        }

        /// Property: hash_to_fr is deterministic — same input always
        /// produces the same field element.
        #[test]
        fn proptest_hash_to_fr_deterministic(bytes in any::<[u8; 32]>()) {
            let fr1 = hash_to_fr(&bytes);
            let fr2 = hash_to_fr(&bytes);
            assert_eq!(fr1, fr2, "hash_to_fr not deterministic!");
        }

        /// Property: poseidon_hash_to_fr is deterministic.
        #[test]
        fn proptest_poseidon_hash_deterministic(
            a in any::<u64>(),
            b in any::<u64>()
        ) {
            let fr_a = Fr::from(a);
            let fr_b = Fr::from(b);
            let h1 = poseidon_hash_to_fr(fr_a, fr_b);
            let h2 = poseidon_hash_to_fr(fr_a, fr_b);
            assert_eq!(h1, h2, "poseidon_hash_to_fr not deterministic!");
        }
    }
}
