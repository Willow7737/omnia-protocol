# Omnia Protocol Monitoring

This directory contains monitoring configuration for Omnia Protocol nodes,
including Grafana dashboards, alert rules, and Prometheus configuration.

**Version:** v4.0.0
**Last Updated:** 2026-03-05

## Directory Structure

```
monitoring/
├── grafana/
│   ├── dashboards/
│   │   └── omnia-node.json       # Grafana dashboard with 9 panels
│   └── alerts/
│       └── omnia-alerts.yml      # Alert rules for critical conditions
└── README.md                     # This file
```

## Dashboard Panels

The `omnia-node.json` dashboard includes the following panels, each tied to Prometheus metrics registered in `node/src/state.rs` (`NodeMetrics` struct):

| Panel | Metric Expression | Type | Description |
|-------|-------------------|------|-------------|
| Finalized Events/sec | `rate(omnia_node_events_finalized_total[1m])` | timeseries | Consensus throughput |
| Peer Count | `omnia_node_peers_connected` | stat | Current P2P connections (thresholds: red=0, yellow=2, green=4) |
| Consensus Round Latency | `histogram_quantile(0.95, rate(omnia_consensus_round_duration_seconds_bucket[5m]))` | timeseries | P95 consensus latency |
| Gossip Message Rate | `rate(omnia_gossip_events_sent_total[1m])` and `rate(omnia_gossip_events_received_total[1m])` | timeseries | Sent and received gossip |
| Slashing Events | `rate(omnia_slashing_events_total[1h])` | timeseries | Slashing frequency (red fixed color) |
| Fee Revenue | `rate(omnia_fees_collected_total[1h])` | timeseries | UBC fees collected (green fixed color) |
| Causal Graph Size | `omnia_causal_graph_total_events` | stat | Total events in DAG (thresholds: green, yellow=100K, red=1M) |
| Memory Usage | `process_resident_memory_bytes` and `process_heap_bytes` | timeseries | RSS and heap memory |
| Events Submitted vs Finalized | `rate(omnia_node_events_submitted_total[1m])` vs `rate(omnia_node_events_finalized_total[1m])` | timeseries | Consensus lag indicator |

## Node-Level Prometheus Metrics

The `NodeMetrics` struct in `node/src/state.rs` registers these metrics with the default Prometheus registry via a `OnceLock` singleton:

| Metric Name | Type | Description | Code Reference |
|---|---|---|---|
| `omnia_node_events_submitted_total` | IntCounter | Total events submitted via the API | `state.rs::NodeMetrics::events_submitted` |
| `omnia_node_events_finalized_total` | IntCounter | Total events finalized by consensus | `state.rs::NodeMetrics::events_finalized` |
| `omnia_node_peers_connected` | IntGauge | Current number of connected peers | `state.rs::NodeMetrics::peers_connected` |
| `omnia_node_consensus_round` | IntGauge | Current consensus round | `state.rs::NodeMetrics::consensus_round` |
| `omnia_node_shard_operations_total` | IntCounter | Total shard operations processed | `state.rs::NodeMetrics::shard_ops_total` |
| `omnia_node_http_requests_total` | IntCounter | Total HTTP requests served | `state.rs::NodeMetrics::http_requests_total` |

The metrics are exposed at the `/metrics` endpoint, which returns all registered Prometheus metrics in the standard text exposition format (see `node/src/http.rs::metrics_handler`).

Additional substrate-level metrics (gossip, slashing, consensus latency, fee revenue, graph size) are registered by the substrate crate's own instrumentation and appear alongside the node metrics.

## Alert Rules

The alert rules in `grafana/alerts/omnia-alerts.yml` are evaluated by Prometheus:

| Alert | Condition | Duration | Severity | Component |
|-------|-----------|----------|----------|-----------|
| FinalityStalled | `rate(omnia_node_events_finalized_total[5m]) == 0` | 5m | Critical | consensus |
| PeerCountDrop | `omnia_node_peers_connected < 2` | 3m | Warning | network |
| HighSlashingRate | `rate(omnia_slashing_events_total[10m]) > 0.1` | 10m | Warning | slashing |
| MemoryGrowth | `deriv(process_resident_memory_bytes[30m]) > 10485760` | 30m | Warning | system |

**Important thresholds explained:**
- **HighSlashingRate:** The `> 0.1` comparison is against the per-second rate of slashing events (measured over a 10-minute window). A rate of 0.1 means approximately 1 slashing event every 10 seconds, which is a very high rate indicating potential Byzantine behavior or misconfiguration.
- **MemoryGrowth:** The `deriv(...)` function computes the derivative (rate of change) of resident memory bytes over a 30-minute window. A value > 10,485,760 (10 MB) means memory is growing faster than ~10 MB per 30 minutes, which could indicate a memory leak.

## Quick Start

1. Start the monitoring stack with Docker Compose:

```sh
cd docker
docker compose --profile monitoring up -d
```

2. Access Grafana at http://localhost:3000 (admin / password from `docker/.env` `GRAFANA_ADMIN_PASSWORD`)

**Note:** The `GRAFANA_ADMIN_PASSWORD` environment variable is required in `docker/.env`. The Grafana container will not start without it. Copy `docker/.env.example` to `docker/.env` and set a secure password.

3. The Omnia Node dashboard is pre-provisioned as the home dashboard via the `GF_DASHBOARDS_DEFAULT_HOME_DASHBOARD_PATH` environment variable in `docker/docker-compose.yml`.

4. Configure Prometheus to scrape your Omnia nodes by editing `docker/monitoring/prometheus.yml`.

## Prometheus Configuration

The Prometheus instance is configured in `docker/monitoring/prometheus.yml`. The default configuration scrapes the 5-node Docker testnet:

```yaml
global:
  scrape_interval: 15s
  evaluation_interval: 15s

scrape_configs:
  - job_name: 'omnia-bootstrap'
    static_configs:
      - targets: ['omnia-bootstrap:9090']
    metrics_path: /metrics

  - job_name: 'omnia-nodes'
    static_configs:
      - targets:
        - 'omnia-node-1:9091'
        - 'omnia-node-2:9092'
        - 'omnia-node-3:9093'
        - 'omnia-node-4:9094'
    metrics_path: /metrics
```

**Port note:** The Docker compose file maps each node to a distinct host port (9090-9094), with each container internally using `OMNIA_HTTP_PORT=8080`. The Prometheus scrape targets use the Docker service names and internal port 8080 for inter-container communication. Each Omnia node exposes metrics at `/metrics` on its HTTP port.

When adding new nodes, add them to the appropriate `static_configs` section in `prometheus.yml`.
