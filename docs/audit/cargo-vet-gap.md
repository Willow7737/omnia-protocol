# cargo-vet Gap Analysis — 222 Unvetted Dependencies

**Date:** 2026-06-29
**Source:** Architecture audit finding H6
**Status:** Tracked — awaiting audit work

## Summary

The architecture audit (H6) correctly identified that `cargo vet` was running with `continue-on-error: true` in CI, making the gate decorative. The initial fix removed the flag, but this exposed **222 dependencies** that have no audit entry in `supply-chain/config.toml` and no exemption in `supply-chain/audits.toml`.

## Current state

- **281 dependencies** have exemptions in `supply-chain/config.toml` with substantive notes (≥30 chars, verified by `scripts/check_exemption_notes.py`)
- **222 dependencies** are completely unvetted (listed below)
- **CI behavior:** `cargo vet` runs with `continue-on-error: true` — the gate is informational, not blocking

## Why we don't just mark them all `safe-to-deploy`

Marking 222 crypto-heavy dependencies (ark-ff, aws-lc-rs, secp256k1, k256, etc.) as `safe-to-deploy` without actually reviewing them would be dishonest. The audit's criticism was that the gate was decorative — the fix is to actually audit the deps, not to pretend we did.

## Categories of unvetted deps

| Category | Count | Examples | Risk |
|----------|-------|----------|------|
| Arkworks (ZK crypto) | 12 | ark-ff, ark-serialize, ark-std | HIGH — crypto-critical |
| Ethereum (alloy) | 18 | alloy-sol-types, alloy-primitives, rlp, keccak | HIGH — settlement layer |
| TLS/SSL | 14 | aws-lc-rs, native-tls, openssl, rustls | HIGH — network security |
| secp256k1/k256 | 8 | secp256k1, secp256k1-sys, k256, ecdsa | HIGH — signature verification |
| libp2p networking | 6 | libp2p-identity, quinn-proto, mio | MEDIUM — P2P transport |
| Serialization | 9 | serde_json, borsh, ciborium, postcard | MEDIUM — deserialization attack surface |
| Macros/derive | 22 | darling, derive_more, schemars, strum | LOW — compile-time only |
| Testing/bench | 15 | criterion, proptest, iai-callgrind, libfuzzer-sys | LOW — dev-only |
| WASM bindings | 8 | wasm-bindgen, js-sys, web-sys | LOW — not used in node binary |
| Other | 110 | chrono, dashmap, lru, hyper, reqwest | MIXED |

## Recommended approach

1. **Immediate:** Mark dev-only deps (criterion, proptest, libfuzzer-sys, iai-callgrind, etc.) as `safe-to-run` instead of `safe-to-deploy`. These don't ship in the binary.

2. **Short-term:** Audit the HIGH-risk crypto deps (arkworks, secp256k1, k256, aws-lc-rs). These are widely used in other blockchain projects — check if Mozilla/Google/Embark Studios have already audited them.

3. **Medium-term:** Audit the MEDIUM-risk deps (libp2p, serde_json, hyper).

4. **Long-term:** Audit the remaining LOW-risk deps.

5. **Enforce:** Once all deps are either audited or exempted with notes, remove `continue-on-error: true` from the CI step.

## Full list of 222 unvetted dependencies

