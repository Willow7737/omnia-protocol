# Feature Flag Reference

> 🎯 Audience: Operators
> 🔗 Context: Feature flags and their operational impact for running Omnia Protocol nodes
> 📅 Last Updated: 2026-05-21

## Feature Flags

### `ethereum-live`

**Crate:** `omnia-adapters`, `omnia-substrate`, `omnia-node`

**Purpose:** Enables real Ethereum settlement via Alloy RPC integration.

**Without flag (default):** Ethereum adapter runs in **Simulated** mode — full architecture, in-memory state transitions, no real on-chain interaction.

**With flag:** Adds `alloy` v1 dependency for real RPC calls to Ethereum nodes. Supports:

- Real batch submission to OmniaRollup.sol
- Groth16 proof verification on-chain
- State root queries
- Deposit and withdrawal operations
- Gas estimation and confirmation waiting

**Requirements:** Rust 1.91+ (alloy dependency requires newer compiler)

**Build:**

```bash
cargo build --features ethereum-live
```

**Docker:**

```bash
docker build --build-arg FEATURES=ethereum-live -f docker/Dockerfile .
```

**CI:** Feature-gated tests run in `.github/workflows/ethereum-settlement.yml` with Anvil.

**Impact on binary size:** Adding `alloy` increases binary size significantly (~300+ sub-crates). Use only when real Ethereum settlement is needed.

---

### `settlement-ffi`

**Crate:** `omnia-adapters`

**Purpose:** Enables FFI-based settlement adapter for production deployments via a pre-compiled C library (`libsettlement.a` or `.so`).

**Without flag (default):** FFI adapter is not compiled. The crate-level `#![deny(unsafe_code)]` lint is enforced normally.

**With flag:** Compiles the `FfiSettlementAdapter` which uses `unsafe` FFI calls to the C library. The FFI module has `#![allow(unsafe_code)]` because FFI intrinsically requires unsafe operations.

**Build:**

```bash
cargo build --features settlement-ffi
```

**Requirements:** Pre-compiled `libsettlement.a` (or `.so`) in the `lib/` directory. If the library is not found, `build.rs` automatically disables this feature.

---

### `arkworks`

**Crate:** `omnia-adapters`, `omnia-substrate`, `omnia-node`

**Purpose:** Enables ZK circuit code — arkworks R1CS constraint system, Groth16 proof generation/verification, Poseidon hash, and trusted setup ceremony.

**Without flag (default):** ZK-related modules (`circuit`, `proof`, `prover`, `setup`, `poseidon`, `operator`) are not compiled. Off-circuit Merkle tree (BLAKE3) is still available.

**With flag:** Full ZK proof system with BN254 curve operations, Poseidon hash gadget, Groth16 proving/verification, and ceremony tooling.

**Build:**

```bash
cargo build --features zk          # via omnia-node
cargo build --features arkworks    # directly on omnia-adapters
```

---

### `pqc`

**Crate:** `omnia-binding`, `omnia-crypto`, `omnia-substrate`, `omnia-node`

**Purpose:** Enables post-quantum cryptography — CRYSTALS-Dilithium digital signatures and ML-KEM-768 key encapsulation.

**Without flag (default in most crates):** PQC signing operations return errors. Dilithium verification always returns `false`. ML-KEM-768 operations are unavailable.

**With flag:** Full hybrid (Ed25519 + Dilithium) and post-quantum-only signing. ML-KEM-768 encapsulation/decapsulation for PQ-secure key exchange.

**Build:**

```bash
cargo build --features pqc
```

---

### `bls`

**Crate:** `omnia-crypto`, `omnia-substrate`, `omnia-node`

**Purpose:** Enables BLS signature aggregation via the `blst` crate.

**Build:**

```bash
cargo build --features bls
```

---

### `metrics`

**Crate:** `omnia-node`

**Purpose:** Enables Prometheus metrics counters and gauges (events submitted, events finalized, peers connected, consensus round, shard ops, HTTP requests).

**Without flag:** The `NodeMetrics` struct and `/metrics` endpoint are not available.

**Build:**

```bash
cargo build --features metrics
```

---

### `persistent-storage`

**Crate:** `omnia-consensus`, `omnia-substrate`

**Purpose:** Enables consensus state persistence across restarts via `RedbConsensusStore`.

**Without flag:** Consensus state is ephemeral (in-memory only). Node must re-sync on restart.

**Build:**

```bash
cargo build --features persistent-storage
```

---

### `swagger-ui`

**Crate:** `omnia-node`

**Purpose:** Embeds Swagger UI assets (~11MB). Required for `/swagger-ui` and `/api-docs/openapi.json` endpoints.

**Without flag (default):** The Swagger UI and OpenAPI JSON endpoints are not available.

**Build:**

```bash
cargo build --features swagger-ui
```

---

## omnia-node Feature Profiles

The `omnia-node` crate provides pre-configured profiles:

| Profile          | Features                       | Build Command                                                      |
| ---------------- | ------------------------------ | ------------------------------------------------------------------ |
| `full` (default) | network, zk, bls, pqc, metrics | `cargo build -p omnia-node`                                        |
| `light`          | minimal                        | `cargo build -p omnia-node --no-default-features --features light` |
| Full + Ethereum  | full + ethereum-live           | `cargo build -p omnia-node --features ethereum-live`               |
| `docker-tests`   | full + docker integration      | `cargo build -p omnia-node --features docker-tests`                |

---

## Runtime Configuration

These are not compile-time feature flags but runtime configuration options that affect node behavior:

### Snapshot Configuration

| Parameter           | Default | Description                          |
| ------------------- | ------- | ------------------------------------ |
| `snapshot_interval` | 10000   | Take a state snapshot every N events |

### Pruning Configuration

| Parameter       | Default | Description                                                              |
| --------------- | ------- | ------------------------------------------------------------------------ |
| `pruning_depth` | 0       | Events older than (finalized_round - depth) are pruned; 0 = archive mode |

### Readiness Configuration

| Parameter                        | Default | Description                                      |
| -------------------------------- | ------- | ------------------------------------------------ |
| `readiness_min_peers`            | 1       | Minimum peers for readiness probe                |
| `readiness_max_finalization_age` | 600     | Max rounds since last finalization for readiness |

### Security Configuration

| Parameter                  | Default | Description                           |
| -------------------------- | ------- | ------------------------------------- |
| `OMNIA_JWT_SECRET`         | (none)  | HMAC secret for JWT auth              |
| `OMNIA_AUTHORIZED_CALLERS` | (none)  | Comma-separated authorized caller IDs |
| `OMNIA_RATE_LIMIT_RPS`     | (none)  | Max requests per second per IP        |

### Logging Configuration

| Parameter         | Default   | Description                                 |
| ----------------- | --------- | ------------------------------------------- |
| `RUST_LOG`        | `info`    | Log level (trace, debug, info, warn, error) |
| `RUST_LOG_FORMAT` | (default) | Set to `json` for structured JSON logging   |

---

🔙 **Back**: [operations/](./) | 🔄 **Related**: [../building/feature-matrix.md](../building/feature-matrix.md)
🚀 **Next**: [../reference/](../reference/) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)
