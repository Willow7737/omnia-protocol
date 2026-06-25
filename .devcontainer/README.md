# Omnia Protocol — Codespace Setup

## Quick Start

1. Click the green **Code** button on the repo → **Codespaces** tab → **Create codespace on dev**
2. Select **4-core** or **8-core** machine (the build needs CPU + RAM)
3. Wait for setup to finish (~5 min for `postCreateCommand`)
4. The node binary is built and ready

## Running a Node

```bash
# Set required env vars
export OMNIA_JWT_SECRET=$(openssl rand -hex 32)
export OMNIA_AUTHORIZED_CALLERS=your-github-username

# Run a single node
cargo run -p omnia-node --features full -- run

# Or use the built binary
./target/debug/omnia-node run
```

The HTTP API will be at `http://localhost:8080` (auto-forwarded to your browser).

## Running the 5-Node Testnet (Docker)

```bash
# Start all 5 nodes + Prometheus + Grafana
GRAFANA_ADMIN_PASSWORD=test ./scripts/start-testnet.sh --monitoring
```

| Service | Port | URL |
|---------|------|-----|
| Node HTTP API | 8080 | `https://<codespace>-8080.app.github.dev` |
| Bootstrap (Docker) | 9090 | `https://<codespace>-9090.app.github.dev` |
| Node 1 (Docker) | 9091 | `https://<codespace>-9091.app.github.dev` |
| Grafana | 3000 | `https://<codespace>-3000.app.github.dev` |

## Development

```bash
# Check compilation
cargo check --workspace

# Run tests
cargo test --workspace --exclude omnia-fuzz

# Run clippy
cargo clippy --workspace -- -D warnings

# Format check
cargo fmt --all -- --check
```

## API Quick Test

```bash
# Health check
curl http://localhost:8080/healthz

# Get JWT token (set OMNIA_JWT_SECRET first)
TOKEN=$(curl -s http://localhost:8080/api/v1/auth/token 2>/dev/null || echo "")
# Or create one manually:
TOKEN=$(node -e "const crypto=require('crypto');const header=Buffer.from(JSON.stringify({alg:'HS256',typ:'JWT'})).toString('base64url');const payload=Buffer.from(JSON.stringify({sub:'admin',exp:Math.floor(Date.now()/1000)+3600})).toString('base64url');const sig=crypto.createHmac('sha256',process.env.OMNIA_JWT_SECRET).update(header+'.'+payload).digest('base64url');console.log(header+'.'+payload+'.'+sig)")

# Submit an event
curl -X POST http://localhost:8080/api/v1/events \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"payload": "test"}'

# Check node info
curl http://localhost:8080/api/v1/node/info

# Mint UBC (requires admin caller)
curl -X POST http://localhost:8080/api/v1/shards/economics/operations \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"operation": "mint", "params": {"did": "did:omnia:test", "amount": 1000}}'

# Check balance
curl http://localhost:8080/api/v1/economics/balance?did=did:omnia:test
```

## VS Code Extensions (auto-installed)

- **rust-analyzer** — Rust language server
- **CodeLLDB** — Debugging
- **crates** — Dependency version checking
- **Even Better TOML** — TOML syntax
- **Docker** — Container management

## Troubleshooting

### Build fails with "No space left on device"
Codespaces have limited disk. Run:
```bash
cargo clean
cargo build -p omnia-node --features full
```

### Port not forwarding
Check the **Ports** tab in VS Code. Right-click → **Forward Port**.

### Node crashes on startup
Set the required env vars:
```bash
export OMNIA_JWT_SECRET=$(openssl rand -hex 32)
export OMNIA_AUTHORIZED_CALLERS=$(whoami)
```
