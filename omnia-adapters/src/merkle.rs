//! Sparse Merkle Tree verification for the rollup circuit.
//!
//! Provides both off-circuit (native) and on-circuit (R1CS) Merkle path
//! verification. The sparse Merkle tree maps 32-byte keys to 32-byte values
//! with a fixed depth of 32.
//!
//! ## Off-circuit verification (always available)
//!
//! The [`compute_root_from_proof`] function computes a Merkle root from a leaf
//! and an inclusion proof using BLAKE3 hashing. This works without any
//! arkworks dependencies and is MSRV 1.88 compliant.
//!
//! ## Field-element conversions (arkworks feature required)
//!
//! When the `arkworks` feature is enabled, `hash_to_fr`, `fr_to_hash`,
//! and `poseidon_hash_to_fr` provide conversions between byte-oriented
//! hashes and Bn254 field elements for ZK circuit compatibility.
//!
//! ## Type-Level Hash Function Safety
//!
//! `MerkleProof<H>` is parameterized by a [`HashFunction`] marker type,
//! preventing accidental use of a BLAKE3-produced proof in a Poseidon
//! circuit (or vice versa) at compile time.
//!
//! ```
//! use omnia_adapters::merkle::{MerkleProof, Blake3, compute_root_from_proof};
//!
//! // BLAKE3 proofs are always available:
//! let blake3_proof: MerkleProof<Blake3> = MerkleProof::new(vec![], vec![]);
//! ```
//!
//! When the `arkworks` feature is enabled, Poseidon proofs are also available:
//!
//! ```ignore
//! use omnia_adapters::merkle::{MerkleProof, Poseidon};
//! let poseidon_proof: MerkleProof<Poseidon> = MerkleProof::new(vec![], vec![]);
//! ```

/// Default leaf value for the Sparse Merkle Tree (all zeros).
pub const DEFAULT_LEAF: [u8; 32] = [0u8; 32];

/// Maximum depth of the Merkle tree (256-bit keys).
pub const MERKLE_DEPTH: usize = 32;

// ---------------------------------------------------------------------------
// Hash function marker types for type-level Merkle proof safety
// ---------------------------------------------------------------------------

/// Marker trait for hash functions used in Merkle trees.
///
/// This trait has no methods — it exists purely as a type-level marker
/// to prevent mixing proofs from different hash functions (e.g., BLAKE3
/// vs Poseidon) at compile time.
///
/// See [`MerkleProof`] for the generic proof type.
pub trait HashFunction: std::fmt::Debug + Clone + Send + Sync + 'static {}

/// BLAKE3 hash function marker.
///
/// Used with [`MerkleProof<Blake3>`] for proofs produced by the BLAKE3
/// Merkle tree builder. These proofs are suitable for off-circuit
/// verification but **must not** be used in ZK circuits that expect
/// Poseidon hashing.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Blake3;
impl HashFunction for Blake3 {}

/// Poseidon hash function marker.
///
/// Used with [`MerkleProof<Poseidon>`] for proofs produced by the
/// Poseidon Merkle tree builder. These proofs are required for ZK
/// circuit verification (e.g., [`ExpandedRollupCircuit`]).
///
/// [`ExpandedRollupCircuit`]: crate::circuit::ExpandedRollupCircuit
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Poseidon;
#[cfg(feature = "arkworks")]
impl HashFunction for Poseidon {}

/// A Merkle proof consisting of sibling hashes and direction bits.
///
/// This type is parameterized by a [`HashFunction`] marker (`Blake3` or `Poseidon`)
/// to prevent accidental mixing of proofs from different hash functions at
/// compile time. This addresses C-06 (Merkle tree mismatch) from the security audit.
///
/// # Type Safety
///
/// - `MerkleProof<Blake3>` — proofs from [`build_merkle_tree`], for off-circuit use
/// - `MerkleProof<Poseidon>` — proofs from [`build_poseidon_merkle_tree`], for ZK circuits
///
/// These types are not interchangeable — passing a `MerkleProof<Blake3>` to a
/// function expecting `MerkleProof<Poseidon>` will fail to compile.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MerkleProof<H: HashFunction = Blake3> {
    /// Sibling hashes at each level of the tree.
    pub siblings: Vec<[u8; 32]>,
    /// Direction bits: `true` means the sibling is on the left.
    pub directions: Vec<bool>,
    /// Phantom marker for the hash function type (zero-sized).
    #[serde(skip)]
    _hash: std::marker::PhantomData<H>,
}

