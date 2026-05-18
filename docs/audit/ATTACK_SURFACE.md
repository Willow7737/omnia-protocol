# Omnia Protocol — Attack Surface Map

**Version:** v4.0.0
**Date:** 2026-03-05
**Document version:** 2.0

---

## 1. Purpose

This document provides a comprehensive map of all attack surfaces in the Omnia Protocol. Each entry identifies the input vector, the trust boundary it crosses, the potential impact of exploitation, a severity assessment, and the current mitigation in place. Auditors should use this map to prioritize their review and to ensure no entry point is overlooked.

---

## 2. Network Inputs

All data arriving over the network is untrusted. The libp2p gossip layer is the primary ingress point for external data.

### 2.1 Libp2p Gossip Messages

**Description:** Events arrive via the gossip protocol and are deserialized into `Event` structs before being inserted into the causal graph. A malicious peer can craft arbitrary gossip messages with forged event data, oversized payloads, or malformed serialization.

**Severity:** Critical

**Impact:** A crafted gossip message could inject invalid events into the causal graph (if signature/hash verification is bypassed), cause deserialization panics (if `unwrap()` is used on parsed data), or exhaust memory via oversized messages.

**Current mitigation:**
- `Event::from_bytes()` uses `postcard::from_bytes()` which returns `Result` — no panic on malformed data
- `Event::validate()` checks hash integrity (`verify_hash()`) and signature validity (`verify_signature()`) before causal graph insertion
- `CausalGraph::insert()` re-validates the hash before inserting
- `GossipConfig::max_pending` (default: 100,000) bounds the pending event queue
- `GossipConfig::max_events_per_message` (default: 100) limits events per gossip message
- `seen_events` HashSet deduplicates incoming events

**Remaining gaps:**
- No per-peer rate limiting — a malicious peer can flood the gossip network
- No peer reputation or blacklisting system
- No maximum event payload size enforcement at the gossip layer

### 2.2 Bootstrap Peer Multiaddresses

**Description:** When a node boots, it connects to a set of bootstrap peers specified by multiaddresses. A malicious bootstrap peer could feed the node a poisoned view of the network.

**Severity:** High

**Impact:** A malicious bootstrap peer could perform an eclipse attack — isolating the node from honest peers and feeding it a fabricated event graph. This could lead the node to accept invalid events or reject valid ones.

**Current mitigation:**
- Bootstrap peers are configured statically (not discovered dynamically)
- The BFT consensus model tolerates up to f < n/3 Byzantine nodes

**Remaining gaps:**
- No peer diversity validation (e.g., ensuring bootstrap peers are in different ASes)
- No fallback bootstrap mechanism if all configured peers are unreachable
- No Sybil resistance — a malicious actor can create many identities

### 2.3 QUIC Connections

**Description:** The libp2p transport uses QUIC for inter-node communication. QUIC connection initiation and handshake are potential attack surfaces.

**Severity:** Medium

**Impact:** QUIC connection flooding could exhaust file descriptors or CPU on the receiver. TLS certificate validation issues could allow man-in-the-middle attacks.

**Current mitigation:**
- libp2p's QUIC implementation handles TLS natively
- Connection limits are delegated to libp2p's internal configuration

**Remaining gaps:**
- No explicit connection rate limiting
- No resource budgeting per QUIC connection

---

## 3. User Inputs

User-submitted data enters the protocol through event payloads, shard operations, and the REST API. These inputs are deserialized, validated, and routed to shard handlers.

### 3.1 HTTP REST API → ✅ Resolved (FIND-001)

**Description:** The `omnia-node` binary exposes a REST API via axum with 9+ endpoints under `/api/v1/`. Phase 0 (FIND-001) added JWT authentication, AuthorizedCallers ACL, per-IP rate limiting, and CORS middleware.

**Previous severity:** Critical

**Previous impact:** An attacker with network access to the HTTP port could:
- Submit unlimited events (no rate limiting, no auth)
- Mint UBC to any DID via `POST /api/v1/shards/economics/operations` with `{"operation": "mint"}`
- Spend any registered DID's UBC via `POST /api/v1/economics/transfer`
- Create governance proposals and cast votes with arbitrary DIDs
- Submit shard operations for any shard

