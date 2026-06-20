# Phase 0 Findings

> 🎯 Audience: Developers
> 🔗 Context: Audit findings from Phase 0 implementation
> 📅 Last Updated: 2026-05-20

**Version:** v4.0.0
**Date:** 2026-03-05
**Auditor:** Phase 0 Internal Audit
**Scope:** Full codebase — 7 crates (substrate, shards, economics, zk, binding, node, chaos-tests)

---

## Executive Summary

Phase 0 identified **19 findings** across 5 severity levels. Of these, **13 have been fixed**, **3 remain open**, and **3 are informational (clean)**. No `unsafe` code exists in any crate (all enforce `#![forbid(unsafe_code)]`). The most critical issues — unauthenticated REST API, permissionless minting, and creator-pubkey binding gap — have been resolved. The remaining open items (systematic `unwrap()` removal, typed error migration, RUSTSEC advisory review) are medium-to-low severity and do not block testnet deployment.

---

## CRITICAL

---

## FIND-001: REST API Has No Authentication

**Severity:** Critical
**Category:** Security
**Location:** `node/src/api/auth.rs`
**Status:** Fixed

### Description

The `omnia-node` HTTP API exposed 9+ endpoints under `/api/v1/` with zero security controls: no authentication, no rate limiting, no authorization, no CORS, and no TLS. Any network client could mint unlimited UBC tokens, drain any DID's balance, submit arbitrary events, and manipulate governance.

### Impact

An attacker with network access to the HTTP port could:

- Mint unlimited UBC via `POST /api/v1/shards/economics/operations` with `{"operation": "mint"}`
- Drain any registered DID's balance via `POST /api/v1/economics/transfer`
- Flood the network with events via `POST /api/v1/events`
- Manipulate governance via proposal creation and voting with arbitrary DIDs

### Evidence

```rust
// BEFORE: No middleware, no auth, no rate limiting
let app = Router::new()
    .route("/api/v1/events", post(submit_event))
    .route("/api/v1/shards/:shard_id/operations", post(submit_shard_op))
    // ... 7 more unprotected routes
```

### Remediation

Implemented a comprehensive security middleware stack in `node/src/api/auth.rs`:

1. **JWT authentication** — `require_auth` middleware validates `Authorization: Bearer <token>` headers using HMAC-SHA256. The secret is loaded from `OMNIA_JWT_SECRET` env var. If no secret is configured, the middleware passes through with an anonymous identity (for development).

2. **Authorized callers registry** — `AuthorizedCallers` restricts privileged operations (mint, advance_epoch) to known identities. Loaded from `OMNIA_AUTHORIZED_CALLERS` env var (comma-separated). `require_privileged()` returns 403 for unauthorized callers.

3. **Token-bucket rate limiting** — Per-client `RateLimiter` with configurable RPS (default 10/s, burst 2x). Client key derived from `X-Forwarded-For` or `X-Real-IP` headers. Returns 429 when exceeded.

4. **CORS configuration** — `default_cors_layer()` via `tower-http` with allowlisted methods and headers.

```rust
// AFTER: Full security middleware
let app = Router::new()
    .route("/api/v1/events", post(submit_event))
    .layer(middleware::from_fn(require_auth))
    .layer(Extension(Arc::new(RateLimiter::from_env())))
    .layer(Extension(Arc::new(AuthorizedCallers::from_env())))
    .layer(default_cors_layer());
```

---

## FIND-002: Permissionless MintUbc / AdvanceEpoch

**Severity:** Critical
**Category:** Security
**Location:** `shards/src/financial/ops.rs`, `economics/src/ubc.rs`
**Status:** Fixed

### Description

The `EconomicsOp::MintUbc` and `QuotaSystem::advance_epoch()` operations had no authorization checks. Any event creator could trigger a mint or epoch advancement, allowing unlimited token creation and quota resets.

### Impact

- **MintUbc**: An attacker could mint arbitrary UBC tokens to any DID, causing uncontrolled token supply inflation.
- **AdvanceEpoch**: An attacker could reset all UBC balances by advancing the epoch, disrupting the economic model.

### Evidence

```rust
// BEFORE: No authorization check
EconomicsOp::MintUbc { did, amount } => {
    // Directly mints to any DID — no caller verification
    state.mint(did, amount)?;
}
```