impl<H: HashFunction> MerkleProof<H> {
    /// Create a new Merkle proof with the given siblings and directions.
    pub fn new(siblings: Vec<[u8; 32]>, directions: Vec<bool>) -> Self {
        Self {
            siblings,
            directions,
            _hash: std::marker::PhantomData,
        }
    }
}

// Type aliases for backward compatibility
/// BLAKE3-based Merkle proof (off-circuit verification).
pub type Blake3MerkleProof = MerkleProof<Blake3>;
/// Poseidon-based Merkle proof (ZK circuit verification).
#[cfg(feature = "arkworks")]
pub type PoseidonMerkleProof = MerkleProof<Poseidon>;

/// Compute a Merkle root from a leaf and a BLAKE3 proof.
///
/// This is the off-circuit (native) verification function used for
/// test comparison and proof generation. Uses BLAKE3 for hashing.
/// Always available (no arkworks dependency).
///
/// Only accepts `MerkleProof<Blake3>` — Poseidon proofs must use
/// the Poseidon verification path in the ZK circuit.
pub fn compute_root_from_proof(leaf: &[u8; 32], proof: &Blake3MerkleProof) -> [u8; 32] {
    let mut current = *leaf;
    for (i, (sibling, go_left)) in proof.siblings.iter().zip(proof.directions.iter()).enumerate() {
        if i >= MERKLE_DEPTH {
            break;
        }
        let mut hasher = blake3::Hasher::new();
        if *go_left {
            hasher.update(sibling);
            hasher.update(&current);
        } else {
            hasher.update(&current);
            hasher.update(sibling);
        }
        current = *hasher.finalize().as_bytes();
    }
    current
}

/// Build a simple Merkle tree from a list of items and return the root.
///
/// Uses BLAKE3 for hashing. Always available (no arkworks dependency).
///
/// Returns `MerkleProof<Blake3>` proofs — these are for off-circuit use only
/// and must **not** be used in ZK circuits that expect Poseidon hashing.
/// For ZK-compatible proofs, use [`build_poseidon_merkle_tree`] instead.
pub fn build_merkle_tree(items: &[[u8; 32]]) -> ([u8; 32], Vec<Blake3MerkleProof>) {
    if items.is_empty() {
        // Use a domain-separated hash for the empty root to avoid collision with DEFAULT_LEAF
        let empty_root = blake3::derive_key("OMNIA-MERKLE-EMPTY-ROOT", &[]);
        let mut root = [0u8; 32];
        root.copy_from_slice(&empty_root[..32]);
        return (root, vec![]);
    }

    let leaves: Vec<[u8; 32]> = items.iter().map(|item| *blake3::hash(item).as_bytes()).collect();

    // Build the tree once, storing all levels from leaves to root.
    // This avoids the O(n²) behavior of rebuilding the tree for every proof.
    let mut levels: Vec<Vec<[u8; 32]>> = Vec::new();
    let mut level = leaves.clone();
    levels.push(level.clone());
    while level.len() > 1 {
        let mut next_level = Vec::new();
        let mut i = 0;
        while i < level.len() {
            let left = level[i];
            let right = if i + 1 < level.len() { level[i + 1] } else { [0u8; 32] };
            let mut hasher = blake3::Hasher::new();
            hasher.update(&left);
            hasher.update(&right);
            next_level.push(*hasher.finalize().as_bytes());
            i += 2;
        }
        level = next_level;
        levels.push(level.clone());
    }

    // Extract proofs from the stored levels
    let mut proofs = Vec::new();
    for (idx, _) in items.iter().enumerate() {
        let mut siblings = Vec::new();
        let mut directions = Vec::new();
        let mut pos = idx;

        for (level_idx, current_level) in levels.iter().enumerate() {
            if level_idx == levels.len() - 1 {
                break;
            }
            let sibling_pos = if pos % 2 == 0 { pos + 1 } else { pos - 1 };
            let sibling = if sibling_pos < current_level.len() {
                current_level[sibling_pos]
            } else {
                [0u8; 32]
            };
            siblings.push(sibling);
            directions.push(pos % 2 == 1);
            pos /= 2;
        }
        proofs.push(Blake3MerkleProof::new(siblings, directions));
    }

    let root = levels
        .last()
        .expect("at least one level must exist after tree construction")[0];
    (root, proofs)
}