**Current mitigations (Phase 0):**
- **JWT authentication** — `node/src/api/auth.rs` validates JWT tokens via `jsonwebtoken`; configured via `OMNIA_JWT_SECRET`
- **AuthorizedCallers ACL** — Only registered caller IDs can access the API; configured via `OMNIA_AUTHORIZED_CALLERS`
- **Rate limiting** — Per-IP token-bucket rate limiter; configured via `OMNIA_RATE_LIMIT_RPS`
- **CORS** — Enforced via `tower-http` CORS middleware
- **Admin JWT** — Privileged operations (MintUbc, AdvanceEpoch) require admin JWT; configured via `OMNIA_AUTHORIZED_ADMINS`
- Payload size check against `omnia_substrate::MAX_PAYLOAD_SIZE` (413 response if exceeded)
- Invalid hex payload returns 400

**Remaining gaps:**
- **No HTTPS** — all API traffic is plaintext (no TLS on the axum server; reverse proxy required)
- **No token revocation** — JWT tokens cannot be revoked before expiry
- **JWT secret management** — `OMNIA_JWT_SECRET` must be kept secure; rotation not automated

### 3.2 Event Payloads

**Description:** The `Event.payload` field contains an opaque byte vector that is deserialized as a `ShardPayload` by the `ShardRouter`. The payload includes a `ShardOp` enum variant with domain-specific data. Malformed or malicious payloads could exploit deserialization logic or bypass validation.

**Severity:** Critical

**Impact:** A malicious payload could trigger a deserialization panic, inject invalid shard operations (e.g., unauthorized minting), or bypass fee enforcement by crafting a payload that maps to a zero-fee operation.

**Current mitigation:**
- `ShardPayload::from_bytes()` uses `postcard::from_bytes()` with proper error handling
- `ShardRouter::route_event()` validates nonces (replay protection) and deducts fees before routing
- Each shard's `validate()` method checks operation-specific constraints before `process_event()`

**Remaining gaps:**
- `MAX_PAYLOAD_SIZE` is enforced at both the HTTP layer and gossip layer (FIND-021)
- Authorization checks on shard operations via admin JWT for privileged ops (FIND-001)
- `postcard::from_bytes()` on cross-shard message payloads processes unvalidated data

### 3.3 Shard Operations

**Description:** Each shard domain (Financial, Computational, Physical, Identity, Biological) defines its own operation types. Operations like `FinancialOp::Mint`, `IdentityOp::CreateDid`, and `ComputationalOp::SubmitTask` carry domain-specific parameters.

**Severity:** High

**Impact:** A crafted shard operation could mint unlimited tokens (if mint authorization is missing), create unauthorized identities, or corrupt shard state.

**Current mitigation:**
- Each shard's `validate()` method enforces business rules
- `FinancialState::apply()` checks for insufficient balance before transfers
- Identity operations require Ed25519 signature verification via the parent event

**Remaining gaps:**
- ACL for privileged operations is now implemented via admin JWT (FIND-001/FIND-002) — mint and advance_epoch require admin authorization
- No per-operation gas or computational limit
- `EconomicsOp::MintUbc` requires admin JWT authorization (FIND-001/FIND-002) — only admin callers can mint

### 3.4 Governance Proposals

**Description:** The governance system (`economics/src/governance.rs`) accepts proposals and votes. Quadratic voting is used to reduce the influence of large token holders.

**Severity:** Medium

**Impact:** A governance attack could pass malicious proposals (e.g., modifying consensus parameters, inflating token supply) by acquiring sufficient voting power.

**Current mitigation:**
- Quadratic voting reduces influence of large holders (voting power = √stake via `isqrt`)
- Governance decay reduces the weight of old votes over time
- Votes require registered stake in `voting_weights`

**Remaining gaps:**
- No minimum quorum for proposals to pass
- No time-locked voting (voters can change votes until the proposal closes)
- No delegation mechanism
- No proposal execution delay (time lock)

### 3.5 Fee Payments

**Description:** The `ShardRouter` deducts UBC fees from the caller's quota before processing operations. The fee amount is determined by the `FeeSchedule`.

**Severity:** Medium

**Impact:** Fee evasion could allow spam attacks. Incorrect fee calculation could overcharge or undercharge users.

**Current mitigation:**
- `FeeSchedule::fee_for_op()` maps each `ShardOp` variant to a fixed `u64` fee
- `QuotaSystem::spend()` atomically deducts from the balance, returning an error on insufficient funds
- Fees are deducted before shard dispatch (no bypass via mid-operation crash)

