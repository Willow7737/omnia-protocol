# Task 3-d Discrepancy Report: Node/Ops/Monitoring/FV/Audit/ADRs

> 🎯 Audience: Security Researchers
> 🔗 Context: Part of the audit documentation section
> 📅 Last Updated: 2026-05-20

**Auditor:** Technical Documentation Auditor (Task 3-d)
**Date:** 2026-03-05
**Domain:** Node crate + Operations + Monitoring + Formal Verification + Audit docs + ADRs for consensus/gossip

---

## A) DISCREPANCY LIST BY FILE

### 1. docs/OPERATIONS.md

| #   | Discrepancy                         | Doc Says                           | Code Reality                                                                                                                                                                        |
| --- | ----------------------------------- | ---------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | sled migration doc is skeletal      | 18-line stub with "TBD" steps      | The node `main.rs` already uses `SledSlashingStore` and `SledNonceStore` via sled 0.34; no migration tool exists. `Cargo.toml` warns about sled alpha quality.                      |
| 2   | Missing operational details         | Only lists migration steps         | No coverage of: CLI subcommands (keygen, setup-contribute, setup-verify, snapshot, restore), HTTP API endpoints, Prometheus metrics, Docker deployment, config file format          |
| 3   | Missing sled 0.34 specific warnings | Generic "crash consistency issues" | Code comment in `Cargo.toml` says: "sled 0.34 is alpha-quality. Production deployments should migrate to rocksdb or redb." No `RocksDbSlashingStore` or `RocksDbNonceStore` exists. |

### 2. ops/RUNBOOK.md

| #   | Discrepancy                  | Doc Says                                                              | Code Reality                                                                                                                                                                                                                                                                                                    |
| --- | ---------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Port mismatch                | Docker compose uses port 9090, healthcheck at `localhost:9090/health` | `CliArgs::http_port` defaults to `8080`. Dockerfile `EXPOSE 9090`. Docker compose maps 9090. The default and Docker config are inconsistent.                                                                                                                                                                    |
| 2   | Invalid OMNIA_NODE_ID values | `OMNIA_NODE_ID=bootstrap`, `OMNIA_NODE_ID=node1`                      | `CliArgs::node_id` is `u64`, validated non-zero. `bootstrap` and `node1` are not valid u64 values — the node would fail to start.                                                                                                                                                                               |
| 3   | Non-existent env var         | `OMNIA_TOTAL_NODES=5` in docker-compose                               | `CliArgs` has no `total_nodes` field. This env var is ignored.                                                                                                                                                                                                                                                  |
| 4   | Missing CLI subcommands      | Only mentions `keygen`                                                | Actual subcommands: `keygen`, `setup-contribute`, `setup-verify`, `snapshot`, `restore`, `run`                                                                                                                                                                                                                  |
| 5   | Wrong API path prefix        | `/api/governance/proposals`, `/api/slashing/{id}`, `/api/node/peers`  | All API routes are under `/api/v1/` prefix. No `/api/slashing/` endpoint exists.                                                                                                                                                                                                                                |
| 6   | Missing API endpoints        | Only governance + node + health mentioned                             | Full API: `/api/v1/node/info`, `/api/v1/node/peers`, `/api/v1/events` (POST), `/api/v1/events/{id}` (GET), `/api/v1/shards/{shard_id}/operations` (POST), `/api/v1/governance/proposals` (POST), `/api/v1/governance/vote` (POST), `/api/v1/economics/balance/{did}` (GET), `/api/v1/economics/transfer` (POST) |
| 7   | Snapshot endpoint            | `curl -X POST http://localhost:8080/api/node/snapshot`                | No such API endpoint exists. Snapshot is a CLI subcommand: `omnia-node snapshot --output snapshot.bin`                                                                                                                                                                                                          |
| 8   | Alert severity description   | ">0.1 slashes/hour"                                                   | Actual alert: `rate(omnia_slashing_events_total[10m]) > 0.1` — this is 0.1 per second, not per hour                                                                                                                                                                                                             |
| 9   | MemoryGrowth description     | ">10 MiB/hour for 30 min"                                             | Actual: `deriv(process_resident_memory_bytes[30m]) > 10485760` — this is 10MB growth rate over 30 min, not per hour                                                                                                                                                                                             |
| 10  | Missing Swagger UI           | Not mentioned                                                         | Code mounts Swagger UI at `/swagger-ui` and OpenAPI spec at `/api-docs/openapi.json`                                                                                                                                                                                                                            |
| 11  | Config file format           | Shows `omnia-node.toml` with `node_id` as u16                         | `NodeConfigFile::node_id` is `Option<u16>` but `NodeConfig::node_id` is `u64`. Inconsistency.                                                                                                                                                                                                                   |

