# Omnia Protocol — Operations Guide

**Version:** v4.0.0
**Last Updated:** 2026-03-05

---

## Overview

This document covers the operational aspects of running Omnia Protocol nodes, including database considerations, configuration, and deployment guidance.

---

## Database Backend: redb

The Omnia node uses **redb** as its embedded key-value database for two critical persistent stores:

1. **Slashing Store** (`RedbSlashingStore`) — Persists validator slash points, offense history, and ejection state across restarts. Configured via `NodeConfig::slashing_data_dir` (default: `<data_dir>/slashing`).

2. **Nonce Store** (`RedbNonceStore`) — Persists per-creator nonce tracking for replay protection across restarts. Configured via `NodeConfig::nonce_data_dir` (default: `<data_dir>/nonces`). Production nodes **MUST** use persistent nonce storage; in-memory nonce tracking (the fallback when no data dir is configured) loses replay protection state on restart.

### redb Properties

redb is a production-quality embedded key-value store written in pure Rust. Key characteristics:

- **ACID transactions** — All writes are atomic and durable
- **Crash-safe** — Uses a write-ahead log (WAL) to guarantee consistency after power failure
- **Simple, reliable on-disk format** — Single-file database with forward compatibility guarantees
- **Pure Rust** — No C dependencies, no unsafe code in the storage layer
- **Active maintenance** — Regularly updated with ongoing releases

### Current Usage in Code

```rust
// node/src/main.rs — Slashing persistence
let mut substrate_config = SubstrateConfig::new(node_id_bytes);
substrate_config.slashing_data_dir = Some(slashing_dir.to_path_buf());

// node/src/main.rs — Nonce persistence
let shard_router = create_shard_router(Some(config.nonce_dir().as_path()))?;
```

The `create_shard_router()` function in `node/src/main.rs` opens a redb database at the configured nonce directory and creates a `RedbNonceStore::open(&db, "nonces")`. If no directory is provided, it falls back to `ShardRouter::new()` with in-memory nonce tracking.

### Data Directory Layout

```
data/
├── slashing/          # RedbSlashingStore database
│   └── slashing.redb  # redb single-file database
├── nonces/            # RedbNonceStore database
│   └── nonces.redb    # redb single-file database
└── snapshots/         # State snapshots (via CLI subcommand)
```

### Backup Procedure

```sh
# 1. Stop the node
systemctl stop omnia-node

# 2. Backup the entire data directory
tar czf omnia-data-backup-$(date +%Y%m%d).tar.gz ./data/

# 3. Restart the node
systemctl start omnia-node
```

### Recovery from Data Loss

If the redb database becomes corrupted:

```sh
# 1. Stop the node
systemctl stop omnia-node

# 2. Remove the corrupted database
rm -rf ./data/slashing/
rm -rf ./data/nonces/

# 3. Restore from backup (if available)
tar xzf omnia-data-backup-YYYYMMDD.tar.gz

# 4. Or restore from a snapshot
omnia-node restore --input snapshot.bin

# 5. Restart the node
systemctl start omnia-node
```

**Warning:** Without backup or snapshot restoration, all slashing state and nonce tracking will be lost. Validators with accumulated slash points would effectively be "reset" to zero offenses. Nonce replay protection would also be lost, potentially allowing replay of previously processed operations.
