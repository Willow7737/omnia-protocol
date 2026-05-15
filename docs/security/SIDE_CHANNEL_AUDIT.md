# Side-Channel Resistance Audit

**Date:** 2026-03-05  
**Auditor:** Automated (Sprint 4, Task D4)  
**Scope:** `omnia-substrate` crate — cryptographic comparison paths

## Executive Summary

This audit identified timing side-channel vulnerabilities in cryptographic
comparison operations within the Omnia Substrate. All identified issues have
been remediated by replacing variable-time `==` comparisons with constant-time
alternatives from the `subtle` crate.

## Methodology

1. Searched all `==` and `!=` comparisons on secret-derived data types
   (32-byte hashes, creator IDs, event IDs, signatures)
2. Classified each comparison as:
   - **PUBLIC**: Comparison against public constants (e.g., zero arrays)
   - **SECRET**: Comparison of secret-derived data where timing could leak information
3. Replaced all SECRET comparisons with `subtle::ConstantTimeEq`

## Findings and Remediation

### Finding 1: Creator-Identity Binding Comparison (CRITICAL — Fixed)

**File:** `substrate/src/event.rs` — `Event::validate_creator_binding()`

**Before:**
```rust
if self.creator != *expected_creator.as_bytes() {
    return Err(EventValidationError::CreatorPubkeyMismatch { ... });
}
```

**Issue:** The `!=` comparison on the 32-byte `creator` field uses Rust's
default `PartialEq`, which short-circuits on the first differing byte. An
attacker measuring response time could progressively recover the expected
creator identity byte-by-byte.

**After:**
```rust
if self.creator.ct_ne(expected_creator.as_bytes()).into() {
    return Err(EventValidationError::CreatorPubkeyMismatch { ... });
}
```

**Severity:** Critical — could enable identity forgery via timing oracle.

### Finding 2: Event Hash Verification (HIGH — Fixed)

**File:** `substrate/src/event.rs` — `Event::verify_hash()`

**Before:**
```rust
pub fn verify_hash(&self) -> bool {
    self.id == self.compute_hash()
}
```

**Issue:** The `==` comparison on the 32-byte `EventId` (SHA-256 hash)
uses variable-time comparison. While this is a hash comparison rather than
a direct secret comparison, timing differences could theoretically leak
information about the hash prefix in multi-target attack scenarios.

**After:**
```rust
pub fn verify_hash(&self) -> bool {
    let computed = self.compute_hash();
    self.id.ct_eq(&computed).into()
}
```

**Severity:** High — hash comparison should use constant-time to prevent
partial-hash leakage.

### Finding 3: Equivocation Detection (MEDIUM — Fixed)

**File:** `substrate/src/slashing.rs` — `SlashingEngine::check_equivocation()`

**Before:**
```rust
pub fn check_equivocation(event_a: &Event, event_b: &Event) -> bool {
    event_a.creator == event_b.creator
        && event_a.sequence == event_b.sequence
        && event_a.id != event_b.id
}
```

**Issue:** The `==` and `!=` comparisons on `creator` (32-byte NodeId) and
`id` (32-byte EventId) use variable-time comparison. While equivocation
detection operates on publicly visible event data, using constant-time
eliminates the risk of timing oracles in any context.

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

**Severity:** Medium — public data but good hygiene to use constant-time.

### Non-Finding: Unsigned Event Check (ACCEPTABLE)

**File:** `substrate/src/event.rs` — `Event::validate()`

```rust
if self.signature == [0u8; 64] || self.creator_pubkey == [0u8; 32] {
    return Err(EventValidationError::UnsignedEvent);
}
```

**Analysis:** This compares against a public constant (all-zeros), not secret
data. No timing information about secrets is leaked. No change needed.

### Non-Finding: CausalGraph Comparisons (ACCEPTABLE)

**File:** `substrate/src/causal_graph.rs`

Various `==` comparisons on event IDs in the causal graph are used for
graph traversal and lookup operations. These operate on public graph state
and do not involve secret-derived data. No change needed.

## Dependency

The `subtle` crate (v2) was added to `substrate/Cargo.toml`:

```toml
# Constant-time comparisons for cryptographic operations — prevents timing side-channels
subtle = "2"
```

The `subtle` crate is the de-facto standard for constant-time cryptographic
operations in Rust, used by `ed25519-dalek`, `ring`, and other crypto crates.

## Testing

- All existing tests continue to pass with the constant-time replacements.
- `verify_hash()` still correctly identifies tampered events.
- `validate_creator_binding()` still rejects mismatched creator identities.
- `check_equivocation()` still correctly detects equivocation.

## Recommendations

1. **Code review guideline:** Any new comparison on `EventId`, `NodeId`,
   `Signature`, or other secret-derived byte arrays should use
   `subtle::ConstantTimeEq` instead of `==`.

2. **Clippy lint:** Consider adding a custom clippy lint or code review
   checklist item to catch variable-time comparisons on cryptographic types.

3. **Future work:** Consider implementing `ConstantTimeEq` directly on
   `EventId` and `NodeId` type aliases, so that `==` automatically uses
   constant-time comparison. This would require wrapping the array types
   in newtypes.
