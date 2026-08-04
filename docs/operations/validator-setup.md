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
export OMNIA_JWT_SIGNING_KEY_PATH="/etc/omnia/jwt-signing.pem"
export OMNIA_JWT_VERIFICATION_KEY_PATH="/etc/omnia/jwt-verification.pem"
export OMNIA_JWT_KEY_ID="jwt-2026-08"
export OMNIA_AUTHORIZED_CALLERS="caller1,caller2"
export OMNIA_RATE_LIMIT_RPS=10

omnia-node --node-id 1 --http-port 8080
```

- `OMNIA_JWT_SIGNING_KEY_PATH` / `OMNIA_JWT_SIGNING_KEY` — RS256 private key for nodes or services that issue JWTs.
- `OMNIA_JWT_VERIFICATION_KEY_PATH` / `OMNIA_JWT_VERIFICATION_KEY` — RS256 public key required by API verifiers; authenticated routes return 503 if no verifier key is configured.
- `OMNIA_JWT_KEY_ID` — Optional `kid` value; when set, verifiers reject tokens with any other key ID.
- `OMNIA_JWT_SECRET` + `OMNIA_JWT_ALLOW_LEGACY_HS256=true` — Temporary migration-only compatibility for pre-RS256 HS256 tokens; remove after the longest old token TTL has elapsed.
- `OMNIA_AUTHORIZED_CALLERS` — Comma-separated list of authorized caller IDs
- `OMNIA_RATE_LIMIT_RPS` — Maximum requests per second per IP
- `OMNIA_MINT_AUTHORITY` — Financial-shard mint authority (see below)
- Privileged operations (mint, advance_epoch) require admin JWT

---

## Mint authority (financial shard)

The financial shard — the **transferable** ledger, not soulbound UBC —
accepts a `Mint` only when the event creator matches a configured
authority. Set it with `--mint-authority` or `OMNIA_MINT_AUTHORITY`, as a
64-character hex Ed25519 public key:

```sh
export OMNIA_MINT_AUTHORITY="ed4928c628d1c2c6eae90338905995612959273a5c63f93636c14614ac8737d1"
```

or in the TOML config:

```toml
mint_authority = "ed4928c628d1c2c6eae90338905995612959273a5c63f93636c14614ac8737d1"
```

> ⚠️ **This is a genesis parameter, not a per-node identity. Every node in
> the network must be configured with the SAME key.**
>
> `FinancialState::apply` checks the mint against *its own* configured
> authority. If node A holds its own key and node B holds its own, a mint
> created by A is accepted by A and **rejected by B**. The two then
> disagree about total supply and every balance derived from it — a state
> divergence consensus cannot repair, because each node is behaving
> correctly according to its own configuration.
>
> Decide the authority once, before launch, and roll it to every node.

**Unset means minting is disabled**, and that is the deliberate default.
A node that quietly substituted its own key would produce exactly the
divergence above, and it would look healthy right up until someone minted.
Transfers work fine with minting disabled — accounts simply start at zero
until an authority is configured network-wide.

The node logs its choice at startup, so `grep` the boot log to confirm all
three nodes agree:

```
INFO Financial shard mint authority configured  authority=ed4928c6…
WARN No mint_authority configured … minting on the financial shard is DISABLED
```

A malformed key fails at startup rather than being ignored — the
alternative is a node that boots cleanly and then rejects every mint,
which is far harder to trace back to a typo.

---

## Multi-Node Lane 0 Validator Testnet

To run a multi-node testnet where transfers reach **Lane 0 fast-path
finality** (ADR-025), every node must share the *same* validator set, keyed
by each node's Ed25519 public key. Because a node's pubkey is derived from
its keypair, the set has to be known before boot — a chicken-and-egg the
setup script resolves by pre-generating the keys.

### One-command setup

```sh
# Generates a persistent keypair per node, assembles OMNIA_LANE0_VALIDATORS
# from their public keys, and writes docker/.env (with a fresh
# OMNIA_JWT_SECRET if none exists, and OMNIA_RATE_LIMIT_RPS=1000).
./scripts/setup-validators.sh            # 3 nodes, stake 1 each
# NODES=5 STAKE=1 ./scripts/setup-validators.sh   # to scale
```

This writes `ops/testnet-keys/nodeK/` (git-ignored secrets):
`validator_key.bin` (the raw 32-byte secret each node loads via
`OMNIA_NODE_KEY_FILE`) and `validator_pubkey.txt`. `docker-compose.testnet.yml`
mounts each into its node, so pubkeys — and therefore the validator set —
stay stable across restarts.

### Bring the testnet up

```sh
docker compose -f docker/docker-compose.testnet.yml up -d --build
# add --profile monitoring for Prometheus (:9095) + Grafana (:3000)
```

Compose reads `docker/.env` automatically, so `OMNIA_LANE0_VALIDATORS`,
`OMNIA_JWT_SECRET`, and `OMNIA_RATE_LIMIT_RPS` flow to all three nodes.

### Verify

```sh
# Each node should report a `lane0` stats object (not null) and its own pubkey.
for p in 9090 9091 9092; do
  curl -s http://localhost:$p/api/v1/node/info \
    | python3 -c 'import json,sys;d=json.load(sys.stdin);print(d["node_id"], d["lane0"], d["validator_pubkey"][:8])'
done
```

Once the nodes have meshed (`/api/v1/node/peers` shows peers), a UBC
transfer's provenance event is acked by the stake-weighted quorum and
`GET /api/v1/events/:id` reports `lane0_final: true`. Capture multi-node
throughput/finality with `scripts/testnet-bench.sh` — see the
[testnet benchmark runbook](./testnet-benchmark.md).

### Rotating keys

Delete `ops/testnet-keys/` and re-run `setup-validators.sh` to regenerate
all validator keypairs and the matching set. (Re-running without deleting
reuses existing keys, keeping the set stable.)

### Security note

`setup-validators.sh` writes **unencrypted** raw keys for a local/dev
testnet. Production validators should use the encrypted keygen path
(`--passphrase`, Step 1 above) and a secrets manager, not committed files.

---

🔙 **Back**: [operations/](./) | 🔄 **Related**: [runbook.md](./runbook.md), [testnet-benchmark.md](./testnet-benchmark.md)
🚀 **Next**: [monitoring.md](./monitoring.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)
