# ADR-006: Shard Trait Contract

**Status**: Proposed
**Date**: 2026-05-14
**Version**: 4.0.0
**Decision**: Define a `Shard` trait with formal contracts for determinism, purity of validation, and reproducible state snapshots, ensuring that all domain shards behave predictably under the substrate's multi-threaded execution model.

## Context

Domain shards are specialized state machines that process events from the causal graph. Each shard (Financial, Identity, Computational, Physical, Biological, Economics) maintains its own state and validation rules, but all shards share the same consensus and causal ordering from Layer 1.

The `Shard` trait (defined in `shards/src/shard.rs`) is the interface that every domain shard must implement. It is used by the `ShardRouter` (`shards/src/router.rs`) to dispatch events to the appropriate shard. The trait must enforce several formal contracts:

1. **Determinism**: Given the same event and the same initial state, `process_event()` must produce the same result every time. This is essential for consensus — all honest nodes must agree on shard state.
2. **Purity of validation**: `validate()` must be a pure function — no side effects, no state mutation.
3. **Reproducible snapshots**: `state_snapshot()` must be byte-for-byte reproducible for the same state.
4. **Thread safety**: `Send + Sync` bounds are required because the substrate runs in a multi-threaded Tokio runtime.

The Financial shard (`shards/src/financial/`) is the most critical shard for Phase 0 and uses strict causal ordering — not CRDTs — for balance updates. This decision has implications for the trait contract.

## Decision

### Trait Definition

The `Shard` trait is defined exactly as follows in `shards/src/shard.rs`:

```rust
pub trait Shard: Send + Sync {
    fn shard_id(&self) -> ShardId;
    fn process_event(&mut self, event: &Event, op: ShardOp) -> Result<(), ShardError>;
    fn state_snapshot(&self) -> Vec<u8>;
    fn validate(&self, op: &ShardOp) -> Result<(), ShardError>;
}
```

The `ShardId` is a 32-byte identifier with well-known constructors for each shard type. The `ShardOp` enum is a union over all domain operations plus a `CrossShard` variant. The `Event` type provides context (creator, vector clock, signature) from the substrate layer.

### The Six Implementations

Six concrete types implement the `Shard` trait, each defined in `shards/src/lib.rs`:

| Implementation | `shard_id()` | State Type | Source File |
|---------------|--------------|------------|-------------|
| `FinancialShard` | `ShardId::financial()` (`"FINANCE0"`) | `FinancialState` | `shards/src/lib.rs` |
| `ComputationalShard` | `ShardId::computational()` (`"COMP___0"`) | `ComputationalState` | `shards/src/lib.rs` |
| `PhysicalShard` | `ShardId::physical()` (`"PHYS___0"`) | `PhysicalState` | `shards/src/lib.rs` |
| `BiologicalShard` | `ShardId::biological()` (`"BIO____0"`) | `BiologicalState` | `shards/src/lib.rs` |
| `IdentityShard` | `ShardId::identity()` (`"IDENT__0"`) | `IdentityState` | `shards/src/lib.rs` |
| `EconomicsShard` | `ShardId::economics()` (`"ECONOM00"`) | `EconomicsShardState` | `shards/src/lib.rs` |

Each shard's `process_event()` implementation validates the operation before applying it, delegates to the domain-specific validator and state, and returns `ShardError` on failure. The `state_snapshot()` method serializes state via `bincode::serialize()` (some shards prefix a version byte).

### Formal Contracts

#### 1. `process_event()` Must Be Deterministic

**Contract**: Given the same event and the same initial shard state, `process_event()` must produce the same resulting state and the same return value on every invocation.

**Rationale**: The Omnia consensus engine requires that all honest nodes reach the same shard state after processing the same sequence of committed events. If `process_event()` were non-deterministic (e.g., using random numbers, thread-local state, or wall-clock time), different nodes would diverge, breaking consensus.

**Implications for implementations**:
- No use of `rand::random()` or `SystemTime::now()` inside `process_event()`.
- No use of `HashMap` iteration order (which is non-deterministic in Rust). Use `BTreeMap` for any data structure that affects state serialization.
- All randomness must come from the event's vector clock or payload (which are deterministic given the same event).
- The Financial shard's `FinancialState::apply()` uses the event's `vector_clock` for causal tracking, which is deterministic.

#### 2. `validate()` Must Be Pure

**Contract**: `validate(&self, op: &ShardOp)` must not mutate `self` or produce any observable side effects. It must return the same result for the same `op` and the same `self` state.

**Rationale**: `validate()` is used for pre-flight checks — determining whether an operation would succeed before committing it. If `validate()` had side effects, calling it would alter state, making it impossible to "test before commit." This is especially important for the Financial shard, where `validate()` checks balance sufficiency without modifying balances.

**Implications for implementations**:
- `validate()` takes `&self` (immutable reference), enforced by the Rust type system.
- No interior mutability (no `Cell`, `RefCell`, or `AtomicU64` inside `validate()`).
- No I/O (no network calls, no file reads, no logging of mutable state).
- The Financial shard's `FinancialValidator` checks balances and amounts without mutating `FinancialState`.

#### 3. `state_snapshot()` Must Be Byte-for-Byte Reproducible

**Contract**: For the same shard state, `state_snapshot()` must always return the exact same byte sequence. Two nodes with the same state must produce identical snapshots.

**Rationale**: State snapshots are used for:
- State root computation (the Merkle root of shard state).
- Cross-shard state verification.
- L1 settlement (the state root is posted to L1 via `SettlementLayer::post_batch()`).

If snapshots were non-reproducible, the same shard state would produce different state roots on different nodes, breaking consensus.