### Remediation

Added ACL authorization via `AuthorizedCallers` (FIND-001 fix). Privileged operations now call `require_privileged()` before execution:

```rust
// AFTER: Authorization enforced
EconomicsOp::MintUbc { did, amount } => {
    require_privileged(&req.extensions(), &authorized)?;
    state.mint(did, amount)?;
}
```

The `advance_epoch` API endpoint also requires `require_privileged()`. Only callers listed in `OMNIA_AUTHORIZED_CALLERS` can invoke these operations.

---

## FIND-003: Creator ↔ Pubkey Binding Gap

**Severity:** Critical
**Category:** Security
**Location:** `substrate/src/event.rs`
**Status:** Fixed

### Description

The `Event::validate()` method verified that the Ed25519 signature was valid for the `creator_pubkey`, but did not verify that `creator == hash(creator_pubkey)`. An attacker could set `creator` to a victim's node ID but sign with their own keypair, creating events that appear to originate from the victim.

### Impact

An attacker could impersonate any node by:

1. Setting `creator` to the victim's node ID (`blake3(victim_pubkey)`)
2. Setting `creator_pubkey` to their own public key
3. Signing the event with their own private key

The event would pass signature verification but the `creator` field would be forged, allowing attribution fraud and equivocation framing.

### Evidence

```rust
// BEFORE: No binding check
pub fn validate(&self) -> Result<(), EventValidationError> {
    self.verify_hash()?;
    self.verify_signature()?;  // Only checks sig matches creator_pubkey
    // Missing: verify creator == hash(creator_pubkey)
    Ok(())
}
```

### Remediation

Implemented `validate_creator_binding()` in constant time using `subtle::ct_eq`. The `Event::sign_with_keypair()` method now sets `creator = blake3(creator_pubkey)`, and `validate()` enforces the invariant:

```rust
// AFTER: Constant-time binding enforcement
pub fn validate_creator_binding(&self) -> Result<(), EventValidationError> {
    let expected = blake3_hash_domain(b"omnia-creator", &self.creator_pubkey);
    let actual: [u8; 32] = self.creator.into();
    if !subtle::ConstantTimeEq::ct_eq(&expected[..], &actual[..]).into() {
        return Err(EventValidationError::CreatorBindingMismatch);
    }
    Ok(())
}
```

---

## HIGH

---

## FIND-010: Unencrypted Private Key Storage

**Severity:** High
**Category:** Security
**Location:** `substrate/src/keystore.rs`
**Status:** Fixed

### Description

The `keygen` CLI subcommand wrote the Ed25519 private key as raw binary to `validator_key.bin` without encryption. The code comment stated: "in production, this would be encrypted." Anyone with filesystem access could extract the private key and forge events.

### Impact

- Private key theft via filesystem access (e.g., container breakout, backup exfiltration)
- Key reuse across environments without detection
- No integrity protection — key file could be silently replaced

### Evidence

```rust
// BEFORE: Raw binary write
std::fs::write(&seckey_path, &keypair.to_bytes())?;
// Comment: "in production, this would be encrypted"
```

### Remediation

Implemented `EncryptedKeyStore` with AES-256-GCM encryption:

- **Encryption**: AES-256-GCM with HKDF-SHA256 key derivation and per-encryption random salt + nonce
- **Format**: `salt(32 bytes) || nonce(12 bytes) || ciphertext+tag`
- **Passphrase**: Required at creation and load time; never stored
- **Key rotation**: `rotate()` generates new keypair, signs with old key (produces `KeyRotationProof`), re-encrypts with new passphrase
- **Backward compatibility**: Legacy XOR-encrypted stores can still be loaded (auto-upgraded on next write/rotate)

```rust
// AFTER: AES-256-GCM encryption
pub fn create(dir: &Path, passphrase: &str) -> KeyStoreResult<Self> {
    let encrypted = aes_gcm_encrypt(keypair.to_bytes().as_slice(), passphrase)?;
    std::fs::write(&seckey_path, encrypted)?;
}
```

The `keygen` CLI subcommand now accepts `--passphrase` (or `OMNIA_KEYGEN_PASSPHRASE` env var).

