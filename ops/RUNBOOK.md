# Omnia Protocol Operations Runbook

This runbook provides step-by-step procedures for common operational tasks
when running Omnia Protocol nodes. Every procedure includes exact CLI commands.

**Table of Contents:**

1. [Node Startup](#node-startup)
2. [Key Rotation](#key-rotation)
3. [Emergency Slashing](#emergency-slashing)
4. [Network Partition Recovery](#network-partition-recovery)
5. [Node Upgrade](#node-upgrade)
6. [Snapshot and Restore](#snapshot-and-restore)
7. [Monitoring and Alerting](#monitoring-and-alerting)

---

## Node Startup

### Prerequisites

- Omnia binary installed at `/usr/local/bin/omnia-node`
- Data directory exists (default: `./data`)
- Config file prepared (optional: `omnia-node.toml`)

### Starting a New Node

```sh
# Generate a validator keypair
omnia-node keygen --output-dir ./keys

# Start the node with default settings
omnia-node --node-id 1 --http-port 8080

# Or start with a config file
omnia-node --config omnia-node.toml

# Or use environment variables
OMNIA_NODE_ID=1 OMNIA_HTTP_PORT=8080 OMNIA_LOG_LEVEL=info omnia-node
```

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

### Starting with Docker Compose

```sh
cd docker

# Start the full testnet
docker compose up -d

# Start with monitoring (Grafana + Prometheus)
docker compose --profile monitoring up -d
```

### Verifying Node Health

```sh
# Check the health endpoint
curl http://localhost:8080/health

# Check metrics
curl http://localhost:8080/metrics

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
or other slashable offenses.

### Detecting Slashing Events

```sh
# Check slashing metrics
curl -s http://localhost:8080/metrics | grep omnia_slashing

# Check Grafana dashboard (Slashing Events panel)
# http://localhost:3000 → Omnia Node Dashboard
```

### Responding to a Slash

```sh
# 1. Identify the slashed validator from logs
journalctl -u omnia-node | grep "slash"

# 2. Check the validator's current slash points
curl http://localhost:8080/api/slashing/{validator_id}

# 3. If the validator has been ejected (exceeded ejection threshold):
#    - The validator is automatically removed from the active set
#    - No manual intervention needed

# 4. If the validator needs emergency ejection:
#    - Submit an emergency governance proposal to increase slash points
curl -X POST http://localhost:8080/api/governance/proposals \
  -H "Content-Type: application/json" \
  -d '{"type": "emergency_slash", "validator_id": "...", "reason": "equivocation"}'
```

### Post-Slash Cleanup

```sh
# The slashed validator's rate limiter state should be reset
# to prevent stale state from affecting network performance.

# Restart the affected node if it becomes unresponsive:
docker restart omnia-node-2

# Verify network health after the slash:
curl -s http://localhost:8080/metrics | grep omnia_node_peers_connected
curl -s http://localhost:8080/metrics | grep omnia_node_events_finalized_total
```

---

## Network Partition Recovery

Network partitions occur when >1/3 of validators become unreachable.

### Detecting a Partition

```sh
# Check the Grafana alert: PeerCountDrop
# Or check peer count directly:
curl -s http://localhost:8080/metrics | grep omnia_node_peers_connected

# Check for partition detection in logs:
journalctl -u omnia-node | grep "partition detected"
```

### Recovery Steps

```sh
# 1. Identify disconnected peers
curl http://localhost:8080/api/node/peers

# 2. Check network connectivity to each peer
ping 172.20.0.3  # Docker network
traceroute 1.2.3.4  # External peer

# 3. Restart the gossip protocol (if needed)
# The gossip protocol will automatically reconnect when peers are reachable.

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
rm -rf /path/to/minority-node/data/causal_graph/*
rm -rf /path/to/minority-node/data/nonce/*

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

# 2a. Drain the node (stop accepting new events)
# The node continues participating in consensus but marks itself as
# "draining" to discourage new event submissions.

# 2b. Stop the node
docker stop omnia-node-1
# or
systemctl stop omnia-node

# 2c. Backup data
cp -r /path/to/omnia-node-1/data /path/to/backup/data-$(date +%Y%m%d)

# 2d. Replace the binary
cp target/release/omnia-node /usr/local/bin/omnia-node

# 2e. Start the node with the new version
docker start omnia-node-1
# or
systemctl start omnia-node

# 2f. Verify the node is healthy
curl http://localhost:8080/health
curl -s http://localhost:8080/metrics | grep omnia_node_peers_connected

# 2g. Wait for the node to sync and catch up
# Monitor: finalized events/sec should return to normal

# 2h. Repeat for the next node
```

### Protocol Version Check

```sh
# Check the current protocol version
omnia-node --version

# Check the protocol version of running nodes via the API
curl http://localhost:8080/api/node/info | jq .protocol_version
```

---

## Snapshot and Restore

### Taking a Snapshot

```sh
# Manual snapshot via the API
curl -X POST http://localhost:8080/api/node/snapshot

# The snapshot is stored in the data directory:
ls -la ./data/snapshots/

# Automated snapshots are taken every `snapshot_interval` events
# (default: 10000). Configure in omnia-node.toml:
# snapshot_interval = 10000
```

### Restoring from a Snapshot

```sh
# 1. Stop the node
systemctl stop omnia-node

# 2. Backup current data (in case restore fails)
cp -r ./data ./data-pre-restore-$(date +%Y%m%d)

# 3. Remove current state
rm -rf ./data/causal_graph/*
rm -rf ./data/nonce/*
rm -rf ./data/slashing/*

# 4. Copy snapshot data
cp -r ./data/snapshots/latest/* ./data/

# 5. Restart the node
systemctl start omnia-node

# 6. Verify the node syncs from the snapshot state
curl http://localhost:8080/health
curl -s http://localhost:8080/metrics | grep omnia_causal_graph_total_events
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

# Access Grafana: http://localhost:3000 (admin/omnia-admin)
# Access Prometheus: http://localhost:9095
```

### Dashboard Panels

The Omnia Node Dashboard (`omnia-node.json`) includes:

- **Finalized Events/sec** — consensus throughput
- **Peer Count** — P2P network connectivity
- **Consensus Round Latency** — p95 consensus time
- **Gossip Message Rate** — sent and received
- **Slashing Events** — slash frequency
- **Fee Revenue** — UBC collected per hour
- **Causal Graph Size** — total events in DAG
- **Memory Usage** — RSS and heap memory

### Alert Rules

| Alert | Condition | Severity |
|-------|-----------|----------|
| FinalityStalled | No finalization for 5 min | Critical |
| PeerCountDrop | <2 peers for 3 min | Warning |
| HighSlashingRate | >0.1 slashes/hour | Warning |
| MemoryGrowth | >10 MiB/hour for 30 min | Warning |

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

### Metric Endpoints

```sh
# Node metrics (Prometheus format)
curl http://localhost:8080/metrics

# Key metrics to monitor:
# - omnia_node_events_submitted_total
# - omnia_node_events_finalized_total
# - omnia_node_peers_connected
# - omnia_node_consensus_round
# - omnia_node_shard_operations_total
# - omnia_node_http_requests_total
```
