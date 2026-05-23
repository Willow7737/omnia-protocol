# CRDT Convergence Proofs
> 🎯 Audience: Developers
> 🔗 Context: Mathematical foundation for why CRDTs guarantee convergence under arbitrary message delivery order
> 📅 Last Updated: 2026-05-20

## Formal Convergence Theorem

> **Theorem.** *For any state-based CRDT C, if replicas start at the same state and apply the same set of merge operations in any order, they converge to the same state.*

**Proof sketch.** A state-based CRDT's merge function must satisfy three algebraic properties for its join semilattice:

1. **Commutativity**: `merge(a, b) = merge(b, a)`
2. **Associativity**: `merge(merge(a, b), c) = merge(a, merge(b, c))`
3. **Idempotency**: `merge(a, a) = a`

These three properties define a **join semilattice** over the state space. In a join semilattice, the least upper bound (lub) of any finite set of elements exists and is unique. Since merge computes the lub of its operands, and lub is unique, the order in which merges are applied cannot affect the final result. Replicas that receive the same set of state updates — regardless of delivery order, duplication, or delay — will converge to the same state.

---

## GCounter — Grow-Only Counter

### State Representation

A GCounter maintains a map `counts: NodeId → u64`, where each node tracks its own monotonically increasing contribution.

### Merge Semantics

```
merge(a, b).counts[node] = max(a.counts[node], b.counts[node])
```

The merge is **pointwise maximum** over all node entries.

### Proof of Properties

**Commutativity.** `max(x, y) = max(y, x)` for all `x, y ∈ u64`. Therefore `merge(a, b) = merge(b, a)` pointwise, hence structurally.

**Associativity.** `max(max(x, y), z) = max(x, max(y, z)) = max(x, y, z)` for all `x, y, z ∈ u64`. Therefore `merge(merge(a, b), c) = merge(a, merge(b, c))`.

**Idempotency.** `max(x, x) = x` for all `x ∈ u64`. Therefore `merge(a, a) = a`.

**Monotonicity.** The total value is `value(a) = Σ_node a.counts[node]`. Since `max(x, y) ≥ x` and `max(x, y) ≥ y`, each entry after merge is at least as large as before. After increment by `δ > 0`, the affected entry increases by `δ`. Therefore `value` is non-decreasing under both increment and merge.

### Property-Based Tests

| Test | Property |
|------|----------|
| `proptest_merge_commutative` | `merge(a, b) == merge(b, a)` |
| `proptest_merge_idempotent` | `merge(a, a) == a` |
| `proptest_merge_associative` | `merge(merge(a, b), c) == merge(a, merge(b, c))` |
| `proptest_monotonic` | `value` never decreases after increment or merge |

---

## OrSet — Observed-Remove Set

### State Representation

An OrSet maintains two maps per element:

- `adds: T → Set<Token>` — all tokens ever added for element `T`
- `removes: T → Set(Token)` — all tokens observed at removal time

A `Token = (NodeId, sequence_number)` uniquely identifies an add operation.

An element is **present** iff `adds[T] \ removes[T] ≠ ∅`.

### Merge Semantics

```
merge(a, b).adds[T]    = a.adds[T] ∪ b.adds[T]
merge(a, b).removes[T] = a.removes[T] ∪ b.removes[T]
```

The merge is **set union** over add-tokens and remove-tokens.

### Proof of Properties

**Commutativity.** Set union is commutative: `X ∪ Y = Y ∪ X`. Therefore `merge(a, b).adds = merge(b, a).adds` and similarly for removes. The observable state (which elements are present) is identical.

**Associativity.** Set union is associative: `(X ∪ Y) ∪ Z = X ∪ (Y ∪ Z)`. Therefore `merge(merge(a, b), c) = merge(a, merge(b, c))`.

**Idempotency.** Set union is idempotent: `X ∪ X = X`. Therefore `merge(a, a) = a`.

### Add-Wins Semantics

The key invariant of an observed-remove set is:

> If an element is concurrently added and removed, the **add wins**.

**Proof.** A remove operation can only observe tokens that existed at the time of the remove. A concurrent add creates a *new* token that the remove cannot have observed. Therefore the new token is in `adds[T]` but not in `removes[T]`, so `adds[T] \ removes[T] ≠ ∅`, and the element remains present after merge.

This property ensures that no addition is silently lost due to a concurrent remove — a critical guarantee for use cases like shopping carts and access control lists.

### Property-Based Tests

