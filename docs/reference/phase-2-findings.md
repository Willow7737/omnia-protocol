# Phase 2 Findings

> 🎯 Audience: Developers
> 🔗 Context: Audit findings from Phase 2 implementation
> 📅 Last Updated: 2026-06-24

**Version:** v5.0.0
**Date:** 2026-05-18
**Auditor:** Phase 2 Internal Review
**Scope:** Cryptographic subsystems, ZK circuit integrity, identity recovery

---

## Executive Summary

Phase 2 pre-work review identified **5 findings** across 3 severity levels. These findings were uncovered during ADR creation (M-4) and dashboard review (M-5). The most critical issues involve incomplete Shamir's Secret Sharing recovery flow, placeholder values in the ZK circuit, and a zero-initialized trusted setup transcript hash. None of these findings represent immediate exploits, but all must be resolved before mainnet deployment.

---

## CRITICAL

---

## FIND-P2-001: SSS Recovery Does Not Update DID Authentication

**Severity:** Critical
**Category:** Security
**Location:** `shards/src/identity/state.rs:66-67`
**Status:** ✅ Closed in Phase 3 (SSS recovery now updates DID authentication with the recovered key)

### Description

The `DidDocument` struct has an explicit TODO comment at line 66:

```rust
// TODO: Recovery should add the new recovered key to authentication
// and optionally remove the compromised old key. Currently untouched.
pub authentication: Vec<[u8; 32]>,
```

When a DID is recovered via `IdentityOp::RecoverDid`, the reconstructed secret is verified but the DID document's `authentication` list is never updated with the new key. The old (potentially compromised) key remains the only valid authentication method, making the recovery operation effectively useless from a security standpoint.

### Impact

- **Recovery is cosmetic**: A user who successfully recovers their DID via Shamir's Secret Sharing cannot authenticate with the new key because the old key is still the only one in the `authentication` list.
- **Compromised key persists**: If recovery was triggered because the old key was compromised, the attacker retains authentication capability while the legitimate owner's new key is unrecognized.
- **False sense of security**: The recovery operation appears to succeed (no error is returned), giving users confidence in a mechanism that provides no actual security benefit.

### Evidence

```rust
// In state.rs, RecoverDid handler:
IdentityOp::RecoverDid { did, shares } => {
    // ... reconstruct secret, verify ...
    doc.recovery_enabled = true;
    // BUG: doc.authentication is NEVER updated with the recovered key
}
```

### Remediation

**Status: ✅ Closed in Phase 3** — Implemented in Phase 3. See `shards/src/identity/state.rs` for the DID authentication update flow:

1. After successful secret reconstruction, derive the new public key from the recovered secret
2. Add the new public key to `doc.authentication`
3. Optionally remove the old public key (configurable: some users may want both keys valid temporarily)
4. Add a `DidUpdate::ReplaceAuthentication` variant for atomic key replacement
5. Estimated effort: 1 sprint

---

## FIND-P2-002: SSS Share Encryption Uses XOR Instead of AES-256-GCM

**Severity:** Critical
**Category:** Security
**Location:** `shards/src/identity/state.rs:407-408`
**Status:** ✅ Closed in Phase 3 (replaced XOR with real AES-256-GCM encryption)

### Description

The `persist_shares()` method encrypts Shamir's Secret Sharing recovery shares using XOR with a BLAKE3-derived key:

```rust
// XOR-encrypt the share value with the derived key (repeating as needed)
let ciphertext = xor_with_key(&share.value, &key);
```

XOR "encryption" provides no authentication (no MAC/tag), making it vulnerable to bit-flipping attacks. An attacker who can modify the persisted shares can alter individual bits in the ciphertext, and the change will propagate undetected through the XOR decryption, producing a different (but valid-looking) share. When combined with other shares, the reconstructed secret will be incorrect, but there is no way to detect the tampering before reconstruction fails.

This is the same class of vulnerability as FIND-010 (unencrypted private key storage) from Phase 0, which was remediated by migrating to AES-256-GCM for the keystore.

### Impact

- **No integrity protection**: Tampered shares produce no error until reconstruction, at which point it's unclear which share was modified.
- **Bit-flipping attacks**: An attacker with write access to the share store can systematically modify shares to produce a chosen reconstructed value.
- **Inconsistent with keystore security**: The keystore was upgraded to AES-256-GCM in Phase 0 (FIND-010), but SSS shares still use the deprecated XOR method.

### Evidence

```rust
/// This is a simple stream cipher suitable for share encryption at the
/// persistence layer. For production, consider AES-256-GCM instead.
fn xor_with_key(data: &[u8], key: &[u8; 32]) -> Vec<u8> {
    data.iter()
        .enumerate()
        .map(|(i, &b)| b ^ key[i % key.len()])
        .collect()
}
```

### Remediation

**Status: ✅ Closed in Phase 3** — Implemented in Phase 3 using real AES-256-GCM, following the same pattern as `substrate/src/keystore.rs`:

1. Use `aes_gcm_encrypt()` / `aes_gcm_decrypt()` with HKDF-SHA256 key derivation
2. Each share gets its own random salt + nonce (same format as keystore: `salt(32) || nonce(12) || ciphertext+tag`)
3. Add backward compatibility for legacy XOR-encrypted shares (auto-upgrade on next write)
4. Estimated effort: 1 sprint

---

## FIND-P2-003: DKG Share Packages Use XOR Encryption

**Severity:** Critical
**Category:** Security
**Location:** `substrate/src/threshold.rs:674-678`
**Status:** ✅ Closed in Phase 3 (replaced XOR with real AES-256-GCM encryption)

### Description

The `DkgSession::generate_shares()` method encrypts DKG share packages using a simple XOR cipher:

```rust
/// Simple XOR encryption for DKG shares (domain-separated).
fn xor_encrypt_dkg(data: &[u8], key: &[u8; 32]) -> Vec<u8> {
    data.iter()
        .enumerate()
        .map(|(i, &b)| b ^ key[i % key.len()])
        .collect()
}
```

DKG shares contain BLS secret key material. XOR encryption provides no integrity protection, making share packages vulnerable to tampering during network transmission. A man-in-the-middle could modify encrypted shares without detection, potentially allowing a malicious participant to inject chosen shares that subvert the threshold scheme.

This mirrors FIND-P2-002 (XOR share encryption in identity recovery) and FIND-010 (XOR keystore encryption from Phase 0).

### Impact

- **DKG integrity**: An active network attacker can modify DKG shares in transit without detection, potentially injecting shares that bias the group key.
- **Threshold scheme subversion**: If a malicious participant can predict or influence the share derivation, they may be able to construct shares that give them disproportionate control over the group key.
- **Inconsistent security posture**: Three separate subsystems now use XOR for secret material encryption, despite the keystore having been migrated to AES-256-GCM in Phase 0.

### Evidence

```rust
// In DkgSession::generate_shares():
let share_data = keypair.secret_key_bytes().to_vec();
let encrypted = xor_encrypt_dkg(&share_data, &share_seed);
```

### Remediation

**Status: ✅ Closed in Phase 3** — `xor_encrypt_dkg()` replaced with AES-256-GCM encryption:

1. Use a per-share random salt + nonce with HKDF-SHA256 key derivation
2. Encrypt the BLS secret key bytes using `aes_gcm_encrypt()` (same as keystore)
3. The `DkgSharePackage.encrypted_shares` field format changes to `salt(32) || nonce(12) || ciphertext+tag`
4. Verify that recipients can decrypt and validate shares before accepting
5. Estimated effort: 1 sprint (combined with FIND-P2-002)

---

## HIGH

---

## FIND-P2-010: ZK Circuit Uses Dummy Field Values for Trusted Setup

**Severity:** High
**Category:** Cryptographic Integrity
**Location:** `omnia-adapters/src/circuit.rs:479-510`
**Status:** ✅ Closed in Phase 3 (dummy field values replaced with proper witness binding)

### Description

The `ExpandedRollupCircuit::empty()` method creates a circuit instance with all witness values set to `Fr::zero()`:

```rust
pub fn empty(num_events: usize, merkle_depth: usize) -> Self {
    // ...
    Self {
        old_state_root: Some(Fr::zero()),
        new_state_root: Some(Fr::zero()),
        event_commitment: Some(Fr::zero()),
        events,          // All Fr::zero()
        merkle_proofs,   // All Fr::zero() / false
        intermediate_roots, // All Fr::zero()
    }
}
```

This method is used for generating trusted setup keys (proving key and verifying key). While zero-valued assignments are valid for key generation (the R1CS constraint system is defined by the circuit structure, not the specific values), the method name `empty()` and its use of `Fr::zero()` everywhere creates a misleading impression. More importantly:

1. The zero-valued `old_state_root` and `new_state_root` satisfy the equality constraint `old_state_root == new_state_root` that is **rejected** by `ProofBundle::verify_integrity()` in production. This means the circuit structure may not properly constrain the `prev_state_root != state_root` invariant.

2. The `event_commitment` of `Fr::zero()` combined with zero-valued Merkle proofs may not exercise all constraint branches in the circuit, potentially leaving some constraints untested during setup.

### Impact

- **Constraint coverage gap**: If the trusted setup is generated with a circuit that doesn't exercise all constraint branches, the resulting proving key may not support proofs that use those branches.
- **Misleading semantics**: `Fr::zero()` values make it unclear whether the circuit properly handles edge cases like zero state roots or empty Merkle paths.
- **Audit concern**: An auditor reviewing the circuit setup process would flag the use of dummy values as a potential weakness.

### Evidence

```rust
// In circuit.rs:
pub fn empty(num_events: usize, merkle_depth: usize) -> Self {
    // All witnesses are Fr::zero() — dummy values
    let events: Vec<Option<EventWitness>> = (0..num_events)
        .map(|_| {
            Some(EventWitness {
                event_hash: Some(Fr::zero()),
                operation_type: Some(Fr::zero()),
                payload_hash: Some(Fr::zero()),
            })
        })
        .collect();
    // ...
}
```

### Remediation