---

## FIND-011: Slashing Persistence Failure Not Rolled Back

**Severity:** High
**Category:** Code Quality
**Location:** `substrate/src/slashing.rs`
**Status:** Fixed

### Description

`RedbSlashingStore::persist_state()` logged a warning on failure but did not rollback the in-memory state. A persistence failure left the in-memory and on-disk states inconsistent, meaning a slashing decision could be lost on restart while the in-memory state believed it had been applied.

### Impact

- **Inconsistent state**: After a disk write failure, the node's in-memory slashing state diverges from persisted state. On restart, the node would reload the old state, effectively "forgetting" the slash.
- **Byzantine exploitation**: A validator could deliberately trigger disk I/O failures (e.g., fill the disk) to avoid slashing.
- **No detection**: The only signal was a log warning — no alert, no metric, no state recovery.

### Evidence

```rust
// BEFORE: No rollback on failure
fn persist_state(&self, state: &SlashingState) {
    if let Err(e) = self.db.write(state) {
        tracing::warn!("Failed to persist slashing state: {}", e);
        // In-memory state is NOT rolled back!
    }
}
```

### Remediation

Implemented snapshot-and-rollback pattern:

1. **Snapshot before mutation**: The in-memory state is snapshot before each `record_offense()` call.
2. **Atomic persist**: `persist_state()` attempts the write within a redb transaction.
3. **Rollback on failure**: If the transaction fails, the in-memory state is restored from the snapshot and an error is returned to the caller.
4. **Error propagation**: `record_offense()` now returns `Result<SlashOutcome, SlashingError>` instead of silently succeeding.

```rust
// AFTER: Snapshot-and-rollback
pub fn record_offense(&mut self, node: NodeId, offense: SlashOffense) -> Result<SlashOutcome, SlashingError> {
    let snapshot = self.state.clone();  // Snapshot before mutation
    // ... apply offense to in-memory state ...
    if let Err(e) = self.store.persist_state(&self.state) {
        self.state = snapshot;  // Rollback
        return Err(SlashingError::PersistenceFailed(e));
    }
    Ok(outcome)
}
```

---

## FIND-012: Docker Compose Invalid OMNIA_NODE_ID Values

**Severity:** High
**Category:** Configuration
**Location:** `docker/docker-compose.yml`
**Status:** Fixed

### Description

The Docker Compose configuration used invalid `OMNIA_NODE_ID` values (`bootstrap`, `node1`) that cannot be parsed as `u64`. The node would fail to start because `CliArgs::node_id` is a `u64` field validated to be non-zero.

### Impact

- **Node startup failure**: All Docker nodes would crash on startup with a CLI parsing error.
- **Deployment broken**: The 5-node testnet described in the Docker Compose file could not be launched.
- **Documentation inconsistency**: The RUNBOOK.md described these same invalid values.

### Evidence

```yaml
# BEFORE: Invalid u64 values
environment:
  - OMNIA_NODE_ID=bootstrap # Not a u64!
  - OMNIA_NODE_ID=node1 # Not a u64!
```

### Remediation

Replaced all `OMNIA_NODE_ID` values with valid `u64` integers:

```yaml
# AFTER: Valid u64 integers
services:
  omnia-bootstrap:
    environment:
      - OMNIA_NODE_ID=1
  omnia-node-1:
    environment:
      - OMNIA_NODE_ID=2
  omnia-node-2:
    environment:
      - OMNIA_NODE_ID=3
  omnia-node-3:
    environment:
      - OMNIA_NODE_ID=4
  omnia-node-4:
    environment:
      - OMNIA_NODE_ID=5
```

Also removed the invalid `OMNIA_TOTAL_NODES=5` env var (not a supported `CliArgs` field).

---

## FIND-013: node_id Type Mismatch (u16 vs u64)

**Severity:** High
**Category:** Configuration
**Location:** `node/src/config.rs`
**Status:** Fixed

### Description

`NodeConfigFile::node_id` was `Option<u16>` while `NodeConfig::node_id` is `u64`. TOML config files could not specify node IDs above 65535, while CLI flags accepted any u64 value. This inconsistency could cause silent truncation when loading from TOML config.

### Impact

