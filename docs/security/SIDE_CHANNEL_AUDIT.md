# Side-Channel Resistance Audit

**Date:** 2026-05-16
**Auditor:** Automated + Manual Review (Task 3-C)
**Scope:** `omnia-substrate`, `omnia-zk`, `omnia-binding` crates — cryptographic comparison paths

**Version**: 4.0.0

## Executive Summary

This audit identifies timing side-channel vulnerabilities in cryptographic
comparison operations across the Omnia Protocol crates. The original audit
(Sprint 4, Task D4) covered only the `omnia-substrate` crate; this updated
audit extends coverage to `omnia-zk` and `omnia-binding`.

All identified issues in the substrate crate have been remediated by replacing
variable-time `==` comparisons with constant-time alternatives from the
`subtle` crate. The ZK and binding crates have several areas that require
attention in future sprints.

## Methodology

1. Searched all `==` and `!=` comparisons on secret-derived data types
   (32-byte hashes, creator IDs, event IDs, signatures, field elements,
   public keys)
2. Classified each comparison as:
   - **PUBLIC**: Comparison against public constants (e.g., zero arrays)
     or in contexts where timing information provides no advantage
   - **SECRET**: Comparison of secret-derived data where timing could
     leak information
3. Assessed each SECRET comparison for practical exploitability

## Findings — Substrate Crate (Previously Audited)

### Finding 1: Creator-Identity Binding Comparison (CRITICAL — Fixed)

**File:** `substrate/src/event.rs` — `Event::validate_creator_binding()`

**Before:**
```rust
if self.creator != *expected_creator.as_bytes() {
    return Err(EventValidationError::CreatorPubkeyMismatch { ... });
}
```

**After:**
```rust
if self.creator.ct_ne(expected_creator.as_bytes()).into() {
    return Err(EventValidationError::CreatorPubkeyMismatch { ... });
}
```

**Status:** Fixed. Uses `subtle::ConstantTimeEq`.

### Finding 2: Event Hash Verification (HIGH — Fixed)

**File:** `substrate/src/event.rs` — `Event::verify_hash()`

**Before:**
```rust
pub fn verify_hash(&self) -> bool {
    self.id == self.compute_hash()
}
```

**After:**
```rust
pub fn verify_hash(&self) -> bool {
    let computed = self.compute_hash();
    self.id.ct_eq(&computed).into()
}
```

**Status:** Fixed. Uses `subtle::ConstantTimeEq`.

### Finding 3: Equivocation Detection (MEDIUM — Fixed)

**File:** `substrate/src/slashing.rs` — `SlashingEngine::check_equivocation()`

**After:**
```rust
pub fn check_equivocation(event_a: &Event, event_b: &Event) -> bool {
    use subtle::ConstantTimeEq;
    let creators_match: bool = event_a.creator.ct_eq(&event_b.creator).into();
    let sequences_match = event_a.sequence == event_b.sequence;
    let ids_differ: bool = event_a.id.ct_ne(&event_b.id).into();
    creators_match && sequences_match && ids_differ
}
```

**Status:** Fixed. Uses `subtle::ConstantTimeEq`.

## Findings — ZK Crate (New Audit)

### Finding 4: Proof Bundle State Root Comparison (MEDIUM — Acceptable)

**File:** `zk/src/proof_bundle.rs` — `ProofBundle::verify_integrity()`

```rust
if self.state_root == self.prev_state_root {
    return Err(ProofBundleError::SameStateRoots);
}
```

**Analysis:** This comparison checks whether two public state roots are equal
as part of integrity validation. Both roots are public inputs that are
committed on-chain. An attacker who can observe the timing of this comparison
gains no useful information because both values are already public knowledge
on L1.

**Severity:** LOW — Both values are public; no secret is leaked.

**Recommendation:** No change needed. If defense-in-depth is desired,
constant-time comparison can be added, but the practical risk is negligible.

### Finding 5: Poseidon Hash Field Element Operations (LOW — Acceptable)

**File:** `zk/src/poseidon.rs` — Various field element operations

