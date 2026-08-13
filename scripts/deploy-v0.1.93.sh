#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# deploy-v0.1.93.sh — Update all 5 geo-distributed WAN nodes to latest main
#
# Usage:
#   1. Copy this script to host A (bootstrap / Nuremberg):
#        scp scripts/deploy-v0.1.93.sh root@78.47.43.136:/root/
#   2. SSH into host A and run:
#        bash /root/deploy-v0.1.93.sh
#
# What it does:
#   Phase 1: On ALL 5 hosts — git pull, verify same commit, docker build
#   Phase 2: Restart A (bootstrap) first, then B-E in parallel
#   Phase 3: Restore monitoring files on A, restart Prometheus
#   Phase 4: Automated verification (peers, finality, ack rejections)
#
# Prerequisites:
#   - SSH key-based access from A to B, C, D, E
#   - docker compose v2 on all hosts
#   - /opt/omnia-protocol exists on all hosts with docker/.env configured
#
# Node topology:
#   A = 78.47.43.136  (Nuremberg,    bootstrap + validator + monitoring)
#   B = 178.156.163.211 (Ashburn,     validator)
#   C = 5.223.85.30    (Singapore,   validator)
#   D = 46.62.218.24   (Helsinki,    validator)
#   E = 46.224.103.217  (Falkenstein, validator)
# ---------------------------------------------------------------------------
set -euo pipefail

NODES=("A:78.47.43.136" "B:178.156.163.211" "C:5.223.85.30" "D:46.62.218.24" "E:46.224.103.217")
REMOTE_NODES=("B:178.156.163.211" "C:5.223.85.30" "D:46.62.218.24" "E:46.224.103.217")
ALL_IPS=("localhost" "178.156.163.211" "5.223.85.30" "46.62.218.24" "46.224.103.217")
ALL_NAMES=("A" "B" "C" "D" "E")
REPO_DIR="/opt/omnia-protocol"
COMPOSE_FILE="docker/docker-compose.wan.yml"
MONITOR_COMPOSE="docker/docker-compose.monitoring.yml"
BUILD_TIMEOUT=600  # 10 min — Rust release builds are slow

colours() {
  RED='\033[0;31m'; GRN='\033[0;32m'; YEL='\033[1;33m'; BLU='\033[0;34m'
  BOLD='\033[1m'; RST='\033[0m'
}
colours

log()  { echo -e "${BLU}[$(date +%H:%M:%S)]${RST} $*"; }
ok()   { echo -e "${GRN}  [OK]${RST} $*"; }
warn() { echo -e "${YEL}  [WARN]${RST} $*"; }
fail() { echo -e "${RED}  [FAIL]${RST} $*"; exit 1; }

# Run a command on a remote host via SSH
remote() {
  local host="$1"; shift
  ssh -o ConnectTimeout=10 -o StrictHostKeyChecking=accept-new "root@$host" "$@"
}

# ---------------------------------------------------------------------------
# Phase 0: Backup monitoring files on A (before git checkout clobbers them)
# ---------------------------------------------------------------------------
phase0_backup() {
  log "Phase 0: Backing up monitoring files on A"
  mkdir -p /root/monitoring-backup
  cp -a "${REPO_DIR}/docker/monitoring/prometheus-wan.yml" /root/monitoring-backup/ 2>/dev/null || true
  cp -a "${REPO_DIR}/docker/monitoring/grafana-cloud-token" /root/monitoring-backup/ 2>/dev/null || true

  # Check if the live prometheus config has remote_write enabled
  if [ -f /root/monitoring-backup/prometheus-wan.yml ] && \
     grep -q '^remote_write:' /root/monitoring-backup/prometheus-wan.yml; then
    ok "Backup contains active remote_write config"
  else
    warn "No active remote_write in backup — Grafana Cloud shipping may need manual setup"
  fi
}

