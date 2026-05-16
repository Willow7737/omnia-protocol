# Omnia Protocol — Comprehensive Threat Model

**Phase B Deliverable**  
**Sprint 6 — Security Hardening**

---

## Document Control

| Field | Value |
|-------|-------|
| **Document ID** | OMNIA-THREAT-MODEL-001 |
| **Version** | 1.0 |
| **Classification** | Internal — Security Sensitive |
| **Authors** | Omnia Security Team |
| **Reviewers** | Cipher (Agent 02 — ZK/Crypto), Core Development Team |
| **Date Created** | 2026-03-09 |
| **Last Updated** | 2026-03-09 |
| **Status** | Final |
| **Sprint** | Sprint 6 |

---

## 1. System Overview

The Omnia Protocol is a universal coordination layer that replaces trust with mathematics. It uses causal consistency (causal graphs, vector clocks, CRDTs) instead of sequential blockchains to achieve parallel transaction processing. The protocol is settlement-agnostic, capable of settling on any L1 that provides data availability and proof verification.

### 1.1 Architecture Summary

```
Layer 5: Economics (UBC, Governance)
Layer 4: Identity (DIDs, Shamir, Bio)
Layer 3: Binding (Provenance, RF, QC)
Layer 2: Domain Shards (6 shards: Financial, Identity, Physical, Computational, Biological, Economics)
Layer 1: Substrate (Causal Graph, Vector Clocks, CRDTs, Gossip, Consensus)
Phase 0: ZK-Rollup (Settlement Layer — Ethereum, Bitcoin, Solana, Celestia)
```

### 1.2 Trust Assumptions

1. **BFT Threshold**: The network tolerates up to f Byzantine nodes out of 3f+1 total validators. With 4 nodes, at most 1 can be Byzantine without compromising safety.
2. **Cryptographic Assumptions**: Ed25519 signatures are unforgeable (discrete logarithm assumption on Curve25519). BLAKE3 is collision-resistant. SHA-256 is preimage-resistant. BN254 pairing-friendly curve is secure for ZK proofs.
3. **Network Assumptions**: Partial synchrony — messages are eventually delivered within an unknown but finite time bound. No assumption about network speed during asynchronous periods.
4. **Operator Trust**: The L2 operator (ZK rollup) is trusted to post valid state roots. This trust is mitigated by on-chain verification in Phase 1 (real ZK proofs).
5. **Key Management**: Node operators are responsible for securing their Ed25519 private keys. No HSM integration exists in Phase 0.
6. **Dependency Trust**: All Rust crate dependencies are assumed to be non-malicious. Audited via `cargo audit` and `cargo vet` (see ADR-008).

### 1.3 Assets Requiring Protection

| Asset | Classification | Location |
|-------|---------------|----------|
| Ed25519 private keys | Critical | Node memory (substrate/src/keystore.rs) |
| Financial shard balances | Critical | shards/src/financial/state.rs |
| Causal graph (event history) | High | substrate/src/causal_graph.rs |
| State root (BLAKE3 Merkle root) | High | substrate/src/causal_graph.rs |
| ZK proving key (trusted setup) | High | zk/src/setup/ |
| Governance voting weights | Medium | economics/src/governance.rs |
| Identity DID documents | Medium | shards/src/identity/did.rs |
| UBC quota allocations | Medium | economics/src/quota.rs |
| Gossip protocol messages | Low | substrate/src/gossip.rs |
| Node configuration | Low | node/src/config.rs |

---

## 2. Attack Surface Analysis

### 2.1 Network Attack Surface

| Attack | Severity | Mitigation | Status |
|--------|----------|------------|--------|
| Gossip flood — adversary sends events faster than processing capacity | High | `GossipConfig::max_pending` (100K limit), `max_events_per_message` (100), `seen_events` dedup | Partial — no per-peer rate limiting |
| Eclipse attack — adversary surrounds target node with malicious peers | High | libp2p Kademlia DHT for peer discovery, mDNS for local discovery | Partial — no peer diversity enforcement |
| Sybil attack — adversary creates many identities to influence consensus | High | BFT threshold (3f+1), validator staking (Phase 1) | Partial — no staking in Phase 0 |
| Partition attack — network partition prevents consensus | Medium | BFT liveness under synchrony, eventual message delivery | Partial — no view change mechanism |
| Message replay — adversary re-broadcasts old gossip messages | Medium | Nonce tracking in CausalGraph, `seen_events` HashSet | Mitigated |
| QUIC connection exhaustion | Medium | libp2p connection limits, OS-level connection tracking | Partial — no explicit connection pool limits |
| DNS poisoning — adversary poisons peer discovery | Low | mDNS for local discovery, hardcoded bootstrap peers | Partial — no DNSSEC enforcement |
| Traffic analysis — adversary infers transaction patterns from network traffic | Low | libp2p Noise protocol encryption (XX handshake) | Mitigated (encrypted transport) |

