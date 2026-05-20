# Metrics & Terminology Glossary
> 🎯 Audience: All
> 🔗 Context: Definitions of key metrics, terms, and abbreviations used across Omnia Protocol documentation
> 📅 Last Updated: 2026-05-20

## Protocol Terms

| Term | Definition |
|------|-----------|
| **Causal Graph** | A directed acyclic graph (DAG) where nodes represent events and edges represent causal relationships between them |
| **Vector Clock** | A data structure (`NodeId → LogicalClock`) that tracks what each node has seen, enabling partial ordering of events |
| **Event** | The fundamental unit of the protocol — a node in the causal graph containing a vector clock, payload, and signature |
| **Shard** | A specialized domain lane that processes a specific type of transaction (Financial, Identity, Physical, etc.) |
| **ShardRouter** | The dispatcher that routes events to the correct shard by domain |
| **CRDT** | Conflict-free Replicated Data Type — a data structure that can be replicated across nodes and merged without conflicts |
| **GCounter** | Grow-only counter CRDT — each node tracks its own monotonically increasing contribution |
| **OrSet** | Observed-remove set CRDT — add-wins semantics for concurrent add/remove |
| **LWWRegister** | Last-write-wins register CRDT — deterministic tiebreaker (version > timestamp > node_id) |
| **UBC** | Universal Basic Compute — soulbound monthly quota token issued to every identity |
| **DID** | Decentralized Identifier — self-sovereign identity in the format `did:omnia:<hex>` |
| **PQC** | Post-Quantum Cryptography — cryptographic algorithms resistant to quantum computing attacks |
| **BFT** | Byzantine Fault Tolerance — ability to maintain correct operation despite up to 1/3 malicious nodes |
| **Supermajority** | Greater than 2/3 of validator nodes — required for BFT consensus finality |
| **Slashing** | Penalty mechanism for misbehaving validators (equivocation, liveness violations, invalid attestations) |
| **Settlement Layer** | An L1 blockchain (Ethereum, Bitcoin, etc.) where ZK-rollup proofs are verified and state transitions are finalized |
| **Groth16** | A zero-knowledge proof system used for the ZK-rollup circuit |
| **Poseidon** | A SNARK-friendly hash function used for on-circuit Merkle path verification |
| **Mempool** | A bounded queue of pending events waiting to be processed by consensus |
| **Epoch** | A time period in the economics layer (default: 30 days) used for UBC quota resets and reputation decay |
| **ECVRF** | Elliptic Curve Verifiable Random Function — used for leader selection in consensus |
| **DKG** | Distributed Key Generation — a protocol for generating threshold key shares without a trusted dealer |

## Prometheus Metrics

| Metric Name | Type | Description |
|-------------|------|-------------|
| `omnia_node_events_submitted_total` | Counter | Total events submitted via the API |
| `omnia_node_events_finalized_total` | Counter | Total events finalized by consensus |
| `omnia_node_peers_connected` | Gauge | Current number of connected peers |
| `omnia_node_consensus_round` | Gauge | Current consensus round |
| `omnia_node_shard_operations_total` | Counter | Total shard operations processed |
| `omnia_node_http_requests_total` | Counter | Total HTTP requests served |
| `omnia_consensus_round_duration_seconds` | Histogram | Consensus round latency |
| `omnia_gossip_events_sent_total` | Counter | Total gossip events sent |
| `omnia_gossip_events_received_total` | Counter | Total gossip events received |
| `omnia_slashing_events_total` | Counter | Total slashing events |
| `omnia_fees_collected_total` | Counter | Total UBC fees collected |
| `omnia_causal_graph_total_events` | Gauge | Total events in the causal graph |

## Economic Terms

| Term | Definition |
|------|-----------|
| **Soulbound** | Non-transferable — UBC tokens cannot be moved between identities |
| **QuotaSystem** | Manages monthly UBC allocation with epoch-based advancement |
| **Quadratic Voting** | Voting power = √stake, preventing whale dominance |
| **PPM** | Parts per million — used for fixed-point reputation decay arithmetic (no f64) |
| **DecayRate** | Rate at which reputation decreases per epoch of inactivity (default: 10% = 100,000 PPM) |
| **Time-Lock** | Stake locked for a minimum duration before gaining voting power (anti flash-loan) |
| **FeeSchedule** | Maps shard operations to fixed UBC fees |
| **Gradual Slashing** | 3-tier penalty escalation: Warning → Jail → Ejection |

## Abbreviations

| Abbreviation | Full Form |
|-------------|-----------|
| ADR | Architecture Decision Record |
| DAG | Directed Acyclic Graph |
| SSS | Shamir's Secret Sharing |
| GF(256) | Galois Field with 256 elements |
| R1CS | Rank-1 Constraint System |
| BN254 | Barreto-Naehrig 254-bit elliptic curve |
| SRS | Structured Reference String (Powers of Tau) |
| PoK | Proof of Knowledge |
| LTO | Link-Time Optimization |
| AEAD | Authenticated Encryption with Associated Data |
| HKDF | HMAC-based Key Derivation Function |
| HD | Hierarchical Deterministic (key derivation) |
| NAT | Network Address Translation |
| DHT | Distributed Hash Table |
| mDNS | Multicast DNS |
| QUIC | Quick UDP Internet Connections |
| RPC | Remote Procedure Call |
| SBOM | Software Bill of Materials |

---
🔙 **Back**: [reference/](./) | 🔄 **Related**: [benchmark-gates.md](./benchmark-gates.md)
🚀 **Next**: [adr-index.md](./adr-index.md) | 📜 **Source of Truth**: [Restructuring Blueprint](./blueprint-reference.md)
