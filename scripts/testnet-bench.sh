#!/usr/bin/env bash
# Omnia Protocol — Multi-Node Testnet Benchmark (ADR-025 Stage 2)
#
# Drives write load against one node of a running testnet and measures how
# fast the resulting events propagate to every other node's DAG, using the
# per-node Prometheus metrics (`omnia_dag_events_total`).
#
# Usage:
#   OMNIA_JWT_SECRET=<secret> ./scripts/testnet-bench.sh
#   OMNIA_JWT_SECRET=<secret> ./scripts/testnet-bench.sh \
#     --nodes http://localhost:9090,http://localhost:9091,http://localhost:9092 \
#     --events 500 --concurrency 8 --timeout 120
#
# Options:
#   --nodes        Comma-separated node base URLs. Events are submitted to
#                  the FIRST node; propagation is measured on all of them.
#                  Default: the 3-node docker-compose.testnet.yml ports.
#   --events       Number of events to submit (default: 500)
#   --concurrency  Parallel submitters (default: 8)
#   --timeout      Seconds to wait for full propagation (default: 120)
#   --out          Output directory for the JSON report
#                  (default: bench-results/)
#
# Requirements: bash, curl, openssl, awk. The node must be built with the
# default features (metrics + network). OMNIA_JWT_SECRET must match the
# secret the nodes were started with.
#
# Methodology notes (keep results honest):
#   - Submission throughput here includes HTTP + JSON + signing overhead on
#     the target node; it is NOT comparable to the in-process hot-path
#     numbers in benchmark-gates.md.
#   - Propagation is measured by DAG growth deltas, so pre-existing events
#     and background chatter do not skew the count.
#   - Record results in docs/reference/benchmark-gates.md with the exact
#     topology (hosts, RTTs, container resources) alongside the numbers.

set -euo pipefail

NODES="http://localhost:9090,http://localhost:9091,http://localhost:9092"
EVENTS=500
CONCURRENCY=8
TIMEOUT=120
OUT_DIR="bench-results"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --nodes)       NODES="$2"; shift 2 ;;
    --events)      EVENTS="$2"; shift 2 ;;
    --concurrency) CONCURRENCY="$2"; shift 2 ;;
    --timeout)     TIMEOUT="$2"; shift 2 ;;
    --out)         OUT_DIR="$2"; shift 2 ;;
    *) echo "Unknown option: $1" >&2; exit 2 ;;
  esac
done

: "${OMNIA_JWT_SECRET:?OMNIA_JWT_SECRET must be set (same secret the nodes run with)}"

IFS=',' read -r -a NODE_URLS <<< "$NODES"
TARGET="${NODE_URLS[0]}"
RUN_ID="$(date +%s)-$$"

# ── Helpers ────────────────────────────────────────────────────────────────

b64url() { openssl base64 -A | tr '+/' '-_' | tr -d '='; }

mint_jwt() {
  local now exp header claims sig
  now=$(date +%s)
  exp=$((now + 3600))
  header=$(printf '{"alg":"HS256","typ":"JWT"}' | b64url)
  claims=$(printf '{"sub":"bench-%s","iat":%d,"exp":%d}' "$RUN_ID" "$now" "$exp" | b64url)
  sig=$(printf '%s.%s' "$header" "$claims" \
    | openssl dgst -sha256 -hmac "$OMNIA_JWT_SECRET" -binary | b64url)
  printf '%s.%s.%s' "$header" "$claims" "$sig"
}

dag_total() {
  # Reads omnia_dag_events_total from a node's /metrics; prints 0 if the
  # endpoint is transiently unavailable (|| true keeps pipefail from
  # aborting the whole run on one failed scrape).
  { curl -sf --max-time 5 "$1/metrics" 2>/dev/null || true; } \
    | awk '/^omnia_dag_events_total[ {]/{v=$NF} END{print (v=="" ? 0 : v)}'
}

metric() {
  { curl -sf --max-time 5 "$1/metrics" 2>/dev/null || true; } \
    | awk -v m="$2" '$0 ~ "^"m"[ {]" {v=$NF} END{print (v=="" ? 0 : v)}'
}

# ── 1. Health checks ───────────────────────────────────────────────────────

