# Safety Justification

This document justifies every `unsafe` block usage in the Omnia Protocol
workspace and explains the `deny(unsafe_code)` (rather than `forbid(unsafe_code)`)
policy applied at the crate level.

## Policy

All workspace crates use `#![deny(unsafe_code)]`. This means any `unsafe`
block written inside an Omnia crate produces a compile error. The
`forbid(unsafe_code)` policy is **not** used at the workspace level because
two Omnia crates legitimately depend on `unsafe` FFI bindings to audited C
libraries — those bindings are required for cryptographic correctness and
cannot be rewritten in safe Rust without losing the audit guarantees.

## blst (BLS12-381 signatures)

`omnia-crypto` wraps [`blst`](https://github.com/supranational/blst), a
high-performance C library for BLS12-381 pairing-based cryptography.
`blst` requires `unsafe` FFI bindings.

- **Audit status**: blst was independently audited by NCC Group in 2021
  ([report](https://research.nccgroup.com/2021/06/09/public-report-supranational-blst-cryptography-library/)).
- **Scope of unsafe usage**: all `unsafe` code is confined to
  `omnia-crypto/src/bls.rs` and the `omnia-crypto/src/bls12_381_scalar.rs`
  helper module. No other Omnia crate writes `unsafe` blocks.
- **Transitive impact**: `omnia-substrate` (the integration crate) re-exports
  `omnia-crypto::bls` types when the `bls` feature is enabled. This is why
  `omnia-substrate` uses `#![deny(unsafe_code)]` rather than
  `#![forbid(unsafe_code)]` — `forbid` is viral and would prevent the `bls`
  feature from being enabled at all. `deny` still catches any `unsafe`
  block written _inside_ `omnia-substrate` itself, which is the policy we
  want.

## Other unsafe usage

None at this time. If a future change requires `unsafe` in an Omnia crate,
the change must:

1. Add an inline `// SAFETY:` comment justifying each `unsafe` block.
2. Update this document with a new section describing the usage.
3. Be reviewed by someone with unsafe Rust expertise.

## Audit References

- Audit finding C-1 (v0.1.68): substrate crate had `#![forbid(unsafe_code)]`
  which contradicted its dependency on `omnia-crypto::bls` (which uses
  `unsafe` FFI to blst). Resolved by downgrading to `#![deny(unsafe_code)]`
  and adding this document.
