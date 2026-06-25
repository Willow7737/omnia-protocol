# Validator Setup Guide

> 🎯 Audience: Operators
> 🔗 Context: Step-by-step guide for setting up and running an Omnia Protocol validator node
> 📅 Last Updated: 2026-06-24

## Prerequisites

- Omnia binary installed at `/usr/local/bin/omnia-node`
- Data directory exists (default: `./data`)
- Config file prepared (optional: `omnia-node.toml`)
- Rust 1.85+ runtime (Docker image: `rust:1.85-slim-bookworm`)

## Step 1: Generate Validator Keypair

```sh
# Generate a keypair with encrypted storage (RECOMMENDED for production)
omnia-node keygen --output-dir ./keys --passphrase "your-secure-passphrase"

# This creates:
# - validator_pubkey.txt — hex-encoded Ed25519 public key
# - validator_key.enc  — AES-256-GCM encrypted private key
```

⚠️ **Security Warning:** Without `--passphrase`, the private key file is written as raw bytes without encryption. Always use `--passphrase` for production.

The `keygen` subcommand also supports BIP-39 mnemonic key generation for HD key derivation (SLIP-0010: `m/44'/6061'/{purpose}'/{index}'`).

## Step 2: Configure the Node

### Via CLI Flags

```sh
omnia-node \
  --node-id 1 \
  --http-port 8080 \
  --data-dir ./data \
  --log-level info
```

### Via TOML Config File

```sh
cat > omnia-node.toml <<EOF
node_id = 1
http_port = 8081
data_dir = "./data"
log_level = "info"
snapshot_interval = 10000
EOF

omnia-node --config omnia-node.toml
```

### Via Environment Variables

```sh
OMNIA_NODE_ID=1 OMNIA_HTTP_PORT=8080 OMNIA_LOG_LEVEL=info omnia-node
```

**Configuration precedence:** CLI flags > env vars > TOML config file > defaults.

### Additional Environment Variables

| Variable                | Default                     | Description                                                              |
| ----------------------- | --------------------------- | ------------------------------------------------------------------------ |
| `OMNIA_NODE_KEY_FILE`   | `data_dir/node_key.bin`     | Path to persistent libp2p node keypair (v0.1.69 fix; falls back to file) |
| `OMNIA_CORS_ORIGINS`    | (none)                      | Comma-separated list of allowed CORS origins for the HTTP API            |

### Important Configuration Notes

- `node_id` must be a non-zero `u64` value (strings like `bootstrap` or `node1` are invalid)
- `http_port` must be non-zero
- `protocol-version` defaults to `"4.0.0"`
- `snapshot_interval` defaults to 10,000 events
- `pruning_depth` of 0 means archive mode (no events pruned)

## Step 3: Start with Bootstrap Peers

```sh
omnia-node \
  --node-id 2 \
  --http-port 8081 \
  --bootstrap-nodes "/ip4/1.2.3.4/udp/4001/quic/p2p/12D3KooWExample"
```

## Step 4: Verify Node Health

```sh
# Liveness probe (always returns 200 if process is alive)
curl http://localhost:8080/healthz

# Readiness probe (returns 200 when node has peers, not syncing, recent finalization)
curl http://localhost:8080/readyz

# Node info
curl http://localhost:8080/api/v1/node/info

# Prometheus metrics
curl http://localhost:8080/metrics
```

## Startup Sequence

When `omnia-node` starts, it follows this sequence:

1. Parse CLI arguments and dispatch subcommands (keygen, setup-contribute, etc.)
2. Build `NodeConfig` from CLI args, env vars, and optional TOML file
3. Validate configuration (`node_id != 0`, `http_port != 0`, valid log level)
4. Initialize structured logging (tracing) — supports JSON output via `RUST_LOG_FORMAT=json`
5. Create data directory if it doesn't exist
6. Initialize substrate with persistent slashing engine (redb)
7. Create shard router with persistent nonce store (redb) and register all 6 shard types
8. Initialize economics state (10% decay, 1000 UBC/month quota)
9. Initialize Prometheus metrics (`NodeMetrics`)
10. Build shared `AppState` and mount HTTP router
11. Start axum HTTP server on `0.0.0.0:{http_port}`
12. Wait for SIGINT/SIGTERM for graceful shutdown

**v0.1.69 audit fixes (added to startup sequence):**

- After keypair generation: Load persistent node keypair via `load_or_generate_node_keypair()` — loads from `OMNIA_NODE_KEY_FILE` or `data_dir/node_key.bin` (v0.1.69 fix: previously always generated ephemeral keys)
- After substrate init: Register node as validator candidate via `substrate.add_validator()` (v0.1.69 fix: previously never registered, node could never be elected leader)
- After HTTP server setup: HTTP server uses `into_make_service_with_connect_info::<SocketAddr>()` (v0.1.69 fix: required for per-client rate limiting)

## Data Directory Layout

```
data/
├── slashing/          # RedbSlashingStore database
│   └── slashing.redb
├── nonces/            # RedbNonceStore database
│   └── nonces.redb
├── consensus/         # RedbConsensusStore database
│   └── consensus.redb
├── snapshots/         # State snapshots (via CLI subcommand)
└── node_key.bin       # Persistent libp2p node keypair (v0.1.69)
```

## Docker Deployment

```sh
cd docker
cp .env.example .env
# Edit .env to set GRAFANA_ADMIN_PASSWORD
docker compose up -d

# With monitoring
docker compose --profile monitoring up -d
```

## REST API Security

The REST API requires JWT authentication:

```sh
# Set required environment variables
export OMNIA_JWT_SECRET="your-hmac-secret"
export OMNIA_AUTHORIZED_CALLERS="caller1,caller2"
export OMNIA_RATE_LIMIT_RPS=10

omnia-node --node-id 1 --http-port 8080
```

- `OMNIA_JWT_SECRET` — HMAC secret for JWT validation (required; API returns 401 if not set)
- `OMNIA_AUTHORIZED_CALLERS` — Comma-separated list of authorized caller IDs
- `OMNIA_RATE_LIMIT_RPS` — Maximum requests per second per IP
- Privileged operations (mint, advance_epoch) require admin JWT

---

🔙 **Back**: [operations/](./) | 🔄 **Related**: [runbook.md](./runbook.md)
🚀 **Next**: [monitoring.md](./monitoring.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)
