# Binary Optimization

> 🎯 Audience: Developers
> 🔗 Context: Binary size and release optimization for the omnia-node binary
> 📅 Last Updated: 2026-05-20

## Release Profile Configuration

The workspace `Cargo.toml` includes aggressive release optimization settings:

```toml
[profile.release]
panic = "abort"        # No unwinding — smaller binary
lto = "fat"            # Full cross-crate LTO for maximum dead-code elimination
codegen-units = 1      # Single codegen unit for maximum optimization
strip = "symbols"      # Strip debug symbols
debug = false          # No debug info
```

**Target:** ≤12MB binary size for `omnia-node`.

## Build Commands

```bash
# Optimized release build
cargo build --release -p omnia-node

# Check binary size
ls -lh target/release/omnia-node

# With Ethereum live mode (adds alloy, increases binary size)
cargo build --release -p omnia-node --features ethereum-live
```

## Size Reduction Techniques

### What We Already Do

1. **`panic = "abort"`** — Removes unwinding infrastructure (~100-200KB savings)
2. **`lto = "fat"`** — Cross-crate inlining and dead-code elimination (significant savings)
3. **`codegen-units = 1`** — Allows the optimizer to see the full picture
4. **`strip = "symbols"`** — Removes debug symbols

### Further Optimization (Future)

- **`upx` compression** — Additional binary compression for distribution
- **Feature-gated dependencies** — Keep heavy deps like `alloy` behind feature flags
- **Minimal runtime** — Exclude unnecessary default features from dependencies

## Reproducible Builds

A reproducible build script is available:

```bash
./scripts/reproducible-build.sh
```

This ensures deterministic binary output across build environments. The script pins the Rust toolchain, sets deterministic environment variables, and verifies the build hash.

---

🔙 **Back**: [building/](./) | 🔄 **Related**: [feature-matrix.md](./feature-matrix.md)
🚀 **Next**: [../operations/](../operations/) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)
