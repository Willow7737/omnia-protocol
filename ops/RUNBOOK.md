# Omnia Protocol Operations Runbook

This runbook provides step-by-step procedures for common operational tasks
when running Omnia Protocol nodes. Every procedure includes exact CLI commands.

**Version:** v4.0.0
**Last Updated:** 2026-03-05

**Table of Contents:**

1. [Node Startup](#node-startup)
2. [Key Rotation](#key-rotation)
3. [Emergency Slashing](#emergency-slashing)
4. [Network Partition Recovery](#network-partition-recovery)
5. [Node Upgrade](#node-upgrade)
6. [Snapshot and Restore](#snapshot-and-restore)
7. [Monitoring and Alerting](#monitoring-and-alerting)
8. [Trusted Setup Ceremony](#trusted-setup-ceremony)
9. [REST API Reference](#rest-api-reference)

---

## Node Startup

### Prerequisites

- Omnia binary installed at `/usr/local/bin/omnia-node`
- Data directory exists (default: `./data`)
- Config file prepared (optional: `omnia-node.toml`)
- Rust 1.85+ runtime (Docker image: `rust:1.85-slim-bookworm`)

### Starting a New Node

The `omnia-node` binary accepts CLI flags, environment variables (`OMNIA_` prefix), and TOML config files. Configuration precedence: CLI flags > env vars > TOML config file > defaults.

```sh
# Generate a validator keypair
omnia-node keygen --output-dir ./keys

# Start the node with default settings
# node_id must be a non-zero u64, http_port must be non-zero
omnia-node --node-id 1 --http-port 8080

# Or start with a config file
omnia-node --config omnia-node.toml

# Or use environment variables
OMNIA_NODE_ID=1 OMNIA_HTTP_PORT=8080 OMNIA_LOG_LEVEL=info omnia-node
```

**Important:** The `--node-id` flag accepts a `u64` value (not a string). Values like `bootstrap` or `node1` are invalid and will cause startup failure. The node validates that `node_id != 0` and `http_port != 0` at startup.

### Starting with Bootstrap Peers

```sh
# Connect to existing network via bootstrap peers
omnia-node \
  --node-id 2 \
  --http-port 8081 \
  --bootstrap-nodes "/ip4/1.2.3.4/udp/4001/quic/p2p/12D3KooWExample"

# Or specify in config file
cat > omnia-node.toml <<EOF
node_id = 2
http_port = 8081
bootstrap_nodes = ["/ip4/1.2.3.4/udp/4001/quic/p2p/12D3KooWExample"]
EOF
omnia-node --config omnia-node.toml
```

**Note:** In the TOML config file, `node_id` is `Option<u64>` (changed from `Option<u16>` in Phase 0, FIND-013). Both CLI and TOML now support the full `u64` range. The previous `u16` limitation (max 65535) has been removed.

### Starting with Docker Compose

```sh
cd docker

# Copy the example env file and set a secure Grafana password
cp .env.example .env
# Edit .env to change GRAFANA_ADMIN_PASSWORD from the default

# Start the full testnet (5 nodes)
docker compose up -d

# Start with monitoring (Grafana + Prometheus)
docker compose --profile monitoring up -d
```

**Docker port mapping:** The Dockerfile `EXPOSE 9090/tcp 9090/udp`, but the default `http_port` in code is `8080`. When using Docker, you must either set `OMNIA_HTTP_PORT=9090` or map the container port to match. The docker-compose.yml currently uses ports 9090-9094 for the nodes, which requires setting `OMNIA_HTTP_PORT` accordingly.

### Startup Sequence

When `omnia-node` starts, it follows this sequence (see `node/src/main.rs`):

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

### Verifying Node Health

```sh
# Check the health endpoint (returns JSON with status, node_id, peers, finalized_height)
curl http://localhost:8080/health

# Check Prometheus metrics
curl http://localhost:8080/metrics

# Check node info via API
curl http://localhost:8080/api/v1/node/info

# Check logs
docker logs -f omnia-node-1  # Docker
journalctl -u omnia-node -f  # systemd
```

---

## Key Rotation

Key rotation generates a new validator keypair and produces a
cryptographic proof that the rotation was authorized by the previous
key holder.

### Step 1: Backup Current Keys

```sh
# Backup the current key directory
cp -r ./keys ./keys-backup-$(date +%Y%m%d)
```

### Step 2: Generate New Keys

```sh
# Generate a new keypair in a temporary directory
omnia-node keygen --output-dir ./keys-new
```

The `keygen` subcommand creates two files:
- `validator_pubkey.txt` — hex-encoded Ed25519 public key
- `validator_key.enc` (encrypted, recommended) or `validator_key.bin` (unencrypted, ⚠️ not for production) — the private key

**Encryption:** When `--passphrase` is provided (or `OMNIA_KEYGEN_PASSPHRASE` env var is set), the private key is encrypted with AES-256-GCM using a key derived from the passphrase via BLAKE3 domain-separated key derivation. The encrypted file is saved as `validator_key.enc` with magic bytes `OMNIA_KEY_ENC` for identification.

**Security Warning:** Without `--passphrase`, the private key file is written as raw bytes without encryption. This is NOT suitable for production. Always use `--passphrase` for production key generation:
```sh
omnia-node keygen --output-dir ./keys --passphrase "your-secure-passphrase"
```

### Step 3: Rotate Keys (Programmatic)

The key rotation is performed via the keystore API:

```sh
# Use the omnia-substrate keystore rotation
# In practice, this would be called from the node's management API
# or via a dedicated rotation tool.

# The rotation produces a KeyRotationProof that must be broadcast
# to other validators so they update their trusted key set.
```

### Step 4: Verify Rotation

```sh
# After rotation, verify the new public key
cat ./keys/validator_pubkey.txt

# Verify the node starts with the new key
omnia-node --node-id 1 --http-port 8080
curl http://localhost:8080/health
```

### Step 5: Distribute Rotation Proof

```sh
# The KeyRotationProof JSON must be shared with all other validators.
# They verify the signature from the old key over the new public key.
# Once >2/3 of validators acknowledge the rotation, it is finalized.
```

---

## Emergency Slashing

Emergency slashing is triggered when a validator is detected performing
equivocation (signing two different events at the same sequence number)
or other slashable offenses. The `SlashingEngine` tracks three offense types:

| Offense Type | Points | Threshold |
|---|---|---|
| Equivocation | 500 | Slash at 500, Eject at 2000 |
| LivenessViolation | 100 | Slash at 500, Eject at 2000 |
| InvalidAttestation | 300 | Slash at 500, Eject at 2000 |

### Detecting Slashing Events

```sh
# Check slashing metrics
curl -s http://localhost:8080/metrics | grep omnia_slashing

# Check Grafana dashboard (Slashing Events panel)
# http://localhost:3000 → Omnia Node Dashboard
```

**Note:** There is no dedicated `/api/v1/slashing/` endpoint. Slashing state is only visible via Prometheus metrics and logs. The `SlashingEngine` is not exposed through the REST API.

### Responding to a Slash

```sh
# 1. Identify the slashed validator from logs
journalctl -u omnia-node | grep "slash"

# 2. Check slashing metrics for the validator
curl -s http://localhost:8080/metrics | grep omnia_slashing

# 3. If the validator has been ejected (exceeded ejection threshold of 2000 points):
#    - The validator is automatically removed from the active set
#    - No manual intervention needed

# 4. If the validator needs emergency ejection:
#    - Submit an emergency governance proposal to increase slash points
curl -X POST http://localhost:8080/api/v1/governance/proposals \
  -H "Content-Type: application/json" \
  -d '{"id": "emergency-slash-validator-xyz", "description": "Emergency slash for equivocation", "expires_at_epoch": 100}'
```

### Post-Slash Cleanup

```sh
# The slashed validator's state should be checked.
# Restart the affected node if it becomes unresponsive:
docker restart omnia-node-2

# Verify network health after the slash:
curl -s http://localhost:8080/metrics | grep omnia_node_peers_connected
curl -s http://localhost:8080/metrics | grep omnia_node_events_finalized_total
```

**Important:** If the slashing state is stored in-memory (the default `SlashingEngine::new_in_memory()`), all slash points are lost on node restart. Production nodes must use `RedbSlashingStore` for persistence, which is configured automatically when running via `omnia-node` (it sets `substrate_config.slashing_data_dir`).

---

## Network Partition Recovery

Network partitions occur when >1/3 of validators become unreachable. The Omnia BFT consensus requires >2/3 of validators to be online for finality.

### Detecting a Partition

```sh
# Check the Grafana alert: PeerCountDrop
# Or check peer count directly:
curl -s http://localhost:8080/metrics | grep omnia_node_peers_connected

# Check connected peers via API
curl http://localhost:8080/api/v1/node/peers

# Check for partition detection in logs:
journalctl -u omnia-node | grep "partition detected"
```

### Recovery Steps

```sh
# 1. Identify disconnected peers
curl http://localhost:8080/api/v1/node/peers

# 2. Check network connectivity to each peer
ping 172.20.0.3  # Docker network
traceroute 1.2.3.4  # External peer

# 3. The gossip protocol will automatically reconnect when peers are reachable.

# 4. Check bootstrap peer configuration
cat omnia-node.toml | grep bootstrap_nodes

# 5. If bootstrap peers are misconfigured, update and restart:
# Edit omnia-node.toml with correct bootstrap nodes, then:
docker restart omnia-node-1

# 6. Verify partition recovery
curl -s http://localhost:8080/metrics | grep omnia_node_peers_connected
# Should show >2/3 of validators connected

# 7. Check for partition healing in logs:
journalctl -u omnia-node | grep "partition healed"
```

### Handling Split-Brain

If the network experienced a true split-brain (two partitions both
producing blocks), manual intervention is required:

```sh
# 1. Stop all nodes
docker compose down

# 2. Identify the longer chain (higher finalized event count)
# Check each node's data directory:
ls -la data/substrate/
ls -la data/causal_graph/

# 3. The partition with >2/3 of validators has the canonical chain.
#    Reset nodes on the minority partition:
rm -rf /path/to/minority-node/data/slashing/*
rm -rf /path/to/minority-node/data/nonces/*

# 4. Restart all nodes with correct bootstrap configuration
docker compose up -d
```

---

## Node Upgrade

### Rolling Upgrade (Zero-Downtime)

```sh
# 1. Build the new version
cargo build --release -p omnia-node

# 2. Upgrade one node at a time, maintaining >2/3 validator availability

# For each node (example with node-1):

# 2a. Stop the node
docker stop omnia-node-1
# or
systemctl stop omnia-node

# 2b. Backup data
cp -r /path/to/omnia-node-1/data /path/to/backup/data-$(date +%Y%m%d)

# 2c. Replace the binary
cp target/release/omnia-node /usr/local/bin/omnia-node

# 2d. Start the node with the new version
docker start omnia-node-1
# or
systemctl start omnia-node

# 2e. Verify the node is healthy
curl http://localhost:8080/health
curl -s http://localhost:8080/metrics | grep omnia_node_peers_connected

# 2f. Wait for the node to sync and catch up
# Monitor: finalized events/sec should return to normal

# 2g. Repeat for the next node
```

### Protocol Version Check

```sh
# Check the binary version
omnia-node --version

# Check the protocol version of running nodes via the API
curl http://localhost:8080/api/v1/node/info | jq .protocol_version

# The protocol_version is set via --protocol-version flag (default: "4.0.0")
# and also exposed as omnia_substrate::PROTOCOL_VERSION in the node info response
```

---

## Snapshot and Restore

Snapshots capture the substrate state (causal graph, slashing state, nonces) at a given height. The snapshot system is implemented in `omnia-substrate::snapshot::StateSnapshot`.

### Taking a Snapshot via CLI

```sh
# Take a snapshot using the CLI subcommand
omnia-node snapshot --output snapshot.bin

# The snapshot includes:
# - version (format version)
# - height (current consensus height)
# - event_count (number of events in the graph)
# - state_root (BLAKE3 Merkle root of the graph state)
# - serialized CausalGraph, SlashingState, and nonce map
# - timestamp (Unix seconds)
```

**Note:** There is no HTTP API endpoint for taking snapshots. Snapshots are only available via the CLI `snapshot` subcommand. The `run_snapshot()` function in `node/src/main.rs` creates a minimal snapshot from a fresh graph — for production use, you would want to snapshot from a running node's state.

### Restoring from a Snapshot via CLI

```sh
# Restore from a snapshot file
omnia-node restore --input snapshot.bin

# The restore subcommand:
# 1. Reads the snapshot file
# 2. Verifies integrity (state root hash check)
# 3. Prints summary information (version, height, event count, state root, timestamp)
```

**Note:** Like `snapshot`, the `restore` subcommand operates on a standalone snapshot file. It does not automatically integrate the restored state into a running node's substrate. Integration of restored state into a live node would require additional tooling.

### Restoring from a Snapshot (Manual)

```sh
# 1. Stop the node
systemctl stop omnia-node

# 2. Backup current data (in case restore fails)
cp -r ./data ./data-pre-restore-$(date +%Y%m%d)

# 3. Remove current state
rm -rf ./data/slashing/*
rm -rf ./data/nonces/*

# 4. Copy snapshot data (if you have a full data backup)
cp -r ./data/snapshots/latest/* ./data/

# 5. Restart the node
systemctl start omnia-node

# 6. Verify the node syncs from the snapshot state
curl http://localhost:8080/health
curl -s http://localhost:8080/metrics | grep omnia_causal_graph_total_events
```

### Automated Snapshots

The substrate supports automatic snapshots via the `snapshot_interval` configuration. When `snapshot_interval > 0`, a snapshot is taken every N events. Configure in `omnia-node.toml`:

```toml
# Take a snapshot every 10,000 events (default)
snapshot_interval = 10000
```

The `snapshot_interval` is passed to `SubstrateConfig` in `node/src/main.rs`:

```rust
substrate_config.snapshot_interval = config.snapshot_interval;
```

### Pruning Configuration

```sh
# Configure pruning depth to limit disk usage
# In omnia-node.toml:
# pruning_depth = 10000  # Keep last 10,000 rounds

# With pruning_depth = 0 (default), no events are pruned (archive mode)
# With pruning_depth > 0, events older than that many rounds are pruned
```

---

## Monitoring and Alerting

### Starting the Monitoring Stack

```sh
cd docker
docker compose --profile monitoring up -d

# Access Grafana: http://localhost:3000 (admin / password from docker/.env)
# Access Prometheus: http://localhost:9095
```

### Dashboard Panels

The Omnia Node Dashboard (`omnia-node.json`) includes 9 panels:

| Panel | Metric | Type |
|-------|--------|------|
| Finalized Events/sec | `rate(omnia_node_events_finalized_total[1m])` | timeseries |
| Peer Count | `omnia_node_peers_connected` | stat |
| Consensus Round Latency | `histogram_quantile(0.95, rate(omnia_consensus_round_duration_seconds_bucket[5m]))` | timeseries |
| Gossip Message Rate | `rate(omnia_gossip_events_sent_total[1m])` / `rate(omnia_gossip_events_received_total[1m])` | timeseries |
| Slashing Events | `rate(omnia_slashing_events_total[1h])` | timeseries |
| Fee Revenue | `rate(omnia_fees_collected_total[1h])` | timeseries |
| Causal Graph Size | `omnia_causal_graph_total_events` | stat |
| Memory Usage | `process_resident_memory_bytes` / `process_heap_bytes` | timeseries |
| Events Submitted vs Finalized | `rate(omnia_node_events_submitted_total[1m])` / `rate(omnia_node_events_finalized_total[1m])` | timeseries |

### Alert Rules

| Alert | Condition | Severity | Component |
|-------|-----------|----------|-----------|
| FinalityStalled | `rate(omnia_node_events_finalized_total[5m]) == 0` for 5m | Critical | consensus |
| PeerCountDrop | `omnia_node_peers_connected < 2` for 3m | Warning | network |
| HighSlashingRate | `rate(omnia_slashing_events_total[10m]) > 0.1` for 10m | Warning | slashing |
| MemoryGrowth | `deriv(process_resident_memory_bytes[30m]) > 10485760` for 30m | Warning | system |

**Note on HighSlashingRate:** The `> 0.1` threshold means more than 0.1 slashing events per second (not per hour). This is a high rate and indicates a serious network issue.

**Note on MemoryGrowth:** The `> 10485760` (10 MB) threshold applies to the rate of change over a 30-minute window. If memory is growing faster than ~10 MB per 30 minutes, the alert fires.

### Node-Level Prometheus Metrics

The `NodeMetrics` struct in `node/src/state.rs` registers these metrics with the default Prometheus registry:

| Metric Name | Type | Description |
|---|---|---|
| `omnia_node_events_submitted_total` | IntCounter | Total events submitted via the API |
| `omnia_node_events_finalized_total` | IntCounter | Total events finalized by consensus |
| `omnia_node_peers_connected` | IntGauge | Current number of connected peers |
| `omnia_node_consensus_round` | IntGauge | Current consensus round |
| `omnia_node_shard_operations_total` | IntCounter | Total shard operations processed |
| `omnia_node_http_requests_total` | IntCounter | Total HTTP requests served |

### Checking Alerts

```sh
# View active alerts in Grafana
# http://localhost:3000/alerting/list

# Check Prometheus alert state
curl http://localhost:9095/api/v1/alerts
```

### Setting Up Alert Notifications

Configure notification channels in Grafana:

1. Navigate to **Alerting → Notification channels**
2. Add channels (Slack, PagerDuty, email, etc.)
3. Configure each alert rule to use the notification channel

---

## Trusted Setup Ceremony

The `omnia-node` binary includes subcommands for managing the Powers of Tau trusted setup ceremony, which is required for Groth16 ZK proofs.

### Contributing to the Ceremony

```sh
# Contribute to the Powers of Tau ceremony with default parameters
omnia-node setup-contribute

# With custom parameters
omnia-node setup-contribute --degree 65536 --min-participants 3

# With a deterministic seed (testing only!)
omnia-node setup-contribute --degree 65536 --min-participants 1 --seed <64-hex-char-seed>
```

The `setup-contribute` subcommand:
1. Initializes a `PowersOfTau` SRS with the specified degree
2. Parses the optional hex seed (must be exactly 32 bytes / 64 hex chars)
3. Creates a contribution with fresh randomness (or the provided seed)
4. Applies the contribution to the SRS
5. Reports the participant ID, contribution count, and transcript hash

### Verifying the Ceremony

```sh
# Verify a completed ceremony with default parameters
omnia-node setup-verify

# With custom parameters
omnia-node setup-verify --degree 65536 --num-contributions 3
```

The `setup-verify` subcommand runs a complete ceremony simulation with the specified number of contributions and verifies each one.

**Security Consideration:** The trusted setup is critical for Groth16 proof soundness. If the ceremony is compromised, a participant can generate false proofs. In production, a multi-party computation (MPC) ceremony with many independent participants is required. The current CLI subcommands support local simulation only.

---

## REST API Reference

The node exposes a REST API under `/api/v1/` with Swagger UI documentation.

### Accessing Swagger UI

```
http://localhost:8080/swagger-ui
```

The OpenAPI specification is available at:
```
http://localhost:8080/api-docs/openapi.json
```

### API Endpoints

All endpoints are defined in `node/src/api/mod.rs` and organized by domain:

| Method | Path | Handler | Description |
|--------|------|---------|-------------|
| GET | `/health` | `http::health_handler` | Node liveness probe |
| GET | `/metrics` | `http::metrics_handler` | Prometheus metrics |
| GET | `/api/v1/node/info` | `api::node::node_info` | Node identity, version, uptime, shard count |
| GET | `/api/v1/node/peers` | `api::node::node_peers` | Connected peer list |
| POST | `/api/v1/events` | `api::events::submit_event` | Submit a new event |
| GET | `/api/v1/events/{id}` | `api::events::get_event` | Retrieve event by hex ID |
| POST | `/api/v1/shards/{shard_id}/operations` | `api::shards::submit_shard_operation` | Submit shard operation |
| POST | `/api/v1/governance/proposals` | `api::governance::create_proposal` | Create governance proposal |
| POST | `/api/v1/governance/vote` | `api::governance::cast_vote` | Cast quadratic-weighted vote |
| GET | `/api/v1/economics/balance/{did}` | `api::economics::get_balance` | Check UBC balance |
| POST | `/api/v1/economics/transfer` | `api::economics::transfer_ubc` | Spend UBC tokens |

**Security Note:** The REST API has **JWT authentication, rate limiting, and ACL-based authorization** (added in Phase 0, FIND-001). These are configured via environment variables:
- `OMNIA_JWT_SECRET` — HMAC secret for JWT token validation (required; API returns 401 if not set)
- `OMNIA_AUTHORIZED_CALLERS` — Comma-separated list of authorized caller IDs (required; API returns 401 if caller not in list)
- `OMNIA_RATE_LIMIT_RPS` — Maximum requests per second per IP (default: unlimited; e.g., `10` for 10 RPS)

Privileged operations (mint UBC, advance epoch) require an admin JWT. Admin callers are configured via `OMNIA_AUTHORIZED_ADMINS`.

**Important:** For production, the API should still be behind a reverse proxy with TLS termination.

### Example API Calls

```sh
# Get node info
curl http://localhost:8080/api/v1/node/info

# Submit an event
curl -X POST http://localhost:8080/api/v1/events \
  -H "Content-Type: application/json" \
  -d '{"payload": "48656c6c6f", "event_type": "generic"}'

# Check UBC balance for a DID
curl http://localhost:8080/api/v1/economics/balance/did:omnia:zTest

# Mint UBC (via economics shard operation)
curl -X POST http://localhost:8080/api/v1/shards/economics/operations \
  -H "Content-Type: application/json" \
  -d '{"operation": "mint", "params": {"did": "did:omnia:zTest", "amount": 100}}'

# Create a governance proposal
curl -X POST http://localhost:8080/api/v1/governance/proposals \
  -H "Content-Type: application/json" \
  -d '{"id": "proposal-1", "description": "Test proposal", "expires_at_epoch": 100}'

# Cast a vote
curl -X POST http://localhost:8080/api/v1/governance/vote \
  -H "Content-Type: application/json" \
  -d '{"did": "did:omnia:zTest", "proposal_id": "proposal-1", "choice": "for"}'
```

### Event Submission Details

When submitting an event via `POST /api/v1/events`:
- `payload` is an optional hex-encoded byte string (empty = no payload)
- `event_type` defaults to `"generic"` if not provided
- Payload size is checked against `omnia_substrate::MAX_PAYLOAD_SIZE` — returns 413 if too large
- Invalid hex in payload returns 400
- The event is signed with a fresh Ed25519 keypair generated via `generate_keypair()`
- Successful submission returns 201 with `{"event_id": "...", "status": "submitted"}`

### Economics Shard Operations

The `POST /api/v1/shards/economics/operations` endpoint supports:

| Operation | Params | Description |
|-----------|--------|-------------|
| `mint` | `did`, `amount` | Mint UBC to a DID |
| `spend` | `did`, `amount` | Spend UBC from a DID |
| `register` | `did` | Register a DID in the quota system |
| `advance_epoch` | (none) | Advance to the next epoch |

Other shards (financial, computational, physical, biological, identity) return `{"status": "accepted", "note": "..."}` — full routing is not yet implemented for these shard types.

### UBC Transfer Details

The `POST /api/v1/economics/transfer` endpoint performs a **spend** operation (not a true transfer). UBC tokens are **soulbound** — they cannot be transferred between identities. The `to_did` field is accepted for API compatibility but tokens are consumed from the sender's balance, not transferred to the recipient.
