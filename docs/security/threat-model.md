# STRIDE Threat Model for Omnia Protocol

**Task**: 6.1 — STRIDE threat model
**Date**: 2026-05-14

## Overview

This document presents a STRIDE threat model for the Omnia Protocol. STRIDE is a threat classification framework developed by Microsoft that categorizes threats into six types: **S**poofing, **T**ampering, **R**epudiation, **I**nformation Disclosure, **D**enial of Service, and **E**levation of Privilege.

For each threat category, we identify the attack vectors, assess their impact, describe current mitigations, note remaining gaps, and assign a priority level.

---

## 1. Spoofing

Spoofing attacks involve an adversary pretending to be another entity — a node, a validator, or an event creator.

### 1.1 Fake Events (Event Spoofing)

**Attack Vector**: An adversary creates events with a forged `creator` field, claiming they originated from a different node. For example, creating a `FinancialOp::Mint` event that appears to come from the treasury node.

**Impact**: HIGH. If accepted, a spoofed mint event would inflate the money supply. A spoofed transfer event would drain a victim's account.

**Current Mitigation**: Every event carries an Ed25519 signature (`event.signature`) over the event hash (`event.id`), which includes the `creator_pubkey`. The `Event::verify_signature()` method (in `substrate/src/event.rs`) checks that the signature is valid for the claimed public key. The `Event::validate()` method enforces both hash integrity and signature validity before any event enters the causal graph.

**Gap**: The system does not currently verify that the `creator` field matches the `creator_pubkey`. An attacker could set `creator = victim_node_id` but sign with their own keypair. This mismatch would pass `verify_signature()` but break the assumption that `creator` identifies the signer.

**Priority**: HIGH. Fix by adding a check in `Event::validate()` that `creator == hash(creator_pubkey)` or by removing the `creator` field and deriving it from `creator_pubkey`.

### 1.2 Fake Identities (Identity Spoofing)

**Attack Vector**: An adversary creates a DID that impersonates a real-world entity (e.g., `did:omnia:bank-of-america`).

**Impact**: MEDIUM. Could enable social engineering or unauthorized access to identity-bound resources.

**Current Mitigation**: The Identity shard (`shards/src/identity/`) uses Ed25519 keypairs bound to DIDs. A DID document includes the controller's public key, which must sign any updates.

**Gap**: No DID verification registry or certificate authority. Any node can create any DID string. This is a Phase 1+ concern (decentralized identity verification).

**Priority**: MEDIUM. Add DID namespace reservations or a verification registry in Phase 1.

### 1.3 Fake Validators (Validator Impersonation)

**Attack Vector**: An adversary joins the network claiming to be a validator node and participates in consensus, potentially influencing finality decisions.

**Impact**: HIGH. A fake validator could prevent events from reaching the >2/3 supermajority threshold, effectively blocking finality.

**Current Mitigation**: The `ConsensusConfig::total_nodes` parameter defines the validator set size. The `supermajority()` function computes the >2/3 threshold. Unknown nodes are not counted as validators.

**Gap**: No validator authentication or on-chain validator registry. Any node that joins the gossip network can create events and participate in the gossip protocol. True validator authentication requires a staking mechanism (planned for Phase 1).

**Priority**: MEDIUM. The BFT consensus tolerates up to f Byzantine nodes out of 3f+1. With 4 nodes, at most 1 can be Byzantine. Validator authentication is needed when the network grows beyond trusted participants.

---

## 2. Tampering

Tampering attacks involve unauthorized modification of data — event payloads, state roots, or shard state.

### 2.1 Event Payload Modification

**Attack Vector**: An adversary intercepts a gossip message, modifies the event payload (e.g., changing a transfer amount), and re-broadcasts it.

**Impact**: HIGH. A modified transfer amount could redirect funds or create tokens.

**Current Mitigation**: The `Event::id` field is a SHA-256 hash of all event fields (including payload and `creator_pubkey`). The `Event::compute_hash()` method (in `substrate/src/event.rs`) computes this hash, and `Event::verify_hash()` checks that it matches. The `CausalGraph::insert()` method also verifies the hash before insertion. Any modification to the payload changes the hash, which invalidates the signature and the hash check.

**Gap**: None significant. The hash-then-sign pattern provides strong tamper protection. However, the hash computation includes `creator_pubkey` but not the signature itself, which is correct (the signature covers the hash, not the other way around).