echo "🔍 Checking node health..."
for url in "${NODE_URLS[@]}"; do
  if curl -sf --max-time 5 "$url/health" > /dev/null; then
    echo "  ✅ $url"
  else
    echo "  ❌ $url unreachable — is the testnet up?" >&2
    exit 1
  fi
done

# ── 2. Baselines ───────────────────────────────────────────────────────────

declare -A BASELINE
for url in "${NODE_URLS[@]}"; do
  BASELINE[$url]=$(dag_total "$url")
  echo "  📏 baseline dag_events_total @ $url = ${BASELINE[$url]}"
done

# ── 3. Submit load ─────────────────────────────────────────────────────────

JWT=$(mint_jwt)
echo ""
echo "🚀 Submitting $EVENTS events to $TARGET (concurrency $CONCURRENCY)..."

RESULTS_TMP=$(mktemp)
trap 'rm -f "$RESULTS_TMP"' EXIT

# Live submission progress: one updating line so a long submit never looks
# stuck. Runs in the background while xargs fires requests; killed after.
SUB_START=$(date +%s)
(
  while :; do
    n=$(wc -l < "$RESULTS_TMP" 2>/dev/null || echo 0)
    pct=$(( EVENTS > 0 ? n * 100 / EVENTS : 0 ))
    filled=$(( pct * 24 / 100 )); (( filled > 24 )) && filled=24
    bar=$(printf '%*s' "$filled" '' | tr ' ' '#')$(printf '%*s' $(( 24 - filled )) '' | tr ' ' '-')
    printf '\r\033[K  🚀 [%s] %d/%d (%d%%)  %ds elapsed' \
      "$bar" "$n" "$EVENTS" "$pct" $(( $(date +%s) - SUB_START ))
    sleep 1
  done
) &
SUB_PROG_PID=$!

START_NS=$(date +%s%N)
seq 1 "$EVENTS" | xargs -P "$CONCURRENCY" -I {} sh -c '
  code=$(curl -s -o /dev/null -w "%{http_code}" --max-time 15 \
    -X POST "'"$TARGET"'/api/v1/events" \
    -H "Authorization: Bearer '"$JWT"'" \
    -H "Content-Type: application/json" \
    -d "{\"payload\": \"$(printf "%s-%s" "'"$RUN_ID"'" "{}" | od -An -tx1 | tr -d " \n")\", \"event_type\": \"bench\"}")
  echo "$code"
' >> "$RESULTS_TMP"
END_NS=$(date +%s%N)
kill "$SUB_PROG_PID" 2>/dev/null || true
wait "$SUB_PROG_PID" 2>/dev/null || true
printf '\r\033[K'

OK_COUNT=$(grep -c "^2" "$RESULTS_TMP" || true)
THROTTLED=$(grep -c "^429$" "$RESULTS_TMP" || true)
SUBMIT_SECS=$(awk -v s="$START_NS" -v e="$END_NS" 'BEGIN{printf "%.2f", (e-s)/1e9}')
SUBMIT_RATE=$(awk -v n="$OK_COUNT" -v t="$SUBMIT_SECS" 'BEGIN{ if (t>0) printf "%.1f", n/t; else print "0" }')

echo "  ✅ $OK_COUNT/$EVENTS accepted in ${SUBMIT_SECS}s (${SUBMIT_RATE} ev/s submit rate)"
if (( THROTTLED > 0 )); then
  echo "  ⚠️  $THROTTLED requests were rate-limited (HTTP 429)."
  echo "     The node defaults to OMNIA_RATE_LIMIT_RPS=10 (burst 20) per client."
  echo "     For benchmarks, start the testnet with e.g. OMNIA_RATE_LIMIT_RPS=1000."
fi
if [[ "$OK_COUNT" -eq 0 ]]; then
  echo "  ❌ No events accepted — check the JWT secret and node logs." >&2
  exit 1
fi

# ── 4. Propagation convergence ─────────────────────────────────────────────

