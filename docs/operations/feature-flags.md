# Feature Flag Reference
> 🎯 Audience: Operators
> 🔗 Context: Feature flags and their operational impact for running Omnia Protocol nodes
> 📅 Last Updated: 2026-05-20

## Feature Flags

### `ethereum-live`

**Crate:** `omnia-adapters` / `zk`

**Purpose:** Enables real Ethereum settlement via Alloy RPC integration.

**Without flag (default):** Ethereum adapter runs in **Simulated** mode — full architecture, in-memory state transitions, no real on-chain interaction.

**With flag:** Adds `alloy` v1 dependency for real RPC calls to Ethereum nodes. Supports:
- Real batch submission to OmniaRollup.sol
- Groth16 proof verification on-chain
- State root queries
- Deposit and withdrawal operations
- Gas estimation and confirmation waiting

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

## Runtime Configuration

These are not compile-time feature flags but runtime configuration options that affect node behavior:

### Snapshot Configuration

| Parameter | Default | Description |
|-----------|---------|-------------|
| `snapshot_interval` | 10000 | Take a state snapshot every N events |

### Pruning Configuration

| Parameter | Default | Description |
|-----------|---------|-------------|
| `pruning_depth` | 0 | Events older than (finalized_round - depth) are pruned; 0 = archive mode |

### Readiness Configuration

| Parameter | Default | Description |
|-----------|---------|-------------|
| `readiness_min_peers` | 1 | Minimum peers for readiness probe |
| `readiness_max_finalization_age` | 600 | Max rounds since last finalization for readiness |

### Security Configuration

| Parameter | Default | Description |
|-----------|---------|-------------|
| `OMNIA_JWT_SECRET` | (none) | HMAC secret for JWT auth |
| `OMNIA_AUTHORIZED_CALLERS` | (none) | Comma-separated authorized caller IDs |
| `OMNIA_AUTHORIZED_ADMINS` | (none) | Comma-separated admin caller IDs |
| `OMNIA_RATE_LIMIT_RPS` | (none) | Max requests per second per IP |

### Logging Configuration

| Parameter | Default | Description |
|-----------|---------|-------------|
| `RUST_LOG` | `info` | Log level (trace, debug, info, warn, error) |
| `RUST_LOG_FORMAT` | (default) | Set to `json` for structured JSON logging |

---
🔙 **Back**: [operations/](./) | 🔄 **Related**: [../building/feature-matrix.md](../building/feature-matrix.md)
🚀 **Next**: [../reference/](../reference/) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)