**Remaining gaps:**
- `FeeSchedule::zero()` exists for testing — could be accidentally used in production
- No dynamic fee adjustment based on network congestion
- No fee burning mechanism (fees are simply deducted from balance)

---

## 4. Cryptographic Boundaries

Cryptographic verification points are where asserted data becomes trusted data. Failure at any of these boundaries undermines the entire protocol.

### 4.1 Ed25519 Signature Verification

**Description:** Every event must carry a valid Ed25519 signature over its hash. Verification occurs in `Event::verify_signature()` and `QuantumCommitment::verify_ed25519()`.

**Severity:** Critical

**Impact:** If signature verification can be bypassed or forged, an attacker can inject arbitrary events into the causal graph, forge commitments, or impersonate any node.

**Current mitigation:**
- `ed25519-dalek` provides constant-time signature verification
- `Event::validate()` enforces both hash and signature checks before graph insertion
- `QuantumCommitment::verify_ed25519()` returns `false` on any deserialization or verification error
- Public key deserialization failures are logged and rejected (not panicked)

**Remaining gaps:**
- `Event::validate()` does not verify that `creator == hash(creator_pubkey)` — an attacker could set `creator` to a victim's node ID but sign with their own keypair (documented in STRIDE threat model §1.1)
- No key rotation or revocation mechanism
- No multi-signature support for high-value operations

### 4.2 Dilithium Signature Verification

**Description:** CRYSTALS-Dilithium signatures are verified in `QuantumCommitment::verify_dilithium()`. This uses the `pqc_dilithium` crate's `verify()` function.

**Severity:** Critical

**Impact:** If Dilithium verification is bypassed, post-quantum security guarantees are voided. In `Hybrid` phase, both Ed25519 and Dilithium must pass, so bypassing Dilithium alone is insufficient — but in `PostQuantum` phase, it would be catastrophic.

**Current mitigation:**
- Sprint 2 replaced the unconditional `return true` stub with real `pqc_dilithium::verify()`
- Empty signature and empty public key are explicitly rejected
- Verification failures are logged (not silently ignored)

**Remaining gaps:**
- No Dilithium signature size validation before calling `verify()` — oversized signatures could cause unexpected behavior in the `pqc_dilithium` crate
- No constant-time guarantee documented for the `pqc_dilithium` crate's `verify()` function

### 4.3 Groth16 Proof Verification

**Description:** ZK rollup proofs are verified using arkworks' Groth16 implementation on the BN254 curve. The verification occurs in `zk/src/proof.rs` and `zk/src/prover.rs`.

**Severity:** High

**Impact:** If proof verification is bypassed or the circuit is unsound, a malicious operator can post fraudulent state roots to L1, claiming invalid state transitions are valid. This would allow the operator to steal bridged assets.

**Current mitigation:**
- arkworks is a well-audited cryptographic library used in production by multiple protocols
- The `RollupCircuit` follows the standard R1CS + Groth16 pattern
- Proof verification is delegated to `ark_groth16::verify_proof()` with the verifying key and public inputs
- The `ExpandedRollupCircuit` adds Merkle path inclusion and per-event state transition constraints

**Remaining gaps:**
- The `ExpandedRollupCircuit` uses a **simplified field-addition hash** as a placeholder for a proper SNARK-friendly hash function (Pedersen or Poseidon). This means the hash constraint is not cryptographically binding.
- The basic `RollupCircuit` has only one meaningful constraint (`enforce_equal` on state roots) — it does not verify the *correctness* of the state transition
- The trusted setup is circuit-specific (not universal) — a new setup is required for every circuit change
- No proof aggregation or recursion (batch proofs are not nested)

### 4.4 BLAKE3 Hashing

**Description:** BLAKE3 is used for data hashing in `QuantumCommitment::hash_data()`, `RollupCircuitLegacy::prove_stub()`, and state root computation in `CausalGraph::state_root()`.

**Severity:** Low

**Impact:** BLAKE3 is a well-studied hash function with no known collisions. The risk is not in BLAKE3 itself but in how hashes are used (e.g., as event IDs, state roots, commitment data hashes).

