# Reproducible Release Baseline

> **Version**: 0.1.95  
> **Status**: Active  
> **Last Updated**: 2026-08-15  
> **Owner**: Release Engineer  
> **Spec Reference**: Financial Specification §17, Gate 0

---

## 1. Purpose

This document establishes the single source of truth for how Omnia Protocol releases are built, versioned, and verified. Per Financial Specification §17 Gate 0, the repository, release automation, build, deployment, five-node testnet, monitoring, and rollback process must be reproducible.

Every release MUST be reproducible from this baseline — any developer cloning the repo at a tagged commit and following these steps MUST arrive at an identical binary.

---

## 2. Release Version Truth

### 2.1 Canonical Version Locations

| File | Role |
|------|-------|
| `Cargo.toml` (workspace root) | `package.version` — read by Cargo at compile time |
| `.release-please-manifest.json` | `".": "x.y.z"` — read by release-please to determine next version |

**Rule**: These two files MUST always agree. If they diverge, the release is broken.

### 2.2 Version Verification

```bash
# Both must print the same version
rg '^version = "' /opt/omnia-protocol/Cargo.toml | head -1
jq -r '."."' /opt/omnia-protocol/.release-please-manifest.json

# Binary must also report it
./target/release/omnia-node --version
```

If all three do not match, **do not proceed**.

### 2.3 Git Tags

Tags follow `v{major}.{minor}.{patch}` format. MUST be annotated. MUST be pushed to `origin`.

```bash
git tag -v v0.1.95
```

---

## 3. Release-Please Configuration

### Working Configuration (`.release-please-config.json`)

```json
{
  "release-type": "rust",
  "extra-files": [
    {"type": "toml", "path": "Cargo.toml"},
    {"type": "toml", "path": "Cargo.lock"}
  ]
}
```

### What Broke Before

The previous config used `"release-type": "simple"` with bare-string `extra-files` (`"Cargo.toml"`). The `"simple"` type cannot parse TOML — it silently skipped Cargo files while advancing tags, causing version drift (tag `v0.1.95` but `Cargo.toml` at `0.1.92`).

### Fix

Changed to `"rust"` release type with typed `extra-files` objects: `{"type": "toml", "path": "Cargo.toml"}`.

---

## 4. Build Reproducibility

### Docker Build (Production)

```bash
cd /opt/omnia-protocol/docker
docker compose -f docker-compose.wan.yml build --no-cache omnia-node
docker compose -f docker-compose.wan.yml up -d
```

**`--no-cache` is mandatory.** Without it, Docker reuses cached Rust compilation layers and a version bump in `Cargo.toml` will NOT be reflected in the running binary.

### Verification Checklist (all 5 nodes)

```bash
docker exec omnia-node-1 omnia-node --version   # Expected: omnia-node 0.1.95
docker exec omnia-node-1 grep '^version' /app/Cargo.toml | head -1
```

### Deterministic Builds (Future)

Full bit-for-bit determinism requires: pinned Rust toolchain (`rust-toolchain.toml`), committed `Cargo.lock`, pinned Docker base image SHA, no timestamp/path embedding. Phase 1 goal.

---

## 5. Release Workflow

### Automated (release-please)

1. Contributor merges conventional commit into `dev`
2. release-please creates/updates Release PR with version bump, CHANGELOG, manifest
3. Maintainer reviews: verify `Cargo.toml`, `Cargo.lock`, `.release-please-manifest.json` all agree
4. Merge Release PR → release-please creates tag and GitHub Release
5. CI builds release binary and publishes Docker image

### Manual Emergency Release

```bash
cargo new-version 0.1.96  # or manual edit
# Edit .release-please-manifest.json to match
git add Cargo.toml Cargo.lock .release-please-manifest.json
git commit -m "chore: bump version to 0.1.96"
git tag -a v0.1.96 -m "Release 0.1.96"
git push origin dev --tags
```

---

## 6. Workspace Crate Versioning

~14 internal crates (omnia-node, omnia-primitives, omnia-rpc, omnia-consensus, etc.) MUST all share the root version. When bumping:

```bash
cargo update --workspace
# Verify
grep -c 'name = "omnia-' Cargo.lock  # should show 14 entries
grep -A1 'name = "omnia-' Cargo.lock | grep version | sort -u  # exactly one version
```

---

## 7. Monitoring Tie-In

- Each node exposes version via `/health` or `/version` endpoint
- Prometheus scrapes it; Grafana dashboard shows all 5 nodes
- If nodes report different versions → alert

---

## 8. Post-Mortem: v0.1.93–v0.1.95 Version Drift

| Field | Detail |
|-------|--------|
| **Detection** | GitHub tag `v0.1.95` but running binary reported `0.1.92` |
| **Root Cause** | `release-please-config.json` used `"simple"` type with bare-string extra-files; TOML silently skipped |
| **Impact** | Three tags (v0.1.93–95) created without updating Cargo.toml/Cargo.lock |
| **Fix** | `"rust"` type + typed extra-files objects; manual version bump; all 5 nodes rebuilt `--no-cache` |
| **Prevention** | This document; version-check in CI; `--no-cache` mandated for production |

---

## 9. Per-Release Checklist

- [ ] `.release-please-manifest.json` matches `Cargo.toml` root version
- [ ] `Cargo.lock` updated — all omnia-* crates same version
- [ ] Git tag is annotated and matches version
- [ ] `docker compose build --no-cache` used
- [ ] All 5 nodes verified: `omnia-node --version` matches tag
- [ ] Grafana dashboard confirms all nodes same version
- [ ] CHANGELOG.md updated
- [ ] GitHub Release published
