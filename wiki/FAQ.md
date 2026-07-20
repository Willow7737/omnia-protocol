# FAQ

### Is Omnia a blockchain?

No. It's a **causal graph** protocol: events reference their parents
directly, so unrelated transactions never queue behind each other. There is
no global chain, no miners, no sequencer. BFT finality comes from validator
quorum certificates over the graph ([Two-Lane Consensus](Two-Lane-Consensus)).

### Is there a token? Was there a sale?

There is **no speculative token and there was no sale**. UBC (Universal
Basic Compute) is a soulbound monthly quota of protocol capacity granted to
every registered identity. The code is CC0 — public domain, no company
gatekeeping.

### What actually works today, versus roadmap?

Working and measured, today: the full consensus substrate; a live
multi-node validator testnet with real BFT finality (10,000/10,000 events
finalized across 5 validators in stress testing); a **geo-distributed
3-region WAN run** (EU/US/Asia, RTTs to ~218 ms) with 100% propagation
and full finality at 10k bursts; anti-entropy self-healing; a shipped
mobile wallet, web dashboard, and website; ZK proof generation with an
Ethereum settlement adapter; TLA+ specs model-checked in CI.

Explicitly not done yet: a permanently-running geo network (the WAN run
was a measured campaign), external security audit, Bitcoin/Solana/Celestia adapters
(stubs), proof-of-useful-work, conviction voting/delegation. The
requirement-level truth lives in
[`docs/reference/status.md`](https://github.com/Willow7737/omnia-protocol/blob/main/docs/reference/status.md)
and the [stub inventory](https://github.com/Willow7737/omnia-protocol/blob/main/docs/stub-inventory.md)
— the project tracks its own gaps publicly.

### How fast is it, really?

Two different questions with two different answers, never mixed:
**~12,000 ops/s** consensus hot path (single node, in-process, CI-gated),
and on the **real network**, 10k-event bursts reach 100% propagation with
full BFT finality — on a 5-node mesh (median convergence under a minute)
and across a 3-region WAN with RTTs up to ~218 ms. Details and caveats:
[Benchmarks](Benchmarks).

### Is it quantum-resistant?

Partially, by design: ML-KEM-768 (the FIPS-203 algorithm) is used for
quantum commitments today, and the signature scheme is migration-planned
(see the crypto-migration reference in `docs/reference/`). Ed25519 remains
the event-signature workhorse for now.

### Can it really settle on any chain?

The architecture is settlement-agnostic (ZK proofs + data availability are
the only requirements of the settlement layer). Ethereum settlement exists
(`OmniaRollup.sol` with a Groth16 verifier); other adapters are interface
stubs awaiting implementation. "Agnostic" describes the design boundary,
not a claim that every adapter exists.

### How can I verify any of these claims?

Everything is reproducible: `cargo test --workspace` (1,300+ tests),
`docker compose up` a testnet and run `scripts/testnet-bench.sh` yourself,
read the benchmark record including its failure forensics
([Benchmarks](Benchmarks)), or model-check the TLA+ specs. Distrust and
verify — the repo is built for it.

### How do I contribute?

[`CONTRIBUTING.md`](https://github.com/Willow7737/omnia-protocol/blob/main/CONTRIBUTING.md).
Development happens on the `dev` branch through PRs with a 22-check CI
gate. Security reports: see
[`SECURITY.md`](https://github.com/Willow7737/omnia-protocol/blob/main/SECURITY.md)
(there's a bounty program).

### Who is behind this?

An open project by [Willow7737](https://github.com/Willow7737) with
AI-assisted engineering, developed fully in the open — every decision is an
ADR, every claim is a benchmark, every gap is in the stub inventory.
