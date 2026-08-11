# Trait Boundaries

> 🎯 Audience: Developers
> 🔗 Context: Cross-cutting trait contracts that define the boundaries between architectural layers
> 📅 Last Updated: 2026-08-11

## Overview

The Omnia Protocol uses Rust traits to define strict boundaries between architectural layers. These traits serve as the contract interface that each component must implement, enabling testability, modularity, and clear separation of concerns.

## Core Traits

### EventProcessor — ADR-001

The `EventProcessor` trait defines how each shard processes events. Every domain shard implements this trait.

```rust
pub trait EventProcessor: Send + Sync {
    fn process_event(&mut self, event: &Event) -> Result<(), EventProcessorError>;
}
```

**Key decisions (ADR-001):**

- `process_event()` takes `&mut self` — explicit mutation
- The trait has a single method. Validation and state snapshot are handled by the `Shard` trait (see below), not `EventProcessor`. F-22 fix — the previous version of this doc showed a 3-method trait that never existed in the code.
- `Send + Sync` bound — allows the processor to be shared across threads via `Arc<Mutex<>>`.
- No interior mutability (`RefCell`, `Mutex`, etc.) in the trait itself — ensures deterministic state machines. The `MutexShardRouter` wrapper provides the outer `Arc<Mutex<>>` for thread safety.

See [ADR-001](../reference/adr-index.md#adr-001-event-processor-trait) for the full decision record.

### SettlementLayer — ADR-002

The `SettlementLayer` trait defines the interface for L1 settlement adapters.

```rust
pub trait SettlementLayer {
    fn post_batch(&self, batch: &RollupBatch) -> Result<SettlementReceipt, SettlementError>;
    fn verify_proof(&self, receipt: &SettlementReceipt) -> Result<bool, SettlementError>;
    fn latest_state_root(&self) -> Result<[u8; 32], SettlementError>;
    fn deposit(&self, deposit: &Deposit) -> Result<DepositReceipt, SettlementError>;
    fn request_withdrawal(&self, withdrawal: &Withdrawal) -> Result<WithdrawalReceipt, SettlementError>;
}
```

**Key decisions (ADR-002):**

- Settlement-agnostic — any L1 with data availability and proof verification can implement this trait
- Returns typed errors (`SettlementError` enum), not strings
- Ethereum adapter has dual mode: Simulated (default) and Live (feature-gated)

> Note: The actual `SettlementLayer` trait is `#[async_trait]` async. See `omnia-adapters/src/settlement/mod.rs` for the current definition. A new `SettlementAdapter` trait (3 methods: `submit_root`, `fetch_finality`, `verify_inclusion`) is the preferred interface for new code.

See [ADR-002](../reference/adr-index.md#adr-002-settlement-layer-trait) for the full decision record.

### Gossip Substrate Interface — ADR-003

Defines the boundary between the gossip protocol and the substrate layer.

**Key decisions (ADR-003):**

- Gossip handles event propagation; substrate handles event ordering and consensus
- Events are immutable once created — gossip cannot modify them
- Deduplication at three levels: gossip `seen_events`, CausalGraph duplicate check, consensus idempotency

See [ADR-003](../reference/adr-index.md#adr-003-gossip-substrate-interface) for the full decision record.

### Proof Bundle Format — ADR-005

Defines the serialized format for ZK proof bundles exchanged between the prover and settlement layer.

See [ADR-005](../reference/adr-index.md#adr-005-proof-bundle-format) for the full decision record.

### Shard Trait Contract — ADR-006

Defines the `Shard` trait that each domain shard must implement, including fee enforcement and nonce tracking.

See [ADR-006](../reference/adr-index.md#adr-006-shard-trait-contract) for the full decision record.

### Binding Shard Interface — ADR-007

Defines how the binding layer interfaces with the shard system for provenance and quantum commitments.

See [ADR-007](../reference/adr-index.md#adr-007-binding-shard-interface) for the full decision record.

## Supporting ADRs

| ADR     | Title                              | Key Decision                               |
| ------- | ---------------------------------- | ------------------------------------------ |
| ADR-008 | Crypto Dependency Audit            | Audit of cryptographic dependency versions |
| ADR-009 | Poseidon Parameter Justification   | Cauchy MDS + BLAKE3 round constants        |
| ADR-010 | Encrypted Keystore Design          | AES-256-GCM + HKDF-SHA256                  |
| ADR-011 | Gradual Slashing Model             | 3-tier Warning → Jail → Ejection           |
| ADR-012 | VRF Construction Choice            | V1 legacy + V2 ECVRF target                |
| ADR-013 | DKG Protocol Selection             | Feldman VSS-based DKG                      |
| ADR-014 | Poseidon Parameter Migration       | Dual-hash transition strategy              |
| ADR-015 | Leader Selection in Consensus Loop | VRF-based, stake-weighted                  |
| ADR-016 | Kademlia DHT Configuration         | `/omnia/kad/1.0.0`, AutoNAT/Relay/DCutr    |
| ADR-017 | GossipSub Peer Scoring Thresholds  | Graylist at -100                           |
| ADR-018 | Consensus State Persistence        | RedbConsensusStore, load_or_new            |
| ADR-019 | Fast-Sync Protocol                 | BLAKE3 checkpoints, supermajority          |
| ADR-020 | Kyber KEM / ML-KEM Integration     | FIPS-203, wire-compatible                  |
| ADR-021 | Gossip Message Compression         | Snappy for >256 bytes                      |

> ADR-012 updated to v2.0.0 in Phase 5: V2 ECVRF (Fiat-Shamir + Ed25519) implemented but V1 (deterministic hash) remains the default.

See [adr-index.md](../reference/adr-index.md) for the complete ADR index with summaries.

---

🔙 **Back**: [architecture/](./) | 🔄 **Related**: [pipeline-design.md](./pipeline-design.md)
🚀 **Next**: [pipeline-design.md](./pipeline-design.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)