**Current mitigation:**
- BLAKE3 is used as the sole hash function throughout the protocol
- State roots are computed as BLAKE3 Merkle roots over event hashes
- Data hashes in commitments are BLAKE3 of the raw data

**Remaining gaps:**
- No domain separation between different uses of BLAKE3 (event hashing vs. state root vs. commitment hashing) — a hash collision across domains could theoretically cause cross-component interference
- Event IDs use SHA-256 (via `Event::compute_hash()`) while state roots use BLAKE3 — two different hash functions increases the code complexity but not the risk

---

## 5. State Transitions

State transitions are where the protocol's invariants must hold. Incorrect transitions lead to consensus divergence, token inflation, or data corruption.

### 5.1 Causal Graph Insertion

**Description:** `CausalGraph::insert()` adds events to the DAG, verifying hash integrity, parent references, and sequence numbers.

**Severity:** Critical

**Impact:** Incorrect insertion logic could allow orphan events (no valid parents), cycle creation, or duplicate events — all of which break consensus determinism.

**Current mitigation:**
- Hash verification is performed before insertion
- Parent references must point to existing events
- Sequence numbers must be strictly increasing per creator
- `seen_events` prevents duplicate insertion

**Remaining gaps:**
- No maximum graph size limit — unbounded growth
- No garbage collection of old events
- No parent reference depth limit — deep chains could cause stack overflows during traversal

### 5.2 Consensus State Machine

**Description:** Events progress through consensus states: `pending → acknowledged → witness → famous → committed`. The `ConsensusEngine` drives these transitions based on witness counts and supermajority thresholds.

**Severity:** Critical

**Impact:** If the state machine allows invalid transitions (e.g., skipping from `pending` to `committed` without sufficient witnesses), safety is violated. If it blocks valid transitions, liveness is lost.

**Current mitigation:**
- Supermajority threshold (>2/3) is enforced for fame decisions
- The TLA+ model checker verifies `Agreement`, `NoEquivocation`, and `Validity` invariants for N=4, f=1
- Property-based tests in `substrate/tests/property_tests.rs` test consensus behavior
- The chaos testing framework (`omnia-chaos-tests`) validates safety and liveness under network partitions, node crashes, and message loss

**Remaining gaps:**
- TLA+ model is bounded (4 nodes, MaxSeq=1) — does not cover all production configurations
- No view change or leader rotation mechanism — consensus can stall if >1/3 of nodes are offline
- No formal verification of the Rust implementation against the TLA+ spec

### 5.3 Slashing State Changes

**Description:** `SlashingEngine::record_offense()` accumulates slash points and transitions a node through Warned → Slashed → Ejected states based on thresholds.

**Severity:** High

**Impact:** Incorrect slashing logic could either fail to penalize malicious validators (allowing them to continue attacking) or penalize honest validators (destroying trust and stake).

**Current mitigation:**
- Points are accumulated using `saturating_add` (no overflow)
- Thresholds are configurable and use `u64` integers (no floating-point)
- Equivocation detection compares `creator + sequence + event_id` (three-field check)
- Persistent storage via `RedbSlashingStore` (configured automatically in `omnia-node`)

**Remaining gaps:**
- **Slashing persistence is opt-in in the library API** — `SlashingEngine::new()` uses `InMemorySlashingStore`, but the `omnia-node` binary always configures redb persistence. A library user who forgets `with_store()` loses slashing state on restart.
- `persist_state()` is called after every mutation but only logs a warning on failure (does not rollback)
- No slashing event emission — other nodes are not notified when a validator is slashed
- No stake locking — the `SlashingEngine` tracks stakes but does not actually lock or confiscate them

### 5.4 Shard State Updates

**Description:** Each shard's `process_event()` method mutates the shard state. The `Shard::validate()` method provides a pre-flight check.

**Severity:** High

**Impact:** Incorrect state updates in any shard could lead to token inflation (Financial shard), identity forgery (Identity shard), or data corruption (Physical/Computational shards).

**Current mitigation:**
- `validate()` and `process_event()` are separate methods, following the command-query separation pattern
- Rust's type system enforces `&self` on `validate()` and `state_snapshot()`, preventing accidental mutation
- The Financial shard checks for insufficient balance before transfers

**Remaining gaps:**
- No atomicity guarantee across shard operations — if `process_event()` fails partway, partial state may be applied
- No state rollback mechanism
- No formal invariant checking after state updates