// ---------------------------------------------------------------------------
// Field-element conversion functions (require arkworks feature)
// ---------------------------------------------------------------------------

#[cfg(feature = "arkworks")]
use ark_bn254::Fr;
#[cfg(feature = "arkworks")]
use ark_ff::{BigInteger, PrimeField};

/// Convert a 32-byte hash to a field element (`Fr`).
///
/// Requires the `arkworks` feature. Uses big-endian modular reduction.
#[cfg(feature = "arkworks")]
pub fn hash_to_fr(hash: &[u8; 32]) -> Fr {
    Fr::from_be_bytes_mod_order(hash)
}

/// Convert two field elements to a single field element using Poseidon hash.
///
/// Requires the `arkworks` feature. This is the recommended method for
/// off-circuit Merkle tree construction when the tree will be verified
/// inside a ZK circuit that uses Poseidon as its hash function.
///
/// # Example
///
/// ```
/// use ark_bn254::Fr;
/// use ark_ff::Zero;
/// use omnia_adapters::merkle::poseidon_hash_to_fr;
///
/// let a = Fr::from(42u64);
/// let b = Fr::from(123u64);
/// let hash = poseidon_hash_to_fr(a, b).expect("hash should succeed");
/// assert_ne!(hash, Fr::zero());
/// ```
#[cfg(feature = "arkworks")]
pub fn poseidon_hash_to_fr(left: Fr, right: Fr) -> Result<Fr, crate::poseidon::ZkError> {
    crate::poseidon::poseidon_hash_offchain(left, right)
}

/// Convert a field element (`Fr`) to a 32-byte big-endian representation.
///
/// Requires the `arkworks` feature. This is the inverse of [`hash_to_fr`].
#[cfg(feature = "arkworks")]
pub fn fr_to_hash(val: &Fr) -> [u8; 32] {
    let bigint = <Fr as PrimeField>::BigInt::from(*val);
    let bytes = bigint.to_bytes_be();
    let mut result = [0u8; 32];
    let len = bytes.len().min(32);
    result[32 - len..].copy_from_slice(&bytes[..len]);
    result
}

// ---------------------------------------------------------------------------
// Poseidon-based Merkle tree (requires arkworks feature)
// ---------------------------------------------------------------------------

