# ADR-005: ProofBundle Universal Proof Format

**Status**: Proposed
**Date**: 2026-05-14
**Decision**: Define a universal `ProofBundle` format that encapsulates all data needed for cross-chain state transition verification, with forward-compatible versioning and dual serialization strategies.

## Context

The Omnia ZK-rollup must post state transition proofs to L1 settlement layers. Each proof must contain enough information for an L1 verifier (smart contract, program, or script) to independently verify that a state transition is valid, without trusting the operator.

The proof bundle must satisfy several requirements:

1. **Chain-agnostic structure.** The same proof bundle must be usable on Ethereum, Bitcoin, Solana, Celestia, or any future L1. The bundle's logical structure is defined once; only the encoding changes per chain.
2. **Forward compatibility.** As the protocol evolves (Phase 0 hash-chain stubs → Phase 1 R1CS proofs → future proof systems), the bundle format must support versioned upgrades without breaking existing verifiers.
3. **State transition verification.** The bundle must contain both the previous and new state roots so that the verifier can confirm the transition.
4. **Data availability.** The bundle must commit to the batch data via a Merkle root, enabling light clients to verify inclusion without downloading the full batch.
5. **Cross-chain anchor.** The bundle must reference an L1 anchor (block hash, slot, etc.) for cross-chain verification.

The proof generation infrastructure is in `zk/src/proof.rs`, the settlement adapters in `zk/src/settlement/`, and the ZK circuit in `zk/src/circuit.rs`.

## Decision

### ProofBundle Structure

We define the following logical structure for a proof bundle:

```rust
struct ProofBundle {
    /// Version field for forward compatibility.
    /// Version 1 = Phase 0 (hash-chain stubs).
    /// Version 2 = Phase 1 (R1CS/Groth16 proofs).
    version: u32,

    /// Previous state root (before the batch was applied).
    /// Must match the L1-confirmed state root.
    prev_state_root: [u8; 32],

    /// New state root (after the batch was applied).
    /// Computed by CausalGraph::state_root() after all batch events are inserted.
    new_state_root: [u8; 32],

    /// Transition proof bytes.
    /// Phase 0: hash-chain commitment (see zk/src/proof.rs::compute_batch_commitment).
    /// Phase 1+: R1CS proof bytes (Groth16, PLONK, or STARK).
    transition_proof: Vec<u8>,

    /// Merkle root of the batch data for data availability.
    /// Enables light clients to verify event inclusion without downloading the full batch.
    batch_merkle_root: [u8; 32],

    /// L1 anchor for cross-chain verification.
    /// On Ethereum: the L1 block hash at the time the batch was created.
    /// On Bitcoin: the block hash.
    /// On Solana: the slot number (as 32 bytes).
    /// On Celestia: the namespace + block height.
    l1_anchor: [u8; 32],
}
```

### Field Justifications

**Version field.** The `version: u32` field enables forward compatibility. When Phase 1 introduces R1CS proofs, the version is incremented to 2. L1 verifiers check the version and reject bundles with unsupported versions. This prevents ambiguity: a Phase 0 verifier encountering a Phase 1 proof can cleanly reject it rather than attempting to parse incompatible bytes.

**State roots (prev + new).** The `prev_state_root` must match the last confirmed root on L1 (obtained via `SettlementLayer::latest_state_root()`). The `new_state_root` is computed by `CausalGraph::state_root()` after inserting all events in the batch. The L1 verifier checks that `prev_state_root` matches its stored root, then verifies the transition proof to confirm that `new_state_root` is correct.

**Transition proof bytes.** In Phase 0, this contains the output of `compute_batch_commitment()` from `zk/src/proof.rs`:

```rust
pub fn compute_batch_commitment(
    old_root: &[u8; 32],
    new_root: &[u8; 32],
    batch_data: &[u8],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(old_root);
    hasher.update(batch_data);
    hasher.update(new_root);
    *hasher.finalize().as_bytes()
}
```

In Phase 1, this will contain a Groth16 or PLONK proof that can be verified on L1 by a smart contract. The `verify_stub_proof()` function validates Phase 0 proofs (minimum 32 bytes, non-empty). The `generate_dummy_proof()` function creates 192-byte stub proofs for testing.

**Batch Merkle root.** This is the root of a Merkle tree built over the batch's events. It enables data availability proofs: a light client can verify that a specific event is included in the batch by checking a Merkle proof against this root. The `CausalGraph::merkle_proof()` method generates these proofs, and `CausalGraph::state_root()` computes the root.

**L1 anchor.** This field ties the L2 proof to a specific L1 block, preventing replay attacks. A proof created at L1 block N cannot be replayed at L1 block M > N because the anchor won't match. The anchor format varies by chain:

- Ethereum: `keccak256(block_hash)`
- Bitcoin: the block hash itself
- Solana: slot number encoded as 32 bytes
- Celestia: namespace + block height

### Serialization Strategy

**Internal serialization: bincode.** Within the Omnia L2 node, `ProofBundle` is serialized using `bincode` for efficiency. Bincode is a compact binary format that produces minimal wire size, which is critical when storing thousands of bundles in memory or transmitting them between L2 nodes.

**Per-chain encoder for L1.** Each `SettlementLayer` adapter is responsible for encoding the `ProofBundle` into a format that its L1 can process:

- **Ethereum**: ABI-encoded calldata for the `OmniaRollup.sol` contract's `verifyAndCommit()` function.
- **Bitcoin**: OP_RETURN script data (limited to 80 bytes, so only the state roots and Merkle root are included).
- **Solana**: Borsh-serialized account data.
- **Celestia**: Namespace-prefixed blob data.

This dual serialization strategy means the `ProofBundle` struct itself is chain-agnostic, while each adapter handles the L1-specific encoding. No chain-specific logic leaks into the core proof generation code.

### Why This Format Enables Multi-Chain Settlement

The `ProofBundle` contains all the information needed to verify a state transition on any chain:

1. **What was the old state?** → `prev_state_root`
2. **What is the new state?** → `new_state_root`
3. **Prove the transition is valid** → `transition_proof`
4. **Prove the data is available** → `batch_merkle_root`
5. **When did this happen on L1?** → `l1_anchor`

Any L1 verifier that can check these five things can settle Omnia state transitions. Adding a new L1 chain only requires implementing the `SettlementLayer` trait and an encoder for the `ProofBundle` — the proof generation logic remains unchanged.

## Consequences

- **Positive**: One proof format serves all chains. Adding a new L1 adapter requires no changes to proof generation.
- **Positive**: Version field enables smooth upgrades from Phase 0 hash-chain stubs to Phase 1 R1CS proofs.
- **Positive**: `batch_merkle_root` enables light client support and data availability verification without full batch download.
- **Negative**: The `transition_proof: Vec<u8>` is untyped — the verifier must know the proof system from the `version` field. Misaligned version/proof-type pairs would cause verification failures.
- **Negative**: Phase 0 proofs (hash-chain stubs) provide no real cryptographic security. They are placeholders that must be replaced before mainnet.
- **Trade-off**: The L1 anchor is fixed at 32 bytes, which may be insufficient for some chains. If a chain needs more than 32 bytes for its anchor, the field would need to be extended or use a hash of the actual anchor data.