**Priority**: LOW. Current mitigation is strong.

### 2.2 State Root Manipulation

**Attack Vector**: An adversary (who controls the ZK operator) posts a fraudulent state root to L1 that does not correspond to the actual L2 state.

**Impact**: CRITICAL. If accepted by L1, a fraudulent state root would allow the operator to steal all bridged assets.

**Current Mitigation**: The `CausalGraph::state_root()` method computes a Merkle root over all event hashes using BLAKE3. This root is deterministic — all nodes with the same graph produce the same root. The `SettlementLayer::latest_state_root()` method enables L2 nodes to verify that the posted root matches their computed root.

**Gap**: Phase 0 uses hash-chain stubs (`verify_stub_proof()`) instead of real ZK proofs. There is no cryptographic proof that the state transition is valid — only a commitment. A malicious operator could post an invalid state root with a valid-looking stub proof.

**Priority**: CRITICAL. Must be addressed before mainnet by implementing real ZK proofs (Groth16/PLONK) in Phase 1.

### 2.3 Shard State Mutation Outside `process_event()`

**Attack Vector**: A bug in a shard implementation allows state mutation through `validate()` or `state_snapshot()`, violating the purity contracts.

**Impact**: MEDIUM. Would break determinism guarantees and cause consensus divergence.

**Current Mitigation**: The Rust type system enforces `&self` on `validate()` and `state_snapshot()`, preventing direct mutation. However, interior mutability (via `Cell`, `RefCell`, `AtomicU64`) could bypass this.

**Gap**: No runtime enforcement of the purity contract. Relies on code review and testing.

**Priority**: MEDIUM. Add property-based tests that verify `validate()` and `state_snapshot()` do not mutate state.

---

## 3. Repudiation

Repudiation attacks involve an entity denying that it performed an action — creating an event, casting a vote, or authorizing a transfer.

### 3.1 Denial of Event Creation

**Attack Vector**: A node creates an event, submits it to the network, and later denies having created it (e.g., to avoid accountability for a fraudulent transfer).

**Impact**: MEDIUM. Could undermine auditability and legal accountability.

**Current Mitigation**: Every event is signed with Ed25519 (`Event::sign_with_keypair()`). The signature is non-repudiable — only the holder of the private key could have produced it. The `Event::verify_signature()` method provides cryptographic proof of authorship.

**Gap**: No mechanism for key revocation or rotation. If a node's key is compromised, the node cannot repudiate events signed before the compromise was detected.

**Priority**: LOW. Key rotation is a Phase 1+ feature. The current design provides strong non-repudiation for the key's lifetime.

### 3.2 Denial of Vote Casting

**Attack Vector**: A validator votes for an event's fame (in consensus) and later denies having voted that way.

**Impact**: LOW. Consensus votes are implicit (via witness events in the causal graph), not explicit messages.

**Current Mitigation**: Consensus participation is recorded in the causal graph itself — a node's witness events and their parent relationships encode their votes. The `ConsensusEngine` determines fame based on ancestry, not explicit vote messages.

**Gap**: No explicit vote logging. This is by design (consensus is implicit in the DAG structure) but makes post-hoc vote auditing harder.

**Priority**: LOW. The DAG structure provides sufficient auditability.

---

## 4. Information Disclosure

Information disclosure attacks involve unauthorized access to sensitive data — ZK proofs, shard state, or private keys.

### 4.1 ZK Proof Leakage

**Attack Vector**: An adversary observes ZK proof data posted to L1 and extracts information about L2 transactions.

**Impact**: MEDIUM. Could reveal transaction amounts, sender/receiver identities, or shard state.

**Current Mitigation**: ZK proofs (Phase 1+) are zero-knowledge by construction — they reveal nothing about the witness (transaction data) beyond the validity of the statement. The `ProofBundle::transition_proof` field contains only the proof, not the transaction data.

**Gap**: Phase 0 uses hash-chain stubs, not real ZK proofs. The `compute_batch_commitment()` function hashes `old_root + batch_data + new_root`, and the batch data is posted to L1 for data availability. This means transaction data is visible on L1 in Phase 0.

**Priority**: HIGH. Real ZK proofs must be implemented before mainnet (Phase 1). In Phase 0, the batch data is intentionally visible for transparency.

