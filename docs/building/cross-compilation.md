# Cross-Compilation Guide
> 🎯 Audience: Developers
> 🔗 Context: Building Omnia Protocol for different target platforms
> 📅 Last Updated: 2026-05-20

## Docker Build (Recommended)

The easiest way to build for any platform is using the provided Dockerfile:

```bash
# Build the Docker image
docker build -f docker/Dockerfile -t omnia-protocol/omnia-node:latest .

# Build for a specific platform
docker build --platform linux/amd64 -f docker/Dockerfile -t omnia-protocol/omnia-node:amd64 .
docker build --platform linux/arm64 -f docker/Dockerfile -t omnia-protocol/omnia-node:arm64 .
```

The Docker image uses `rust:1.85-slim-bookworm` as the build environment and produces a minimal runtime image.

## Native Cross-Compilation

### Linux x86_64 (Default)

```bash
cargo build --release --target x86_64-unknown-linux-gnu
```

### Linux ARM64 (aarch64)

```bash
rustup target add aarch64-unknown-linux-gnu
cargo build --release --target aarch64-unknown-linux-gnu
```

### macOS

```bash
rustup target add x86_64-apple-darwin
cargo build --release --target x86_64-apple-darwin

# Apple Silicon
rustup target add aarch64-apple-darwin
cargo build --release --target aarch64-apple-darwin
```

### Windows

```bash
rustup target add x86_64-pc-windows-msvc
cargo build --release --target x86_64-pc-windows-msvc
```

## Docker Compose 5-Node Testnet

The Docker Compose configuration provides a complete 5-node testnet:

```bash
# Clone and build
git clone https://github.com/Willow7737/omnia-protocol.git
cd omnia-protocol

# Create .env file
cp docker/.env.example docker/.env
# Edit docker/.env to set your Grafana admin password

# Start the 5-node testnet
docker compose -f docker/docker-compose.yml up -d

# Start with monitoring (Grafana + Prometheus)
docker compose -f docker/docker-compose.yml --profile monitoring up -d
```

## Kubernetes (Helm)

For production deployment, use the Helm chart:

```bash
# Build and push the Docker image
docker build -f docker/Dockerfile -t omnia-protocol/omnia-node:latest .
docker push omnia-protocol/omnia-node:latest

# Configure values
cp helm/omnia-node/values.yaml my-values.yaml
# Edit my-values.yaml

# Install
helm install omnia-node ./helm/omnia-node -f my-values.yaml
```

---
🔙 **Back**: [building/](./) | 🔄 **Related**: [binary-optimization.md](./binary-optimization.md)
🚀 **Next**: [binary-optimization.md](./binary-optimization.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)