# ---------------------------------------------------------------------------
# Phase 1: Pull & build on ALL 5 hosts
# ---------------------------------------------------------------------------
phase1_pull_build() {
  log "Phase 1: Pulling and building on all 5 hosts"
  log "  (Rust release builds take 3-7 min per host — building in parallel)"

  local pids=()
  local tmpfiles=()

  for entry in "${NODES[@]}"; do
    local name="${entry%%:*}"
    local ip="${entry##*:}"
    local tmpf=$(mktemp)
    tmpfiles+=("$tmpf")

    if [ "$name" = "A" ]; then
      # Local — run directly
      (
        cd "$REPO_DIR"
        git fetch origin 2>&1 && git checkout -B main origin/main 2>&1
        git log -1 --format='%h %s' > "$tmpf"
        docker compose -f "$COMPOSE_FILE" build 2>&1 | tail -5
        echo "BUILD_EXIT=$?" >> "$tmpf"
      ) &
    else
      # Remote
      (
        remote "$ip" "cd $REPO_DIR && git fetch origin 2>&1 && git checkout -B main origin/main 2>&1; git log -1 --format='%h %s' > /tmp/deploy-hash-$name; docker compose -f $COMPOSE_FILE build 2>&1 | tail -5; echo BUILD_EXIT=\$? >> /tmp/deploy-hash-$name; cat /tmp/deploy-hash-$name" > "$tmpf" 2>&1
      ) &
    fi
    pids+=($!)
    log "  Building on $name ($ip)..."
  done

  # Wait for all builds
  local failed=()
  for i in "${!pids[@]}"; do
    if ! wait "${pids[$i]}" 2>/dev/null; then
      failed+=("${NODES[$i]%%:*}")
    fi
  done

  # Check exit codes
  for entry in "${NODES[@]}"; do
    local name="${entry%%:*}"
    local ip="${entry##*:}"
    local hash
    if [ "$name" = "A" ]; then
      hash=$(cd "$REPO_DIR" && git log -1 --format='%h %s')
      ok "$name: $hash"
    else
      hash=$(remote "$ip" "cd $REPO_DIR && git log -1 --format='%h %s'")
      ok "$name: $hash"
    fi
  done

  if [ ${#failed[@]} -gt 0 ]; then
    fail "Build failed on: ${failed[*]}"
  fi

  log "  All builds complete. Verifying all hosts are on the same commit..."
  local first_hash=""
  for entry in "${NODES[@]}"; do
    local name="${entry%%:*}"
    local ip="${entry##*:}"
    local hash
    if [ "$name" = "A" ]; then
      hash=$(cd "$REPO_DIR" && git rev-parse HEAD)
    else
      hash=$(remote "$ip" "cd $REPO_DIR && git rev-parse HEAD")
    fi
    if [ -z "$first_hash" ]; then
      first_hash="$hash"
    elif [ "$hash" != "$first_hash" ]; then
      fail "$name is on a different commit ($hash) vs A ($first_hash)"
    fi
  done
  ok "All 5 hosts on same commit: $(echo "$first_hash" | cut -c1-8)"

  # Cleanup
  rm -f "${tmpfiles[@]}"
}

# ---------------------------------------------------------------------------
# Phase 2: Restart — A first (bootstrap), then B-E
# ---------------------------------------------------------------------------
phase2_restart() {
  log "Phase 2: Restarting nodes (A first, then B-E)"

  log "  Restarting A (bootstrap)..."
  cd "$REPO_DIR" && docker compose -f "$COMPOSE_FILE" up -d 2>&1
  ok "  A restarted"

  # Give bootstrap 15s to be dialable before the others try
  log "  Waiting 15s for bootstrap to accept connections..."
  sleep 15

  log "  Restarting B, C, D, E in parallel..."
  local pids=()
  for entry in "${REMOTE_NODES[@]}"; do
    local name="${entry%%:*}"
    local ip="${entry##*:}"
    (remote "$ip" "cd $REPO_DIR && docker compose -f $COMPOSE_FILE up -d" 2>&1 && echo "$name ok") &
    pids+=($!)
  done
  for pid in "${pids[@]}"; do wait "$pid"; done
  ok "  B, C, D, E restarted"
}

# ---------------------------------------------------------------------------
# Phase 3: Restore monitoring on A
# ---------------------------------------------------------------------------
phase3_monitoring() {
  log "Phase 3: Restoring monitoring on A"

  # Restore live prometheus config (with real remote_write credentials)
  if [ -f /root/monitoring-backup/prometheus-wan.yml ]; then
    cp /root/monitoring-backup/prometheus-wan.yml "${REPO_DIR}/docker/monitoring/prometheus-wan.yml"
    ok "  Restored prometheus-wan.yml"
  else
    warn "  No prometheus backup found — skipping restore"
  fi

  # Restore Grafana Cloud token
  if [ -f /root/monitoring-backup/grafana-cloud-token ]; then
    cp /root/monitoring-backup/grafana-cloud-token "${REPO_DIR}/docker/monitoring/grafana-cloud-token"
    chown 65534:65534 "${REPO_DIR}/docker/monitoring/grafana-cloud-token"
    chmod 644 "${REPO_DIR}/docker/monitoring/grafana-cloud-token"
    ok "  Restored grafana-cloud-token (chown 65534:65534 chmod 644)"
  else
    warn "  No grafana-cloud-token backup found — remote_write will not work"
  fi

  # Verify remote_write is present
  local rw_count
  rw_count=$(grep -c '^remote_write' "${REPO_DIR}/docker/monitoring/prometheus-wan.yml" 2>/dev/null || echo 0)
  if [ "$rw_count" -eq 1 ]; then
    ok "  remote_write block present"
  else
    warn "  remote_write block count is $rw_count (expected 1) — check the config"
  fi

  # Restart Prometheus to pick up new code + restored config
  log "  Restarting Prometheus..."
  cd "$REPO_DIR" && docker compose -f "$MONITOR_COMPOSE" up -d 2>&1
  ok "  Prometheus restarted"
}

# ---------------------------------------------------------------------------
# Phase 4: Verification
# ---------------------------------------------------------------------------
phase4_verify() {
  log "Phase 4: Verification (waiting 30s for mesh to form)..."
  sleep 30

  local errors=0

  # 4a. Peer count — all must show 4
  log ""
  log "  Peer count (must be 4 on all nodes):"
  for i in "${!ALL_IPS[@]}"; do
    local ip="${ALL_IPS[$i]}"
    local name="${ALL_NAMES[$i]}"
    local peers
    peers=$(curl -sf --connect-timeout 5 "http://$ip:9090/metrics" 2>/dev/null | \
      awk '/^omnia_node_peers_connected /{print $2; exit}')
    if [ -z "$peers" ]; then
      fail "  $name ($ip): UNREACHABLE"
    elif [ "$peers" -eq 4 ]; then
      ok "  $name ($ip): peers=$peers"
    else
      warn "  $name ($ip): peers=$peers (expected 4)"
      errors=$((errors + 1))
    fi
  done

  # 4b. Lane 0 health
  log ""
  log "  Lane 0 health (acks_ok, acks_rej, events_finalized):"
  for i in "${!ALL_IPS[@]}"; do
    local ip="${ALL_IPS[$i]}"
    local name="${ALL_NAMES[$i]}"
    local info
    info=$(curl -sf --connect-timeout 5 "http://$ip:9090/api/v1/node/info" 2>/dev/null | \
      python3 -c "import sys,json;d=json.load(sys.stdin);l=d.get('lane0',d);print(f\"acks_ok={l.get('acks_accepted','?')} acks_rej={l.get('acks_rejected','?')} final={l.get('events_finalized','?')}\")" 2>/dev/null)
    if [ -z "$info" ]; then
      warn "  $name ($ip): could not parse /api/v1/node/info"
      errors=$((errors + 1))
    else
      echo -e "  $name ($ip): $info"
      # Check for ack rejections
      local rej
      rej=$(echo "$info" | grep -oP 'acks_rej=\K\d+')
      if [ -n "$rej" ] && [ "$rej" -gt 0 ]; then
        warn "    -> ack rejections detected — possible version mismatch"
        errors=$((errors + 1))
      fi
    fi
  done

  # 4c. Ack batch decode failures in docker logs
  log ""
  log "  Checking for ack batch decode failures (docker logs):"
  local batch_fails
  batch_fails=$(docker logs omnia-node 2>&1 | grep -ci "ack batch rejected" || true)
  if [ "$batch_fails" -eq 0 ]; then
    ok "  No ack batch decode failures"
  else
    warn "  $batch_fails 'ack batch rejected' lines in logs — version skew likely"
    errors=$((errors + 1))
  fi

  # 4d. Prometheus scrape targets
  log ""
  log "  Prometheus scrape targets:"
  local targets_json
  targets_json=$(curl -sf --connect-timeout 5 'localhost:9095/api/v1/targets' 2>/dev/null | \
    python3 -c "import sys,json
data=json.load(sys.stdin)
for t in data.get('data',{}).get('activeTargets',[]):
    print(f\"  {t['labels'].get('node','?'):5s} {t['labels'].get('instance','?'):25s} {t['health']}\")" 2>/dev/null)
  if [ -n "$targets_json" ]; then
    echo "$targets_json"
    local unhealthy
    unhealthy=$(echo "$targets_json" | grep -c 'down' || true)
    if [ "$unhealthy" -eq 0 ]; then
      ok "  All Prometheus targets healthy"
    else
      warn "  $unhealthy target(s) down"
      errors=$((errors + 1))
    fi
  else
    warn "  Could not query Prometheus targets (Prometheus may still be starting)"
  fi

  # 4e. Remote write check
  log ""
  log "  Remote write status (bytes shipped to Grafana Cloud):"
  local rw_bytes
  rw_bytes=$(curl -sf --connect-timeout 5 'localhost:9095/metrics' 2>/dev/null | \
    awk '/^prometheus_remote_storage_bytes_total/{print $2; exit}')
  if [ -n "$rw_bytes" ] && [ "$rw_bytes" -gt 0 ]; then
    ok "  Remote write active: $rw_bytes bytes shipped"
  else
    warn "  Remote write not shipping (0 bytes or unreachable) — check token/config"
    errors=$((errors + 1))
  fi

  # Summary
  log ""
  log "======================================"
  if [ "$errors" -eq 0 ]; then
    ok "All verifications passed. Network is healthy."
  else
    warn "$errors issue(s) found. See above. The mesh may need investigation."
  fi
  log "======================================"
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
main() {
  echo -e "${BOLD}"
  echo "  Omnia Protocol — Deploy v0.1.93 to 5-node WAN testnet"
  echo "  $(date -u '+%Y-%m-%d %H:%M:%S UTC')"
  echo -e "${RST}"
  echo "  Nodes: A (Nuremberg), B (Ashburn), C (Singapore), D (Helsinki), E (Falkenstein)"
  echo ""

  phase0_backup
  echo ""
  phase1_pull_build
  echo ""
  phase2_restart
  echo ""
  phase3_monitoring
  echo ""
  phase4_verify
}

main "$@"