The Poseidon hash implementation uses standard `ark-ff` field arithmetic
operations (`+`, `*`, `square()`). These are NOT constant-time at the
hardware level — modular multiplication on large integers can have
data-dependent timing. However:

1. These operations occur inside the ZK circuit (R1CS constraints), where
   timing is irrelevant — the prover computes offline and the verifier only
   checks the proof.
2. Off-circuit Poseidon (`poseidon_hash_offchain`) is used for Merkle tree
   construction, not for secret comparison.

**Severity:** LOW — ZK circuit operations are inherently timing-irrelevant.
Off-circuit hash computation timing may leak whether inputs are zero or
non-zero, but this is not exploitable for collision finding.

**Recommendation:** No change needed for the ZK circuit. If off-circuit
Poseidon is ever used in a context where input secrecy matters, consider
constant-time field arithmetic.

### Finding 6: Groth16 Proof Verification (LOW — Acceptable)

**File:** `zk/src/prover.rs` — `verify_proof()`

```rust
pub fn verify_proof(
    vk: &VerifyingKey,
    public_inputs: &[ark_bn254::Fr],
    proof: &Proof,
) -> Result<bool, ProverError> {
    Groth16::<Bn254>::verify(vk, public_inputs, proof)
        .map_err(|e| ProverError::VerificationFailed(e.to_string()))
}
```

**Analysis:** Groth16 verification uses pairing operations (Miller loop +
final exponentiation) on public inputs and a public verifying key. All
inputs to `verify()` are public — the proof, public inputs, and verifying
key are all known to the verifier. No secrets are involved in verification.

**Severity:** LOW — All verification inputs are public.

**Recommendation:** No change needed.

### Finding 7: Contribution Proof Verification (LOW — Acceptable)

**File:** `zk/src/setup/contribution.rs` — `verify_pok()`

```rust
fn verify_pok(
    proof: &ContributionProof,
    old_transcript_hash: &[u8],
    new_transcript_hash: &[u8],
) -> bool {
    // Recompute challenge
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"OMNIA-POK-V1");
    hasher.update(&proof.commitment);
    hasher.update(old_transcript_hash);
    hasher.update(new_transcript_hash);
    let challenge_bytes = hasher.finalize();

    // Verify challenge matches
    if challenge_bytes.as_bytes() != proof.challenge.as_slice() {
        return false;
    }
    // ... elliptic curve operations ...
}
```

**Analysis:** The `challenge_bytes.as_bytes() != proof.challenge.as_slice()`
comparison is on a computed challenge value (derived from public data)
and the claimed challenge. The challenge is not a secret — it's derived
via Fiat-Shamir from public transcript data. A timing difference here
reveals only whether the challenge was computed correctly, which is the
intended verification result.

**Severity:** LOW — The challenge value is derived from public data;
no secret is leaked.

**Recommendation:** Consider using constant-time comparison for
defense-in-depth, but practical risk is negligible.

## Findings — Binding Crate (New Audit)

### Finding 8: Quantum Commitment Data Hash Verification (HIGH — Needs Attention)

**File:** `binding/src/quantum_commit.rs` — `QuantumCommitment::verify()`

```rust
pub fn verify(&self, public_key: &PqPublicKey, data: &[u8], phase: CommitmentPhase) -> bool {
    let hash = blake3::hash(data);
    if hash.as_bytes() != &self.data_hash {
        return false;
    }
    // ... signature verification ...
}
```

**Analysis:** The comparison `hash.as_bytes() != &self.data_hash` uses
variable-time comparison on a 32-byte hash. If the `data` input is
secret (e.g., a private transaction payload), the timing of this comparison
could leak information about the hash prefix, potentially enabling a
length-extension-style oracle.

In practice, the `data` being committed is typically the provenance log
bytes (`provenance_log.to_bytes()`), which are public. However, if the
binding layer is ever used to commit to private data, this would become
a vulnerability.

**Severity:** MEDIUM — Currently safe because committed data is public,
but the API accepts arbitrary `data: &[u8]` which could be private.

**Recommendation:** Replace with constant-time comparison:

```rust
use subtle::ConstantTimeEq;
if hash.as_bytes().ct_eq(&self.data_hash).unwrap_u8() == 0 {
    return false;
}
```