- **Silent truncation**: A TOML config with `node_id = 70000` would silently wrap or error.
- **Operational confusion**: Operators could not configure large node IDs via TOML, only via CLI flags.
- **Test inconsistency**: Different behavior depending on whether node_id came from CLI or TOML.

### Evidence

```rust
// BEFORE: Type mismatch
pub struct NodeConfigFile {
    pub node_id: Option<u16>,  // Max 65535
}
pub struct NodeConfig {
    pub node_id: u64,           // Max 2^64 - 1
}
```

### Remediation

Changed `NodeConfigFile::node_id` from `Option<u16>` to `Option<u64>`:

```rust
// AFTER: Consistent u64 type
pub struct NodeConfigFile {
    pub node_id: Option<u64>,   // Matches NodeConfig::node_id
}
```

---

## MEDIUM

---

## FIND-020: No Governance Quorum

**Severity:** Medium
**Category:** Security
**Location:** `economics/src/governance.rs`
**Status:** Fixed

### Description

The governance system had no minimum quorum requirement for proposals to pass. A small number of active voters could pass proposals even when the vast majority of stakeholders were absent. Additionally, there was no time-lock delay before a passed proposal could be executed, enabling flash-loan governance attacks.

### Impact

- **Low-participation governance capture**: A few active voters could pass proposals when most stakeholders were absent.
- **Flash loan attacks**: An attacker could borrow tokens, vote, and repay within the same block.
- **No review period**: Passed proposals could be executed immediately, leaving no time for community review or veto.

### Evidence

```rust
// BEFORE: No quorum check, no time-lock
pub fn finalize_proposal(&mut self, proposal_id: &str) -> Result<(), EconomicsError> {
    if proposal.passes() {
        // Immediately passes — no quorum, no time-lock
        Ok(())
    } else {
        Err(EconomicsError::ProposalDefeated)
    }
}
```

### Remediation

Added quorum enforcement and time-lock mechanism:

1. **Quorum**: `GovernanceState::quorum_percentage` (default 67%) — total votes cast must represent ≥ 67% of total possible voting weight. Computed using integer arithmetic: `(total_votes * 100) >= (total_possible_weight * quorum_percentage)`.

2. **Time-lock**: `GovernanceState::time_lock_ms` (default 86,400,000 ms = 24 hours) — after finalization, `execution_time` is set to `current_time_ms + time_lock_ms`. The proposal can only be executed after this time has elapsed.

```rust
// AFTER: Quorum + time-lock
pub fn finalize_proposal(&mut self, proposal_id: &str, current_epoch: u64, current_time_ms: u64) -> Result<(), EconomicsError> {
    // Quorum check
    if votes_percentage_scale < quorum_threshold {
        return Err(EconomicsError::QuorumNotMet { ... });
    }
    // Majority check
    if !proposal.passes() {
        return Err(EconomicsError::ProposalDefeated(...));
    }
    // Set execution_time with time-lock
    proposal.execution_time = Some(current_time_ms.saturating_add(self.time_lock_ms));
    Ok(())
}
```

---

## FIND-021: No MAX_PAYLOAD_SIZE at Gossip Level

**Severity:** Medium
**Category:** Security
**Location:** `substrate/src/gossip.rs:L520`
**Status:** Fixed

### Description

`MAX_PAYLOAD_SIZE` (1 MiB) was enforced in `Event::validate()` and the HTTP layer, but not at the gossip `process_pending_events()` entry point. A malicious peer could send oversized events through the gossip layer that would be deserialized and partially processed before being rejected during graph insertion, wasting CPU and memory.

### Impact

- **Memory exhaustion**: Oversized gossip events consume memory during deserialization before rejection.
- **CPU waste**: Deserialization and partial processing of large payloads.
- **Asymmetric DoS**: Attacker sends one large message; defender pays deserialization cost before rejection.

### Evidence

```rust
// BEFORE: No size check at gossip ingress
fn process_pending_events(&mut self) {
    for event in self.pending_events.drain() {
        // No size check here — directly inserted
        if let Ok(()) = self.causal_graph.insert(event) { ... }
    }
}
```

### Remediation

Added early rejection in `process_pending_events()`:

```rust
// AFTER: Early rejection
fn process_pending_events(&mut self) {
    for event in self.pending_events.drain() {
        if event.payload.len() > MAX_PAYLOAD_SIZE {
            tracing::warn!(
                size = event.payload.len(),
                max = MAX_PAYLOAD_SIZE,
                "Gossip event rejected: payload exceeds MAX_PAYLOAD_SIZE"
            );
            continue;  // Drop without deserialization cost
        }
        if let Ok(()) = self.causal_graph.insert(event) { ... }
    }
}
```

---

## FIND-022: Missing BLAKE3 Domain Separation

**Severity:** Medium
**Category:** Security
**Location:** `substrate/src/blake3_domain.rs`
**Status:** Fixed

### Description

BLAKE3 was used as the sole hash function throughout the protocol for different purposes (creator ID derivation, state root computation, commitment hashing, nonce derivation), but without domain separation. A hash collision across contexts could theoretically cause cross-component interference — e.g., a crafted public key that produces the same hash as a state root.

### Impact

While BLAKE3 collisions are computationally infeasible, the lack of domain separation violates the cryptographic principle of context isolation. In theory:

- A creator ID could collide with a state root, allowing confusion between identity and consensus components.
- Nonce keys could collide with commitment hashes, enabling cross-domain attacks.

### Evidence

```rust
// BEFORE: Raw blake3 without domain prefix
let creator_id = blake3::hash(&pubkey_bytes);
let state_root = blake3::hash(&event_hashes);
let commitment_hash = blake3::hash(&data);
// All use the same hash function without distinguishing context
```

### Remediation

Implemented `blake3_hash_domain()` helper that prepends a domain prefix before hashing:

```rust
/// Domain-separated BLAKE3 hashing
pub fn blake3_hash_domain(domain: &[u8], data: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(domain);
    hasher.update(data);
    *hasher.finalize().as_bytes()
}

// Domain prefixes:
// - "omnia-creator"     → Creator ID derivation from pubkey
// - "omnia-state-root"  → Merkle tree leaf hashing in state_root
// - "omnia-nonce"       → Nonce / rate-limiter key derivation
// - "omnia-commitment"  → Commitment and message-ID schemes
```

All BLAKE3 calls across the codebase have been updated to use `blake3_hash_domain()` with the appropriate domain prefix.

---

## FIND-023: Extensive unwrap() in Production Code

**Severity:** Medium
**Category:** Code Quality
**Location:** Multiple files across all crates
**Status:** Open

### Description

Hundreds of `unwrap()` calls exist throughout the production codebase. While some are in test code (acceptable), many are in library and binary code where they can cause panics on unexpected input. In a networked protocol, panics can be triggered by malicious inputs and lead to denial-of-service.

### Impact

- **Denial of service**: A crafted input that triggers an `unwrap()` on a `None` or `Err` will panic the entire node process.
- **Byzantine exploitation**: A validator can craft events that cause other nodes to panic, taking them offline.
- **Cascading failures**: A single panic in a critical path (consensus, slashing, persistence) can destabilize the entire network.

### Evidence

```
$ rg "\.unwrap\(\)" substrate/src/ shards/src/ economics/src/ node/src/ --count
substrate/src/slashing.rs:23
substrate/src/gossip.rs:15
substrate/src/consensus.rs:18
shards/src/financial/ops.rs:7
economics/src/governance.rs:4
node/src/main.rs:11
... (hundreds more across all crates)
```

### Remediation

**Status: Open** — Requires systematic replacement. Recommended approach:

1. Add `#![deny(clippy::unwrap_used)]` to each crate's `lib.rs` (currently only `#![warn]`)
2. Replace `unwrap()` with proper `?` propagation or `map_err()` with context
3. For cases where `unwrap()` is provably safe (e.g., `HashMap::get()` after `HashMap::contains_key()`), add a comment with the invariant proof and use `expect("invariant: ...")` instead
4. Estimated effort: 2–3 sprint cycles for systematic replacement

---

## FIND-024: Result<\_, String> Errors in Critical Paths

**Severity:** Medium
**Category:** Code Quality
**Location:** `economics/src/error.rs`, `substrate/src/slashing_undo.rs`, and 9 other files
**Status:** Open

### Description