### 3. monitoring/README.md

| #   | Discrepancy              | Doc Says                                    | Code Reality                                                                                                                                                                                                                                               |
| --- | ------------------------ | ------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Prometheus port example  | `omnia-bootstrap:9090`, `omnia-node-1:9090` | Actual `prometheus.yml` uses `omnia-bootstrap:9090`, `omnia-node-1:9091`, etc. The README example shows both on port 9090, which is wrong.                                                                                                                 |
| 2   | Missing metrics detail   | Lists metric names without code reference   | `NodeMetrics` in `state.rs` defines 6 metrics: `omnia_node_events_submitted_total`, `omnia_node_events_finalized_total`, `omnia_node_peers_connected`, `omnia_node_consensus_round`, `omnia_node_shard_operations_total`, `omnia_node_http_requests_total` |
| 3   | Grafana dashboard panels | Lists 9 panels                              | Dashboard JSON has 9 panels (correct), but panel names differ slightly from the code metric names                                                                                                                                                          |

### 4. formal-verification/README.md

| #   | Discrepancy                                   | Doc Says                                                  | Code Reality                                                                                                                                                     |
| --- | --------------------------------------------- | --------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Line count wrong                              | "123 lines" for OmniaConsensus.tla                        | Actual file is 191 lines                                                                                                                                         |
| 2   | "3 rounds" in config                          | `MaxRounds = 3` in model config                           | TLA+ uses `MaxSeq` (not `MaxRounds`). The `MaxSeq` constant controls maximum sequence number, default model uses `MaxSeq = 1`. There is no `MaxRounds` constant. |
| 3   | Missing Validity and Liveness in model config | Config only shows `Agreement`, `NoEquivocation`, `TypeOK` | The spec also defines `Validity` and `Liveness` invariants, and the README's own "Properties Verified" section lists 5 properties                                |
| 4   | Missing OmniaCRDT details                     | Brief mention of CRDT spec                                | `OmniaCRDT.tla` (213 lines) models GCounter, OrSet, and LWWRegister with convergence proofs. Not detailed in README.                                             |
| 5   | Missing .cfg files listing                    | References `OmniaCRDT.cfg`                                | No `OmniaCRDT.cfg` file exists in the directory. Only `OmniaConsensus.cfg` is referenced.                                                                        |
| 6   | Model config field names wrong                | `MaxByzantine = 1`, `MaxRounds = 3`                       | Actual spec uses `ByzantineNodes` (a set, not a count) and `MaxSeq` (not `MaxRounds`).                                                                           |

### 5. docs/audit/ATTACK_SURFACE.md

| #   | Discrepancy                            | Doc Says          | Code Reality                                                                                                                                                                   |
| --- | -------------------------------------- | ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 1   | Stale commit reference                 | `SPRINT_3_COMMIT` | Should reference current v4.0.0 codebase                                                                                                                                       |
| 2   | Missing HTTP API attack surface        | Not listed        | The node exposes 9+ HTTP endpoints with no authentication, no rate limiting, and no authorization. Anyone can submit events, mint UBC, create proposals, transfer tokens, etc. |
| 3   | Missing keygen security concerns       | Not mentioned     | `run_keygen()` writes private key as raw binary (`validator_key.bin`), not encrypted. Doc comment says "in production, this would be encrypted."                               |
| 4   | Missing trusted setup ceremony risks   | Not mentioned     | `setup-contribute` and `setup-verify` subcommands expose ZK trusted setup ceremony operations. A compromised ceremony allows forging proofs.                                   |
| 5   | Missing SledNonceStore persistence gap | Not mentioned     | Nonce state is persisted to sled, but `SledNonceStore::open()` failures during operation could cause replay protection loss.                                                   |
| 6   | Missing node_id type mismatch          | Not mentioned     | `NodeConfig::node_id` is `u64` but `NodeConfigFile::node_id` is `Option<u16>` — potential truncation when loading from TOML config.                                            |

