# Omnia Protocol — Operational Runbook

**Version**: 1.0  
**Last Updated**: 2026-05-22  
**Applies To**: Phase 0 — 3-Node Testnet

---

## 1. Quick Start

### Prerequisites

- Docker Engine ≥ 24.0
- Docker Compose ≥ 2.20
- 4 GB RAM minimum, 8 GB recommended
- 20 GB free disk space

### Deploy 3-Node Testnet

```bash
# Clone the repository
git clone https://github.com/Willow7737/omnia-protocol.git
cd omnia-protocol

# Build and start the 3-node testnet
docker compose -f docker/docker-compose.testnet.yml up -d

# Verify all nodes are healthy
docker compose -f docker/docker-compose.testnet.yml ps

# Check bootstrap node health
curl http://localhost:9090/health
curl http://localhost:9090/readyz

# Check peer nodes
curl http://localhost:9091/health
curl http://localhost:9092/health
```

### Deploy with Monitoring

```bash
# Start testnet with Prometheus + Grafana
GRAFANA_ADMIN_PASSWORD=your-secret-password \
  docker compose -f docker/docker-compose.testnet.yml --profile monitoring up -d

# Access Grafana dashboard
open http://localhost:3000
# Login: admin / your-secret-password

# Access Prometheus
open http://localhost:9095
```

---

## 2. Node Operations

### Check Node Status

```bash
# Health check
curl http://localhost:9090/health

# Readiness check (includes consensus state)
curl http://localhost:9090/readyz

# Prometheus metrics
curl http://localhost:9090/metrics
```

### Submit Events

```bash
# Submit a new event
curl -X POST http://localhost:9090/api/v1/events \
  -H "Content-Type: application/json" \
  -d '{
    "payload": "hello world",
    "shard": "financial"
  }'
```

### Query Events

```bash
# Get recent events
curl http://localhost:9090/api/v1/events?limit=10

# Get specific event
curl http://localhost:9090/api/v1/events/{event_id}
```

### View Node Info

```bash
# Node information
curl http://localhost:9090/api/v1/node/info

# Connected peers
curl http://localhost:9090/api/v1/node/peers
```

---

## 3. Monitoring and Alerting

### Key Metrics to Monitor

| Metric | Description | Alert Threshold |
|--------|-------------|-----------------|
| `omnia_consensus_tps` | Transactions per second | < 100 TPS sustained |
| `omnia_consensus_finality_latency_seconds` | Time to finality | p95 > 5s |
| `omnia_gossip_propagation_latency_seconds` | Gossip propagation time | p99 > 500ms |
| `omnia_dag_events_total` | Total DAG events | Sudden drop = issue |
| `omnia_dag_insertion_latency_seconds` | DAG insertion time | p99 > 200µs |
| `omnia_node_memory_rss_bytes` | Process memory | > 2 GB RSS |

### PromQL Queries

```promql
# TPS (5-minute average)
rate(omnia_consensus_tps[5m])

# Finality latency p95
histogram_quantile(0.95, rate(omnia_consensus_finality_latency_seconds_bucket[5m]))

# Gossip latency p99
histogram_quantile(0.99, rate(omnia_gossip_propagation_latency_seconds_bucket[5m]))

# Memory usage
omnia_node_memory_rss_bytes

# State root agreement (should be identical across nodes)
omnia_consensus_state_root_hash
```

### Grafana Dashboard

The pre-configured Grafana dashboard (`monitoring/grafana/dashboards/omnia-node.json`) shows:
- Real-time TPS graph
- Finality latency distribution
- Gossip latency heatmap
- Memory usage trend
- Event count by status (Pending/Acknowledged/Witness/Committed)
- Connected peers

---

## 4. Troubleshooting

### Node Won't Start

**Symptoms**: Container exits immediately or health check fails.

**Diagnostics**:
```bash
# Check container logs
docker compose -f docker/docker-compose.testnet.yml logs omnia-bootstrap

# Common issues:
# 1. Port conflict (8080 or 4001 in use)
lsof -i :8080 -i :4001

# 2. Data directory permissions
ls -la docker/volumes/

# 3. Insufficient memory
docker stats
```

**Resolution**:
- Kill conflicting processes on ports 8080/4001
- Fix data directory permissions: `chmod -R 755 docker/volumes/`
- Allocate more memory to Docker (minimum 4 GB)

### Nodes Cannot Discover Each Other

**Symptoms**: Nodes start but report 0 peers.

**Diagnostics**:
```bash
# Check if bootstrap is healthy before peers start
curl http://localhost:9090/readyz

# Check DNS resolution inside containers
docker exec omnia-node-1 nslookup omnia-bootstrap

# Check network connectivity
docker network inspect omnia-testnet
```

**Resolution**:
- Ensure `omnia-bootstrap` is fully healthy before peers start (health check dependency)
- Verify Docker network is created correctly
- Check that `OMNIA_BOOTSTRAP_NODES` env var is set correctly

### Consensus Not Progressing

**Symptoms**: Events are submitted but not being committed; round number stays at 0.

