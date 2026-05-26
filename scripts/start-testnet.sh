#!/usr/bin/env bash
# Omnia Protocol — 5-Node Testnet Launcher
#
# Usage:
#   ./scripts/start-testnet.sh              # Start 5-node testnet
#   ./scripts/start-testnet.sh --monitoring  # Start with Prometheus + Grafana
#   ./scripts/start-testnet.sh --down        # Tear down
#   ./scripts/start-testnet.sh --logs        # Follow logs
#   ./scripts/start-testnet.sh --status      # Check node health
#
# The testnet runs:
#   - Bootstrap node: http://localhost:9090
#   - Node 1:         http://localhost:9091
#   - Node 2:         http://localhost:9092
#   - Node 3:         http://localhost:9093
#   - Node 4:         http://localhost:9094
#   - Prometheus:     http://localhost:9095  (with --monitoring)
#   - Grafana:        http://localhost:3000  (with --monitoring)
#
# Environment variables:
#   GRAFANA_ADMIN_PASSWORD  Required when using --monitoring (default: admin)
#   RUST_LOG                Log level (default: info)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
COMPOSE_FILE="$PROJECT_ROOT/docker/docker-compose.yml"

# Defaults
GRAFANA_ADMIN_PASSWORD="${GRAFANA_ADMIN_PASSWORD:-admin}"
RUST_LOG="${RUST_LOG:-info}"

case "${1:-}" in
  --down)
    echo "🛑 Stopping Omnia testnet..."
    docker compose -f "$COMPOSE_FILE" --profile monitoring down -v 2>/dev/null || \
      docker compose -f "$COMPOSE_FILE" down -v
    echo "✅ Testnet stopped and volumes removed."
    ;;

  --logs)
    echo "📋 Following testnet logs (Ctrl+C to stop)..."
    docker compose -f "$COMPOSE_FILE" logs -f
    ;;

  --status)
    echo "🔍 Checking node health..."
    for port in 9090 9091 9092 9093 9094; do
      name="node-$((port - 9090))"
      if curl -sf "http://localhost:$port/health" > /dev/null 2>&1; then
        echo "  ✅ $name (:$port) — healthy"
      else
        echo "  ❌ $name (:$port) — unreachable"
      fi
    done
    ;;

  --monitoring)
    echo "🚀 Starting Omnia 5-node testnet with monitoring..."
    GRAFANA_ADMIN_PASSWORD="$GRAFANA_ADMIN_PASSWORD" \
      docker compose -f "$COMPOSE_FILE" --profile monitoring up -d --build
    echo ""
    echo "⏳ Waiting for bootstrap node to become healthy..."
    for i in $(seq 1 60); do
      if curl -sf "http://localhost:9090/health" > /dev/null 2>&1; then
        echo "  ✅ Bootstrap node is healthy!"
        break
      fi
      sleep 2
    done
    echo ""
    echo "📊 Monitoring endpoints:"
    echo "  Prometheus:  http://localhost:9095"
    echo "  Grafana:     http://localhost:3000 (admin/$GRAFANA_ADMIN_PASSWORD)"
    echo ""
    echo "🌐 Node HTTP APIs:"
    echo "  Bootstrap:   http://localhost:9090"
    echo "  Node 1:      http://localhost:9091"
    echo "  Node 2:      http://localhost:9092"
    echo "  Node 3:      http://localhost:9093"
    echo "  Node 4:      http://localhost:9094"
    ;;

  *)
    echo "🚀 Starting Omnia 5-node testnet..."
    RUST_LOG="$RUST_LOG" docker compose -f "$COMPOSE_FILE" up -d --build
    echo ""
    echo "⏳ Waiting for bootstrap node to become healthy..."
    for i in $(seq 1 60); do
      if curl -sf "http://localhost:9090/health" > /dev/null 2>&1; then
        echo "  ✅ Bootstrap node is healthy!"
        break
      fi
      sleep 2
    done
    echo ""
    echo "🌐 Node HTTP APIs:"
    echo "  Bootstrap:   http://localhost:9090"
    echo "  Node 1:      http://localhost:9091"
    echo "  Node 2:      http://localhost:9092"
    echo "  Node 3:      http://localhost:9093"
    echo "  Node 4:      http://localhost:9094"
    echo ""
    echo "💡 Add --monitoring to enable Prometheus + Grafana"
    echo "💡 Use --status to check node health"
    echo "💡 Use --logs to follow logs"
    echo "💡 Use --down to stop and clean up"
    ;;
esac