### 4.2 Shard State Exposure

**Attack Vector**: An adversary queries `state_snapshot()` on a shard and extracts sensitive information (e.g., account balances in the Financial shard).

**Impact**: MEDIUM. Financial privacy is a user expectation; exposure could enable targeted attacks.

**Current Mitigation**: The `Shard::state_snapshot()` method is only available to the local node — it is not exposed via RPC or gossip. The state root (a 32-byte hash) is posted to L1, but the full state is not.

**Gap**: No encryption of state snapshots. If a node is compromised, the attacker has full access to shard state.

**Priority**: MEDIUM. Encrypted state snapshots are a Phase 2+ feature. Node operators should use standard OS-level security (disk encryption, access controls) in the interim.

### 4.3 Private Key Compromise

**Attack Vector**: An adversary extracts the Ed25519 private key from a node's memory or storage.

**Impact**: CRITICAL. A compromised key allows the attacker to sign arbitrary events, including minting tokens or transferring assets.

**Current Mitigation**: Keys are generated using `ed25519_dalek::SigningKey::generate(&mut OsRng)`, which uses the operating system's secure random number generator. Keys are stored in memory only (not persisted to disk by default).

**Gap**: No hardware security module (HSM) integration. No key encryption at rest. No multi-signature support for high-value operations.

**Priority**: HIGH. Add HSM support and key encryption before mainnet.

---

## 5. Denial of Service

Denial of service attacks aim to make the system unavailable or unusable.

### 5.1 Gossip Flooding

**Attack Vector**: An adversary floods the gossip network with events, consuming bandwidth and processing capacity.

**Impact**: HIGH. Could prevent legitimate events from being gossiped in a timely manner, degrading finality latency.

**Current Mitigation**: The `GossipConfig::max_pending` parameter (default: 100,000) limits the pending event queue size. The `GossipConfig::max_events_per_message` parameter (default: 100) limits the number of events per gossip message. The `seen_events` HashSet deduplicates incoming events.

**Gap**: No rate limiting on incoming gossip messages. A malicious peer could send events as fast as the network allows, filling the `pending_events` deque. No peer reputation or blacklisting system.

**Priority**: HIGH. Add per-peer rate limiting and a reputation system.

### 5.2 Consensus Stall

**Attack Vector**: A Byzantine validator refuses to create witness events, preventing the network from reaching the >2/3 supermajority threshold.

**Impact**: HIGH. Events would never achieve finality, effectively freezing the protocol.

**Current Mitigation**: The BFT consensus engine tolerates up to f Byzantine nodes out of 3f+1 total. With 4 nodes, 1 can be Byzantine without stalling consensus (3 honest nodes still reach >2/3 threshold).

**Gap**: No timeout-based progress mechanism. If 2 out of 4 nodes go offline, the remaining 2 cannot reach the threshold. No view change or leader rotation mechanism.

**Priority**: MEDIUM. Add a timeout-based view change mechanism for liveness.

### 5.3 Shard Overload

**Attack Vector**: An adversary submits many events with expensive shard operations (e.g., large batch transfers in the Financial shard) that consume excessive CPU time during `process_event()`.

**Impact**: MEDIUM. Could slow down the substrate's run loop, increasing finality latency.

**Current Mitigation**: The `Shard::process_event()` method takes `&mut self`, which means it is called sequentially. There is no timeout on individual `process_event()` calls.

**Gap**: No per-event processing timeout. No gas or fee mechanism to price expensive operations. No circuit breaker to skip events that take too long.

**Priority**: MEDIUM. Add a gas/fee mechanism in Phase 1 to price expensive operations and prevent abuse.

---

## 6. Elevation of Privilege

Elevation of privilege attacks involve an adversary gaining higher privileges than they are authorized for.

### 6.1 Validator Takeover

**Attack Vector**: An adversary gains control of a validator node (e.g., via key compromise or social engineering) and uses it to influence consensus.

**Impact**: CRITICAL. A compromised validator can create arbitrary events, vote against valid events, and influence finality decisions.

**Current Mitigation**: The BFT consensus engine tolerates up to f Byzantine validators out of 3f+1. A single compromised validator cannot independently finalize invalid events (needs >2/3 agreement).

**Gap**: No slashing mechanism. A compromised validator faces no economic penalty for misbehavior. No validator rotation or ejection mechanism.