**Status: ✅ Closed in Phase 3** — Dummy values replaced with proper witness binding:

1. Use `Fr::rand()` for all witness values in `empty()` to ensure all constraint branches are exercised during setup
2. Rename `empty()` to `for_setup()` to clarify its purpose
3. Add a test that verifies the circuit synthesized from `for_setup()` has the expected number of constraints
4. Consider adding a `ProofBundle::verify_integrity()`-compatible circuit constructor for integration tests
5. Estimated effort: 0.5 sprint

---

## MEDIUM

---

## FIND-P2-011: Trusted Setup Transcript Hash Initialized to Zero

**Severity:** Medium
**Category:** Cryptographic Integrity
**Location:** `omnia-adapters/src/setup/powers_of_tau.rs:113`
**Status:** ✅ Closed in Phase 3 (transcript hash now initialized with proper BLAKE3 commitment)

### Description

The `PowersOfTau::new()` method initializes the `transcript_hash` field to `[0u8; 32]`:

```rust
pub fn new(degree: usize) -> Result<Self, SetupError> {
    // ...
    Ok(Self {
        g1_powers,
        g2_powers,
        contribution_count: 0,
        transcript_hash: [0u8; 32],  // ← Zero-initialized hash
    })
}
```

The transcript hash is used in the Proof of Knowledge (PoK) verification during contributions (`contribution.rs`): `c = H(R || old_transcript_hash || new_transcript_hash)`. A zero-initialized `old_transcript_hash` means the first contribution's PoK challenge is computed with a predictable input, which weakens the binding between the first contribution and the initial state.

After the first contribution is applied, the transcript hash is updated to a proper BLAKE3 hash, so only the first contribution is affected. However, in a multi-party ceremony, the first contribution sets the foundation for all subsequent contributions, making its integrity particularly important.

### Impact

- **First contribution binding**: The PoK challenge for the first contribution is computed with a predictable `old_transcript_hash`, meaning the first contributor could potentially precompute their contribution before the ceremony begins (grinding attack).
- **No initial state commitment**: The zero hash does not commit to the initial SRS state, so there is no verifiable link between the generator-initialized SRS and the first contribution.
- **Audit concern**: A zero-initialized hash is a common red flag in cryptographic ceremony implementations.

### Evidence

```rust
// In powers_of_tau.rs:
transcript_hash: [0u8; 32],  // Should be BLAKE3 of initial generator state

// In contribution.rs, the first contribution's PoK uses this zero hash:
// c = H(R || [0u8; 32] || new_transcript_hash)
```

### Remediation

**Status: ✅ Closed in Phase 3** — Transcript hash initialized with a proper commitment to the initial SRS state:

1. Compute `transcript_hash = BLAKE3("OMNIA-CEREMONY-INIT-V1" || to_transcript())` in `PowersOfTau::new()`
2. This creates a verifiable binding between the initial generator state and the first contribution
3. Update `run_ceremony()` to verify the initial transcript hash before accepting contributions
4. Estimated effort: 0.5 sprint

---

## Finding Summary Table

| ID          | Title                                                | Severity | Status |
| ----------- | ---------------------------------------------------- | -------- | ------ |
| FIND-P2-001 | SSS recovery does not update DID authentication      | Critical | ✅ Closed in Phase 3   |
| FIND-P2-002 | SSS share encryption uses XOR instead of AES-256-GCM | Critical | ✅ Closed in Phase 3   |
| FIND-P2-003 | DKG share packages use XOR encryption                | Critical | ✅ Closed in Phase 3   |
| FIND-P2-010 | ZK circuit uses dummy field values for trusted setup | High     | ✅ Closed in Phase 3   |
| FIND-P2-011 | Trusted setup transcript hash initialized to zero    | Medium   | ✅ Closed in Phase 3   |

**Totals**: 5 Fixed (all closed in Phase 3), 0 Open

---

## Related Architecture Decision Records

| ADR     | Finding                  | Relationship                                                                                                                                                              |
| ------- | ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| ADR-010 | FIND-P2-002, FIND-P2-003 | ADR-010 documents AES-256-GCM as the accepted keystore encryption; FIND-P2-002/003 identify that this standard is not consistently applied to SSS shares and DKG packages |
| ADR-011 | —                        | ADR-011 designs the gradual slashing model; implementation will create new FIND entries when the current binary model is replaced                                         |
| ADR-012 | —                        | ADR-012 accepts the non-standard VRF construction; no finding needed since the deviation is documented                                                                    |
| ADR-013 | FIND-P2-003              | ADR-013 documents the Feldman VSS-based DKG; FIND-P2-003 identifies that the share encryption in DKG uses XOR instead of the AES-256-GCM standard                         |
| ADR-014 | FIND-P2-010              | ADR-014 documents the non-standard Poseidon parameters; FIND-P2-010 identifies that the circuit setup uses dummy values that may not exercise all constraint branches     |

---

🔙 **Back**: [Reference Index](../) | 🔄 **Related**: [Roadmap](./roadmap.md)
🚀 **Next**: [Blueprint Reference](./blueprint-reference.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)
