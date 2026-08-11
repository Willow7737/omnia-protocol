# Monitoring Setup

> 🎯 Audience: Operators
> 🔗 Context: Grafana dashboards, Prometheus configuration, and alert rules for Omnia nodes
> 📅 Last Updated: 2026-08-11

## Quick Start

```sh
cd docker
docker compose --profile monitoring up -d

# Access Grafana: http://localhost:3000 (admin / password from docker/.env)
# Access Prometheus: http://localhost:9095
```

The `GRAFANA_ADMIN_PASSWORD` environment variable is required in `docker/.env`. Copy `docker/.env.example` to `docker/.env` and set a secure password.

## Dashboard Panels

The `omnia-node.json` dashboard includes the following panels:

| Panel                         | Metric Expression                                                                              | Type       | Description                                                    |
| ----------------------------- | ---------------------------------------------------------------------------------------------- | ---------- | -------------------------------------------------------------- |
| Finalized Events/sec          | `rate(omnia_node_events_finalized_total[1m])`                                                  | timeseries | Consensus throughput                                           |
| Peer Count                    | `omnia_node_peers_connected`                                                                   | stat       | Current P2P connections (thresholds: red=0, yellow=2, green=4) |
| Consensus Round Latency       | `histogram_quantile(0.95, rate(omnia_consensus_round_duration_seconds_bucket[5m]))`            | timeseries | P95 consensus latency                                          |
| Gossip Message Rate           | `rate(omnia_gossip_events_sent_total[1m])` and `rate(omnia_gossip_events_received_total[1m])`  | timeseries | Sent and received gossip                                       |
| Slashing Events               | `rate(omnia_slashing_events_total[1h])`                                                        | timeseries | Slashing frequency                                             |
| Fee Revenue                   | `rate(omnia_fees_collected_total[1h])`                                                         | timeseries | UBC fees collected                                             |
| Causal Graph Size             | `omnia_causal_graph_total_events`                                                              | stat       | Total events in DAG (thresholds: green, yellow=100K, red=1M)   |
| Memory Usage                  | `process_resident_memory_bytes` and `process_heap_bytes`                                       | timeseries | RSS and heap memory                                            |
| Events Submitted vs Finalized | `rate(omnia_node_events_submitted_total[1m])` vs `rate(omnia_node_events_finalized_total[1m])` | timeseries | Consensus lag indicator                                        |

## Node-Level Prometheus Metrics

The `NodeMetrics` struct in `node/src/state.rs` registers these metrics with the default Prometheus registry:

| Metric Name                                  | Type       | Description                                       |
| -------------------------------------------- | ---------- | ------------------------------------------------- |
| `omnia_node_events_submitted_total`          | IntCounter | Total events submitted via the API                |
| `omnia_node_events_finalized_total`          | IntCounter | Total events finalized by consensus               |
| `omnia_node_peers_connected`                 | IntGauge   | Current number of connected peers                 |
| `omnia_node_consensus_round`                 | IntGauge   | Current consensus round                           |
| `omnia_node_shard_operations_total`          | IntCounter | Total shard operations processed                  |
| `omnia_node_http_requests_total`             | IntCounter | Total HTTP requests served                        |
| `omnia_consensus_tps`                         | IntCounter | Consensus transactions per second                 |
| `omnia_consensus_finality_latency_seconds`   | Histogram  | Latency from event submission to finality         |
| `omnia_gossip_propagation_latency_seconds`   | Histogram  | Latency for gossip message propagation            |
| `omnia_dag_events_total`                     | IntCounter | Total DAG events processed                        |
| `omnia_dag_insertion_latency_seconds`        | Histogram  | Latency for DAG event insertion                   |
| `omnia_node_memory_rss_bytes`                | IntGauge   | Current resident set size (RSS) in bytes          |

Additional substrate-level metrics (gossip, slashing, consensus latency, fee revenue, graph size) are registered by the substrate crate's own instrumentation.

## Alert Rules

The alert rules in `monitoring/grafana/alerts/omnia-alerts.yml` are evaluated by Prometheus:

| Alert            | Condition                                              | Duration | Severity | Component |
| ---------------- | ------------------------------------------------------ | -------- | -------- | --------- |
| FinalityStalled  | `rate(omnia_node_events_finalized_total[5m]) == 0`     | 5m       | Critical | consensus |
| PeerCountDrop    | `omnia_node_peers_connected < 4`                       | 3m       | Warning  | network   |
| HighSlashingRate | `rate(omnia_slashing_events_total[10m]) > 0.1`         | 10m      | Warning  | slashing  |
| MemoryGrowth     | `deriv(process_resident_memory_bytes[30m]) > 10485760` | 30m      | Warning  | system    |

**Threshold explanations:**

- **HighSlashingRate:** `> 0.1` means more than 0.1 slashing events per second (approximately 1 every 10 seconds), indicating potential Byzantine behavior or misconfiguration.
- **MemoryGrowth:** `> 10,485,760` (10 MB) means memory is growing faster than ~10 MB per 30 minutes, which could indicate a memory leak.

## Prometheus Configuration

The Prometheus instance is configured in `docker/monitoring/prometheus.yml`:

```yaml
global:
  scrape_interval: 15s
  evaluation_interval: 15s

scrape_configs:
  - job_name: "omnia-bootstrap"
    static_configs:
      - targets: ["omnia-bootstrap:8080"]
    metrics_path: /metrics

  - job_name: "omnia-nodes"
    static_configs:
      - targets:
          - "omnia-node-1:8080"
          - "omnia-node-2:8080"
          - "omnia-node-3:8080"
          - "omnia-node-4:8080"
    metrics_path: /metrics
```

---

🔙 **Back**: [operations/](./) | 🔄 **Related**: [deployment.md](./deployment.md)
🚀 **Next**: [deployment.md](./deployment.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)
