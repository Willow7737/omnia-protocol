#!/usr/bin/env bash
set -euo pipefail

# Omnia Protocol — Fuzz Testing Script
#
# Runs each fuzz target for a configurable duration (default: 60 seconds).
# Requires cargo-fuzz: cargo install cargo-fuzz
#
# Usage:
#   ./scripts/fuzz.sh              # Run all targets for 60s each
#   FUZZ_TIME=300 ./scripts/fuzz.sh  # Run all targets for 5 minutes each

FUZZ_TIME=${FUZZ_TIME:-60}
CRATE_DIR="$(cd "$(dirname "$0")/.." && pwd)"

echo "=== Omnia Protocol Fuzz Testing ==="
echo "Running each target for ${FUZZ_TIME}s..."
echo ""

TARGETS=(
    fuzz_event_deserialization
    fuzz_gossip_message
    fuzz_zk_proof_deserialization
    fuzz_consensus_state_transition
    fuzz_vector_clock_merge
    fuzz_rate_limiter
    fuzz_snapshot_deserialization
)

FAILED=0

for target in "${TARGETS[@]}"; do
    echo "--- Running $target ---"
    if cargo fuzz run "$target" -- -max_total_time="$FUZZ_TIME" 2>&1; then
        echo "✅ $target completed without crashes"
    else
        echo "❌ Fuzz target $target found issues!"
        FAILED=$((FAILED + 1))
    fi
    echo ""
done

if [ "$FAILED" -eq 0 ]; then
    echo "=== All fuzz targets passed ==="
else
    echo "=== $FAILED fuzz target(s) failed ==="
    exit 1
fi