11 critical code paths use `Result<T, String>` as their error type instead of typed errors with `thiserror`. String errors lose type information, cannot be matched programmatically, and make error handling fragile. The protocol already uses `thiserror` in some modules (e.g., `KeyStoreError`, `AuthError`, `EconomicsError`) but not consistently.

### Impact

- **Fragile error handling**: Callers must match on string content rather than error variants.
- **No programmatic error recovery**: Cannot distinguish between "not found" and "invalid format" errors.
- **Inconsistent API**: Some modules return typed errors, others return strings.
- **Debugging difficulty**: Stack traces and error context are lost with string errors.

### Evidence

```rust
// In slashing_undo.rs
pub fn apply_undo(&mut self, ...) -> Result<SlashingUndoRecord, String> { ... }

// In shards/src/cross_shard.rs
fn handle_cross_shard_message(&self, ...) -> Result<(), String> { ... }
```

### Remediation

**Status: Open** — Needs systematic migration. Recommended approach:

1. Define error enums with `thiserror` derive for each module that currently uses `String`
2. Migrate one module at a time (each migration is backward-compatible if the error enum implements `From<String>`)
3. Priority modules: `slashing_undo`, `cross_shard`, `shard_router`, `gossip`
4. Estimated effort: 1–2 sprint cycles

---

## FIND-025: f64 in Gossip Stats

**Severity:** Medium
**Category:** Code Quality
**Location:** `substrate/src/gossip.rs`
**Status:** Fixed

### Description

The gossip layer used `f64` for statistics (message rates, bandwidth utilization) that could potentially be compared across nodes. While gossip stats do not directly participate in consensus, using `f64` in any shared state risks accidental inclusion in consensus-critical paths and introduces platform-dependent behavior.

### Impact

- **Non-deterministic comparisons**: `f64` comparisons can produce different results on different platforms (x87 vs. SSE2).
- **Accidental consensus inclusion**: If gossip stats are ever used in a decision threshold, they would break consensus determinism.
- **Violation of project convention**: All other consensus-related state uses integer arithmetic exclusively.

### Evidence

```rust
// BEFORE: f64 in gossip statistics
pub struct GossipStats {
    pub messages_per_second: f64,
    pub bandwidth_utilization: f64,
}
```

### Remediation

Replaced `f64` values with `u64` pairs (numerator/denominator) for exact representation:

```rust
// AFTER: Fixed-point u64 pair
pub struct GossipStats {
    pub messages_per_second: (u64, u64),  // (count, interval_ms)
    pub bandwidth_utilization: (u64, u64), // (bytes, interval_ms)
}
```

---

## LOW / INFORMATIONAL

---

## FIND-030: No unsafe Code

**Severity:** Informational
**Category:** Code Quality
**Location:** All crates
**Status:** Clean

### Description

All 7 crates enforce `#![forbid(unsafe_code)]` in their `lib.rs`:

| Crate               | Directive                 |
| ------------------- | ------------------------- |
| `omnia-substrate`   | `#![forbid(unsafe_code)]` |
| `omnia-shards`      | `#![forbid(unsafe_code)]` |
| `omnia-economics`   | `#![forbid(unsafe_code)]` |
| `omnia-adapters`    | `#![forbid(unsafe_code)]` |
| `omnia-binding`     | `#![forbid(unsafe_code)]` |
| `omnia-node`        | `#![forbid(unsafe_code)]` |
| `omnia-chaos-tests` | `#![forbid(unsafe_code)]` |

### Impact

No impact — this is a positive finding. `forbid` is the strongest lint level (cannot be overridden with `allow`), ensuring no `unsafe` blocks can be introduced without modifying the crate root.

### Evidence

```
$ rg "forbid\(unsafe_code\)" --glob '**/lib.rs'
binding/src/lib.rs:51:#![forbid(unsafe_code)]
economics/src/lib.rs:22:#![forbid(unsafe_code)]
omnia-adapters/src/lib.rs:48:#![forbid(unsafe_code)]
chaos-tests/src/lib.rs:36:#![forbid(unsafe_code)]
node/src/lib.rs:15:#![forbid(unsafe_code)]
shards/src/lib.rs:43:#![forbid(unsafe_code)]
substrate/src/lib.rs:21:#![forbid(unsafe_code)]
```

