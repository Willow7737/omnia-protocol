# Security Policy

> 🎯 Audience: Operators, Architects
> 🔗 Context: Vulnerability disclosure policy, security review process, and threat model references
> 📅 Last Updated: 2026-06-24

**Document version**: 5.0
**Last Updated**: 2026-06-24

## Supported Versions

The following versions of Omnia Protocol are currently being supported with security updates.

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |
| < 0.1   | :x:                |

**Note**: The current crate versions in `Cargo.toml` are `0.1.0` for both
`omnia-adapters` and `omnia-binding`. Security patches are applied to the `0.1.x`
line until a stable `1.0.0` release.

## Reporting a Vulnerability

We take the security of Omnia Protocol seriously. If you believe you have found a security vulnerability, please report it to us by following these steps:

1. **Do not** open a public GitHub issue or disclose the vulnerability in any public forum.
2. Send an email to **security@omnia-protocol.org** with a detailed description of the vulnerability.
3. Include the following information where possible:
   - The affected component(s) (e.g., substrate, shards, economics, zk, binding)
   - Steps to reproduce the issue
   - The potential impact and severity (e.g., data loss, unauthorized access, consensus failure)
   - Any proof-of-concept code or exploit details
   - Your preferred contact method for follow-up

### Response Timeline

| Milestone                                      | Target                                                                                    |
| ---------------------------------------------- | ----------------------------------------------------------------------------------------- |
| Acknowledgment of report                       | Within 48 hours                                                                           |
| Initial assessment and severity classification | Within 5 business days                                                                    |
| Fix development and patch release              | Depends on severity (critical: 7 days, high: 14 days, medium: 30 days, low: next release) |
| Public disclosure                              | Coordinated with reporter after patch is available                                        |

We are committed to keeping reporters informed throughout the process. If you do not receive an acknowledgment within 48 hours, please follow up via the same email channel.

### Scope

The following are considered in scope for vulnerability reports:

- Cryptographic implementation flaws in `omnia-adapters/` or `binding/`
  - Groth16 proof soundness in `omnia-adapters/src/prover.rs`
  - Poseidon hash correctness in `omnia-adapters/src/poseidon.rs`
  - Trusted setup ceremony integrity in `omnia-adapters/src/setup/`
  - Quantum commitment verification in `binding/src/quantum_commit.rs`
  - PQC key rotation in `binding/src/key_rotation.rs`
- Consensus or state corruption vulnerabilities in `shards/`, `omnia-consensus/`, or `substrate/`
- Authentication or authorization bypass
- Denial-of-service vectors in the protocol layer
- Data integrity violations in the provenance log or state snapshots
- Side-channel vulnerabilities in cryptographic comparison operations
- **Newly hardened surfaces (v0.1.69)** — bounty researchers should re-examine:
  - Identity recovery with `secret_commitment` (`shards/src/identity/state.rs`)
  - Biological ZK with non-empty `public_inputs` (`shards/src/biological/state.rs`)
  - Cross-shard causal proof verification (`shards/src/router.rs`)
  - Economics `verifier_pubkey` fail-closed (`economics/src/economics_shard.rs`)
  - Ethereum `verify_proof_with_root` (`omnia-adapters/src/settlement/ethereum/mod.rs`)
  - Per-client rate limiting (`node/src/api/auth.rs`)
  - Persistent node keypair (`node/src/main.rs`)

The following are out of scope:

- Theoretical attacks without practical exploit demonstration
- Social engineering attacks
- Issues in third-party dependencies (report to the upstream maintainer)

## v0.1.69 Critical Security Hardening (2026-06-22)

A comprehensive audit of the codebase identified 16 critical security
vulnerabilities. All 16 were remediated in commit `5d3d776` (2026-06-22)
and pushed to the `dev` branch. Below is a summary of each fix with the
affected file and the attack it closes.

### Cryptographic Security Fixes

