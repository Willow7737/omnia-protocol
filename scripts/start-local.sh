#!/usr/bin/env bash
# Omnia Protocol — Local Testnet Startup Script
#
# Starts the Omnia Protocol nodes and web dashboard locally.
# Designed for GitHub Codespaces or local development machines.
#
# Prerequisites:
#   - Rust 1.91+ installed (https://rustup.rs)
#   - Node.js 18+ or Bun installed
#   - At least 2GB free RAM (each debug node uses ~50MB, release ~15MB)
#
# Usage:
#   ./start-local.sh              # Start everything
#   ./start-local.sh --release    # Use release builds (smaller, faster)
#   ./start-local.sh --nodes 3    # Start 3 nodes instead of 5
#   ./start-local.sh --stop       # Stop all running nodes

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROTOCOL_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
WEB_DIR="$PROTOCOL_DIR/../omnia-web"
DATA_BASE="$PROTOCOL_DIR/../omnia-data"

# Defaults
RELEASE=false
NODE_COUNT=5
STOP=false

# Parse arguments
for arg in "$@"; do
  case "$arg" in
    --release) RELEASE=true ;;
    --nodes) shift; NODE_COUNT="${1:-5}" ;;
    --stop) STOP=true ;;
    --help)
      echo "Usage: $0 [--release] [--nodes N] [--stop]"
      echo ""
      echo "  --release   Build and run release binaries (smaller, faster)"
      echo "  --nodes N   Number of nodes to start (default: 5)"
      echo "  --stop      Stop all running omnia-node processes"
      exit 0
      ;;
  esac
done

# Stop all nodes
if [ "$STOP" = true ]; then
  echo "🛑 Stopping all omnia-node processes..."
  pkill -f "omnia-node" 2>/dev/null || true
  echo "✅ All nodes stopped"
  exit 0
fi

# Determine binary path
if [ "$RELEASE" = true ]; then
  BINARY="$PROTOCOL_DIR/target/release/omnia-node"
  BUILD_ARGS="--release"
  echo "🔨 Building release binary..."
else
  BINARY="$PROTOCOL_DIR/target/debug/omnia-node"
  BUILD_ARGS=""
  echo "🔨 Building debug binary..."
fi

# Build the node binary
cd "$PROTOCOL_DIR"
cargo build -p omnia-node --no-default-features --features "light,metrics" $BUILD_ARGS 2>&1 | tail -3

if [ ! -f "$BINARY" ]; then
  echo "❌ Build failed — binary not found at $BINARY"
  exit 1
fi

echo "✅ Binary ready: $BINARY ($(du -h "$BINARY" | cut -f1))"

# Clean up old data
rm -rf "$DATA_BASE"
mkdir -p "$DATA_BASE"

# ── Start Bootstrap Node ─────────────────────────────────────────────────
echo ""
echo "🚀 Starting bootstrap node (node-id=1, HTTP=8080)..."

mkdir -p "$DATA_BASE/bootstrap"
nohup "$BINARY" \
  --node-id 1 \
  --http-port 8080 \
  --listen-addr "/ip4/0.0.0.0/udp/4001/quic-v1" \
  --data-dir "$DATA_BASE/bootstrap" \
  > "$DATA_BASE/bootstrap.log" 2>&1 &
BOOTSTRAP_PID=$!
echo "   PID: $BOOTSTRAP_PID"

# Wait for bootstrap to become healthy
echo "   Waiting for bootstrap to start..."
for i in $(seq 1 30); do
  if curl -sf http://localhost:8080/healthz > /dev/null 2>&1; then
    echo "   ✅ Bootstrap node is alive!"
    break
  fi
  sleep 1
done

# ── Start Peer Nodes ─────────────────────────────────────────────────────
for i in $(seq 2 "$NODE_COUNT"); do
  HTTP_PORT=$((8080 + i - 1))
  QUIC_PORT=$((4001 + i - 1))
  NODE_DIR="$DATA_BASE/node-$((i-1))"

  echo "🚀 Starting node-$((i-1)) (node-id=$i, HTTP=$HTTP_PORT)..."

  mkdir -p "$NODE_DIR"
  nohup "$BINARY" \
    --node-id "$i" \
    --http-port "$HTTP_PORT" \
    --listen-addr "/ip4/0.0.0.0/udp/$QUIC_PORT/quic-v1" \
    --bootstrap-nodes "" \
    --data-dir "$NODE_DIR" \
    > "$NODE_DIR/node.log" 2>&1 &
  echo "   PID: $!"
  sleep 2
done

# ── Test the API ─────────────────────────────────────────────────────────
echo ""
echo "📊 Testing bootstrap node API..."
echo "   /healthz:     $(curl -sf http://localhost:8080/healthz 2>/dev/null || echo 'FAILED')"
echo "   /api/v1/node/info: $(curl -sf http://localhost:8080/api/v1/node/info 2>/dev/null | head -c 100 || echo 'FAILED')..."

# ── Start Web Dashboard ──────────────────────────────────────────────────
if [ -d "$WEB_DIR" ]; then
  echo ""
  echo "🌐 Starting omnia-web dashboard..."

  # Build the list of internal node URLs
  INTERNAL_URLS="http://localhost:8080"
  for i in $(seq 2 "$NODE_COUNT"); do
    HTTP_PORT=$((8080 + i - 1))
    INTERNAL_URLS="$INTERNAL_URLS,http://localhost:$HTTP_PORT"
  done

  cd "$WEB_DIR"

  OMNIA_NODE_INTERNAL_URLS="$INTERNAL_URLS" \
  OMNIA_API_URL="http://localhost:8080" \
  NEXT_PUBLIC_LIVE_MODE=true \
  NEXT_PUBLIC_OMNIA_API_URL="http://localhost:8080" \
  NEXT_PUBLIC_POLL_INTERVAL_MS=5000 \
  nohup bun run dev > "$DATA_BASE/web.log" 2>&1 &
  WEB_PID=$!
  echo "   PID: $WEB_PID"
  echo "   URL: http://localhost:3000"
else
  echo ""
  echo "⚠️  omnia-web directory not found at $WEB_DIR"
  echo "   Clone it with: git clone https://github.com/Willow7737/omnia-web.git"
fi

# ── Summary ──────────────────────────────────────────────────────────────
echo ""
echo "═══════════════════════════════════════════════════════"
echo "  Omnia Protocol Local Testnet is Running!"
echo "═══════════════════════════════════════════════════════"
echo ""
echo "  Nodes:"
for i in $(seq 1 "$NODE_COUNT"); do
  HTTP_PORT=$((8080 + i - 1))
  NAME="Node $((i-1))"
  [ "$i" -eq 1 ] && NAME="Bootstrap"
  echo "    $NAME: http://localhost:$HTTP_PORT"
done
echo ""
echo "  API Endpoints (bootstrap):"
echo "    Health:   http://localhost:8080/healthz"
echo "    Ready:    http://localhost:8080/readyz"
echo "    Info:     http://localhost:8080/api/v1/node/info"
echo "    Peers:    http://localhost:8080/api/v1/node/peers"
echo "    Metrics:  http://localhost:8080/metrics"
echo ""
if [ -d "$WEB_DIR" ]; then
  echo "  Dashboard: http://localhost:3000"
  echo ""
fi
echo "  To stop: $0 --stop"
echo "═══════════════════════════════════════════════════════"