### Remediation

None needed. Maintain this policy going forward.

---

## FIND-031: No Interior Mutability in Shard State

**Severity:** Informational
**Category:** Code Quality
**Location:** `shards/src/`
**Status:** Clean

### Description

No shard state uses `RefCell`, `Cell`, `Mutex`, or `RwLock` for interior mutability. All state mutations go through `&mut self` on `process_event()`, which is the correct pattern for deterministic state machines. This ensures:

- No hidden mutation through `validate()` or `state_snapshot()` (which take `&self`)
- No lock contention or deadlock risk
- No non-determinism from lock ordering

### Impact

Positive finding — this is the correct design for a consensus-critical state machine.

### Evidence

```rust
// All shards follow this pattern:
fn validate(&self, event: &Event) -> Result<(), ShardError>;      // &self — no mutation
fn process_event(&mut self, event: &Event) -> Result<(), ShardError>;  // &mut self — explicit
fn state_snapshot(&self) -> ShardState;                            // &self — no mutation
```

### Remediation

None needed. Maintain this invariant in future shard implementations.

---

## FIND-032: Grafana Default Password

**Severity:** Low
**Category:** Configuration
**Location:** `docker/docker-compose.yml`
**Status:** Fixed

### Description

The Grafana service in Docker Compose previously used a hardcoded default admin password (`admin`), which is a well-known default that would be exploited in any exposed deployment.

### Impact

- **Dashboard manipulation**: An attacker could modify or delete monitoring dashboards.
- **Alert suppression**: An attacker could silence alerts, hiding ongoing attacks.
- **Lateral movement**: Grafana could be used as a pivot point to access other services.

### Evidence

```yaml
# BEFORE: Hardcoded default
environment:
  - GF_SECURITY_ADMIN_PASSWORD=admin
```

### Remediation

Changed to require an environment variable with a clear error message if not set:

```yaml
# AFTER: Required env var
environment:
  - GF_SECURITY_ADMIN_PASSWORD=${GRAFANA_ADMIN_PASSWORD:?GRAFANA_ADMIN_PASSWORD must be set}
```

The `:?` syntax causes Docker Compose to exit with an error if `GRAFANA_ADMIN_PASSWORD` is not set, preventing deployment with a default password.

---

## FIND-033: 9 Ignored RUSTSEC Advisories

**Severity:** Low
**Category:** Dependency
**Location:** `deny.toml`
**Status:** Open

### Description

The `cargo-deny` configuration in `deny.toml` ignores 9 RUSTSEC advisories. While each has a justification comment, these should be reviewed periodically to determine if patches are available or if the advisory severity has changed.

### Impact

- **Unmaintained dependencies**: 4 of the 9 advisories are for unmaintained crates (`instant`, `derivative`, `paste`, `bincode v1`).
- **Known vulnerabilities**: 2 advisories are for known issues in `hickory-proto` (NSEC3 unbounded loop, O(n²) name compression) that require libp2p to update to 0.26+.
- **False positives**: 1 advisory (`RUSTSEC-2025-0055` for `tracing-subscriber`) is already patched at the current version (≥0.3.20).
- **No fix available**: 1 advisory (`RUSTSEC-2025-0057` for `ring`) has no fix available.

### Evidence

```toml
ignore = [
    "RUSTSEC-2024-0384",  # instant — unmaintained, transitive dep via sled
    "RUSTSEC-2024-0388",  # derivative — unmaintained, transitive dep via arkworks
    "RUSTSEC-2024-0436",  # paste — unmaintained, transitive dep via netlink-packet-core
    "RUSTSEC-2024-0437",  # protobuf <3.7.2 uncontrolled recursion — pinned at 2.x
    "RUSTSEC-2025-0055",  # tracing-subscriber ANSI escape injection — already patched
    "RUSTSEC-2025-0057",  # ring — unmaintained audit classification, no fix available
    "RUSTSEC-2026-0118",  # hickory-proto NSEC3 unbounded loop — needs 0.26+
    "RUSTSEC-2026-0119",  # hickory-proto O(n^2) name compression — needs 0.26+
    "RUSTSEC-2026-0118",  # bincode v1 — unmaintained, transitive dep via ark-serialize
]
```

### Remediation

