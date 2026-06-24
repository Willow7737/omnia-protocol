# Deployment Procedures

> 🎯 Audience: Operators
> 🔗 Context: Deployment procedures for Docker Compose and Kubernetes/Helm
> 📅 Last Updated: 2026-05-20

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
   curl http://localhost:9090/healthz
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
   kubectl port-forward svc/omnia-node 9090:8080
   curl http://localhost:9090/healthz
   ```

> **Note**: The Helm chart exposes BOTH TCP 8080 (HTTP API) AND UDP 4001 (P2P/QUIC). Without UDP 4001, Helm-deployed nodes cannot participate in P2P gossip. This was fixed in the v0.1.69 audit.

## Configuration

### Environment Variables

| Variable                   | Default                 | Description                                      |
| -------------------------- | ----------------------- | ------------------------------------------------ |
| `RUST_LOG`                 | `info`                  | Log level (trace, debug, info, warn, error)      |
| `OMNIA_NODE_ID`            | (required)              | Unique node identifier (u64)                     |
| `OMNIA_BOOTSTRAP_NODES`    | (empty)                 | Comma-separated list of bootstrap node addresses |
| `OMNIA_LISTEN_ADDR`        | `/ip4/0.0.0.0/udp/4001/quic-v1` | Libp2p listen address                            |
| `OMNIA_TOTAL_NODES`        | `5`                     | Expected number of nodes in the network          |
| `OMNIA_DATA_DIR`           | `/app/data`             | Data directory for persistent storage            |
| `OMNIA_JWT_SECRET`         | (none)                  | HMAC secret for JWT auth                         |
| `OMNIA_AUTHORIZED_CALLERS` | (none)                  | Comma-separated authorized caller IDs            |
| `OMNIA_RATE_LIMIT_RPS`     | (none)                  | Max requests per second per IP                   |

### Configuration File

See `node/omnia-node.toml.example` for the full configuration file format.

## Upgrade Procedure

1. Build the new Docker image with the updated version
2. For Docker Compose: `docker compose pull && docker compose up -d`
3. For Kubernetes: `helm upgrade omnia-node ./helm/omnia-node -f my-values.yaml`
4. Monitor the health endpoint: `curl http://localhost:9090/healthz`
5. Check logs for errors: `docker logs omnia-bootstrap` or `kubectl logs -l app=omnia-node`

> **Note**: A sled-to-redb migration runs automatically on startup if a legacy sled database is detected. See `substrate/src/migration.rs`.

## Rollback Procedure

1. For Docker Compose: `docker compose down && docker compose up -d <previous-version>`
2. For Kubernetes: `helm rollback omnia-node <previous-revision>`
3. Verify the rollback by checking the health endpoint and monitoring dashboards

## Security Considerations

- Never expose the Grafana dashboard to the public internet without authentication
- Always change the default Grafana password (see `docker/.env.example`)
- Run the node as a non-root user (the Docker image uses UID 1000 by default)
- Keep the operating system and Docker/Kubernetes runtime updated
- Regularly audit the supply chain using `cargo vet` and `cargo deny`

---

🔙 **Back**: [operations/](./) | 🔄 **Related**: [monitoring.md](./monitoring.md)
🚀 **Next**: [runbook.md](./runbook.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)