**Implications for implementations**:
- All serialization must use deterministic formats. `bincode` (used by all shard `to_bytes()` methods) is deterministic by default.
- Map types must use `BTreeMap` (deterministic iteration order), not `HashMap` (non-deterministic iteration order). The `FinancialState` and other shards use `HashMap`, which is a known issue — they must be migrated to `BTreeMap` before mainnet.
- Floating-point numbers must not appear in serialized state (NaN != NaN, -0.0 != +0.0). The economics crate enforces this: all calculations use fixed-point PPM arithmetic.
- No padding bytes or uninitialized memory in the serialized output.
- Some shards prefix a version byte (`FinancialState::FINANCIAL_STATE_VERSION = 1`, `EconomicsState::ECONOMICS_STATE_VERSION = 1`) to support future format migrations.

#### 4. `Send + Sync` Requirements

**Contract**: All `Shard` implementations must be `Send + Sync`.

**Rationale**: The substrate runs in a multi-threaded Tokio runtime. The `ShardRouter` holds shards in a collection that may be accessed from multiple tasks. While individual `process_event()` calls are sequential (due to `&mut self`), the router itself must be `Send + Sync` to be stored in the substrate's `shard_processor` field, which is accessed across await points in the `run()` loop.

**Implications for implementations**:
- No `Rc` or `RefCell` in shard state (they are not `Send + Sync`).
- `Arc` is acceptable for shared read-only data.
- `Mutex` and `RwLock` are acceptable for interior mutability, but care must be taken to avoid deadlocks.

### Error Handling: ShardError Variants

The `ShardError` enum (in `shards/src/shard.rs`) provides seven variants:

| Variant | When to Use | Example |
|---------|-------------|---------|
| `InvalidOperation` | The operation is not valid for this shard type | Calling a financial op on the identity shard |
| `ValidationFailed` | The operation would violate a business rule | Insufficient balance for transfer |
| `StateConflict` | A state-level conflict is detected | Double-spend attempt, task already exists |
| `CrossShardError` | A cross-shard communication failure | Timeout waiting for another shard |
| `DeserializationError` | Failed to decode the shard payload | Malformed `ShardOp` bytes |
| `UnknownShard` | The target shard was not found in the router | Routing to a non-existent shard |
| `InsufficientFee` | The caller lacks UBC quota to pay the fee | Quota exceeded for the operation |

`ValidationFailed` vs `StateConflict`: Use `ValidationFailed` for rule violations that are caught by the `validate()` method (e.g., insufficient balance). Use `StateConflict` for violations detected during `process_event()` that represent actual state corruption (e.g., a double-spend where the same UTXO is spent twice). The distinction is important because `StateConflict` may trigger slashing, while `ValidationFailed` simply rejects the operation.

`InsufficientFee` is returned by the `ShardRouter::route_event()` method when the caller's UBC balance cannot cover the fee determined by `FeeSchedule::fee_for_op()`.

### ShardRouter: Dispatch and Fee Enforcement

The `ShardRouter` (`shards/src/router.rs`) is the central dispatch point. It:

1. **Deserializes** the event payload into a `ShardPayload`.
2. **Checks nonces** for replay protection (persists via `NonceStore`).
3. **Deducts fees** from the caller's UBC quota via `QuotaSystem::spend()`.
4. **Routes** the operation to the target shard based on the `ShardOp` variant.

The router also implements `omnia_substrate::EventProcessor`, making it compatible with the substrate's event processing pipeline.

### Financial Shard: Strict Causal Ordering, NOT CRDTs

The Financial shard (`shards/src/financial/`) uses strict causal ordering for balance updates. This is a deliberate design choice:

- **Why not CRDTs?** CRDTs (Conflict-free Replicated Data Types) like the `GCounter` (used in the substrate layer for other purposes) only support increment operations. Financial operations require decrement (transfer, burn), which is not commutative. A `Transfer { to, amount }` and another `Transfer { to, amount }` from the same sender are not commutative if the sender's balance is insufficient for both — the order matters.

- **How strict ordering works**: The `FinancialState::apply()` method processes events in causal order (determined by the event's vector clock). The `AccountBalance::decrement()` method checks for insufficient funds and returns `ShardError::ValidationFailed` if the balance is too low. The `last_update` vector clock on each account tracks the causal context of the last modification, enabling conflict detection.

- **Concurrent transfer handling**: If two transfers from the same account arrive concurrently (their vector clocks are `CausalOrder::Concurrent`), the Financial shard processes them in the topological order determined by the `CausalGraph::topological_order()` method. This order is deterministic across all nodes because it is based on event hashes and timestamps, which are invariant.

- **FinancialOp variants**: The Financial shard supports four operations: `Transfer { to: AccountId, amount: u64 }`, `Mint { to: AccountId, amount: u64 }`, `Burn { from: AccountId, amount: u64 }`, and `BalanceQuery { account: AccountId }` (read-only).

## Consequences

- **Positive**: Formal contracts ensure that all shards behave predictably, which is essential for consensus.
- **Positive**: `validate()` purity enables pre-flight checks without side effects.
- **Positive**: Reproducible snapshots enable state root computation and cross-node verification.
- **Positive**: The `ShardError::InsufficientFee` variant enables clean fee enforcement at the router level.
- **Negative**: The determinism contract prohibits `HashMap` in serialized state. All shard states currently use `HashMap` and must be migrated to `BTreeMap` before mainnet.
- **Negative**: Strict causal ordering for the Financial shard means that concurrent transfers from the same account are serialized, which limits parallelism. This is a necessary trade-off for financial correctness.
- **Trade-off**: The `ShardError` enum is shared across all shard types, which means some error variants may not apply to all shards. This is simpler than per-shard error types but reduces type safety at the router level.