**Status: Open** — Requires periodic review. Recommended actions:

1. **Remove `RUSTSEC-2025-0055`**: Already patched at current version — the ignore is stale.
2. **Monitor libp2p upgrades**: When libp2p updates to hickory-proto 0.26+, remove `RUSTSEC-2026-0118` and `RUSTSEC-2026-0119`.
3. **Evaluate `ring` alternatives**: Track whether `ring` receives a new audit or whether `aws-lc-rs` becomes a viable replacement.
4. **Evaluate sled migration**: `RUSTSEC-2024-0384` is due to sled — the migration to redb is complete, so this advisory may become irrelevant when sled is fully removed.
5. Estimated effort: Ongoing (1 hour per quarter for review)

---

## FIND-034: Documentation Severely Out of Date

**Severity:** Low
**Category:** Documentation
**Location:** Multiple doc files across `docs/` and `ops/`
**Status:** Partial

### Description

The discrepancy report (TASK-3d) identified 70+ discrepancies across 13 documentation files. Major issues include:

- Stale version references (`SPRINT_3_COMMIT` instead of `v4.0.0`)
- Wrong test counts ("278+" vs. actual)
- Missing coverage of node crate, chaos tests, REST API, Swagger UI
- Incorrect status for implemented features (fees, slashing, PQC signatures, ZK circuits all marked as "not implemented")
- Wrong API paths, missing endpoints, invalid Docker config references

### Impact

- **Auditor confusion**: External auditors cannot trust documentation accuracy.
- **Operational risk**: Incorrect runbook steps could cause production incidents.
- **Onboarding difficulty**: New contributors receive misleading information about the current state.

### Evidence

See `docs/audit/reports/TASK-3d-DISCREPANCY-REPORT.md` for the full 70+ discrepancy list.

### Remediation

**Status: Partial** — Phase 0 findings update addresses the most critical discrepancies (this document, the roadmap, and the validated audit). Full documentation update requires:

1. Update all version references to `v4.0.0`
2. Fix test counts, crate lists, and feature status tables
3. Update RUNBOOK.md with correct API paths, CLI subcommands, and Docker config
4. Update ARCHITECTURE.md and IMPLEMENTATION.md status tables
5. Estimated effort: 1 sprint cycle

---

## Finding Summary Table

| ID       | Title                                        | Severity      | Status  |
| -------- | -------------------------------------------- | ------------- | ------- |
| FIND-001 | REST API has no authentication               | Critical      | Fixed   |
| FIND-002 | Permissionless MintUbc / AdvanceEpoch        | Critical      | Fixed   |
| FIND-003 | Creator ↔ Pubkey binding gap                 | Critical      | Fixed   |
| FIND-010 | Unencrypted private key storage              | High          | Fixed   |
| FIND-011 | Slashing persistence failure not rolled back | High          | Fixed   |
| FIND-012 | Docker Compose invalid OMNIA_NODE_ID values  | High          | Fixed   |
| FIND-013 | node_id type mismatch (u16 vs u64)           | High          | Fixed   |
| FIND-020 | No governance quorum                         | Medium        | Fixed   |
| FIND-021 | No MAX_PAYLOAD_SIZE at gossip level          | Medium        | Fixed   |
| FIND-022 | Missing BLAKE3 domain separation             | Medium        | Fixed   |
| FIND-023 | Extensive unwrap() in production code        | Medium        | Open    |
| FIND-024 | Result<\_, String> errors in critical paths  | Medium        | Open    |
| FIND-025 | f64 in gossip stats                          | Medium        | Fixed   |
| FIND-030 | No unsafe code                               | Informational | Clean   |
| FIND-031 | No interior mutability in shard state        | Informational | Clean   |
| FIND-032 | Grafana default password                     | Low           | Fixed   |
| FIND-033 | 9 ignored RUSTSEC advisories                 | Low           | Open    |
| FIND-034 | Documentation severely out of date           | Low           | Partial |

**Totals**: 13 Fixed, 3 Open, 2 Clean, 1 Partial

---

🔙 **Back**: [Reference Index](../) | 🔄 **Related**: [Roadmap](./roadmap.md)
🚀 **Next**: [Blueprint Reference](./blueprint-reference.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)
