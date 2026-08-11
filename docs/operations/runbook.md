# Operations Runbook

> 🎯 Audience: Operators
> 🔗 Context: Step-by-step procedures for common operational tasks when running Omnia Protocol nodes
> 📅 Last Updated: 2026-08-11

## Table of Contents

1. [Node Startup](#node-startup)
2. [Key Rotation](#key-rotation)
3. [Emergency Slashing](#emergency-slashing)
4. [Network Partition Recovery](#network-partition-recovery)
5. [Node Upgrade](#node-upgrade)
6. [Snapshot and Restore](#snapshot-and-restore)
7. [Trusted Setup Ceremony](#trusted-setup-ceremony)
8. [REST API Reference](#rest-api-reference)

---

## Node Startup

### Starting a New Node

```sh
# Generate a validator keypair (use --passphrase for production!)
omnia-node keygen --output-dir ./keys --passphrase "your-secure-passphrase"

# Start the node with default settings
omnia-node --node-id 1 --http-port 8080

# Or start with a config file
omnia-node --config omnia-node.toml

# Or use environment variables
OMNIA_NODE_ID=1 OMNIA_HTTP_PORT=8080 OMNIA_LOG_LEVEL=info omnia-node
```

**Important:** `--node-id` accepts a `u64` value (not a string). Values like `bootstrap` or `node1` are invalid.

### Starting with Bootstrap Peers

```sh
omnia-node \
  --node-id 2 \
  --http-port 8081 \
  --bootstrap-nodes "/ip4/1.2.3.4/udp/4001/quic/p2p/12D3KooWExample"
```

### Starting with Docker Compose

```sh
cd docker
cp .env.example .env
# Edit .env to change GRAFANA_ADMIN_PASSWORD from the default
docker compose up -d

# With monitoring
docker compose --profile monitoring up -d
```

### Verifying Node Health

```sh
curl http://localhost:8080/healthz     # Liveness probe
curl http://localhost:8080/readyz       # Readiness probe
curl http://localhost:8080/api/v1/node/info  # Node info
curl http://localhost:8080/metrics      # Prometheus metrics
```

---

## Key Rotation

### Step 1: Backup Current Keys

```sh
cp -r ./keys ./keys-backup-$(date +%Y%m%d)
```

### Step 2: Generate New Keys

```sh
omnia-node keygen --output-dir ./keys-new --passphrase "your-secure-passphrase"
```

### Step 3: Rotate Keys

The key rotation is performed via the keystore API. The rotation produces a `KeyRotationProof` that must be broadcast to other validators so they update their trusted key set.

### Step 4: Verify Rotation

```sh
cat ./keys/validator_pubkey.txt
omnia-node --node-id 1 --http-port 8080
curl http://localhost:8080/healthz
```

### Step 5: Distribute Rotation Proof

The `KeyRotationProof` must be shared with all other validators. They verify the signature from the old key over the new public key. Once >2/3 of validators acknowledge the rotation, it is finalized.

> **v0.1.69**: Node keypair is now persistent. Use `OMNIA_NODE_KEY_FILE` to specify the keypair path. To rotate: generate a new keypair with `omnia-node keygen`, replace the file, and restart.

---

## Emergency Slashing

The `SlashingEngine` tracks three offense types with gradual escalation:

| Offense            | 1st                | 2nd                 | 3rd+              |
| ------------------ | ------------------ | ------------------- | ----------------- |
| Equivocation       | Jailed (5%, 1000r) | Jailed (25%, 5000r) | Ejected (100%)    |
| LivenessViolation  | Warning (1%)       | Warning (1%)        | Jailed (5%, 500r) |
| InvalidAttestation | Warning (2%)       | Jailed (10%, 2000r) | Ejected (100%)    |

### Detecting Slashing Events

```sh
# Check slashing metrics
curl -s http://localhost:8080/metrics | grep omnia_slashing

# Check Grafana dashboard (Slashing Events panel)
```

### Responding to a Slash

```sh
# 1. Identify the slashed validator from logs
journalctl -u omnia-node | grep "slash"

# 2. Check slashing metrics
curl -s http://localhost:8080/metrics | grep omnia_slashing

# 3. If ejected (exceeded threshold): validator automatically removed from active set

# 4. For emergency ejection via governance:
curl -X POST http://localhost:8080/api/v1/governance/proposals \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $JWT" \
  -d '{"id": "emergency-slash-validator-xyz", "description": "Emergency slash", "expires_at_epoch": 100}'
```

**Important:** Production nodes must use `RedbSlashingStore` for persistence. If using in-memory slashing, all slash points are lost on restart.

---

## Network Partition Recovery

### Detecting a Partition

```sh
# Check peer count
curl -s http://localhost:8080/metrics | grep omnia_node_peers_connected

# Check connected peers
curl http://localhost:8080/api/v1/node/peers
```

### Recovery Steps

```sh
# 1. Identify disconnected peers
curl http://localhost:8080/api/v1/node/peers

# 2. Check network connectivity
ping 172.20.0.3
traceroute 1.2.3.4

# 3. Gossip protocol will auto-reconnect when peers are reachable

# 4. Verify recovery
curl -s http://localhost:8080/metrics | grep omnia_node_peers_connected
```

### Handling Split-Brain

```sh
# 1. Stop all nodes
docker compose down

# 2. Identify the longer chain (higher finalized event count)
# Check each node's data directory

# 3. The partition with >2/3 of validators has the canonical chain
# Reset minority partition nodes and restart
docker compose up -d
```

> **Readiness contract**: `/readyz` is an operational routing signal. It returns 200 when the node has at least `readiness_min_peers` connected peers and is not in fast-sync. It includes `finalized_height`, `lane0_enabled`, and `lane0_finalized_events` for visibility, but quiet networks with no recent Lane 1 commits or Lane 0 preconfirmations are still ready. 503 reasons are reachability/sync blockers: `no_peers` or `syncing`.
>
> **v0.1.69**: `/readyz` now reports actual peer count via `GossipProtocol::connected_peer_count()`. The background consensus loop polls this every 1s and updates `AppState.peers`.

---

## Node Upgrade

### Rolling Upgrade (Zero-Downtime)

```sh
# For each node (maintaining >2/3 validator availability):

# 1. Stop the node
docker stop omnia-node-1

# 2. Backup data
cp -r /path/to/data /path/to/backup/data-$(date +%Y%m%d)

# 3. Replace the binary
cp target/release/omnia-node /usr/local/bin/omnia-node

# 4. Start the node
docker start omnia-node-1

# 5. Verify health
curl http://localhost:8080/healthz

# 6. Wait for sync, then repeat for next node
```

### Protocol Version Check

```sh
omnia-node --version
curl http://localhost:8080/api/v1/node/info | jq .protocol_version
```

---

## Snapshot and Restore

### Taking a Snapshot

```sh
omnia-node snapshot --output snapshot.bin
```

### Restoring from a Snapshot

```sh
omnia-node restore --input snapshot.bin
```

### Automated Snapshots

Configure in `omnia-node.toml`:

```toml
snapshot_interval = 10000  # Snapshot every 10,000 events
```

### Pruning Configuration

```toml
pruning_depth = 10000  # Keep last 10,000 rounds
# pruning_depth = 0    # Archive mode (no pruning)
```

---

## Trusted Setup Ceremony

### Contributing to the Ceremony

```sh
# Contribute with default parameters
omnia-node setup-contribute

# With custom parameters
omnia-node setup-contribute --degree 65536 --min-participants 3
```

### Verifying the Ceremony

```sh
omnia-node setup-verify --degree 65536 --num-contributions 3
```

**Security Consideration:** The trusted setup is critical for Groth16 proof soundness. Production requires multi-party coordination with independent participants.

---

## REST API Reference

| Method | Path                                  | Description                  |
| ------ | ------------------------------------- | ---------------------------- |
| GET    | `/healthz`                            | Liveness probe               |
| GET    | `/readyz`                             | Readiness probe              |
| GET    | `/metrics`                            | Prometheus metrics           |
| GET    | `/api/v1/node/info`                   | Node identity and status     |
| GET    | `/api/v1/node/peers`                  | Connected peer list          |
| POST   | `/api/v1/events`                      | Submit a new event           |
| GET    | `/api/v1/events/:id`                  | Retrieve event by ID         |
| POST   | `/api/v1/shards/:shard_id/operations` | Submit shard operation       |
| POST   | `/api/v1/governance/proposals`        | Create governance proposal   |
| POST   | `/api/v1/governance/vote`             | Cast quadratic-weighted vote |
| GET    | `/api/v1/economics/balance/:did`      | Check UBC balance            |
| POST   | `/api/v1/economics/transfer`          | Spend UBC tokens             |
| GET    | `/api/v1/ceremony/state`              | Current MPC ceremony state   |
| POST   | `/api/v1/ceremony/contribute`         | Submit ceremony contribution |
| GET    | `/api/v1/ceremony/transcript`         | Download ceremony transcript |
| POST   | `/api/v1/ceremony/finalize`           | Finalize ceremony            |
| GET    | `/api/v1/errors`                      | Recent error log             |

**Security:** Write endpoints require JWT authentication; 5 read endpoints are public: `/api/v1/node/info`, `/api/v1/node/peers`, `/api/v1/errors`, `/api/v1/ceremony/state`, `/api/v1/ceremony/transcript`. Privileged operations require admin JWT. See [validator-setup.md](./validator-setup.md) for configuration.

Swagger UI: `http://localhost:8080/swagger-ui`
OpenAPI spec: `http://localhost:8080/api-docs/openapi.json`

---

🔙 **Back**: [operations/](./) | 🔄 **Related**: [validator-setup.md](./validator-setup.md)
🚀 **Next**: [feature-flags.md](./feature-flags.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)
