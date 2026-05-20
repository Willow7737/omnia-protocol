# Feature Matrix
> 🎯 Audience: Developers
> 🔗 Context: Feature flags and build profiles for the Omnia Protocol workspace
> 📅 Last Updated: 2026-05-20

## Workspace Feature Flags

### `ethereum-live` (omnia-adapters / zk crate)

Enables real Ethereum settlement via `alloy` v1 dependency. Without this flag, the Ethereum adapter runs in **Simulated** mode (in-memory state transitions only).

```toml
# In omnia-adapters/Cargo.toml
[features]
default = []
ethereum-live = ["alloy"]
```

**Effect on build:**
- **Without flag**: Lightweight build, no alloy dependency (300+ sub-crates excluded)
- **With flag**: Adds `alloy` v1 with contract bindings, real RPC calls, gas estimation
- **CI**: Feature-gated tests run separately in `.github/workflows/ethereum-settlement.yml`

### Default Features

The workspace builds with minimal features by default. All core consensus, shards, binding, economics, and node functionality is available without feature flags.

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

## Crate-Level Features

| Crate | Feature | Effect |
|-------|---------|--------|
| `omnia-adapters` | `ethereum-live` | Real Ethereum RPC via Alloy (default: simulated) |
| `substrate` | (none) | All substrate features included by default |
| `shards` | (none) | All shard types included by default |
| `binding` | (none) | PQC signatures, provenance included by default |
| `economics` | (none) | UBC, governance, quota included by default |
| `node` | (none) | All node features included by default |

## Verification

```bash
# Check feature resolution
cargo tree --features ethereum-live

# Build with all features
cargo build --all-features

# Build without default features
cargo build --no-default-features
```

---
🔙 **Back**: [building/](./) | 🔄 **Related**: [cross-compilation.md](./cross-compilation.md)
🚀 **Next**: [cross-compilation.md](./cross-compilation.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)