### 6. docs/audit/AUDIT_README.md

| #   | Discrepancy                           | Doc Says                                                                                | Code Reality                                                                                                                                                                                                                                |
| --- | ------------------------------------- | --------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Stale commit reference                | `SPRINT_3_COMMIT`                                                                       | Should reference v4.0.0                                                                                                                                                                                                                     |
| 2   | Crate count wrong                     | "5 crates"                                                                              | Now 7 crates: substrate, shards, economics, zk, binding, node, chaos-tests                                                                                                                                                                  |
| 3   | Test count wrong                      | "278+ tests"                                                                            | Additional tests in node (config, API) and chaos-tests crates                                                                                                                                                                               |
| 4   | Fuzz targets wrong                    | 4 targets: `causal_graph_insert`, `event_validate`, `shard_route`, `vector_clock_merge` | `scripts/fuzz.sh` lists 7 targets: `fuzz_event_deserialization`, `fuzz_gossip_message`, `fuzz_zk_proof_deserialization`, `fuzz_consensus_state_transition`, `fuzz_vector_clock_merge`, `fuzz_rate_limiter`, `fuzz_snapshot_deserialization` |
| 5   | Missing node crate in repo structure  | Not listed                                                                              | `node/` crate with `main.rs`, `lib.rs`, `config.rs`, `http.rs`, `state.rs`, `api/`                                                                                                                                                          |
| 6   | Missing chaos-tests in repo structure | Not listed                                                                              | `chaos-tests/` crate with `ChaosNetwork` and `ChaosNode`                                                                                                                                                                                    |
| 7   | Missing node binary build command     | `cargo build --bin omnia-node (once added in Sprint 3)`                                 | Node binary is fully implemented. Build: `cargo build -p omnia-node`                                                                                                                                                                        |
| 8   | TLA+ line count wrong                 | "123 lines"                                                                             | 191 lines                                                                                                                                                                                                                                   |
| 9   | Chaos tests marked as "planned"       | "planned for Sprint 3 and may not be available"                                         | `omnia-chaos-tests` crate exists with `ChaosNetwork`, `ChaosNode`, partition/crash/drop-rate injection, safety/liveness checks                                                                                                              |
| 10  | Missing ExpandedRollupCircuit         | §6 says "ZK circuit has been expanded" with ExpandedRollupCircuit                       | More detail needed about the simplified field-addition hash placeholder                                                                                                                                                                     |
| 11  | Missing SledNonceStore                | Only mentions SledSlashingStore                                                         | `main.rs` creates `SledNonceStore` for persistent replay protection. `ShardRouter::with_nonce_store()` takes `Arc<dyn NonceStore>`.                                                                                                         |
| 12  | Missing Swagger UI                    | Not mentioned                                                                           | HTTP router serves Swagger UI at `/swagger-ui` and OpenAPI spec at `/api-docs/openapi.json` via `utoipa` + `utoipa-swagger-ui`                                                                                                              |
| 13  | Missing subcommands                   | Not mentioned                                                                           | `keygen`, `setup-contribute`, `setup-verify`, `snapshot`, `restore`, `run`                                                                                                                                                                  |
| 14  | Dependency graph wrong                | Shows 5 crate graph                                                                     | Should include `omnia-node` depending on substrate, shards, economics, binding, zk. And `omnia-chaos-tests` depending on substrate.                                                                                                         |

### 7. docs/audit/AUDIT_SCOPE.md

