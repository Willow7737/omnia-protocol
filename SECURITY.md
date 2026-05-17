# Security Policy

**Document version**: 4.0
**Last Updated**: 2026-05-16

## Supported Versions

The following versions of Omnia Protocol are currently being supported with security updates.

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |
| < 0.1   | :x:                |

**Note**: The current crate versions in `Cargo.toml` are `0.1.0` for both
`omnia-zk` and `omnia-binding`. Security patches are applied to the `0.1.x`
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

| Milestone | Target |
|-----------|--------|
| Acknowledgment of report | Within 48 hours |
| Initial assessment and severity classification | Within 5 business days |
| Fix development and patch release | Depends on severity (critical: 7 days, high: 14 days, medium: 30 days, low: next release) |
| Public disclosure | Coordinated with reporter after patch is available |

We are committed to keeping reporters informed throughout the process. If you do not receive an acknowledgment within 48 hours, please follow up via the same email channel.

### Scope

The following are considered in scope for vulnerability reports:

- Cryptographic implementation flaws in `zk/` or `binding/`
  - Groth16 proof soundness in `zk/src/prover.rs`
  - Poseidon hash correctness in `zk/src/poseidon.rs`
  - Trusted setup ceremony integrity in `zk/src/setup/`
  - Quantum commitment verification in `binding/src/quantum_commit.rs`
  - PQC key rotation in `binding/src/key_rotation.rs`
- Consensus or state corruption vulnerabilities in `shards/` or `substrate/`
- Authentication or authorization bypass
- Denial-of-service vectors in the protocol layer
- Data integrity violations in the provenance log or state snapshots
- Side-channel vulnerabilities in cryptographic comparison operations

The following are out of scope:

- Theoretical attacks without practical exploit demonstration
- Social engineering attacks
- Issues in third-party dependencies (report to the upstream maintainer)

## Security Review Process

All changes to the Omnia Protocol codebase are subject to security review according to the following rules:

### Mandatory Security Review

Every pull request that touches the following directories **requires** security review before merge:

- `substrate/` — Core causal graph and CRDT primitives
- `shards/` — Shard state machines and operation handlers
- `economics/` — UBC token logic and governance
- `zk/` — Zero-knowledge proof circuits, Poseidon hash, trusted setup ceremony, and verification
- `binding/` — Quantum commitments, RF fingerprinting, provenance logs, PQC key rotation

### Review Roles

| Role | Responsibility | Scope |
|------|---------------|-------|
| **Cipher** | Cryptographic correctness, key management, proof soundness, Poseidon parameters, trusted setup integrity | `zk/`, `binding/`, any PR with cryptographic changes |
| **Sentry** | Protocol integrity, state consistency, denial-of-service resistance | `substrate/`, `shards/`, `economics/` |

Cryptographic PRs (any change to `zk/`, `binding/quantum_commit.rs`, `binding/rf_fingerprint.rs`, `binding/key_rotation.rs`, or cryptographic key handling) require **both Cipher and Sentry review** — no exceptions.

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

Fuzzing is integrated into CI and runs on every merge to the main branch. New fuzz targets should be added whenever a new shard type or state format is introduced.

## Side-Channel Resistance

All cryptographic comparison operations in the substrate crate use constant-time
comparisons via the `subtle` crate. See [`docs/security/SIDE_CHANNEL_AUDIT.md`](docs/security/SIDE_CHANNEL_AUDIT.md)
for the full audit report.

**Note**: The ZK and binding crates have not yet undergone a dedicated
side-channel audit. Priority areas for future audit include:
- Poseidon hash field-element comparisons in `zk/src/poseidon.rs`
- Ed25519 and Dilithium signature verification paths in `binding/src/quantum_commit.rs`
- `PqPublicKey` comparison in `binding/src/key_rotation.rs`

## Responsible Disclosure

We follow a coordinated disclosure process:

1. **Private reporting**: Vulnerabilities are reported privately via security@omnia-protocol.org and are not disclosed publicly until a fix is available.
2. **90-day disclosure deadline**: If a fix has not been shipped within 90 days of the initial report, the reporter may disclose the vulnerability publicly. We will make every effort to ship fixes well before this deadline.
3. **Coordinated disclosure**: When a fix is ready, we coordinate with the reporter on the disclosure timeline. We prefer to publish a security advisory (CVE) alongside the patch release.
4. **Credit**: Reporters who follow responsible disclosure receive credit in our security advisories, unless they request anonymity.
5. **No legal action**: We will not pursue legal action against security researchers who act in good faith, follow our reporting process, and avoid unnecessary harm to users or systems.