| Test | Property |
|------|----------|
| `proptest_merge_commutative` | Observable elements of `merge(a, b)` == `merge(b, a)` |
| `proptest_merge_idempotent` | `merge(a, a) == a` |
| `proptest_add_wins` | Concurrently added element survives remove after merge |

---

## LWWRegister — Last-Writer-Wins Register

### State Representation

An LWWRegister stores:

- `value: Option<T>` — the current value
- `version: u64` — monotonic version counter
- `timestamp: u64` — wall-clock time of the write
- `node_id: NodeId` — the node that performed the write

### Merge Semantics

```
merge(a, b):
  if b.should_win(a):
    take b's (value, timestamp, node_id, version)
  else:
    keep a's (value, timestamp, node_id, version)
  merge vector clocks
```

Where `should_win` uses a deterministic three-level tiebreaker:

1. **Higher version wins** (primary ordering)
2. **Higher timestamp wins** (secondary ordering, if versions equal)
3. **Higher node_id wins** (tertiary tiebreaker, if both above equal)

### Proof of Properties

**Determinism.** For any two registers `a` and `b`, exactly one of `a.should_win(b)` or `b.should_win(a)` is true (or both false for equal states). The tiebreaker over `node_id` is a total order since `NodeId` is a fixed-width byte array with lexicographic comparison, which is a total order. Therefore `merge(a, b)` always produces the same result.

**Commutativity of outcome.** `merge(a, b)` and `merge(b, a)` both select the register with the winning metadata. Since `should_win` is deterministic and antisymmetric (for `a ≠ b`, exactly one wins), both merge directions produce the same winning value.

**Idempotency.** `a.should_win(a)` is false (version, timestamp, and node_id are all equal, and `a.node_id > a.node_id` is false). Therefore `merge(a, a)` keeps `a`'s value. The vector clock merge is also idempotent (pointwise max of identical clocks). Thus `merge(a, a) = a`.

### Why the Tiebreaker Order Matters

The tiebreaker order `version > timestamp > node_id` is carefully chosen:

- **Version first**: A monotonic version counter is more reliable than a wall-clock timestamp because it cannot be affected by clock skew. This ensures that causally newer writes always win.
- **Timestamp second**: When versions are equal (e.g., concurrent first writes), the timestamp provides a reasonable best-effort ordering.
- **Node ID last**: When all else is equal, a deterministic comparison of node identifiers ensures that every replica makes the same choice, guaranteeing convergence even in the degenerate case of identical timestamps.

### Property-Based Tests

| Test | Property |
|------|----------|
| `proptest_merge_deterministic` | `merge(a, b)` always returns the same result |
| `proptest_merge_idempotent` | `merge(a, a) == a` |
| `proptest_newer_wins` | Register with higher version/timestamp/node_id wins |

---

## Why These Properties Guarantee Convergence

In a distributed system, messages between replicas can be:

- **Delayed**: arbitrarily long delivery time
- **Reordered**: messages arrive in a different order than sent
- **Duplicated**: the same message delivered multiple times

State-based CRDTs handle all three cases through their algebraic properties:

| Scenario | Property that handles it |
|----------|--------------------------|
| Reordered messages | Commutativity: merge order doesn't matter |
| Duplicated messages | Idempotency: merging the same state twice is harmless |
| Arbitrary fan-in | Associativity: grouping of merges doesn't matter |

Together, these properties mean that a replica can apply incoming state updates in any order, at any time, and the result will be the same as if they were applied in any other order. This is precisely the guarantee needed for eventual consistency in the Omnia Protocol's gossip-based synchronization.

### Formal Statement

For any CRDT type `C` implementing `CvRDT` with a merge function satisfying commutativity, associativity, and idempotency:

```
∀ replicas r₁, r₂: if r₁ and r₂ receive the same multiset of state updates S,
then after processing all updates: state(r₁) = state(r₂)
```

This holds regardless of the order in which each replica processes the updates in `S`, or whether some updates are processed multiple times.

---

## References

- Shapiro, M., Preguiça, N., Baquero, C., & Zawirski, M. (2011). *Conflict-free Replicated Data Types*. SSS 2011.
- Almeida, P. S., Baquero, C., & Preguiça, N. (2018). *Delta State Replicated Data Types*. Journal of Parallel and Distributed Computing.

---
🔙 **Back**: [architecture/](./) | 🔄 **Related**: [vector-clock-reconciliation.md](./vector-clock-reconciliation.md)
🚀 **Next**: [vector-clock-reconciliation.md](./vector-clock-reconciliation.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)
