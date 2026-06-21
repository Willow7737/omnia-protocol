# Omnia Consensus — Formal Specification

**Status:** Draft (post-audit v0.1.68 — A-2 fix)
**Scope:** Consensus engine (`omnia-consensus/src/consensus.rs`) and its interaction with the causal graph (`omnia-consensus/src/causal_graph.rs`).
**Purpose:** Provide a precise, reviewable specification of the consensus algorithm sufficient for an external auditor to verify the safety and liveness arguments. TLA+ exists in `OmniaConsensus.tla`; this document gives the English-language companion spec.

---

## 1. Fault Model

- **Byzantine faults**: Up to `f` validators may behave arbitrarily (equivocate, withhold messages, sign invalid data, etc.).
- **Honest majority**: Total validators `n = 3f + 1` (strict BFT). The protocol is correct when at most `f` validators are Byzantine.
- **Network**: Partial synchrony — messages may be delayed arbitrarily between two `GST` (Global Stabilization Time) events, but eventually arrive in finite time after GST.
- **Cryptographic assumptions**: Ed25519 signatures are unforgeable; BLAKE3 is collision-resistant; BLS12-381 aggregation is sound.
- **Clock model**: Loose clock synchronization (`MAX_TIMESTAMP_DRIFT_MS = 120_000` — 2 minutes). Events outside this drift window are rejected.

---

## 2. Event DAG Structure

The consensus state is a directed acyclic graph (DAG) of **events**.

### 2.1 Event fields

| Field            | Type              | Notes                                                    |
| ---------------- | ----------------- | -------------------------------------------------------- |
| `id`             | `[u8; 32]`        | BLAKE3 hash of all fields except `signature`             |
| `creator`        | `NodeId`          | BLAKE3 hash of `creator_pubkey` (domain-separated)       |
| `sequence`       | `u64`             | Monotonic per-creator (0-indexed, genesis = 0)           |
| `timestamp`      | `u64`             | Wall-clock ms — for replay protection, not ordering      |
| `vector_clock`   | `VectorClock`     | Logical clock for happened-before relations              |
| `self_parent`    | `Option<EventId>` | Creator's previous event (`None` for genesis)            |
| `other_parent`   | `Option<EventId>` | Event received from another creator (`None` for genesis) |
| `payload`        | `Vec<u8>`         | Opaque to consensus; ≤ `MAX_PAYLOAD_SIZE` (1 MiB)        |
| `creator_pubkey` | `[u8; 32]`        | Ed25519 public key — verified to bind to `creator`       |
| `signature`      | `[u8; 64]`        | Ed25519 signature over `id`                              |

### 2.2 Edges