| #   | Discrepancy                      | Doc Says                | Code Reality                                                                                                        |
| --- | -------------------------------- | ----------------------- | ------------------------------------------------------------------------------------------------------------------- |
| 1   | Stale commit reference           | `SPRINT_3_COMMIT`       | Should reference v4.0.0                                                                                             |
| 2   | Missing node crate from in-scope | Not listed              | `node/` crate is security-relevant: HTTP API with no auth, CLI keygen writing unencrypted keys, TOML config parsing |
| 3   | Missing chaos-tests              | Not listed              | `chaos-tests/` is in-scope for verifying safety/liveness claims                                                     |
| 4   | Missing HTTP API attack surface  | Not in trust boundaries | REST API has no authentication; anyone can submit events, mint UBC, transfer tokens                                 |
| 5   | Missing SledNonceStore           | Not mentioned           | Nonce persistence is critical for replay protection                                                                 |
| 6   | Missing trusted setup ceremony   | Not in scope            | `setup-contribute` and `setup-verify` subcommands affect ZK security                                                |

### 8. docs/audit/SELF_ASSESSMENT.md

| #   | Discrepancy                        | Doc Says                                             | Code Reality                                                                                           |
| --- | ---------------------------------- | ---------------------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| 1   | Stale commit reference             | `SPRINT_3_COMMIT`                                    | Should reference v4.0.0                                                                                |
| 2   | Fuzz targets count wrong           | 4 fuzz targets                                       | `scripts/fuzz.sh` lists 7 targets                                                                      |
| 3   | Missing chaos tests                | "No chaos testing framework (planned for Sprint 3+)" | `omnia-chaos-tests` crate exists with full ChaosNetwork/ChaosNode framework                            |
| 4   | Test count wrong                   | "278+ tests"                                         | Additional tests in node and chaos-tests crates                                                        |
| 5   | Missing SledNonceStore             | Only mentions SledSlashingStore                      | `SledNonceStore` provides persistent nonce tracking across restarts                                    |
| 6   | Missing REST API security concerns | Not mentioned                                        | REST API has no authentication, no rate limiting, no CORS, no authorization                            |
| 7   | Missing keygen security            | Not mentioned                                        | `run_keygen()` writes unencrypted private keys as raw bytes                                            |
| 8   | Missing trusted setup ceremony     | Not mentioned                                        | `setup-contribute` and `setup-verify` subcommands are available but ceremony security is not addressed |
| 9   | §3.6 could be more detailed        | Lists features but not specific endpoints            | Should enumerate all 9 API endpoints and the Swagger UI                                                |

### 9. docs/adr/ADR-001-event-processor-trait.md

| #   | Discrepancy        | Doc Says     | Code Reality                                                                                          |
| --- | ------------------ | ------------ | ----------------------------------------------------------------------------------------------------- |
| 1   | Generally accurate | Matches code | Minor: The ADR describes the `EventProcessor` trait correctly. The `process_event` signature matches. |

### 10. docs/adr/ADR-003-gossip-substrate-interface.md

| #   | Discrepancy        | Doc Says     | Code Reality                                                                                   |
| --- | ------------------ | ------------ | ---------------------------------------------------------------------------------------------- |
| 1   | Generally accurate | Matches code | Minor: The event flow description matches the substrate's gossip → graph → consensus pipeline. |

### 11. docs/DEPENDENCY_POLICY.md

| #   | Discrepancy                      | Doc Says                              | Code Reality                                                                                                                                                         |
| --- | -------------------------------- | ------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Missing sled alpha warning       | No mention of sled 0.34 alpha quality | `node/Cargo.toml` explicitly warns: "sled 0.34 is alpha-quality. Production deployments should migrate to rocksdb or redb." This should be in the dependency policy. |
| 2   | Missing utoipa/utoipa-swagger-ui | Not mentioned                         | These are new dependencies for the node crate's OpenAPI spec generation                                                                                              |
| 3   | Missing version constraints      | Says "Latest" for all deps            | Actual versions are pinned in Cargo.toml: `axum = "0.7"`, `sled = "0.34"`, `utoipa = "5"`, etc.                                                                      |

### 12. docs/specifications/ARCHITECTURE.md

