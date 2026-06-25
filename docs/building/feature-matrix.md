# Feature Matrix

> 🎯 Audience: Developers
> 🔗 Context: Feature flags and build profiles for the Omnia Protocol workspace
> 📅 Last Updated: 2026-06-24

## Workspace Feature Flags

### `ethereum-live` (omnia-adapters, omnia-substrate, omnia-node)

Enables real Ethereum settlement via `alloy` v1 dependency. Without this flag, the Ethereum adapter runs in **Simulated** mode (in-memory state transitions only). Requires Rust 1.91+.

```toml
# In omnia-adapters/Cargo.toml
[features]
ethereum-live = ["dep:alloy"]
```

**Effect on build:**

- **Without flag**: Lightweight build, no alloy dependency (300+ sub-crates excluded)
- **With flag**: Adds `alloy` v1 with contract bindings, real RPC calls, gas estimation
- **CI**: Feature-gated tests run separately in `.github/workflows/ethereum-settlement.yml`

### `arkworks` (omnia-adapters, omnia-substrate, omnia-node)

Enables ZK circuit code (arkworks R1CS + Groth16 proof system on BN254). Without this flag, ZK-related modules are not compiled.

```toml
# In omnia-adapters/Cargo.toml
[features]
arkworks = [
    "dep:ark-ff", "dep:ark-ec", "dep:ark-serialize", "dep:ark-groth16",
    "dep:ark-bn254", "dep:ark-relations", "dep:ark-r1cs-std", "dep:ark-snark",
    "dep:ark-crypto-primitives", "dep:rand", "dep:rand_chacha",
]
```

### `settlement-ffi` (omnia-adapters)

Enables the FFI-based settlement adapter for production deployments via a pre-compiled C library. Requires `libsettlement.a` (or `.so`) in the `lib/` directory.

```toml
# In omnia-adapters/Cargo.toml
[features]
settlement-ffi = []
```

### `pqc` (omnia-binding, omnia-crypto, omnia-substrate, omnia-node)

Enables post-quantum cryptography: CRYSTALS-Dilithium signatures and ML-KEM-768 key encapsulation. Without this flag, PQC operations return errors and Dilithium verification always fails.

```toml
# In omnia-binding/Cargo.toml
[features]
pqc = ["dep:pqc_dilithium", "dep:ml-kem"]
```

### `bls` (omnia-crypto, omnia-substrate, omnia-node)

Enables BLS signature aggregation via the `blst` crate.

```toml
# In omnia-crypto/Cargo.toml
[features]
bls = ["dep:blst", "dep:getrandom", "dep:aes-gcm", "dep:hkdf"]
```

### `keystore` (omnia-crypto)

Enables encrypted keystore with AES-256-GCM, BIP-39 mnemonic support, and HKDF key derivation. This is a **default feature**.

```toml
# In omnia-crypto/Cargo.toml
[features]
default = ["keystore"]
keystore = ["dep:aes-gcm", "dep:bip39", "dep:hkdf"]
```

### `persistent-storage` (omnia-consensus, omnia-substrate)

Enables consensus state persistence across restarts via `RedbConsensusStore`.

```toml
# In omnia-consensus/Cargo.toml
[features]
persistent-storage = []
```

### `network` (omnia-network, omnia-substrate, omnia-node)

Enables libp2p-based P2P networking with gossipsub, Kademlia DHT, AutoNAT, and relay. This is a **default feature** in `omnia-network`.

```toml
# In omnia-network/Cargo.toml
[features]
default = ["network"]
network = ["dep:libp2p", "dep:tokio", "dep:futures"]
```

### `metrics` (omnia-node)

Enables Prometheus metrics counters and gauges via the `prometheus` crate.

### `swagger-ui` (omnia-node)

Enables Swagger UI via `utoipa-swagger-ui`.

### `snapshot` (omnia-network)

Enables snapshot sync support for fast state synchronization.

### `cosmos` / `ceremony` (omnia-adapters)

Compile-time gates for Cosmos settlement adapter and trusted setup ceremony tooling.

## omnia-node Feature Profiles

The `omnia-node` crate provides two pre-configured feature profiles:

```toml
# In omnia-node/Cargo.toml
[features]
default = ["full"]
full = ["network", "zk", "bls", "pqc", "metrics"]
light = []  # Minimal build: no networking, no ZK, no heavy crypto
```

