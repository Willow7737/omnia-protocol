# Omnia Protocol — Codespace Setup

## Quick Start

1. Open this repo in a GitHub Codespace (4-core or larger recommended)
2. Wait for `postCreateCommand` to finish (`cargo fetch`)
3. Start the 5-node testnet:

```bash
./scripts/start-testnet.sh
```

4. For monitoring (Prometheus + Grafana):

```bash
GRAFANA_ADMIN_PASSWORD=yourpassword ./scripts/start-testnet.sh --monitoring
```

## Accessing Nodes

Codespaces auto-forwards ports. Once the testnet is running:

| Service | Port | URL |
|---------|------|-----|
| Bootstrap Node | 9090 | `https://<codespace-name>-9090.app.github.dev` |
| Node 1 | 9091 | `https://<codespace-name>-9091.app.github.dev` |
| Node 2 | 9092 | `https://<codespace-name>-9092.app.github.dev` |
| Node 3 | 9093 | `https://<codespace-name>-9093.app.github.dev` |
| Node 4 | 9094 | `https://<codespace-name>-9094.app.github.dev` |
| Prometheus | 9095 | `https://<codespace-name>-9095.app.github.dev` |
| Grafana | 3000 | `https://<codespace-name>-3000.app.github.dev` |

## Health Check

```bash
./scripts/start-testnet.sh --status
```

Or manually:
```bash
curl -s https://<codespace-name>-9090.app.github.dev/health | jq .
```

## Connecting from omnia-web

Set the `NEXT_PUBLIC_OMNIA_API_URL` environment variable to the bootstrap node's
forwarded URL:

```
NEXT_PUBLIC_OMNIA_API_URL=https://<codespace-name>-9090.app.github.dev
```

## Useful Commands

```bash
# Check testnet status
./scripts/start-testnet.sh --status

# Follow logs
./scripts/start-testnet.sh --logs

# Tear down
./scripts/start-testnet.sh --down

# Run tests locally (no Docker)
cargo test --workspace --exclude chaos-tests

# Run chaos tests
cargo test -p chaos-tests
```