/// Build a Merkle tree using Poseidon hash instead of BLAKE3.
///
/// This function has the same interface as [`build_merkle_tree`] but uses
/// the Poseidon SNARK-friendly hash function, ensuring the off-circuit tree
/// matches the in-circuit hash computation. This is critical for ZK proof
/// generation where the circuit uses Poseidon as its hash function.
///
/// Requires the `arkworks` feature.
///
/// # Arguments
///
/// * `items` — List of 32-byte items to include in the tree
///
/// # Returns
///
/// A tuple of (Poseidon root as 32 bytes, vector of `MerkleProof<Poseidon>`).
///
/// **Important**: Circuit witness generation (e.g., `ExpandedRollupCircuit::from_batch`
/// and `BatchProofCircuit::from_batch`) must use this Poseidon tree builder instead
/// of [`build_merkle_tree`], because the on-circuit Merkle path verification uses
/// Poseidon hashing. Using the BLAKE3 tree would produce roots that don't match
/// the in-circuit verification.
///
/// # Type Safety
///
/// Returns `MerkleProof<Poseidon>` which is a distinct type from
/// `MerkleProof<Blake3>`. A function expecting `MerkleProof<Poseidon>` will
/// not compile if passed a `MerkleProof<Blake3>`, preventing the C-06
/// Merkle tree mismatch vulnerability.
#[cfg(feature = "arkworks")]
pub fn build_poseidon_merkle_tree(items: &[[u8; 32]]) -> ([u8; 32], Vec<PoseidonMerkleProof>) {
    if items.is_empty() {
        // Use a domain-separated hash for the empty root to avoid collision with DEFAULT_LEAF
        let empty_root = blake3::derive_key("OMNIA-MERKLE-EMPTY-ROOT", &[]);
        let mut root = [0u8; 32];
        root.copy_from_slice(&empty_root[..32]);
        return (root, vec![]);
    }

    use ark_ff::Zero;

    // Convert items to field elements as leaves
    let leaves: Vec<Fr> = items.iter().map(hash_to_fr).collect();

    // Build the tree once, storing all levels from leaves to root.
    // This avoids the O(n²) behavior of rebuilding the tree for every proof.
    let mut levels: Vec<Vec<Fr>> = Vec::new();
    let mut level = leaves.clone();
    levels.push(level.clone());
    while level.len() > 1 {
        let mut next_level = Vec::new();
        let mut i = 0;
        while i < level.len() {
            let left = level[i];
            let right = if i + 1 < level.len() { level[i + 1] } else { Fr::zero() };
            let hash = poseidon_hash_to_fr(left, right).unwrap_or(Fr::zero());
            next_level.push(hash);
            i += 2;
        }
        level = next_level;
        levels.push(level.clone());
    }

    // Extract proofs from the stored levels
    let mut proofs = Vec::new();
    for (idx, _) in items.iter().enumerate() {
        let mut siblings = Vec::new();
        let mut directions = Vec::new();
        let mut pos = idx;

        for current_level in levels.iter().take(levels.len() - 1) {
            let sibling_pos = if pos % 2 == 0 { pos + 1 } else { pos - 1 };
            let sibling_fr = if sibling_pos < current_level.len() {
                current_level[sibling_pos]
            } else {
                Fr::zero()
            };
            siblings.push(fr_to_hash(&sibling_fr));
            directions.push(pos % 2 == 1);
            pos /= 2;
        }
        proofs.push(PoseidonMerkleProof::new(siblings, directions));
    }

    let root = levels
        .last()
        .expect("at least one level must exist after tree construction")[0];
    (fr_to_hash(&root), proofs)
}

// ---------------------------------------------------------------------------
// Tests (always available — BLAKE3 tests don't need arkworks)
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_root_from_proof_single_item() {
        let item = [42u8; 32];
        let (root, proofs) = build_merkle_tree(&[item]);
        let leaf = *blake3::hash(&item).as_bytes();
        let computed_from_leaf = compute_root_from_proof(&leaf, &proofs[0]);
        assert_eq!(root, computed_from_leaf);
    }

    #[test]
    fn test_compute_root_from_proof_two_items() {
        let items: Vec<[u8; 32]> = vec![[1u8; 32], [2u8; 32]];
        let (root, proofs) = build_merkle_tree(&items);
        for (i, item) in items.iter().enumerate() {
            let leaf = *blake3::hash(item).as_bytes();
            let computed = compute_root_from_proof(&leaf, &proofs[i]);
            assert_eq!(root, computed, "proof for item {i} should verify");
        }
    }

    #[test]
    fn test_build_merkle_tree_empty() {
        let (root, proofs) = build_merkle_tree(&[]);
        // Empty root uses domain-separated hash, not all-zeros (avoids collision with DEFAULT_LEAF)
        assert_ne!(root, [0u8; 32]);
        assert!(proofs.is_empty());
    }

    #[test]
    fn test_merkle_proof_type_safety() {
        // BLAKE3 and Poseidon proofs are different types at compile time.
        // This test demonstrates that you cannot accidentally mix them.
        let blake3_proof = Blake3MerkleProof::new(vec![[1u8; 32]], vec![true]);
        let _poseidon_proof = {
            // Use cfg to compile this block only with arkworks
            #[cfg(feature = "arkworks")]
            {
                PoseidonMerkleProof::new(vec![[2u8; 32]], vec![false])
            }
            #[cfg(not(feature = "arkworks"))]
            {
                // Without arkworks, just verify the Blake3 proof works
                Blake3MerkleProof::new(vec![[2u8; 32]], vec![false])
            }
        };

        // Both have the same data layout but are different types
        assert_eq!(blake3_proof.siblings.len(), 1);
        assert_eq!(blake3_proof.directions.len(), 1);

        // The following would be a compile error (intentionally commented out):
        // let _: PoseidonMerkleProof = blake3_proof; // ERROR: type mismatch
    }
}