---

## 6. Economic Boundaries

Economic state transitions involve tokens, fees, and staking. Errors here have direct financial consequences.

### 6.1 Fee Deduction

**Severity:** Medium
**Description:** `ShardRouter::route_event()` deducts UBC fees from the caller's quota before routing.
**Current mitigation:** Atomic `QuotaSystem::spend()` with error on insufficient balance; fee deducted before dispatch.
**Remaining gaps:** No fee refund on operation failure; no dynamic fee adjustment.

### 6.2 UBC Minting

**Severity:** High
**Description:** `UbcToken::mint_monthly()` is called during epoch transitions to reset all balances to the monthly quota.
**Current mitigation:** Minting only occurs in `QuotaSystem::advance_epoch()`, which is an explicit operation.
**Remaining gaps:** No cap on total UBC supply; no governance vote required for epoch advancement; `advance_epoch()` is permissionless — any caller can trigger it.

### 6.3 Quota Spending

**Severity:** Medium
**Description:** `QuotaSystem::spend()` deducts from a DID's UBC balance.
**Current mitigation:** Returns `EconomicsError` on insufficient balance; no negative balances possible.
**Remaining gaps:** No double-spend protection across concurrent calls (single-threaded assumption).

### 6.4 Slashing Confiscation

**Severity:** High
**Description:** When a validator is slashed, their stake should be confiscated. Currently, `SlashOutcome::Slashed { amount }` reports the stake amount but does not actually transfer or lock it.
**Current mitigation:** The `SlashingEngine` tracks stakes in-memory, but confiscation is advisory only.
**Remaining gaps:** No actual fund transfer on slashing; no slashing event propagated to other nodes; no re-staking cooldown.

### 6.5 Governance Decay

**Severity:** Low
**Description:** The governance system applies decay to reduce the weight of old votes over time.
**Current mitigation:** `governance.rs` implements quadratic voting with time-based decay.
**Remaining gaps:** Decay parameters are not governance-controlled; no minimum quorum; no execution time lock.

---

## 7. Data Flows

The primary data flow through the protocol is:

```
Event (user input / API)
  → HTTP API (JWT auth + ACL + rate limit + CORS)  [FIND-001]
  → Gossip (network broadcast)
    → CausalGraph::insert() (graph storage + hash/signature verification)
      → ConsensusEngine (fame decision + finality)
        → ShardRouter::route_event() (deserialization + fee deduction + nonce check)
          → Shard::process_event() (state mutation)
            → ZK Operator (batch aggregation)
              → RollupCircuit (proof generation)
                → SettlementLayer (L1 posting)
```

Each arrow represents a trust boundary crossing. Data flows from untrusted (left) to trusted (right) as verification steps are applied. The key risk is that a verification step may be incomplete or bypassed.

---

## 8. Privilege Escalation Paths

### 8.1 Validator → Consensus Manipulation

**Severity:** Critical
**Description:** A validator who controls >1/3 of the voting power can prevent consensus from reaching finality (liveness attack). A validator who controls >2/3 can finalize arbitrary events (safety attack).
**Current mitigation:** BFT model requires 3f+1 nodes to tolerate f Byzantine; supermajority threshold (>2/3) for finality.
**Remaining gaps:** No Sybil resistance — an attacker can create many validator identities. No stake-weighted validation. No validator rotation.

### 8.2 Shard Operator → State Corruption

**Severity:** High
**Description:** A shard operator (or any event creator) can submit operations that mutate shard state. Without authorization checks, a malicious operator can mint tokens, modify balances, or corrupt identity state.
**Current mitigation:** Business rule validation in each shard's `validate()` method; signature verification on parent events.
**Remaining gaps:** ACL for privileged operations is implemented via admin JWT (FIND-001/FIND-002); `EconomicsOp::MintUbc` requires admin authorization; `AdvanceEpoch` requires admin authorization.

### 8.3 API Client → Arbitrary Operations → ✅ Resolved (FIND-001)

**Previous severity:** Critical
**Description:** Any network client that could reach the HTTP API was able to submit arbitrary events, mint UBC, create governance proposals, and perform shard operations without authentication or authorization.
**Current mitigations (Phase 0):** JWT authentication, AuthorizedCallers ACL, per-IP rate limiting, admin JWT for privileged operations (FIND-001).
**Remaining gaps:** No TLS on the node itself (reverse proxy needed); no JWT token revocation; JWT secret must be kept secure.