echo ""
echo "⏳ Waiting for propagation (timeout ${TIMEOUT}s)..."
declare -A CONVERGED_AT
WAIT_START=$(date +%s)
DEADLINE=$(( WAIT_START + TIMEOUT ))
while :; do
  now=$(date +%s)
  all_done=1
  min_pct=100
  status=""
  for url in "${NODE_URLS[@]}"; do
    port="${url##*:}"
    if [[ -n "${CONVERGED_AT[$url]:-}" ]]; then
      status+="  ${port}:100%"
      continue
    fi
    delta=$(( $(dag_total "$url") - BASELINE[$url] ))
    if (( delta >= OK_COUNT )); then
      CONVERGED_AT[$url]=$(awk -v s="$START_NS" -v e="$(date +%s%N)" 'BEGIN{printf "%.2f", (e-s)/1e9}')
      printf '\r\033[K'
      echo "  ✅ $url converged (+$delta) at ${CONVERGED_AT[$url]}s"
      status+="  ${port}:100%"
    else
      all_done=0
      pct=$(( OK_COUNT > 0 ? delta * 100 / OK_COUNT : 0 ))
      status+="  ${port}:${pct}%"
      (( pct < min_pct )) && min_pct=$pct
    fi
  done
  if (( all_done )); then
    printf '\r\033[K'
    break
  fi
  if (( now >= DEADLINE )); then
    printf '\r\033[K'
    echo "  ⚠️  Timeout — reporting partial propagation."
    break
  fi
  # Live status line: elapsed timer, slowest-node progress bar, per-node %.
  # Redrawn in place so a slow repair tail is visibly moving, never "stuck".
  elapsed=$(( now - WAIT_START ))
  filled=$(( min_pct * 24 / 100 )); (( filled > 24 )) && filled=24
  bar=$(printf '%*s' "$filled" '' | tr ' ' '#')$(printf '%*s' $(( 24 - filled )) '' | tr ' ' '-')
  printf '\r\033[K  ⏱  %4ds/%ds  [%s]%s' "$elapsed" "$TIMEOUT" "$bar" "$status"
  sleep 2
done

# ── 5. Report ──────────────────────────────────────────────────────────────

mkdir -p "$OUT_DIR"
REPORT="$OUT_DIR/testnet-bench-$(date +%Y%m%d-%H%M%S).json"

{
  echo "{"
  echo "  \"run_id\": \"$RUN_ID\","
  echo "  \"timestamp\": \"$(date -u +%Y-%m-%dT%H:%M:%SZ)\","
  echo "  \"target\": \"$TARGET\","
  echo "  \"events_requested\": $EVENTS,"
  echo "  \"events_accepted\": $OK_COUNT,"
  echo "  \"concurrency\": $CONCURRENCY,"
  echo "  \"submit_seconds\": $SUBMIT_SECS,"
  echo "  \"submit_rate_eps\": $SUBMIT_RATE,"
  echo "  \"nodes\": ["
  first=1
  for url in "${NODE_URLS[@]}"; do
    (( first )) || echo "    ,"
    first=0
    delta=$(( $(dag_total "$url") - BASELINE[$url] ))
    finalized=$(metric "$url" "omnia_node_events_finalized_total")
    peers=$(metric "$url" "omnia_node_peers_connected")
    rss=$(metric "$url" "omnia_node_memory_rss_bytes")
    pct=$(awk -v d="$delta" -v n="$OK_COUNT" 'BEGIN{ if (n>0) printf "%.1f", 100*d/n; else print "0" }')
    echo "    {\"url\": \"$url\", \"dag_delta\": $delta, \"propagation_pct\": $pct,"
    echo "     \"converged_at_s\": ${CONVERGED_AT[$url]:-null}, \"finalized_total\": $finalized,"
    echo "     \"peers\": $peers, \"rss_bytes\": $rss}"
  done
  echo "  ]"
  echo "}"
} > "$REPORT"

echo ""
echo "📊 Summary"
printf '  %-28s %10s %8s %14s\n' "node" "dag Δ" "prop %" "converged (s)"
for url in "${NODE_URLS[@]}"; do
  delta=$(( $(dag_total "$url") - BASELINE[$url] ))
  pct=$(awk -v d="$delta" -v n="$OK_COUNT" 'BEGIN{ if (n>0) printf "%.1f", 100*d/n; else print "0" }')
  printf '  %-28s %10s %8s %14s\n' "$url" "$delta" "$pct" "${CONVERGED_AT[$url]:-—}"
done
echo ""
echo "  Submit: $OK_COUNT events in ${SUBMIT_SECS}s → ${SUBMIT_RATE} ev/s"
echo "  Report: $REPORT"
echo ""
echo "💡 Record the numbers + topology in docs/reference/benchmark-gates.md"