| # | Fix | File | Attack Closed |
|---|-----|------|---------------|
| 1 | Phase 2 ceremony fail-closed | `omnia-adapters/src/setup/circuit_setup.rs` | `derive_keys_deterministic_from_srs` derived toxic waste from the PUBLIC SRS transcript — anyone with the transcript could forge proofs. Now fails-closed unless `unsafe-phase2-deterministic` feature is enabled. |
| 2 | Identity recovery secret commitment | `shards/src/identity/state.rs` | `RecoverDid` accepted forged shares — never compared the reconstructed secret to a stored commitment. Added `RecoveryConfig.secret_commitment` field, verified before key rotation. |
| 3 | Biological ZK non-empty public inputs | `shards/src/biological/state.rs` | ZK verification accepted attacker-supplied VerifyingKey with empty `public_inputs`. Now derives non-empty inputs from `(subject, consumer)` via BLAKE3→BN254-Fr, rejects empty inputs, enforces consent expiry. |
| 4 | Cross-shard causal proof verification | `shards/src/router.rs` | `route_cross_shard` ignored `msg.causal_proof` entirely. Now requires non-empty `causal_proof` and verifies it happened-before the event's vector clock. |
| 5 | Nonce store fail-closed | `shards/src/router.rs` | `with_nonce_store` silently reset replay protection to empty on load failure. Now panics loudly; added `with_nonce_store_checked` returning `Result`. |
| 6 | Economics verifier pubkey required | `economics/src/economics_shard.rs` | `verifier_pubkey.unwrap_or([0u8; 32])` allowed forged work proofs (all-zeros pubkey has a known secret key). Now returns `Unauthorized` error when unset. |
| 7 | Ethereum verify_proof_with_root | `omnia-adapters/src/settlement/ethereum/mod.rs` | `verify_proof` extracted `batch_merkle_root` from the prover's own proof bytes. Added `verify_proof_with_root` requiring the root as a trusted on-chain parameter. |

### Node Infrastructure Fixes

| # | Fix | File | Attack Closed |
|---|-----|------|---------------|
| 8 | Per-client rate limiting | `node/src/main.rs` | `axum::serve` was called without `into_make_service_with_connect_info`, so all clients shared one rate-limit bucket. Now injects `ConnectInfo<SocketAddr>` for per-client rate limiting. |
| 9 | /readyz peer tracking | `node/src/main.rs`, `omnia-network/src/gossip.rs` | `state.peers` was never populated — `/readyz` always returned 503. Added `GossipProtocol::connected_peer_count()` and a peer-refresh step in the consensus loop. |
| 10 | Validator registration | `node/src/main.rs` | `validator_candidates` was never populated — the node could never be elected leader. Now calls `substrate.add_validator()` at startup. |
| 11 | Unified EconomicsState | `node/src/api/shards.rs` | `/shards/economics/operations` applied mints to the EconomicsShard's internal state, invisible to `/economics/balance`. Now applies directly to the shared `AppState.economics` instance. |
| 12 | Shard ops no longer bypass consensus | `node/src/api/shards.rs` | Previous handler created `Event::genesis` with EMPTY payload and called `router.route()` — events never entered the causal graph. Now applies directly to shared state. |
| 13 | Helm chart TCP/UDP fix | `helm/omnia-node/` | `listenAddr` used TCP 9090 (should be UDP QUIC 4001); container/service only exposed TCP; probe paths/ports mismatched Dockerfile. All fixed. |
| 14 | Substrate fail-closed persistence | `substrate/src/lib.rs` | `Substrate::new` silently fell back to in-memory slashing/consensus on persistence failure. Now panics loudly with actionable error messages. |
| 15 | Persistent node keypair | `node/src/main.rs` | Always generated an ephemeral keypair at startup, breaking identity continuity. Added `load_or_generate_node_keypair` loading from `OMNIA_NODE_KEY_FILE` or `data_dir/node_key.bin`. |
| 16 | Genesis hex validation | `substrate/src/genesis.rs` | `hex::decode(...).unwrap_or_default()` silently returned empty Vec on malformed keys. Now propagates `GenesisError::InvalidPublicKey`. |