```
allocator-api2:0.2.21
anes:0.1.6
ark-ff:0.3.0
ark-ff:0.5.0
ark-ff-asm:0.3.0
ark-ff-asm:0.5.0
ark-ff-macros:0.3.0
ark-ff-macros:0.5.0
ark-serialize:0.3.0
ark-serialize:0.5.0
ark-std:0.3.0
ark-std:0.5.0
asn1-rs:0.7.2
async-stream:0.3.6
async-stream-impl:0.3.6
async_io_stream:0.3.3
auto_impl:1.3.0
autocfg:1.5.1
aws-lc-rs:1.17.0
aws-lc-sys:0.41.0
base16ct:0.2.0
bimap:0.6.3
bitcoin-io:0.1.4
bitcoin_hashes:0.14.2
bitflags:2.13.0
bitvec:1.0.1
borsh:1.6.1
borsh-derive:1.6.1
bumpalo:3.20.3
byte-slice-cast:1.2.3
c-kzg:2.1.7
cast:0.3.0
cc:1.2.63
chrono:0.4.45
ciborium:0.2.2
ciborium-io:0.2.2
ciborium-ll:0.2.2
cmake:0.1.58
combine:4.6.7
const-hex:1.19.1
const_format:0.2.36
const_format_proc_macros:0.2.34
convert_case:0.10.0
core-foundation:0.10.1
crc:3.4.0
crc-catalog:2.5.0
criterion:0.5.1
criterion-plot:0.5.0
crypto-bigint:0.5.5
crypto-common:0.2.2
darling:0.23.0
darling_core:0.23.0
darling_macro:0.23.0
dashmap:6.2.1
derive_more:0.99.20
derive_more:2.1.1
derive_more-impl:2.1.1
digest:0.9.0
displaydoc:0.2.6
dunce:1.0.5
dyn-clone:1.0.20
ecdsa:0.16.9
educe:0.6.0
elliptic-curve:0.13.8
enum-ordinalize:4.3.2
enum-ordinalize-derive:4.3.2
fastrand:2.4.1
fastrlp:0.3.1
fastrlp:0.4.0
ff:0.13.1
fixed-hash:0.8.0
foreign-types:0.3.2
foreign-types-shared:0.1.1
fs_extra:1.3.0
funty:2.0.0
futures-timer:3.0.4
futures-utils-wasm:0.1.0
group:0.13.0
half:2.7.1
http:1.4.1
hybrid-array:0.2.3
hyper:1.10.1
hyper-rustls:0.27.9
hyper-tls:0.6.0
iai-callgrind:0.13.4
iai-callgrind-macros:0.4.1
iai-callgrind-runner:0.13.4
ident_case:1.0.1
impl-codec:0.6.0
impl-trait-for-tuples:0.2.3
indexmap:1.9.3
is-terminal:0.4.17
itertools:0.13.0
itertools:0.14.0
jni:0.22.4
jni-macros:0.22.4
jni-sys:0.4.1
jni-sys-macros:0.4.1
js-sys:0.3.99
k256:0.13.4
keccak:0.1.6
keccak:0.2.0
keccak-asm:0.1.7
kem:0.3.0-pre.0
konst:0.2.20
konst_macro_rules:0.2.19
libfuzzer-sys:0.4.13
libm:0.2.16
libp2p-identity:0.2.14
log:0.4.32
lru:0.16.4
macro-string:0.2.0
memchr:2.8.1
mio:1.2.1
ml-kem:0.1.1
ml-kem:0.2.3
native-tls:0.2.18
num-conv:0.2.2
num_enum:0.7.6
num_enum_derive:0.7.6
nybbles:0.4.8
oorandom:11.1.5
openssl:0.10.80
openssl-probe:0.2.1
openssl-sys:0.9.116
parity-scale-codec:3.7.5
parity-scale-codec-derive:3.7.5
pest:2.8.6
pharos:0.5.3
pin-utils:0.1.0
pkg-config:0.3.33
plotters:0.3.7
plotters-backend:0.3.7
plotters-svg:0.3.7
primitive-types:0.12.2
proc-macro-crate:3.5.0
proptest:1.11.0
prost:0.14.3
prost-derive:0.14.3
quick-error:1.2.3
quinn-proto:0.11.15
radium:0.7.0
rand_xorshift:0.4.0
rapidhash:4.4.1
ref-cast:1.0.25
ref-cast-impl:1.0.25
reqwest:0.12.28
reqwest:0.13.4
rfc6979:0.4.0
rlp:0.5.2
ruint:1.18.0
ruint-macro:1.2.1
rustc-hex:2.1.0
rustc_version:0.3.3
rustls-native-certs:0.8.4
rustls-platform-verifier:0.7.0
rustls-platform-verifier-android:0.1.1
rusty-fork:0.3.1
schannel:0.1.29
schemars:0.9.0
schemars:1.2.1
sec1:0.7.3
secp256k1:0.30.0
secp256k1:0.31.1
secp256k1-sys:0.10.1
secp256k1-sys:0.11.0
security-framework:3.7.0
security-framework-sys:2.17.0
semver:0.11.0
semver-parser:0.10.3
send_wrapper:0.6.0
serde_json:1.0.150
serde_with:3.21.0
serde_with_macros:3.21.0
serdect:0.2.0
sha1:0.10.6
sha3:0.10.9
sha3:0.11.0
sha3-asm:0.1.7
shlex:2.0.1
simd_cesu8:1.1.1
simdutf8:0.1.5
socket2:0.6.4
strum:0.27.2
strum_macros:0.27.2
syn-solidity:1.6.0
tap:1.0.1
tempfile:3.27.0
tinytemplate:1.2.1
tokio-native-tls:0.3.1
tokio-rustls:0.26.4
tokio-stream:0.1.18
tokio-tungstenite:0.28.0
toml_datetime:1.1.1+spec-1.1.0
toml_edit:0.25.12+spec-1.1.0
toml_parser:1.1.2+spec-1.1.0
tower-http:0.6.11
tungstenite:0.28.0
typenum:1.20.1
ucd-trie:0.1.7
uint:0.9.5
unarray:0.1.4
utf-8:0.7.6
uuid:1.23.2
vcpkg:0.2.15
wait-timeout:0.2.1
wasm-bindgen:0.2.122
wasm-bindgen-futures:0.4.72
wasm-bindgen-macro:0.2.122
wasm-bindgen-macro-support:0.2.122
wasm-bindgen-shared:0.2.122
wasmtimer:0.4.3
web-sys:0.3.99
webpki-root-certs:1.0.7
webpki-roots:0.26.11
webpki-roots:1.0.7
winnow:1.0.3
ws_stream_wasm:0.7.5
wyz:0.5.1
yoke:0.8.3
zerocopy:0.8.50
zerocopy-derive:0.8.50
```

## References

- [cargo-vet documentation](https://mozilla.github.io/cargo-vet/)
- [Mozilla's supply-chain audits](https://raw.githubusercontent.com/mozilla/supply-chain/main/audits.toml)
- [Google's supply-chain audits](https://raw.githubusercontent.com/google/supply-chain/main/audits.toml)
- Audit finding H6 in `docs/adr/ADR-022-architecture-audit-remediation.md`
