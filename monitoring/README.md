# Omnia Protocol Monitoring

This directory contains monitoring configuration for Omnia Protocol nodes,
including Grafana dashboards, alert rules, and Prometheus configuration.

## Directory Structure

```
monitoring/
├── grafana/
│   ├── dashboards/
│   │   └── omnia-node.json       # Grafana dashboard with 9+ panels
│   └── alerts/
│       └── omnia-alerts.yml      # Alert rules for critical conditions
└── README.md                     # This file
```

## Dashboard Panels

The `omnia-node.json` dashboard includes the following panels:

| Panel | Metric | Description |
|-------|--------|-------------|
| Finalized Events/sec | `rate(omnia_node_events_finalized_total[1m])` | Consensus throughput |
| Peer Count | `omnia_node_peers_connected` | Current P2P connections |
| Consensus Round Latency | `histogram_quantile(0.95, ...)` | P95 consensus latency |
| Gossip Message Rate | `rate(omnia_gossip_events_*[1m])` | Sent and received gossip |
| Slashing Events | `rate(omnia_slashing_events_total[1h])` | Slashing frequency |
| Fee Revenue | `rate(omnia_fees_collected_total[1h])` | UBC fees collected |
| Causal Graph Size | `omnia_causal_graph_total_events` | Total events in DAG |
| Memory Usage | `process_resident_memory_bytes` | RSS and heap memory |
| Events Submitted vs Finalized | Comparison rates | Consensus lag indicator |

## Alert Rules

| Alert | Condition | Severity |
|-------|-----------|----------|
| FinalityStalled | No events finalized for 5m | Critical |
| PeerCountDrop | Fewer than 2 peers for 3m | Warning |
| HighSlashingRate | >0.1 slashes/hour for 10m | Warning |
| MemoryGrowth | >10 MiB/hour sustained for 30m | Warning |

## Quick Start

1. Start the monitoring stack with Docker Compose:

```sh
cd docker
docker compose --profile monitoring up -d
```

2. Access Grafana at http://localhost:3000 (admin/omnia-admin)

3. The Omnia Node dashboard is pre-provisioned as the home dashboard.

4. Configure Prometheus to scrape your Omnia nodes by editing
   `docker/monitoring/prometheus.yml`.

## Prometheus Configuration

The Prometheus instance is configured in `docker/monitoring/prometheus.yml`.
Add your Omnia node endpoints as scrape targets:

```yaml
scrape_configs:
  - job_name: 'omnia-node'
    static_configs:
      - targets: ['omnia-bootstrap:9090', 'omnia-node-1:9090']
```

Each Omnia node exposes metrics at `/metrics` on its HTTP port.