### Post-Fix Verification

All fixes were verified with:
- `cargo check --workspace`: clean compilation
- `cargo test -p omnia-shards`: 180 tests pass
- `cargo test -p omnia-economics`: 36 tests pass
- `cargo test -p omnia-substrate --features network`: 89 tests pass
- `cargo test -p omnia-node --features full`: 85 tests pass
- `cargo test -p omnia-adapters --features arkworks`: 142 tests pass
- `cargo clippy --workspace -- -D warnings`: clean
- `cargo audit` (with CI ignore list): exit 0

## Security Review Process

All changes to the Omnia Protocol codebase are subject to security review according to the following rules:

### Mandatory Security Review

Every pull request that touches the following directories **requires** security review before merge:

- `omnia-primitives/` — Core types (Event, VectorClock, wire format)
- `omnia-crypto/` — Cryptographic primitives (Ed25519, BLS12-381, AES-256-GCM, keystore, VRF, threshold)
- `omnia-consensus/` — Consensus engine, DAG, CRDTs, slashing, event pool
- `omnia-network/` — P2P networking (libp2p, gossipsub, Kademlia, fast-sync)
- `omnia-adapters/` — Zero-knowledge proof circuits, Poseidon hash, trusted setup ceremony, settlement adapters
- `binding/` — Quantum commitments, RF fingerprinting, provenance logs, PQC key rotation
- `shards/` — Shard state machines and operation handlers
- `economics/` — UBC token logic and governance
- `substrate/` — Integration facade (re-exports + genesis/snapshot/migration)
- `node/` — Node binary (HTTP API, auth, rate limiting, consensus loop)

### Review Roles

| Role       | Responsibility                                                                                           | Scope                                                            |
| ---------- | -------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------- |
| **Cipher** | Cryptographic correctness, key management, proof soundness, Poseidon parameters, trusted setup integrity | `omnia-adapters/`, `binding/`, any PR with cryptographic changes |
| **Sentry** | Protocol integrity, state consistency, denial-of-service resistance                                      | `substrate/`, `shards/`, `economics/`                            |

Cryptographic PRs (any change to `omnia-adapters/`, `binding/quantum_commit.rs`, `binding/rf_fingerprint.rs`, `binding/key_rotation.rs`, or cryptographic key handling) require **both Cipher and Sentry review** — no exceptions.

### Review Rules

1. **No PR merges with unresolved security comments.** Any review comment tagged `security` or `sec-review` is blocking. It must be resolved — either by code change or by explicit sign-off from the reviewer — before the PR can merge.
2. Security reviewers have 48 hours to provide initial feedback after a review request. If no response is received, the PR author may request a second reviewer.
3. Emergency patches (critical severity) follow an expedited process: a single Cipher or Sentry approval is sufficient for initial merge, with full review completed retroactively within 72 hours.

### Weekly Security Report

The Sentry role produces a weekly security report covering:

- Number of security reviews completed
- Outstanding security review requests (age in days)
- Vulnerabilities discovered (severity, status, resolution)
- Dependency audit updates
- Any security incidents or near-misses

Report format is maintained in `docs/security/weekly-reports/`.

## Threat Model

A detailed threat model for Omnia Protocol is maintained at:

- [`docs/security/THREAT_MODEL.md`](docs/security/THREAT_MODEL.md) — Structured attack surface inventory, unmitigated risks, and STRIDE threat classification

The document covers:

- Adversary capabilities and attack surfaces
- Trust assumptions for the causal graph, shard state, and binding layers
- Cryptographic assumptions and fallback postures
- Supply-chain integrity and dependency risks
- ZK proof system security (trusted setup, Poseidon hash, Groth16 soundness)
- Binding layer threats (RF fingerprinting bypass, quantum commitment forgery, PQC key rotation attacks)

All security reviews should reference both threat model documents when evaluating potential impact.

## Fuzzing

Omnia Protocol employs fuzz testing to uncover edge cases in serialization, deserialization, and state transitions. Fuzz targets are maintained in the [`fuzz/`](fuzz/) directory and cover:

- `from_bytes()` deserialization for all shard state types (including version-byte validation)
- Operation application via `apply()` with arbitrary inputs
- Provenance chain construction and verification
- Zero-knowledge circuit edge cases (including `fuzz_zk_proof_deserialization`)
- Vector clock merge operations
- Gossip message deserialization
- Consensus state transitions
- Rate limiter behavior
- Causal graph insertion (out-of-order events)
- Event validation (signature, timestamp, payload size)
- Shard routing (nonce, fee, replay protection)
- Raw vector clock binary format parsing

12 fuzz targets are maintained in the [`fuzz/`](fuzz/) directory. Fuzzing is
integrated into CI via `scripts/fuzz.sh` (runs 7 of 12 targets — the
remaining 4 are invoked manually). New fuzz targets should be added
whenever a new shard type or state format is introduced.

## Side-Channel Resistance

All cryptographic comparison operations in the substrate crate use constant-time
comparisons via the `subtle` crate. See [`docs/security/SIDE_CHANNEL_AUDIT.md`](docs/security/SIDE_CHANNEL_AUDIT.md)
for the full audit report covering the substrate crate.

The ZK and binding crates have been audited for side-channel resistance in Phase 5.
See [`docs/security/SIDE_CHANNEL_AUDIT_ZK_BINDING.md`](docs/security/SIDE_CHANNEL_AUDIT_ZK_BINDING.md)
for the full audit report covering:

- Poseidon hash field-element operations in `omnia-adapters/src/poseidon.rs`
- Ed25519 and Dilithium signature verification paths in `binding/src/quantum_commit.rs`
- ML-KEM key encapsulation operations in `binding/src/quantum_commit.rs`
- Trusted setup contribution operations in `omnia-adapters/src/setup/contribution.rs`

**Remaining concern**: The `pqc-dilithium` crate has not been formally audited for timing
side-channels. Monitor upstream updates and consider switching to a formally verified
implementation (e.g., `liboqs` bindings) for mainnet.

**v0.1.69 hardening**: The biological shard's ZK proof verification was
hardened against an attacker-supplied VerifyingKey attack. Public inputs
are now derived from `(subject, consumer)` via BLAKE3→BN254-Fr, preventing
proof reuse across different consent records. The economics verifier
now requires a configured `verifier_pubkey`, closing the all-zeros pubkey
forgery. The identity recovery path now verifies a `secret_commitment`
before rotating keys, closing the forged-shares attack.

## Bug Bounty Program

Omnia Protocol operates a bug bounty program that rewards security researchers for
responsible disclosure of vulnerabilities. See [`SECURITY_BOUNTY.md`](SECURITY_BOUNTY.md)
for full details including scope, reward tiers, and reporting guidelines.

**Reward range**: $100 – $50,000 depending on severity.

## Responsible Disclosure

We follow a coordinated disclosure process:

1. **Private reporting**: Vulnerabilities are reported privately via security@omnia-protocol.org and are not disclosed publicly until a fix is available.
2. **90-day disclosure deadline**: If a fix has not been shipped within 90 days of the initial report, the reporter may disclose the vulnerability publicly. We will make every effort to ship fixes well before this deadline.
3. **Coordinated disclosure**: When a fix is ready, we coordinate with the reporter on the disclosure timeline. We prefer to publish a security advisory (CVE) alongside the patch release.
4. **Credit**: Reporters who follow responsible disclosure receive credit in our security advisories, unless they request anonymity.
5. **No legal action**: We will not pursue legal action against security researchers who act in good faith, follow our reporting process, and avoid unnecessary harm to users or systems.

---

🔙 **Back**: [README.md](./README.md) | 🔄 **Related**: [docs/reference/security-audit.md](./docs/reference/security-audit.md)  
🚀 **Next**: [SECURITY_BOUNTY.md](./SECURITY_BOUNTY.md) | 📜 **Source of Truth**: [Restructuring Blueprint](./docs/reference/blueprint-reference.md)