| Profile                                  | Features Enabled               | Binary Size | Use Case                            |
| ---------------------------------------- | ------------------------------ | ----------- | ----------------------------------- |
| `full` (default)                         | network, zk, bls, pqc, metrics | ~12 MB      | Full node operator                  |
| `light`                                  | none (minimal)                 | ~4 MB       | Development, testing                |
| `--features ethereum-live`               | full + ethereum-live           | ~14 MB      | Live Ethereum settlement            |
| `--no-default-features --features light` | minimal                        | ~4 MB       | Embedded / constrained environments |

## Default Features

Most workspace crates build with minimal features by default. The exceptions are:

| Crate           | Default Feature(s) |
| --------------- | ------------------ |
| `omnia-crypto`  | `keystore`         |
| `omnia-network` | `network`          |
| `omnia-node`    | `full`             |

All core consensus, shards, binding, economics, and node functionality is available without additional feature flags.

## Build Profiles

### Release Profile

Configured in workspace `Cargo.toml` for maximum optimization:

```toml
[profile.release]
panic = "abort"        # No unwinding — smaller binary, no catch_unwind needed
lto = "fat"            # Cross-crate inlining and dead-code elimination
codegen-units = 1      # Maximum optimization opportunities
strip = "symbols"      # Strip debug symbols from binary
debug = false          # No debug info in release
```

Target: ≤12MB binary size for `omnia-node`.

### Bench Profile

Inherits release but keeps debug info for profiling:

```toml
[profile.bench]
inherits = "release"
debug = true
strip = false
```

## Complete Feature Reference

| Crate             | Feature              | Effect                                        |
| ----------------- | -------------------- | --------------------------------------------- |
| `omnia-adapters`  | `arkworks`           | ZK circuit (arkworks R1CS + Groth16 on BN254) |
| `omnia-adapters`  | `ethereum-live`      | Real Ethereum RPC via Alloy                   |
| `omnia-adapters`  | `settlement-ffi`     | FFI-based settlement via C library            |
| `omnia-adapters`  | `cosmos`             | Cosmos settlement adapter (compile-time gate) |
| `omnia-adapters`  | `ceremony`           | Trusted setup ceremony tooling                |
| `omnia-binding`   | `pqc`                | Post-quantum crypto (Dilithium + ML-KEM-768)  |
| `omnia-crypto`    | `bls`                | BLS signature aggregation (blst)              |
| `omnia-crypto`    | `pqc`                | Post-quantum crypto (Dilithium + ML-KEM)      |
| `omnia-crypto`    | `keystore`           | Encrypted keystore (AES-256-GCM + BIP-39)     |
| `omnia-consensus` | `persistent-storage` | Consensus state persistence (redb)            |
| `omnia-network`   | `network`            | libp2p P2P networking (default)               |
| `omnia-network`   | `snapshot`           | Snapshot sync support                         |
| `omnia-substrate` | `network`            | P2P networking (via omnia-network)            |
| `omnia-substrate` | `zk`                 | ZK circuit (via omnia-adapters/arkworks)      |
| `omnia-substrate` | `bls`                | BLS signatures (via omnia-crypto/bls)         |
| `omnia-substrate` | `pqc`                | Post-quantum crypto (via omnia-crypto/pqc)    |
| `omnia-substrate` | `persistent-storage` | Consensus state persistence                   |
| `omnia-substrate` | `ethereum-live`      | Live Ethereum settlement                      |
| `omnia-substrate` | `migration`          | sled migration support                        |
| `omnia-node`      | `full`               | network + zk + bls + pqc + metrics (default)  |
| `omnia-node`      | `light`              | Minimal build                                 |
| `omnia-node`      | `metrics`            | Prometheus metrics                            |
| `omnia-node`      | `swagger-ui`         | Swagger UI                                    |
| `omnia-node`      | `ethereum-live`      | Live Ethereum settlement                      |

## Verification

```bash
# Check feature resolution
cargo tree --features ethereum-live

# Build with all features
cargo build --all-features

# Build without default features
cargo build --no-default-features

# Build node with light profile
cargo build -p omnia-node --no-default-features --features light

# Build with PQC support
cargo build --features pqc
```

---

🔙 **Back**: [building/](./) | 🔄 **Related**: [cross-compilation.md](./cross-compilation.md)
🚀 **Next**: [cross-compilation.md](./cross-compilation.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)