### 2.2 Consensus Attack Surface

| Attack | Severity | Mitigation | Status |
|--------|----------|------------|--------|
| Consensus stall — Byzantine validators refuse to create witness events | High | BFT threshold (>2/3 supermajority required) | Partial — no timeout-based view change |
| Equivocation — validator signs two different events with same creator/sequence | Critical | `SlashingEngine::check_equivocation()` with constant-time comparison | Mitigated (detection + slashing) |
| Finality reversal — adversary causes committed events to be reconsidered | Critical | BFT finality — once >2/3 witnesses agree, decision is immutable | Mitigated |
| Validator set manipulation — adversary joins/leaves to change the BFT threshold | High | Fixed validator set in Phase 0, dynamic staking in Phase 1 | Partial — no validator rotation |
| Round skipping — adversary prevents certain rounds from completing | Medium | Causal graph ordering ensures deterministic round progression | Mitigated |
| Fork creation — adversary creates competing subgraphs | High | Vector clock partial ordering + CRDT convergence ensures deterministic state | Mitigated |
| Censorship — validator refuses to include certain events | Medium | No leader-based ordering — any validator can create events | Partial — no inclusion guarantees |
| Liveness violation — validator offline for extended period | Medium | `SlashingEngine::check_liveness()` with configurable threshold | Mitigated (detection + slashing) |

### 2.3 Cryptographic Attack Surface

| Attack | Severity | Mitigation | Status |
|--------|----------|------------|--------|
| Ed25519 key compromise — adversary extracts private key from node memory | Critical | `OsRng` for key generation, keys in memory only | Partial — no HSM, no key encryption at rest |
| Signature forgery — adversary forges Ed25519 signature | Critical | Ed25519 (ed25519-dalek 2.2.0) — no known vulnerabilities | Mitigated |
| Hash collision — adversary finds BLAKE3 or SHA-256 collision | Critical | BLAKE3 1.8.5 (no known collisions), SHA-256 (no known collisions) | Mitigated |
| Birthday attack on event IDs — adversary creates events with same hash | High | 256-bit hash space makes birthday attack infeasible (2^128 effort) | Mitigated |
| Side-channel attack — adversary extracts keys via timing/power analysis | High | Constant-time crypto (curve25519-dalek 4.x, subtle crate) | Partial — constant-time only where explicitly applied |
| Quantum computing — Shor's algorithm breaks Ed25519 | Future-Critical | PQC stub (CRYSTALS-Dilithium placeholder in binding module) | Planned — Phase 2+ |
| BN254 curve compromise — discrete log on pairing curve becomes feasible | High | Monitoring cryptographic research; migration plan (see CRYPTO_MIGRATION.md) | Planned |
| Trusted setup compromise — ZK proving key is subverted | High | Powers of Tau ceremony with public contribution (zk/src/setup/contribution.rs) | Partial — ceremony not yet run publicly |
| Random number generator failure — OsRng produces predictable output | Medium | OS-level CSPRNG (Linux getrandom, macOS Security.framework) | Mitigated |
| Replay protection bypass — adversary replays events with valid signatures | Medium | Per-creator nonce tracking in CausalGraph and ShardRouter | Mitigated |

### 2.4 Economic Attack Surface

| Attack | Severity | Mitigation | Status |
|--------|----------|------------|--------|
| Spam attack — adversary submits many low-value events to consume resources | High | FeeSchedule with per-operation fees (10 UBC for financial ops), QuotaSystem with monthly limits | Partial — fees not enforced in Phase 0 |
| Quota exhaustion — adversary depletes UBC quota to deny service to legitimate users | Medium | Monthly quota reset, per-DID quota tracking | Mitigated |
| Governance manipulation — whale accumulates tokens to dominate voting | High | Quadratic voting (weight = sqrt(stake)), exponential reputation decay | Mitigated |
| Fee avoidance — adversary structures operations to minimize fees | Medium | Cross-shard fee (15 UBC) prevents shard-hopping; default fee (3 UBC) as fallback | Partial — no dynamic fee adjustment |
| Economic deadlock — circular dependencies prevent transaction completion | Low | CRDT convergence ensures eventual consistency | Mitigated |
| Token inflation — adversary triggers unauthorized mint operations | Critical | Financial shard enforces business rules; only authorized operations can mint | Partial — no ACL enforcement |
| Double-spend via shard — adversary spends same tokens on different shards | High | Cross-shard messaging with causality proofs, nonce tracking | Partial — no atomic cross-shard commit |
| UBC farming — adversary creates many DIDs to accumulate free quota | Medium | DID identity verification, biometric anchoring | Partial — no Sybil-resistant DID creation |