- **Self-edge**: `event → self_parent` (forms the creator's chain).
- **Other-edge**: `event → other_parent` (cross-creator link, forms the DAG).
- **Genesis events**: `self_parent = None ∧ other_parent = None`.

### 2.3 Invariants

- For each creator `c`, sequences are **strictly monotonic**: if event `e2` has `self_parent = Some(e1.id)`, then `e2.sequence = e1.sequence + 1`.
- The graph is acyclic: no event can be its own ancestor.
- Cycle detection uses the creator-sequence monotonicity check (O(1)) rather than BFS traversal.

---

## 3. Famousness Algorithm

Omnia uses a Hashgraph-style "famous witness" algorithm to determine consensus order.

### 3.1 Rounds

Events are partitioned into **rounds**:

- An event `e` is in round `0` if it is a genesis event or its parents are in round `0`.
- An event `e` (with parents in rounds `r_self`, `r_other`) is in round `r` where:
  ```
  r = max(r_self, r_other) + (strongly_sees_quorum(e) ? 1 : 0)
  ```
- "Strongly sees quorum": event `e` strongly sees at least `2f + 1` events in round `r - 1` (transitively, through unique paths).

### 3.2 Witnesses

An event is a **witness** for its round if it is the first event its creator created in that round.

### 3.3 Famous witnesses

A witness `w` in round `r` is **famous** if a majority of witnesses in round `r + 1` vote "yes" on it. Voting proceeds as follows:

1. Each witness in round `r + 1` votes "yes" if it strongly sees `w`, else "no".
2. If a round `r + k` witness observes that ≥ `2f + 1` round `r + k - 1` witnesses voted the same way, it copies that vote.
3. Otherwise, it votes with the majority of round `r + k - 1` witnesses it strongly sees (coin flip on tie — handled via the round seed in `ConsensusConfig`).
4. The first round where ≥ `2f + 1` witnesses vote the same way decides `w`'s fame.

---

## 4. Commitment Rule

A round `r` is **decided** when all witnesses in round `r` have their fame decided.

Once round `r` is decided:

1. Every event `e` in round `r` whose `self_parent` and `other_parent` are both famous witnesses is **committed**.
2. The committed events are placed in canonical order (deterministic topological sort by `(round, creator, sequence)`).
3. Each committed event triggers:
   - Shard routing (`ShardRouter::route_event`)
   - State finality (CRDT merge)
   - Pruning eligibility (after `pruning_depth` more rounds)

### 4.1 Finality

An event is **final** when:

- It is committed in a decided round, AND
- `commit_delay_rounds` additional rounds have been decided (default: 1).

The `commit_delay_rounds` parameter provides safety margin against reorgs in case of late-arriving events that could change witness fame.

---

## 5. Safety Argument

**Theorem (Safety)**: Two honest validators cannot commit conflicting events (events with the same `(creator, sequence)` but different content) in the same decided round.

**Sketch**:

1. **Equivocation detection**: When validator `c` creates two events `e1, e2` with the same `sequence` but different content (different `id`), the consensus engine detects this via the `first_event_for_sequence` map (keyed on `(creator, sequence)`). The second event triggers a slashing offense and is rejected from the consensus state.
2. **Pruned-metadata check (C-3 fix)**: If `e1` has been pruned by the time `e2` arrives, the engine compares `e2.content_hash()` against `e1`'s stored `PrunedEventMetadata.content_hash`:
   - Equal hashes → `e2` is a duplicate re-submission, silently dropped.
   - Different hashes → `e2` is an equivocation, slash.
3. **Quorum intersection**: Two decided rounds cannot exist where one commits `e1` and the other commits `e2` (with the same `(creator, sequence)`) because any two `2f + 1`-sized quorums intersect in at least `f + 1` validators, of which at least 1 is honest — and the honest validator would have flagged the equivocation.
4. **Deterministic ordering**: Within a decided round, the canonical order is a pure function of `(round, creator, sequence)`, so all honest validators compute the same order.

---

## 6. Liveness Argument

**Theorem (Liveness)**: After GST, all events created by honest validators are eventually committed.

**Sketch**:

1. **Gossip propagation**: After GST, every event reaches every honest validator within bounded time (gossipsub `broadcast` guarantee).
2. **Sequence buffer**: Out-of-order events are buffered in `SequenceBuffer` (per-creator, bounded by `MAX_SEQUENCE_BUFFER_PER_CREATOR = 256` and gap-limited by `MAX_SEQUENCE_GAP = 100`). When the missing predecessor arrives, the buffer drains consecutively.
3. **Round advancement**: After GST, honest validators regularly create new events (heartbeat interval), which advance rounds. Once `2f + 1` validators are creating events in round `r`, round `r + 1` witnesses strongly see them and fame is decided.
4. **Finality**: After `commit_delay_rounds + 1` rounds are decided (default: 2 rounds after the event's round), the event is final.

**Failure modes that break liveness**:

- More than `f` Byzantine validators (BFT assumption violated).
- Network partition lasting longer than `MAX_EVENT_AGE_MS` (events age out before GST).
- Per-creator buffer overflow (creator spams > 256 out-of-order events — addressed by H-4 fix).

---

## 7. Anti-Spam Mechanisms

- **Per-event Ed25519 signature**: Required in `Event::validate()` (H-13 verified — line 519 of `omnia-primitives/src/event.rs`). Unsigned events are rejected.
- **Creator-pubkey binding**: `creator = blake3_hash_domain(b"omnia-creator", creator_pubkey)` — prevents impersonation.
- **Payload size cap**: `MAX_PAYLOAD_SIZE = 1 MiB` — checked BEFORE deserialization (H-2 verified — line 225 of `shards/src/router.rs`).
- **Timestamp drift**: `MAX_TIMESTAMP_DRIFT_MS = 120_000` and `MAX_EVENT_AGE_MS = 31_536_000_000` (1 year).
- **Nonce enforcement**: Per-creator strictly-increasing nonces with gap limit (`NONCE_GAP_LIMIT`).
- **Fee burning (C-6 fix)**: Fees deducted before shard dispatch and NOT refunded on failure — anti-spam by cost.
- **Per-creator buffer cap (H-4 fix)**: LRU-bounded creator map prevents unbounded memory growth from attacker-registered NodeIds.

---

## 8. State Persistence

Consensus state is persisted to `redb` via `ConsensusStore`:

- Current round number
- `first_event_for_sequence` map (for equivocation detection)
- Event states (Pending, Committed, Rejected)
- Node info (last seen sequence, etc.)

On corrupt database (M-10 fix): the file is renamed to `.corrupt`, an ERROR is logged, and a fresh database is created. Slash history is lost; manual operator review is required before rejoining consensus.

---

## 9. References

- `omnia-consensus/src/consensus.rs` — Consensus engine implementation
- `omnia-consensus/src/causal_graph.rs` — DAG structure and pruning
- `omnia-consensus/src/slashing.rs` — Slashing engine (C-3, C-4, M-10 fixes)
- `formal-verification/OmniaConsensus.tla` — TLA+ specification
- ADR-011 — Gradual slashing model
- ADR-012 — VRF construction choice (currently deterministic hash, ECVRF migration planned)
- ADR-015 — Leader selection consensus loop
- ADR-018 — Consensus state persistence