| #   | Discrepancy                             | Doc Says                                                                  | Code Reality                                                                                                                                                           |
| --- | --------------------------------------- | ------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Test count wrong                        | "200+ tests"                                                              | Now 278+ tests across all crates                                                                                                                                       |
| 2   | REST API status wrong                   | No mention of REST API                                                    | Full REST API with 9 endpoints, Swagger UI, and OpenAPI spec generation exists                                                                                         |
| 3   | Fee Structure status wrong              | "🌑 Not Implemented"                                                      | `FeeSchedule` is fully implemented with standard fees (2-15 UBC per operation). `QuotaSystem` deducts fees atomically.                                                 |
| 4   | Slashing status wrong in security table | "🌑 Not implemented" for economic security (slashing, staking)            | `SlashingEngine` is fully implemented with equivocation/liveness/invalid attestation detection, persistent sled storage, and configurable thresholds                   |
| 5   | Quantum Commitments status wrong        | "⚠️ Stub" — "What's not real: 🌑 Requires CRYSTALS-Dilithium integration" | Dilithium verification is real (not a stub). `verify_dilithium()` calls `pqc_dilithium::verify()`. The hybrid Ed25519+Dilithium scheme is implemented.                 |
| 6   | ZK circuit status wrong                 | "⚠️ Stub" for ZK circuit                                                  | `RollupCircuit` is a real R1CS circuit with Groth16 proving/verification. `ExpandedRollupCircuit` adds Merkle path verification. Both use simplified hash placeholder. |
| 7   | Layer 4 is labeled "Identity Layer"     | Separate layer                                                            | Identity is implemented within the shards crate (`IdentityShard`), not as a separate crate/layer                                                                       |
| 8   | Version wrong                           | "Version: 2.0"                                                            | Should be v4.0.0                                                                                                                                                       |
| 9   | Missing node crate                      | Not in architecture                                                       | `omnia-node` provides the binary entrypoint, HTTP server, REST API, CLI, and configuration                                                                             |
| 10  | Missing chaos tests                     | Not mentioned                                                             | `omnia-chaos-tests` provides partition/crash/Byzantine simulation                                                                                                      |

### 13. docs/specifications/IMPLEMENTATION.md

| #   | Discrepancy                | Doc Says                                                                             | Code Reality                                                                                                         |
| --- | -------------------------- | ------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------- |
| 1   | REST API status wrong      | "🌑 REST API (all interaction is via Rust library)"                                  | Full REST API with Swagger UI exists at `/api/v1/*`                                                                  |
| 2   | Fee mechanism status wrong | "🌑 Fee mechanism"                                                                   | `FeeSchedule` + `QuotaSystem` fully implemented with fee deduction before shard dispatch                             |
| 3   | Slashing status wrong      | "🌑 Slashing mechanism"                                                              | `SlashingEngine` fully implemented with persistent sled storage, equivocation/liveness/invalid attestation detection |
| 4   | Test count wrong           | "200+ tests, all passing"                                                            | Now 278+ tests across all crates                                                                                     |
| 5   | Missing node binary        | Not mentioned                                                                        | `omnia-node` binary with CLI, HTTP server, 6 subcommands                                                             |
| 6   | Missing chaos tests        | Not mentioned                                                                        | `omnia-chaos-tests` crate exists                                                                                     |
| 7   | Phase 1 table wrong        | Lists "Fee mechanism", "Slashing", "Real PQC signatures (Dilithium)" as "📋 Planned" | Fee mechanism: ✅ Implemented. Slashing: ✅ Implemented. Dilithium: ✅ Implemented (real verification).              |
| 8   | ZK circuit status wrong    | "🌑 Full ZK circuit (arkworks R1CS) — Not yet started"                               | `RollupCircuit` and `ExpandedRollupCircuit` exist with real R1CS constraints and Groth16 proving/verification        |
| 9   | Version wrong              | "Version: 2.0"                                                                       | Should be v4.0.0                                                                                                     |
| 10  | Missing Swagger/OpenAPI    | Not mentioned                                                                        | `utoipa` + `utoipa-swagger-ui` provide interactive API docs at `/swagger-ui`                                         |
| 11  | Missing Docker deployment  | Not mentioned                                                                        | Dockerfile + docker-compose with 5-node testnet + monitoring stack                                                   |

---

🔙 **Back**: [Audit](./) | 🔄 **Related**: [Attack Surface](../ATTACK_SURFACE.md)
🚀 **Next**: [Self Assessment](../SELF_ASSESSMENT.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../../reference/blueprint-reference.md)