---

## 9. Key Management (NEW)

### 9.1 Unencrypted Private Keys → ✅ Resolved (FIND-010)

**Previous severity:** High
**Description:** The `keygen` CLI subcommand previously wrote the Ed25519 private key as raw binary to `validator_key.bin` without encryption.
**Current mitigations (Phase 0):**
- With `--passphrase` (or `OMNIA_KEYGEN_PASSPHRASE` env var), the private key is encrypted with AES-256-GCM using a key derived from the passphrase via BLAKE3 domain-separated key derivation, and saved as `validator_key.enc`.
- The `EncryptedKeyStore` module (`substrate/src/keystore.rs`, 856 lines) provides encrypted key storage with AES-256-GCM + HKDF-SHA256.
- The `load_encrypted_key()` function decrypts keys for runtime use.
**Remaining gaps:** Without `--passphrase`, unencrypted key output is still the default (with prominent warning). No HSM integration. No automatic file permission enforcement.

### 9.2 Trusted Setup Ceremony

**Severity:** High
**Description:** The `setup-contribute` and `setup-verify` CLI subcommands manage the Powers of Tau trusted setup ceremony for Groth16 ZK proofs. A compromised ceremony allows forging proofs.
**Current mitigation:** The ceremony supports multiple participants and transcript verification.
**Remaining gaps:** No multi-party network coordination (ceremony is local simulation only); no audit trail for ceremony participants; deterministic seed option (`--seed`) could be misused to compromise the ceremony.

---

## 10. Data Persistence (NEW)

### 10.1 Redb Database Reliability

**Severity:** Low
**Description:** Both `RedbSlashingStore` and `RedbNonceStore` use redb as their embedded database. redb provides ACID transactions, crash-safe durability, and a simple single-file database format with forward compatibility guarantees. The previous sled 0.34 alpha-quality dependency has been replaced.
**Current mitigation:** Both stores are configured with explicit data directories for persistence. redb uses a write-ahead log (WAL) for crash safety.
**Remaining gaps:** No automated backup or replication of database files; no database repair tooling.

### 10.2 Nonce Store Persistence

**Severity:** Medium
**Description:** `RedbNonceStore` provides persistent replay protection across restarts. If the nonce database is corrupted or deleted, replay protection is lost.
**Current mitigation:** `create_shard_router()` in `main.rs` creates a `RedbNonceStore` when `nonce_data_dir` is provided (the default in `omnia-node`).
**Remaining gaps:** No integrity check on nonce database startup; no backup mechanism; no recovery procedure if nonce database is corrupted.

---

## 11. Summary

| Attack Surface | Severity | Critical Gaps |
|---|---|---|
| Gossip message injection | Medium | Payload size limit enforced (FIND-021); no rate limiting |
| Bootstrap peer eclipse | High | No peer diversity checks |
| **HTTP REST API** | **Medium** | **JWT auth + ACL + rate limiting (FIND-001); no TLS, no token revocation** |
| Event payload deserialization | Medium | Authorization ACL via admin JWT for privileged ops (FIND-001) |
| Ed25519 signature bypass | Medium | `creator` ↔ `creator_pubkey` binding with constant-time validation (FIND-003) |
| Dilithium verification | Critical | No constant-time guarantee |
| Groth16 proof soundness | Medium | Poseidon hash with BLAKE3-derived round constants (needs audit vs Filecoin/Neptune) |
| Causal graph insertion | Critical | No graph size limit, no GC |
| Consensus state machine | Critical | No view change, bounded TLA+ model |
| Slashing enforcement | Medium | Snapshot-and-rollback (FIND-011); no fund confiscation |
| UBC minting | Medium | Admin JWT required for mint + epoch advance (FIND-001/FIND-002) |
| Shard state mutation | High | No atomicity, no rollback |
| Validator Sybil attack | Critical | No Sybil resistance, no staking |
| **Encrypted key files** | **Medium** | **AES-256-GCM with --passphrase (FIND-010); unencrypted default without --passphrase** |
| **Trusted setup ceremony** | **High** | **No multi-party coordination, deterministic seed risk** |
| **redb persistence** | **Low** | **Production-quality redb with ACID transactions (replaced sled)** |