**Diagnostics**:
```bash
# Check consensus metrics
curl -s http://localhost:9090/metrics | grep omnia_consensus

# Check node info
curl -s http://localhost:9090/api/v1/node/info | jq .
```

**Resolution**:
- Ensure at least 3 nodes are running (supermajority requires 2N/3+1 = 3 of 4 default)
- Check that nodes are not slashed (look for slash events in logs)
- Verify `total_nodes` in config matches actual node count

### High Memory Usage

**Symptoms**: Node RSS exceeds 2 GB at 100 TPS.

**Diagnostics**:
```bash
# Check memory metrics
curl -s http://localhost:9090/metrics | grep omnia_node_memory

# Check event count
curl -s http://localhost:9090/api/v1/node/info | jq .events
```

**Resolution**:
- Reduce pruning depth (events are retained longer than needed)
- Check for unbounded growth in event pool (bug in PruningAwarePool)
- Restart the node to force garbage collection

### Gossip Latency High

**Symptoms**: p99 gossip latency exceeds 500ms.

**Diagnostics**:
```bash
# Check gossip metrics
curl -s http://localhost:9090/metrics | grep omnia_gossip

# Check bloom filter stats
curl -s http://localhost:9090/metrics | grep bloom
```

**Resolution**:
- Verify GossipSub parameters in `config/gossip_config.toml`
- Check network bandwidth between containers
- Reduce bloom filter rotation interval if FPR is too high
- Increase priority queue capacity for finality-critical events

---

## 5. Emergency Procedures

### Node Crash Recovery

1. The node automatically recovers from `RedbConsensusStore` on restart
2. `load_or_new()` restores consensus state without replaying genesis
3. Fast sync catches up on missed events from peers

```bash
# Restart a crashed node
docker compose -f docker/docker-compose.testnet.yml restart omnia-node-1

# Monitor recovery
docker compose -f docker/docker-compose.testnet.yml logs -f omnia-node-1
```

### Network Partition

If a network partition occurs:

1. **Detection**: Partition is auto-detected when >1/3 peers are silent
2. **Safety**: BFT guarantees ensure no commit during partition (need 2N/3+1)
3. **Recovery**: When partition heals, nodes resync via fast sync protocol

```bash
# Check for partition detection in logs
docker compose -f docker/docker-compose.testnet.yml logs | grep PartitionDetected
```

### Data Corruption

If a node's data is corrupted:

```bash
# Stop the node
docker compose -f docker/docker-compose.testnet.yml stop omnia-node-1

# Remove corrupted data
docker volume rm omnia-protocol_node1-data

# Restart — node will resync from peers
docker compose -f docker/docker-compose.testnet.yml start omnia-node-1
```

---

## 6. Performance Tuning

### GossipSub Parameters

Edit `config/gossip_config.toml`:

```toml
[gossipsub]
# Faster heartbeat = lower latency, more bandwidth
heartbeat_interval = 500

# More peers in mesh = more reliable delivery, more CPU
mesh_n = 4

# Higher fanout = faster propagation, more bandwidth
fanout = 4
```

### Batch Processing

Edit batch configuration in node startup:

```toml
[batch]
# Larger batches = higher throughput, higher latency
max_batch_size = 100
flush_size = 50
flush_timeout_ms = 100
```

### Sharded Consensus

The number of worker threads is auto-detected from CPU count. Override:

```bash
# Set worker thread count
OMNIA_VALIDATION_WORKERS=4 ./omnia-node
```

---

## 7. Log Reference

### Log Levels

| Level | Usage |
|-------|-------|
| ERROR | Safety violations, consensus failures |
| WARN | Timeouts, equivocation detection, partition detection |
| INFO | Round advancement, node startup, peer discovery |
| DEBUG | Event processing, gossip propagation, pruning |
| TRACE | Per-event processing details |

### Key Log Messages

| Message | Severity | Action |
|---------|----------|--------|
| `Equivocation detected` | WARN | Investigate node behavior |
| `Round N timed out` | WARN | Check network latency |
| `PartitionDetected` | WARN | Check network connectivity |
| `Rejecting event from slashed node` | INFO | Expected for Byzantine nodes |
| `Consensus engine state restored` | INFO | Normal recovery |
| `Pruned finalized events` | DEBUG | Normal operation |

---

## 8. Backup and Recovery

### Backup Node State

```bash
# Stop the node
docker compose -f docker/docker-compose.testnet.yml stop omnia-bootstrap

# Backup the data volume
docker run --rm -v omnia-protocol_bootstrap-data:/data -v $(pwd):/backup \
  alpine tar czf /backup/bootstrap-backup.tar.gz -C /data .

# Restart the node
docker compose -f docker/docker-compose.testnet.yml start omnia-bootstrap
```

### Restore Node State

```bash
# Stop the node
docker compose -f docker/docker-compose.testnet.yml stop omnia-bootstrap

# Restore the data volume
docker run --rm -v omnia-protocol_bootstrap-data:/data -v $(pwd):/backup \
  alpine tar xzf /backup/bootstrap-backup.tar.gz -C /data

# Restart the node
docker compose -f docker/docker-compose.testnet.yml start omnia-bootstrap
```
