#!/usr/bin/env bash
# Codespace setup script — runs after container creation.
# Installs system deps, fetches Rust crates, and builds the project.
set -euo pipefail

echo "=== Omnia Protocol Codespace Setup ==="

# Install system dependencies for Rust + Docker + monitoring
echo "--- Installing system packages ---"
sudo apt-get update -qq
sudo apt-get install -y -qq \
    build-essential \
    pkg-config \
    libssl-dev \
    valgrind \
    gnuplot \
    jq \
    curl \
    git \
    protobuf-compiler \
    2>&1 | tail -3

# Fetch Rust dependencies (don't build yet — let rust-analyzer do that)
echo "--- Fetching Rust dependencies ---"
cargo fetch 2>&1 | tail -3 || echo "WARN: cargo fetch failed (may need network)"

# Build the node binary in debug mode for development
echo "--- Building omnia-node (debug, this takes a few minutes) ---"
cargo build -p omnia-node --features full 2>&1 | tail -5 || echo "WARN: build failed — run 'cargo build -p omnia-node --features full' manually"

# Create data directory
mkdir -p /workspaces/omnia-protocol/data

echo ""
echo "=== Setup Complete ==="
echo ""
echo "Quick start:"
echo "  1. Build:        cargo build -p omnia-node --features full"
echo "  2. Run node:     cargo run -p omnia-node --features full -- run"
echo "  3. Testnet:      ./scripts/start-testnet.sh"
echo "  4. Monitoring:   GRAFANA_ADMIN_PASSWORD=test ./scripts/start-testnet.sh --monitoring"
echo ""
echo "Node HTTP API:    http://localhost:8080"
echo "P2P (QUIC):       udp://localhost:4001"
echo ""
echo "Set JWT secret:   export OMNIA_JWT_SECRET=your-secret-here"
echo "Set admin caller: export OMNIA_AUTHORIZED_CALLERS=your-github-username"
