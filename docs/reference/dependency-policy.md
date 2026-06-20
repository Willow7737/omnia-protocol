# Dependency Policy

> 🎯 Audience: Developers
> 🔗 Context: Policy for managing dependencies — pinning, audits, and exemptions
> 📅 Last Updated: 2026-05-20

## Pinning

- All dependencies MUST be pinned via `Cargo.lock`
- `Cargo.lock` MUST be committed to the repository
- CI MUST fail if `Cargo.lock` is out of date (enforced via `cargo test --locked --workspace --no-run`)

## Audit Requirements

- All dependencies MUST pass `cargo audit --deny warnings`
- All dependencies SHOULD pass `cargo vet` (exemptions allowed with justification)
- RUSTSEC critical/high advisories MUST be patched within 48 hours
- RUSTSEC medium advisories MUST be patched within 1 week

## Adding New Dependencies

New dependencies MUST be reviewed for:

- **Maintenance status**: Recent commits, responsive maintainer, active issue tracker
- **Known vulnerabilities**: `cargo audit` must be clean
- **License compatibility**: Must be MIT, Apache-2.0, BSD-2/3, or MPL-2.0
- **Transitive dependency count**: Prefer minimal dependencies
- **Unsafe code**: Crates with `unsafe` in their public API must be explicitly approved

### Review Process

1. Run `cargo audit` and `cargo vet` after adding the dependency
2. Review the crate's README, CHANGELOG, and any security advisories
3. Check the crate's transitive dependency tree for unexpected additions
4. Document the review in the commit message or PR description

## Forbidden Dependencies

- Crates with `unsafe` in their public API that haven't been audited
- Crates with known unpatched RUSTSEC advisories
- Crates that don't build with `--deny warnings`
- Crates with obfuscated or minified source code
- Crates that download code at build time (supply chain attack risk)

## Database Dependencies

### redb

**Used by:** `omnia-node` (via `omnia-substrate` and `omnia-shards`)

**Purpose:** Embedded key-value database for persistent slashing state (`RedbSlashingStore`), nonce tracking (`RedbNonceStore`), and consensus state (`RedbConsensusStore`).

**Properties:**

- ACID transactions with crash-safe durability
- Pure Rust implementation with no unsafe code in the storage layer
- Single-file database with forward compatibility guarantees
- Actively maintained with regular releases

**Migration note:** The codebase previously used sled 0.34 (alpha-quality), which has been replaced with redb. Sled has been fully removed from the dependency tree.

## Supply Chain Auditing

### cargo-audit

Checks the `Cargo.lock` against the RustSec advisory database for known vulnerabilities.

```bash
cargo audit --deny warnings
```

### cargo-vet

Audits dependencies for correctness and safety by importing audit records from trusted sources (Mozilla, Google) and maintaining our own audit records in `supply-chain/audits.toml`.

```bash
cargo vet --check
```

### CycloneDX SBOM

Generate a Software Bill of Materials for supply chain transparency:

```bash
./scripts/generate-sbom.sh
```

## Exemptions

Crates that cannot be fully audited should be listed in `supply-chain/config.toml` under `[[exemptions]]` with a comment explaining why. Exemptions MUST be reviewed at least quarterly and either audited or removed.

---

🔙 **Back**: [reference/](./) | 🔄 **Related**: [security-audit.md](./security-audit.md)
🚀 **Next**: [crypto-migration.md](./crypto-migration.md) | 📜 **Source of Truth**: [Restructuring Blueprint](./blueprint-reference.md)