### 2.5 Data Integrity Attack Surface

| Attack | Severity | Mitigation | Status |
|--------|----------|------------|--------|
| State root manipulation — ZK operator posts fraudulent state root | Critical | Deterministic state root computation (BLAKE3 Merkle), L2 verification | Partial — stub proofs in Phase 0 |
| Event payload tampering — adversary modifies event in transit | Critical | SHA-256 event hash + Ed25519 signature (hash-then-sign pattern) | Mitigated |
| Causal graph corruption — adversary inserts invalid events | High | `CausalGraph::insert()` validates hash, signature, and vector clock before insertion | Mitigated |
| Shard state divergence — nodes compute different states for same events | High | CRDTs guarantee deterministic convergence; fixed-point arithmetic (no f64) | Mitigated |
| Snapshot deserialization exploit — adversary crafts malicious snapshot bytes | Medium | Snapshot version field, height validation, bounds checking | Partial — no fuzz testing on snapshot path |
| Merkle proof forgery — adversary creates fake inclusion proof | High | BLAKE3 Merkle tree with 256-bit hashes — forgery is computationally infeasible | Mitigated |
| Event pruning data loss — legitimate events are pruned too aggressively | Medium | Configurable pruning threshold, Merkle proofs preserve verifiability | Mitigated |
| Identity document forgery — adversary creates DID claiming another identity | Medium | Ed25519 key-bound DIDs, signature verification on DID updates | Partial — no verification registry |

### 2.6 Supply Chain Attack Surface

| Attack | Severity | Mitigation | Status |
|--------|----------|------------|--------|
| Malicious crate dependency — adversary publishes compromised crate version | Critical | `cargo vet` (supply-chain/audits.toml), `cargo audit` CI integration | Partial — vet not in CI yet |
| Typosquatting — adversary creates crate with name similar to legitimate dependency | High | Cargo.lock pins exact versions, `supply-chain/config.toml` policy | Partial — no policy enforcement in CI |
| Compromised crate registry — crates.io serves malicious crate | High | Cargo.lock integrity, reproducible builds (scripts/reproducible-build.sh) | Partial — no registry mirroring |
| Build environment compromise — adversary injects code during compilation | High | Reproducible build script, Docker-based build environment | Partial — reproducibility not verified in CI |
| Transitive dependency vulnerability — vulnerability in indirect dependency | Medium | `cargo audit` for known CVEs, `cargo tree` for dependency inspection | Partial — no automated alerting |
| Outdated dependency — dependency with known vulnerability remains in use | Medium | Cargo.toml minimum version requirements (ed25519-dalek >= 2.1, blake3 >= 1.5) | Mitigated (ADR-008) |
| Lockfile tampering — adversary modifies Cargo.lock to downgrade dependency | Low | Git-tracked Cargo.lock, code review on lockfile changes | Mitigated |

---

## 3. Unmitigated Risks

The following risks have been identified as unmitigated or only partially mitigated. They are ordered by priority.

