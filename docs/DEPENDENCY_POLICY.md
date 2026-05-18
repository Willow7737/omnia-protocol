# Dependency Policy

**Version:** v4.0.0
**Last Updated:** 2026-03-05

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

**Purpose:** Embedded key-value database for persistent slashing state (`RedbSlashingStore`) and nonce tracking (`RedbNonceStore`).

**Properties:**
- ACID transactions with crash-safe durability
- Pure Rust implementation with no unsafe code in the storage layer
- Single-file database with forward compatibility guarantees
- Actively maintained with regular releases

**Migration note:** The codebase previously used sled 0.34 (alpha-quality), which has been replaced with redb. See `docs/OPERATIONS.md` for operational details.

### Phase 0 Security Dependencies

The following dependencies were added during Phase 0 to address critical security findings:

| Dependency | Version | Purpose | Finding |
|---|---|---|---|
| `jsonwebtoken` | 9.x | JWT authentication for REST API | FIND-001 |
| `aes-gcm` | 0.10.x | AES-256-GCM encryption for EncryptedKeyStore | FIND-010 |
| `hkdf` | 0.12.x | HKDF-SHA256 key derivation from passphrases | FIND-010 |
| `sha2` | 0.10.x | SHA-256 for HKDF key derivation | FIND-010 |
| `tower-http` | 0.6.x | CORS middleware for REST API | FIND-001 |
| `subtle` | 2.x | Constant-time comparisons for creator binding | FIND-003 |

## Supply Chain Auditing

We use two tools for supply chain security:

### cargo-audit

Checks the `Cargo.lock` against the RustSec advisory database for known
vulnerabilities. This runs in CI on every push and PR.

```bash
cargo audit --deny warnings
```

### cargo-vet

Audits dependencies for correctness and safety by importing audit records
from trusted sources (Mozilla, Google) and maintaining our own audit records
in `supply-chain/audits.toml`.

```bash
cargo vet --check
```

### CycloneDX SBOM

Generate a Software Bill of Materials for supply chain transparency:

```bash
./scripts/generate-sbom.sh
```

This produces CycloneDX JSON and XML files in the `sbom/` directory.

## Node Crate Specific Dependencies

The `omnia-node` crate (`node/Cargo.toml`) has these notable dependencies:

| Dependency | Version | Purpose | Security Note |
|---|---|---|---|
| `axum` | "0.7" | HTTP framework | Well-maintained; no auth/rate-limiting built in (application's responsibility) |
| `clap` | "4" (derive, env) | CLI parsing | Well-maintained; env var override support is a feature, not a risk |
| `utoipa` | "5" | OpenAPI spec generation | No security implications; auto-docs only |
| `utoipa-swagger-ui` | "8" (axum feature) | Swagger UI | Serves static assets; no dynamic code execution |
| `redb` | "2" | Embedded database | Production-quality; ACID transactions, pure Rust, crash-safe |
| `toml` | "0.8" | Config file parsing | Well-maintained; `deny_unknown_fields` prevents config injection |
| `chrono` | "0.4" (serde feature) | Timestamp handling | No known issues |
| `uuid` | "1" (v4 feature) | ID generation | Cryptographically random v4 UUIDs |
| `hex` | "0.4" | Hex encoding | No security implications |
| `jsonwebtoken` | 9.x | JWT token creation and validation | Well-maintained; used for REST API auth (FIND-001) |
| `aes-gcm` | 0.10.x | AES-256-GCM encryption for key storage | NIST-standardized AEAD; well-audited (FIND-010) |
| `hkdf` | 0.12.x | HKDF-SHA256 key derivation for key encryption | RFC 5869; well-audited (FIND-010) |
| `sha2` | 0.10.x | SHA-256 for HKDF key derivation | NIST-standardized; well-audited |
| `tower-http` | 0.6.x | CORS middleware for REST API | Well-maintained; no security implications (FIND-001) |
| `subtle` | 2.x | Constant-time comparisons for creator binding | Well-audited; standard constant-time library (FIND-003) |

## Exemptions

Crates that cannot be fully audited should be listed in `supply-chain/config.toml`
under `[[exemptions]]` with a comment explaining why:

```toml
[[exemptions]]
name = "some-crate"
version = "1.0.0"
criteria = "safe-to-deploy"
notes = "Uses unsafe for FFI bindings to system library; API surface reviewed, no unsafe in public API"
```

Exemptions MUST be reviewed at least quarterly and either audited or removed.
