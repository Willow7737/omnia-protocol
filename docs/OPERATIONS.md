# Omnia Protocol — Operations Guide

**Version:** v4.0.0
**Last Updated:** 2026-03-05

---

## Overview

This document covers the operational aspects of running Omnia Protocol nodes, including database considerations, configuration, and deployment guidance.

---

## Database Backend: sled 0.34

The Omnia node uses **sled 0.34** as its embedded key-value database for two critical persistent stores:

1. **Slashing Store** (`SledSlashingStore`) — Persists validator slash points, offense history, and ejection state across restarts. Configured via `NodeConfig::slashing_data_dir` (default: `<data_dir>/slashing`).

2. **Nonce Store** (`SledNonceStore`) — Persists per-creator nonce tracking for replay protection across restarts. Configured via `NodeConfig::nonce_data_dir` (default: `<data_dir>/nonces`). Production nodes **MUST** use persistent nonce storage; in-memory nonce tracking (the fallback when no data dir is configured) loses replay protection state on restart.

### sled 0.34 Alpha Warning

As noted in `node/Cargo.toml`:

> ⚠️ sled 0.34 is alpha-quality. Production deployments should migrate to rocksdb or redb. See this document for migration guidance.

**Known risks of sled 0.34:**
- Crash consistency issues — data loss on power failure is possible
- No ongoing maintenance — the author has stated it is not recommended for production
- Performance characteristics may change between patch versions
- No formal guarantee of forward compatibility for on-disk format

### Current Usage in Code

```rust
// node/src/main.rs — Slashing persistence
let mut substrate_config = SubstrateConfig::new(node_id_bytes);
substrate_config.slashing_data_dir = Some(slashing_dir.to_path_buf());

// node/src/main.rs — Nonce persistence
let shard_router = create_shard_router(Some(config.nonce_dir().as_path()))?;
```

The `create_shard_router()` function in `node/src/main.rs` opens a sled database at the configured nonce directory and creates a `SledNonceStore::open(&db, "nonces")`. If no directory is provided, it falls back to `ShardRouter::new()` with in-memory nonce tracking.

### Migration Path: sled → rocksdb (Planned)

Migration to rocksdb (or redb) is planned to address the alpha-quality concerns. The migration requires:

1. **Implement `RocksDbSlashingStore`** — A new struct implementing the `SlashingStore` trait using rocksdb as the backend. The trait interface is defined in `substrate/src/slashing.rs` and includes methods like `get_points()`, `set_points()`, `get_status()`, `set_status()`, and `persist_state()`.

2. **Implement `RocksDbNonceStore`** — A new struct implementing the `NonceStore` trait using rocksdb. The trait interface is defined in `shards/src/router.rs` (or `shards/src/lib.rs`) and includes `get_nonce()`, `set_nonce()`, and `check_and_increment()`.

3. **Add a migration tool** — A CLI subcommand or standalone tool that reads existing sled databases and writes the data to rocksdb format. This must handle:
   - Slashing state: per-validator points, status, and offense history
   - Nonce state: per-creator-pubkey nonce counters

4. **Swap the default in `main.rs`** — Replace `SledSlashingStore::open()` and `SledNonceStore::open()` with their rocksdb equivalents. Add a `--db-backend` CLI flag to allow operators to choose.

5. **Update `Cargo.toml`** — Add `rocksdb` dependency, make `sled` optional (or remove it after migration is complete).

### Data Directory Layout

```
data/
├── slashing/          # SledSlashingStore database
│   └── db-*.sled      # sled page files
├── nonces/            # SledNonceStore database
│   └── db-*.sled      # sled page files
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

If the sled database becomes corrupted:

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