**Priority**: HIGH. Implement slashing and validator rotation before mainnet (Phase 1).

### 6.2 Governance Manipulation

**Attack Vector**: An adversary accumulates governance tokens and uses them to pass proposals that benefit themselves (e.g., increasing their validator stake, modifying protocol parameters).

**Impact**: MEDIUM. Could centralize control of the protocol.

**Current Mitigation**: The Economics shard (`economics/src/governance.rs`) implements quadratic voting, which reduces the influence of large token holders. Each voter's voting power is the square root of their token holdings.

**Gap**: No delegation mechanism. No time-locked voting (voters cannot change their vote after a deadline). No minimum quorum for proposals.

**Priority**: MEDIUM. Strengthen governance mechanisms in Phase 1.

### 6.3 Shard-Level Privilege Escalation

**Attack Vector**: An adversary exploits a bug in a shard implementation to escalate privileges within the shard (e.g., minting tokens without authorization in the Financial shard).

**Impact**: HIGH. Could create unlimited tokens or modify account balances.

**Current Mitigation**: The `FinancialState::apply()` method enforces business rules (e.g., insufficient balance check). The `Shard::validate()` method provides pre-flight validation. The `ShardError::InvalidOperation` and `ShardError::ValidationFailed` variants catch unauthorized operations.

**Gap**: No formal access control list (ACL) for shard operations. Any event can trigger any operation — there is no check that the event creator is authorized to perform the operation (e.g., only the treasury can mint).

**Priority**: HIGH. Add ACL checks in `Shard::process_event()` that verify the event creator is authorized for the operation type.

---

## Threat Summary

| Category | Threat | Impact | Priority | Current Status |
|----------|--------|--------|----------|----------------|
| Spoofing | Fake Events | HIGH | HIGH | Mitigated by Ed25519 signatures; gap in `creator` ↔ `creator_pubkey` binding |
| Spoofing | Fake Identities | MEDIUM | MEDIUM | Mitigated by key-bound DIDs; gap in DID verification |
| Spoofing | Fake Validators | HIGH | MEDIUM | Mitigated by BFT threshold; gap in validator authentication |
| Tampering | Event Payload Mod. | HIGH | LOW | Strongly mitigated by hash-then-sign |
| Tampering | State Root Manip. | CRITICAL | CRITICAL | Phase 0 stubs provide no real security |
| Tampering | Shard State Mutation | MEDIUM | MEDIUM | Mitigated by Rust type system; gap in runtime enforcement |
| Repudiation | Denial of Event Creation | MEDIUM | LOW | Non-repudiable Ed25519 signatures |
| Repudiation | Denial of Vote Casting | LOW | LOW | Implicit consensus voting in DAG |
| Info Disclosure | ZK Proof Leakage | MEDIUM | HIGH | No real ZK in Phase 0; data visible on L1 |
| Info Disclosure | Shard State Exposure | MEDIUM | MEDIUM | No external API; gap in encryption at rest |
| Info Disclosure | Private Key Compromise | CRITICAL | HIGH | Keys in memory only; no HSM |
| DoS | Gossip Flooding | HIGH | HIGH | Queue bounds exist; no rate limiting |
| DoS | Consensus Stall | HIGH | MEDIUM | BFT threshold helps; no view change |
| DoS | Shard Overload | MEDIUM | MEDIUM | No gas/fee mechanism |
| Elevation | Validator Takeover | CRITICAL | HIGH | BFT helps; no slashing |
| Elevation | Governance Manip. | MEDIUM | MEDIUM | Quadratic voting; no delegation |
| Elevation | Shard Privilege Esc. | HIGH | HIGH | No ACL for shard operations |

## Priority Action Items

1. **CRITICAL — Phase 1 ZK proofs**: Replace hash-chain stubs with real Groth16/PLONK proofs to prevent state root manipulation.
2. **HIGH — Fix `creator` ↔ `creator_pubkey` binding**: Add a validation check in `Event::validate()`.
3. **HIGH — Per-peer rate limiting**: Add gossip rate limiting and reputation system.
4. **HIGH — Slashing mechanism**: Implement economic penalties for Byzantine validators.
5. **HIGH — Shard ACL**: Add authorization checks for privileged shard operations (minting, burning).
6. **HIGH — HSM support**: Add hardware security module integration for key protection.
