# Building Omnia Protocol

> 🎯 Audience: Developers
> 🔗 Context: Index for build guides, feature profiles, and binary optimization
> 📅 Last Updated: 2026-08-11

## Prerequisites

- Rust 1.85+ (see `rust-toolchain.toml` for exact version)
- System dependencies: `build-essential`, `pkg-config`, `libssl-dev`
- Docker and Docker Compose (for containerized deployment)

## Build Documents

| Document                                         | Description                                                          |
| ------------------------------------------------ | -------------------------------------------------------------------- |
| [feature-matrix.md](feature-matrix.md)           | Feature flags and build profiles — `ethereum-live`, default features |
| [cross-compilation.md](cross-compilation.md)     | Cross-compilation guide — Docker builds, target triples              |
| [binary-optimization.md](binary-optimization.md) | Binary size and release optimization — LTO, codegen-units, strip     |

## Quick Build

```bash
# Full workspace build
cargo build --workspace

# With Ethereum live mode (adds alloy dependency)
cargo build --features ethereum-live

# Release build (optimized)
cargo build --release --workspace
```

---

🔙 **Back**: [docs/](../) | 🔄 **Related**: [operations/](../operations/)
🚀 **Next**: [feature-matrix.md](./feature-matrix.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)
