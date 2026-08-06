# Omnia Protocol

**The Universal Coordination Layer for Reality** — a settlement-agnostic
protocol that replaces trust with mathematics, using causal graph consensus
(DAG + vector clocks + CRDTs) for parallel transaction processing.

> 🟢 **Live right now:** a multi-node Lane 0 validator testnet at
> [`78.47.43.136.sslip.io`](https://78.47.43.136.sslip.io/api/v1/node/info),
> a shipped [mobile wallet](https://github.com/Willow7737/Omnia-Wallet),
> a [web dashboard](https://github.com/Willow7737/omnia-protocol-interface),
> and a [website](https://github.com/Willow7737/omnia-web).

## Why Omnia?

Blockchains serialize the world into one global queue. Omnia doesn't: events
form a **causal graph**, so transactions that don't depend on each other are
processed in parallel, and consensus finalizes the graph — not a chain. The
protocol is **settlement-agnostic**: it can settle to Ethereum, Bitcoin,
Solana, or any L1 with data availability and proof verification, via
ZK-rollup proofs.

## Headline numbers (measured live, July 2026)

| What | Result |
|---|---|
| 10,000-event burst, 5-node validator mesh | **100% propagation + 10,000/10,000 BFT-finalized on every node — zero loss** |
| 10,000-event burst, **3-region WAN** (EU/US/Asia, ≤218 ms RTT) | **100% propagation + full BFT finality on every node — zero loss** |
| Consensus hot path (single node) | ~12,000 ops/s, ~25 µs finality p50 |
| Burst self-healing | Anti-entropy repair recovers everything live gossip drops under overload |
| Formal verification | TLA+ models (`OmniaTwoLane`, `OmniaConsensus`, `OmniaCRDT`) model-checked in CI on every PR |

Full methodology and history: [Benchmarks](Benchmarks).

## Start here

| You are… | Go to |
|---|---|
| New and curious | [What Is Omnia](Architecture-Overview) |
| A developer who wants a node running | [Getting Started](Getting-Started) |
| An operator joining/running the testnet | [Testnet Guide](Testnet-Guide) |
| Interested in the consensus design | [Two-Lane Consensus](Two-Lane-Consensus) |
| A wallet user | [Wallet & Ecosystem](Wallet-and-Ecosystem) |
| Skeptical (good!) | [Benchmarks](Benchmarks) and [FAQ](FAQ) |

## Project facts

- **License:** CC0 (public domain) — no token sale, no company gatekeeping.
- **Language:** Rust (MSRV 1.91), ~86,000 lines, 1,300+ tests,
  `#![deny(unsafe_code)]`.
- **Status:** Phase 6 (Public Testnet) — multi-node validator network live;
  geo-distributed rollout and external audit next.
- **Repository:** [Willow7737/omnia-protocol](https://github.com/Willow7737/omnia-protocol)

> **Note on freshness:** this wiki is a guided entry point. The canonical,
> always-current references live in the repository under
> [`docs/`](https://github.com/Willow7737/omnia-protocol/tree/main/docs) —
> every page here links to its source of truth. The wiki itself is
> version-controlled in the repo (`wiki/`) and auto-published, so edits go
> through normal pull-request review.