### Finding 9: RF Fingerprint Verification (MEDIUM — Acceptable)

**File:** `binding/src/rf_fingerprint.rs` — `RfFingerprint::verify()`

```rust
pub fn verify(&self, current_measurement: &[u8; 32]) -> bool {
    let distance = hamming_distance(&self.spectral_hash, current_measurement);
    let similarity = 1.0 - (distance as f64 / 256.0);
    similarity > self.confidence
}
```

**Analysis:** The `hamming_distance()` function iterates over all 32 bytes
and always performs XOR + count_ones on every byte, regardless of the
values. This is inherently constant-time because the loop body has no
early-exit or data-dependent branching. The result is a single integer
that is then compared with a threshold.

The `similarity > self.confidence` comparison is on computed values, not
directly on secret data. The `confidence` threshold is public.

**Severity:** LOW — `hamming_distance()` is naturally constant-time;
the final comparison is on derived values.

**Recommendation:** No change needed.

### Finding 10: PqPublicKey Comparison in Key Rotation (LOW — Acceptable)

**File:** `binding/src/key_rotation.rs` — `PqcKeyRotationManager::is_key_in_transition()`

```rust
pub fn is_key_in_transition(&self, key: &PqPublicKey, current_round: u64) -> bool {
    self.pending
        .iter()
        .any(|r| &r.old_key == key && current_round < r.sunset_at)
}
```

**Analysis:** The `PqPublicKey` type implements `PartialEq` (derived), which
compares the `ed25519: [u8; 32]` and `dilithium: Vec<u8>` fields. The
ed25519 field comparison uses variable-time byte comparison. However,
public keys are public by definition — they are not secrets.

**Severity:** LOW — Public keys are not secrets.

**Recommendation:** No change needed. If key privacy becomes a requirement
(e.g., stealth addresses), constant-time comparison should be used.

## Non-Findings (Acceptable)

### Unsigned Event Check (Substrate)
**File:** `substrate/src/event.rs` — `Event::validate()`

Comparison against a public constant (all-zeros). No timing information
about secrets is leaked. No change needed.

### CausalGraph Comparisons (Substrate)
**File:** `substrate/src/causal_graph.rs`

Various `==` comparisons on event IDs in the causal graph are used for
graph traversal and lookup operations. These operate on public graph state
and do not involve secret-derived data. No change needed.

### ProvenanceLog Chain Verification (Binding)
**File:** `binding/src/provenance.rs` — `ProvenanceLog::verify_chain()`

The `links_to()` method compares `data_hash` values of consecutive
commitments. These hashes are derived from committed data and are part
of the public provenance log. No change needed.

## Dependency

The `subtle` crate (v2) is required in `substrate/Cargo.toml`:

```toml
subtle = "2"
```

**Recommendation:** Add `subtle` to `binding/Cargo.toml` as well, to
support constant-time comparisons in `QuantumCommitment::verify()` and
future secret-handling code in the binding crate.

## Recommendations

1. **HIGH PRIORITY**: Replace variable-time hash comparison in
   `QuantumCommitment::verify()` (`binding/src/quantum_commit.rs`) with
   `subtle::ConstantTimeEq`. Add `subtle = "2"` to `binding/Cargo.toml`.

2. **MEDIUM PRIORITY**: Add `subtle = "2"` to `zk/Cargo.toml` and replace
   the challenge comparison in `verify_pok()` (`zk/src/setup/contribution.rs`)
   with constant-time comparison for defense-in-depth.

3. **Code review guideline**: Any new comparison on `EventId`, `NodeId`,
   `Signature`, `data_hash`, or other secret-derived byte arrays should use
   `subtle::ConstantTimeEq` instead of `==`.

4. **Clippy lint**: Consider adding a custom clippy lint or code review
   checklist item to catch variable-time comparisons on cryptographic types.

5. **Future work**: Implement `ConstantTimeEq` directly on `EventId`,
   `NodeId`, and `PqPublicKey` type aliases/newtypes, so that `==`
   automatically uses constant-time comparison.
