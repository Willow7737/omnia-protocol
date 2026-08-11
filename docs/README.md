# Omnia Protocol Documentation

> 🎯 Audience: All
> 🔗 Context: Central navigation hub for all Omnia Protocol documentation
> 📅 Last Updated: 2026-08-11

Welcome to the Omnia Protocol documentation hub. This index provides structured navigation to every document in the repository, organized by audience and topic.

---

## 🚪 Choose Your Path

| If you are...           | Start Here                                                           | Next Step                                                            |
| ----------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| 🌱 New to Omnia         | [use-cases/](use-cases/)                                             | [Quickstart](#quick-start)                                           |
| 💻 Contributor          | [CONTRIBUTING.md](../CONTRIBUTING.md)                                | [architecture/](architecture/)                                       |
| 🏗️ Systems Architect    | [reference/blueprint-reference.md](reference/blueprint-reference.md) | [architecture/trait-boundaries.md](architecture/trait-boundaries.md) |
| 📦 Validator Operator   | [building/feature-matrix.md](building/feature-matrix.md)             | [operations/validator-setup.md](operations/validator-setup.md)       |
| 📊 Performance Engineer | [reference/benchmark-gates.md](reference/benchmark-gates.md)         | [architecture/pipeline-design.md](architecture/pipeline-design.md)   |
| 🔒 Security Researcher  | [reference/security-audit.md](reference/security-audit.md)           | [reference/crypto-migration.md](reference/crypto-migration.md)       |

---

## Quick Start

```bash
git clone https://github.com/Willow7737/omnia-protocol.git
cd omnia-protocol
cargo test --workspace
cargo bench --no-run
```

---

## 📂 Documentation Structure

### [architecture/](architecture/) — Crate Layout, DAG, Traits, Pipeline, Cache Alignment

Deep technical documentation on the protocol's layered architecture, trait contracts, and consensus pipeline.

| Document                                                                      | Description                                                                 |
| ----------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| [README.md](architecture/README.md)                                           | Architecture index and layer overview                                       |
| [layer-1-substrate.md](architecture/layer-1-substrate.md)                     | Substrate layer — VectorClock, Event, CausalGraph, CRDTs, Gossip, Consensus |
| [layer-2-shards.md](architecture/layer-2-shards.md)                           | Domain Shards — 6 shards, ShardRouter, cross-shard messaging                |
| [layer-3-binding.md](architecture/layer-3-binding.md)                         | Binding Layer — ProvenanceLog, PhysicalAnchor, PQC signatures               |
| [layer-4-identity.md](architecture/layer-4-identity.md)                       | Identity Hardening — DIDs, Shamir, Biometrics, AI Agents                    |
| [layer-5-economics.md](architecture/layer-5-economics.md)                     | Economics — UBC, Governance, Decay                                          |
| [zk-rollup-settlement.md](architecture/zk-rollup-settlement.md)               | ZK-Rollup Phase 0 — Settlement-agnostic architecture                        |
| [trait-boundaries.md](architecture/trait-boundaries.md)                       | Trait contracts — EventProcessor, SettlementLayer, Shard                    |
| [pipeline-design.md](architecture/pipeline-design.md)                         | Consensus pipeline, mempool, leader selection                               |
| [crdt-convergence.md](architecture/crdt-convergence.md)                       | CRDT convergence proofs                                                     |
| [vector-clock-reconciliation.md](architecture/vector-clock-reconciliation.md) | Vector clock reconciliation strategy                                        |
| [consensus-queue.md](architecture/consensus-queue.md)                         | Consensus queue invariants and O(new_events) processing                     |
| [full-spec.md](architecture/full-spec.md)                                     | Comprehensive architecture specification (all layers)                       |

### [building/](building/) — Feature Profiles, Cross-Compilation, Binary Optimization

Guides for building the Omnia node binary with various feature configurations.

| Document                                                  | Description                          |
| --------------------------------------------------------- | ------------------------------------ |
| [README.md](building/README.md)                           | Building index                       |
| [feature-matrix.md](building/feature-matrix.md)           | Feature flags and build profiles     |
| [cross-compilation.md](building/cross-compilation.md)     | Cross-compilation guide              |
| [binary-optimization.md](building/binary-optimization.md) | Binary size and release optimization |

### [operations/](operations/) — Validator Setup, Monitoring, Deployment, Feature Flags

Operational runbooks and deployment guides for running Omnia nodes.

| Document                                            | Description                                                              |
| --------------------------------------------------- | ------------------------------------------------------------------------ |
| [README.md](operations/README.md)                   | Operations index                                                         |
| [validator-setup.md](operations/validator-setup.md) | Validator setup guide                                                    |
| [monitoring.md](operations/monitoring.md)           | Monitoring setup — Grafana, Prometheus, alerts                           |
| [deployment.md](operations/deployment.md)           | Deployment procedures — Docker, Kubernetes                               |
| [runbook.md](operations/runbook.md)                 | Operations runbook — startup, key rotation, slashing, partition recovery |
| [feature-flags.md](operations/feature-flags.md)     | Feature flag reference                                                   |
| [cli-and-api.md](operations/cli-and-api.md)         | CLI subcommands and REST API endpoint reference                          |

### [reference/](reference/) — Roadmap, Benchmarks, Blueprint, Metrics Glossary

Reference documentation including roadmaps, benchmark data, policy documents, and phase reports.

| Document                                                           | Description                                         |
| ------------------------------------------------------------------ | --------------------------------------------------- |
| [README.md](reference/README.md)                                   | Reference index                                     |
| [roadmap.md](reference/roadmap.md)                                 | Implementation roadmap — Phase 0 through Phase 5+   |
| [benchmark-gates.md](reference/benchmark-gates.md)                 | Performance baselines and benchmark data            |
| [blueprint-reference.md](reference/blueprint-reference.md)         | Blueprint/spec reference — implementation status    |
| [metrics-glossary.md](reference/metrics-glossary.md)               | Metrics and terminology glossary                    |
| [adr-index.md](reference/adr-index.md)                             | Architecture Decision Record index                  |
| [security-audit.md](reference/security-audit.md)                   | Security audit package — findings, validated audits |
| [dependency-policy.md](reference/dependency-policy.md)             | Dependency policy — pinning, audits, exemptions     |
| [crypto-migration.md](reference/crypto-migration.md)               | Cryptographic migration playbook                    |
| [economic-analysis.md](reference/economic-analysis.md)             | Economic parameter analysis for mainnet readiness   |
| [implementation-spec.md](reference/implementation-spec.md)         | Protocol implementation specifications              |
| [phase-0-findings.md](reference/phase-0-findings.md)               | Phase 0 audit findings                              |
| [phase-0-validated-audit.md](reference/phase-0-validated-audit.md) | Phase 0 validated audit results                     |
| [phase-1-summary.md](reference/phase-1-summary.md)                 | Phase 1 summary                                     |
| [phase-2-findings.md](reference/phase-2-findings.md)               | Phase 2 audit findings                              |
| [phase-2-summary.md](reference/phase-2-summary.md)                 | Phase 2 summary                                     |
| [phase-3-summary.md](reference/phase-3-summary.md)                 | Phase 3 summary                                     |
| [phase-4-summary.md](reference/phase-4-summary.md)                 | Phase 4 summary                                     |
| [phase-5-summary.md](reference/phase-5-summary.md)                 | Phase 5 summary                                     |
| [project-dashboard.md](reference/project-dashboard.md)             | Project dashboard — health, status, risks           |
| [status.md](reference/status.md)                                   | Requirements and status tracking                    |
| [security-bounty.md](reference/security-bounty.md)                 | Security bounty program                             |

### [use-cases/](use-cases/) — Real-World Scenarios, Phase Alignment, User Impact

Use case documentation and user-facing guides.

| Document                                                     | Description                                           |
| ------------------------------------------------------------ | ----------------------------------------------------- |
| [README.md](use-cases/README.md)                             | Use cases index                                       |
| [real-world-scenarios.md](use-cases/real-world-scenarios.md) | Real-world application scenarios                      |
| [phase-alignment.md](use-cases/phase-alignment.md)           | Phase alignment and user impact per development phase |
| [faq.md](use-cases/faq.md)                                   | Frequently asked questions                            |
| [governance.md](use-cases/governance.md)                     | Governance system — voting, treasury, reputation      |

### Additional Sections

| Section                      | Description                                                 |
| ---------------------------- | ----------------------------------------------------------- |
| [adr/](adr/)                 | Architecture Decision Records (ADR-001 through ADR-021)     |
| [audit/](audit/)             | Security audit reports, attack surface, and self-assessment |
| [security/](security/)       | Threat model and side-channel audit documentation           |
| [performance/](performance/) | Performance baseline measurements                           |

---

## 🔗 Key Root Documents

| Document                                     | Description                                         |
| -------------------------------------------- | --------------------------------------------------- |
| [README.md](../README.md)                    | Project landing page with workspace overview        |
| [ARCHITECTURE.md](architecture/full-spec.md) | Architecture overview (links to docs/architecture/) |
| [CONTRIBUTING.md](../CONTRIBUTING.md)        | Contribution guidelines                             |
| [SECURITY.md](../SECURITY.md)                | Security policy and reporting                       |
| [CHANGELOG.md](../CHANGELOG.md)              | Version changelog                                   |

---

🔙 **Back**: [README.md](../) | 🔄 **Related**: [ARCHITECTURE.md](architecture/full-spec.md)
🚀 **Next**: [architecture/](architecture/) | 📜 **Source of Truth**: [Restructuring Blueprint](reference/blueprint-reference.md)