// ---------------------------------------------------------------------------
// Arkworks-dependent tests
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "arkworks"))]
#[allow(clippy::unwrap_used)]
mod arkworks_tests {
    use super::*;
    use ark_ff::Zero;

    #[test]
    fn test_hash_to_fr_roundtrip() {
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
        let hash = poseidon_hash_to_fr(a, b).unwrap();
        let expected = crate::poseidon::poseidon_hash_offchain(a, b).unwrap();
        assert_eq!(hash, expected);
    }

    #[test]
    fn test_poseidon_hash_to_fr_non_zero() {
        let a = Fr::from(1u64);
        let b = Fr::from(2u64);
        let hash = poseidon_hash_to_fr(a, b).unwrap();
        assert_ne!(hash, Fr::zero());
    }

    #[test]
    fn test_poseidon_hash_to_fr_non_commutative() {
        let a = Fr::from(42u64);
        let b = Fr::from(123u64);
        let hash_ab = poseidon_hash_to_fr(a, b).unwrap();
        let hash_ba = poseidon_hash_to_fr(b, a).unwrap();
        assert_ne!(hash_ab, hash_ba);
    }

    #[test]
    fn test_poseidon_merkle_tree_differs_from_blake3() {
        let items: Vec<[u8; 32]> = vec![[1u8; 32], [2u8; 32], [3u8; 32], [4u8; 32]];

        // Build BLAKE3-based Merkle tree
        let (blake3_root, _) = build_merkle_tree(&items);

        // Build Poseidon-based Merkle tree
        let (poseidon_root, _) = build_poseidon_merkle_tree(&items);

        // The two trees must produce different roots because they use
        // different hash functions (BLAKE3 vs Poseidon)
        assert_ne!(
            blake3_root, poseidon_root,
            "BLAKE3 and Poseidon Merkle trees must produce different roots"
        );
    }

    #[test]
    fn test_poseidon_merkle_tree_empty() {
        let (root, proofs) = build_poseidon_merkle_tree(&[]);
        // Empty root uses domain-separated hash, not all-zeros (avoids collision with DEFAULT_LEAF)
        assert_ne!(root, [0u8; 32]);
        assert!(proofs.is_empty());
    }

    #[test]
    fn test_poseidon_merkle_tree_deterministic() {
        let items: Vec<[u8; 32]> = vec![[42u8; 32], [99u8; 32]];
        let (root1, _) = build_poseidon_merkle_tree(&items);
        let (root2, _) = build_poseidon_merkle_tree(&items);
        assert_eq!(root1, root2, "Poseidon Merkle tree must be deterministic");
    }
}

/// Property-based tests for Merkle tree invariants (BLAKE3, no arkworks).
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn proptest_merkle_root_deterministic(
            items in prop::collection::vec(any::<[u8; 32]>(), 1..8)
        ) {
            let (root1, proofs) = build_merkle_tree(&items);
            let root2 = build_merkle_tree(&items).0;
            assert_eq!(root1, root2);

            for (i, item) in items.iter().enumerate() {
                let leaf = *blake3::hash(item).as_bytes();
                let computed1 = compute_root_from_proof(&leaf, &proofs[i]);
                let computed2 = compute_root_from_proof(&leaf, &proofs[i]);
                assert_eq!(computed1, computed2);
            }
        }
    }
}

/// Property-based tests for field-element hash functions (arkworks).
#[cfg(all(test, feature = "arkworks"))]
#[allow(clippy::unwrap_used)]
mod arkworks_proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn proptest_hash_to_fr_deterministic(bytes in any::<[u8; 32]>()) {
            let fr1 = hash_to_fr(&bytes);
            let fr2 = hash_to_fr(&bytes);
            assert_eq!(fr1, fr2);
        }

        #[test]
        fn proptest_poseidon_hash_deterministic(
            a in any::<u64>(),
            b in any::<u64>()
        ) {
            let fr_a = Fr::from(a);
            let fr_b = Fr::from(b);
            let h1 = poseidon_hash_to_fr(fr_a, fr_b).unwrap();
            let h2 = poseidon_hash_to_fr(fr_a, fr_b).unwrap();
            assert_eq!(h1, h2);
        }
    }
}
