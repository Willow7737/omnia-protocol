# Omnia Protocol Deployment Guide

## Prerequisites

- Rust 1.85+ (see `rust-toolchain.toml`)
- Docker and Docker Compose (for containerized deployment)
- Kubernetes cluster with Helm 3+ (for production deployment)
- At least 4 CPU cores and 8 GB RAM per node

## Quick Start: Docker Compose (Development)

1. Clone the repository:
   ```bash
   git clone https://github.com/Willow7737/omnia-protocol.git
   cd omnia-protocol
   ```

2. Create a `.env` file from the example:
   ```bash
   cp docker/.env.example docker/.env
   # Edit docker/.env to set your Grafana admin password
   ```

3. Start the 5-node testnet:
   ```bash
   docker compose -f docker/docker-compose.yml up -d
   ```

4. Start monitoring (optional):
   ```bash
   docker compose -f docker/docker-compose.yml --profile monitoring up -d
   ```

5. Verify the nodes are running:
   ```bash
   curl http://localhost:9090/health
   ```

## Production Deployment: Kubernetes

1. Build and push the Docker image:
   ```bash
   docker build -f docker/Dockerfile -t omnia-protocol/omnia-node:latest .
   docker push omnia-protocol/omnia-node:latest
   ```

2. Configure the Helm chart values:
   ```bash
   cp helm/omnia-node/values.yaml my-values.yaml
   # Edit my-values.yaml with your configuration
   ```

3. Install the Helm chart:
   ```bash
   helm install omnia-node ./helm/omnia-node -f my-values.yaml
   ```

4. Verify the deployment:
   ```bash
   kubectl get pods -l app=omnia-node
   kubectl port-forward svc/omnia-node 9090:9090
   curl http://localhost:9090/health
   ```

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `RUST_LOG` | `info` | Log level (trace, debug, info, warn, error) |
| `OMNIA_NODE_ID` | (required) | Unique node identifier |
| `OMNIA_BOOTSTRAP_NODES` | (empty) | Comma-separated list of bootstrap node addresses |
| `OMNIA_LISTEN_ADDR` | `/ip4/0.0.0.0/tcp/9090` | Libp2p listen address |
| `OMNIA_TOTAL_NODES` | `5` | Expected number of nodes in the network |
| `OMNIA_DATA_DIR` | `/app/data` | Data directory for persistent storage |

### Configuration File

See `node/omnia-node.toml.example` for the full configuration file format.

## Monitoring

### Metrics

The node exposes Prometheus metrics at `:9090/metrics`. Key metrics include:

- `omnia_consensus_events_total` — Total events processed
- `omnia_consensus_finalized_total` — Total finalized events
- `omnia_gossip_peers_connected` — Current peer count
- `omnia_gossip_messages_sent_total` — Total gossip messages sent

### Grafana Dashboard

A pre-configured Grafana dashboard is available in `monitoring/grafana/dashboards/omnia-node.json`. It shows:

- Event throughput and latency
- Consensus round times
- Peer connectivity
- Resource utilization

### Alerts

Alert rules are defined in `monitoring/grafana/alerts/omnia-alerts.yml`.

## Upgrade Procedure

1. Build the new Docker image with the updated version
2. For Docker Compose: `docker compose pull && docker compose up -d`
3. For Kubernetes: `helm upgrade omnia-node ./helm/omnia-node -f my-values.yaml`
4. Monitor the health endpoint: `curl http://localhost:9090/health`
5. Check logs for errors: `docker logs omnia-bootstrap` or `kubectl logs -l app=omnia-node`

## Rollback Procedure

1. For Docker Compose: `docker compose down && docker compose up -d <previous-version>`
2. For Kubernetes: `helm rollback omnia-node <previous-revision>`
3. Verify the rollback by checking the health endpoint and monitoring dashboards

## Troubleshooting

### Node fails to start
- Check that the data directory is writable
- Verify that the port 9090 is not in use
- Check the logs for configuration errors

### Node cannot connect to peers
- Verify the bootstrap node address is correct
- Check firewall rules allow TCP/UDP on port 9090
- Ensure the node ID matches the configured identity

### Data corruption
- The node uses redb for persistent storage (ACID-compliant)
- If corruption occurs, stop the node, backup the data directory, and restart
- If the node cannot recover, delete the data directory and re-sync from peers

## Security Considerations

- Never expose the Grafana dashboard to the public internet without authentication
- Always change the default Grafana password (see `docker/.env.example`)
- Run the node as a non-root user (the Docker image uses UID 1000 by default)
- Keep the operating system and Docker/Kubernetes runtime updated
- Regularly audit the supply chain using `cargo vet` and `cargo deny`
