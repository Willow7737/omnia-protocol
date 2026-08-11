# Architecture Overview

Omnia is a layered stack. Every layer below is implemented and tested in the
repository today — this page is the map, the code is the territory.

```
┌──────────────────────────────────────────────────┐
│  LAYER 5: Economics (UBC, Governance)            │
├──────────────────────────────────────────────────┤
│  LAYER 4: Identity (DIDs, Shamir, Biometrics)    │
├──────────────────────────────────────────────────┤
│  LAYER 3: Binding (Provenance, RF, Quantum)      │
├──────────────────────────────────────────────────┤
│  LAYER 2: Domain Shards (6 shards)               │
├──────────────────────────────────────────────────┤
│  LAYER 1: Substrate (Causal Graph Consensus)     │
├──────────────────────────────────────────────────┤
│  LAYER 0: ZK-Rollup Settlement (chain-agnostic)  │
└──────────────────────────────────────────────────┘
```

## Layer 1 — The Substrate (the heart)

Instead of a blockchain's single ordered chain, Omnia's substrate is a
**causal graph**: every event names its parents, vector clocks capture
"happened-before," and CRDTs make concurrent state merges deterministic.
Consequences:

- **Parallelism** — events that don't causally depend on each other commit
  independently; there is no global sequencer to queue behind.
- **Self-healing** — because state is CRDT-merged and the graph is
  content-addressed, nodes that fall behind reconcile by exchanging
  frontiers and re-fetching exactly what they miss (anti-entropy repair).
- **BFT finality** — consensus finalizes cuts of the graph with Byzantine
  fault tolerance (see [Two-Lane Consensus](Two-Lane-Consensus)).

Crates: `omnia-primitives` (events, vector clocks), `omnia-consensus`
(causal graph, finality), `omnia-network` (libp2p QUIC + gossipsub mesh,
anti-entropy), `substrate` (the consensus round loop).

## Layer 0 — Settlement without lock-in

State transitions are proven with **Groth16 ZK proofs** (arkworks, BN254)
and can settle on any L1 that verifies proofs and stores data — an Ethereum
adapter (`OmniaRollup.sol`) exists today; a Bitcoin adapter is also live
(`bitcoin-live` feature flag); Solana/Celestia adapters
are stubs by design. Omnia is a coordination layer, not another L1
competing for your liquidity.

## Layers 2–5 in one paragraph each

- **Domain shards** — six typed shards (financial, identity, provenance, …)
  route events by domain, with cross-shard messaging and per-shard fees.
- **Binding** — append-only provenance logs tie physical objects and
  real-world processes into the graph (RF fingerprints and quantum
  commitments are the research edge here).
- **Identity** — self-sovereign `did:omnia:` identities derived from
  Ed25519 keys; social recovery via Shamir secret sharing (GF(256), AES-GCM
  encrypted shares); verifiable credentials; AI-agent identities with typed
  capabilities.
- **Economics** — **Universal Basic Compute (UBC)**: every registered DID
  receives a soulbound monthly quota of protocol capacity. Governance is
  quadratic voting with integer decay. There is deliberately no speculative
  token.

## Security posture (selected)

- Ed25519 (`verify_strict`) event signatures; BLAKE3 domain-separated
  hashing everywhere.
- Post-quantum: ML-KEM-768 (FIPS-203 algorithm) commitments.
- Gradual slashing (Warning → Jail → Ejection), equivocation detection in
  constant time.
- `#![deny(unsafe_code)]`, zero production `unwrap()`, 34 typed error enums.
- TLA+ specifications model-checked in CI on every pull request.

## Where to go deeper

| Topic | Canonical doc |
|---|---|
| Full blueprint | [`docs/reference/blueprint-reference.md`](https://github.com/Willow7737/omnia-protocol/blob/main/docs/reference/blueprint-reference.md) |
| Trait boundaries / crate map | [`docs/architecture/`](https://github.com/Willow7737/omnia-protocol/tree/main/docs/architecture) |
| Consensus design decisions | [ADR index](https://github.com/Willow7737/omnia-protocol/blob/main/docs/reference/adr-index.md) |
| Requirement-level status | [`docs/reference/status.md`](https://github.com/Willow7737/omnia-protocol/blob/main/docs/reference/status.md) |