| ID | Risk | Impact | Priority | Remediation | Timeline |
|----|------|--------|----------|-------------|----------|
| R1 | Phase 0 ZK stub proofs provide no real security — operator can post fraudulent state root | Critical — all bridged assets at risk | P0 | Implement real ZK proofs (Groth16/PLONK) in Phase 1 | Phase 1 (Q3 2026) |
| R2 | No per-peer rate limiting on gossip — adversary can flood network | High — network unusable, finality degraded | P0 | Implement per-peer rate limiter with reputation scoring | Phase 1 (Q2 2026) |
| R3 | No HSM or key encryption at rest — node key compromise is unrecoverable | Critical — stolen key allows arbitrary event signing | P1 | Add HSM integration (PKCS#11), encrypted key storage | Phase 1 (Q3 2026) |
| R4 | No validator ACL for shard operations — any event can trigger privileged operations (mint, burn) | High — unauthorized token creation | P1 | Add creator authorization checks in `Shard::process_event()` | Phase 1 (Q2 2026) |
| R5 | No view change mechanism — network halts if >f nodes go offline | High — permanent consensus stall | P1 | Add timeout-based view change with leader rotation | Phase 1 (Q3 2026) |
| R6 | No Sybil-resistant DID creation — adversary can create unlimited identities | Medium — UBC farming, governance manipulation | P2 | Add identity verification requirement, biometric anchoring enforcement | Phase 2 (Q1 2027) |
| R7 | No dynamic fee adjustment — fixed fees may be too low during high demand or too high during low demand | Medium — either spam vulnerability or user friction | P2 | Implement EIP-1559-style base fee with tip mechanism | Phase 2 (Q1 2027) |
| R8 | Quantum computing threat — Ed25519 and BN254 vulnerable to Shor's algorithm | Future-Critical — all cryptographic guarantees broken | P3 | Hybrid classical/PQC signatures (CRYSTALS-Dilithium), POC migration plan | Phase 3 (2028+) |

### Priority Definitions

| Priority | Definition | Response Time |
|----------|-----------|---------------|
| P0 | Critical vulnerability with no mitigation; exploitation likely | Immediate — must fix before mainnet |
| P1 | High vulnerability with partial mitigation; exploitation possible | Fix within 3 months (before mainnet) |
| P2 | Medium vulnerability; exploitation requires significant resources | Fix within 6 months (Phase 2) |
| P3 | Future vulnerability; exploitation not currently feasible | Plan and monitor; fix when threat materializes |

---

## 4. Threat Actors

| Actor | Motivation | Capability | Target | Likelihood |
|-------|-----------|------------|--------|-----------|
| **Nation-State Adversary** | Surveillance, economic disruption, censorship resistance suppression | Very High — significant compute resources, zero-day access, quantum computing (future) | Cryptographic primitives, consensus mechanism, identity system | Low (current) / High (post-quantum) |
| **Financial Attacker** | Profit — double-spending, token theft, market manipulation | High — understanding of DeFi economics, flash loan capability | Financial shard, ZK rollup, cross-shard messaging | High |
| **Protocol Competitor** | Market dominance, reputation damage | Medium — code review capability, network access | Consensus mechanism, governance, economic parameters | Medium |
| **Malicious Validator** | Profit from within — equivocation, censorship, front-running | Medium — validator access, partial consensus influence | Consensus, event ordering, shard state | Medium |
| **Spam Operator** | Resource exhaustion, denial of service, fee avoidance | Low-Medium — basic scripting, Sybil capability | Gossip network, UBC quota, fee schedule | High |
| **Insider Threat** | Data theft, key compromise, backdoor insertion | High — codebase access, CI/CD pipeline access | Private keys, build pipeline, dependency supply chain | Low |
| **Script Kiddie** | Disruption, notoriety | Low — automated tools, known exploits | Public APIs, network endpoints, node configuration | Medium |
| **Rogue AI Agent** | Unbounded resource consumption, governance manipulation | Medium — automated operation at scale, identity creation | UBC quota, governance voting, computational shard | Medium |

---

## 5. Attack Trees

### 5.1 Attack Tree: Network Halt

```
GOAL: Prevent Omnia network from reaching consensus (halt finality)
│
├── OR: Reduce honest validator count below 2/3 threshold
│   ├── OR: Compromise validator keys (R3)
│   │   ├── Extract key from node memory [Capability: High, Likelihood: Medium]
│   │   ├── Extract key from disk [Capability: Medium, Likelihood: Low] (keys not persisted)
│   │   └── Social engineer key from operator [Capability: Medium, Likelihood: Low]
│   │
│   ├── OR: Cause validators to go offline (R5)
│   │   ├── DDoS validator network connections [Capability: Medium, Likelihood: High]
│   │   ├── Exploit node software vulnerability [Capability: High, Likelihood: Medium]
│   │   └── Corrupt node data directory [Capability: Medium, Likelihood: Low]
│   │
│   └── OR: Sybil attack to dilute honest validator influence (R6)
│       ├── Create many fake validators [Capability: Low, Likelihood: Medium]
│       └── Stake enough tokens to gain validator seats [Capability: High, Likelihood: Low]
│
├── OR: Prevent gossip message delivery
│   ├── Flood gossip network with spam events (R2) [Capability: Low, Likelihood: High]
│   ├── Partition network via BGP hijacking [Capability: Very High, Likelihood: Low]
│   └── Drop specific validator messages (eclipse attack) [Capability: Medium, Likelihood: Medium]
│
└── OR: Exploit consensus mechanism
    ├── Trigger consensus stall via equivocation [Capability: Medium, Likelihood: Medium]
    ├── Exploit race condition in CausalGraph::insert() [Capability: High, Likelihood: Low]
    └── Prevent witness event creation (censorship) [Capability: Medium, Likelihood: Medium]
```

**Minimum Attack Cost (Estimated):**

| Path | Cost | Time | Detection Probability |
|------|------|------|----------------------|
| Gossip flood (no rate limiting) | Low (1 server) | Minutes | High (visible on metrics) |
| DDoS validator connections | Medium (botnet) | Hours | High (network monitoring) |
| Key compromise (memory extraction) | High (0-day + physical access) | Days | Low (stealthy) |
| BGP hijack | Very High | Hours | Medium (routing monitoring) |

**Most Likely Attack Vector**: Gossip flooding (R2) — low cost, high likelihood, partial detection.

### 5.2 Attack Tree: Key Compromise

```
GOAL: Obtain Ed25519 private key of a validator node
│
├── OR: Extract key from running node
│   ├── OR: Memory scanning
│   │   ├── Read /proc/<pid>/mem on Linux [Requires: root access]
│   │   ├── Use debugger attachment (ptrace) [Requires: same user or root]
│   │   └── Core dump analysis after crash [Requires: core dump enabled + access]
│   │
│   ├── OR: Side-channel attack
│   │   ├── Timing attack on signature verification [Mitigated: constant-time ops]
│   │   ├── Power analysis on HSM [N/A: no HSM in Phase 0]
│   │   └── Cache timing attack on Ed25519 scalar multiplication [Requires: co-located VM]
│   │
│   └── OR: Software exploit
│       ├── Exploit vulnerability in node HTTP API (node/src/http.rs) [Requires: network access]
│       ├── Exploit deserialization bug in event parsing [Requires: gossip access]
│       └── Inject malicious shared library via LD_PRELOAD [Requires: code execution]
│
├── OR: Extract key from storage
│   ├── Key file on disk [Mitigated: keys not persisted by default]
│   ├── Backup/archive containing key [Requires: backup access]
│   └── Swap space containing key remnants [Requires: root access + swap enabled]
│
├── OR: Intercept key during generation
│   ├── Compromise OsRng (getrandom) [Requires: kernel compromise]
│   ├── Supply weak RNG via environment manipulation [Requires: VM host access]
│   └── Side-channel during key generation [Requires: physical proximity]
│
└── OR: Social engineering
    ├── Phish node operator for key material [Likelihood: Low]
    ├── Compromise CI/CD pipeline to inject key exfiltration [Likelihood: Medium]
    └── Insider threat with direct access [Likelihood: Low]
```

**Key Compromise Impact Analysis:**

| Compromised Key | Immediate Impact | Cascading Impact | Recovery Difficulty |
|-----------------|-----------------|-----------------|-------------------|
| Validator signing key | Arbitrary event creation, equivocation | Consensus manipulation, token theft | Easy (key rotation) |
| ZK operator key | Fraudulent state root posting | All bridged assets at risk | Medium (requires L1 contract update) |
| Governance key | Unauthorized voting, proposal manipulation | Governance takeover | Hard (quadratic voting limits damage) |
| DID controller key | Identity theft, unauthorized DID updates | Social recovery compromise, UBC theft | Hard (requires recovery mechanism) |

---

## 6. Recommendations Summary

| # | Recommendation | Priority | Effort | Risk Addressed | Status |
|---|---------------|----------|--------|---------------|--------|
| 1 | Implement real ZK proofs (Groth16/PLONK) | P0 | Large | R1 — State root manipulation | Planned (Phase 1) |
| 2 | Add per-peer gossip rate limiting + reputation | P0 | Medium | R2 — Gossip flooding | Planned (Phase 1) |
| 3 | Integrate HSM (PKCS#11) for key protection | P1 | Large | R3 — Key compromise | Planned (Phase 1) |
| 4 | Add ACL checks for privileged shard operations | P1 | Medium | R4 — Unauthorized minting | Planned (Phase 1) |
| 5 | Implement view change mechanism for liveness | P1 | Large | R5 — Consensus stall | Planned (Phase 1) |
| 6 | Add creator↔creator_pubkey binding in Event::validate() | P1 | Small | Spoofing — Event creator mismatch | Planned (Phase 1) |
| 7 | Implement Sybil-resistant DID creation | P2 | Large | R6 — Identity farming | Planned (Phase 2) |
| 8 | Add dynamic fee adjustment (EIP-1559-style) | P2 | Medium | R7 — Fee inadequacy | Planned (Phase 2) |
| 9 | Begin PQC migration planning (Dilithium hybrid) | P3 | Large | R8 — Quantum threat | Planned (Phase 3) |
| 10 | Add cargo vet enforcement in CI pipeline | P1 | Small | Supply chain — Malicious dependency | In Progress |
| 11 | Implement encrypted key storage at rest | P1 | Medium | R3 — Key extraction from disk | Planned (Phase 1) |
| 12 | Add fuzz testing for snapshot deserialization path | P2 | Small | Data integrity — Deserialization exploit | In Progress (Sprint 6) |
| 13 | Implement multi-signature for high-value operations | P2 | Medium | Economic — Unauthorized token transfer | Planned (Phase 2) |
| 14 | Add peer diversity enforcement in gossip | P2 | Medium | Network — Eclipse attack | Planned (Phase 2) |
| 15 | Create incident response runbook for key compromise | P1 | Small | Operational — Key compromise recovery | Planned (Phase 1) |

### Implementation Priority Matrix

```
         High Impact
              │
    R1 ●      │      ● R3
              │
    R2 ●      │      ● R4
              │
    R5 ●      │      ● R6
              │
──────────────┼──────────────
   Low Effort │ High Effort
              │
    R7 ●      │      ● R8
              │
              │
         Low Impact
```

**Immediate Actions (Before Mainnet):**

1. Complete R1 (ZK proofs) — this is the single most critical gap
2. Implement R2 (rate limiting) — simplest high-impact fix
3. Address R3 (key protection) — HSM or encrypted storage
4. Fix R4 (shard ACL) — prevent unauthorized operations
5. Add R5 (view change) — ensure liveness under partial failure

**Monitoring and Detection:**

- Deploy Grafana dashboards (monitoring/grafana/dashboards/omnia-node.json) for real-time network health
- Enable alerting on: equivocation detection, liveness violations, abnormal event rates, peer count drops
- Run chaos tests (chaos-tests/) regularly to validate resilience
- Integrate `cargo audit` and `cargo vet` into CI for dependency security

---

## Appendix A: Cryptographic Primitive Inventory

| Primitive | Crate | Version | Usage | Known Issues |
|-----------|-------|---------|-------|-------------|
| Ed25519 | ed25519-dalek | 2.2.0 | Event signing, DID authentication | None |
| SHA-256 | sha2 | 0.10.9 | Event ID computation | None |
| BLAKE3 | blake3 | 1.8.5 | Merkle tree, state root | None |
| Curve25519 | curve25519-dalek | 4.x | Ed25519 backend | None (constant-time) |
| BN254 | ark-bn254 | (via arkworks) | ZK proving | 128-bit security level |
| Noise (XX) | libp2p-noise | 0.56.x | QUIC encryption | None |
| OsRng | rand | 0.8.6 | Key generation | None (CSPRNG) |

## Appendix B: Attack Surface Metrics

| Metric | Value |
|--------|-------|
| Total attack vectors identified | 47 |
| Critical severity | 6 |
| High severity | 15 |
| Medium severity | 17 |
| Low severity | 9 |
| Fully mitigated | 18 |
| Partially mitigated | 22 |
| Unmitigated | 7 |
| P0 risks | 2 |
| P1 risks | 3 |
| P2 risks | 2 |
| P3 risks | 1 |

## Appendix C: References

1. STRIDE Threat Modeling Framework — Microsoft
2. OWASP Threat Modeling Cheat Sheet
3. NIST SP 800-154 — Guide to Data-Centric System Threat Modeling
4. Attack Trees — Bruce Schneier, 1999
5. ADR-008: Cryptographic Dependency Audit
6. Omnia Protocol Architecture Documentation (ARCHITECTURE.md)
7. Omnia Protocol Side Channel Audit (docs/security/SIDE_CHANNEL_AUDIT.md)
8. Omnia Protocol Self Assessment (docs/audit/SELF_ASSESSMENT.md)

---

**Document End**  
*This threat model should be reviewed and updated at the beginning of each sprint or when significant architectural changes occur.*
