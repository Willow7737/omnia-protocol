#!/usr/bin/env bash
# Omnia Protocol — Multi-Node Validator Setup (ADR-025 Lane 0)
#
# Prepares a reproducible N-node validator testnet:
#   1. Generates a persistent Ed25519 keypair per node (via `omnia-node
#      keygen`) into ops/testnet-keys/nodeK/ — validator_key.bin (raw
#      32-byte secret the node loads via OMNIA_NODE_KEY_FILE) plus
#      validator_pubkey.txt (hex public key).
#   2. Assembles OMNIA_LANE0_VALIDATORS = pk0:stake,pk1:stake,… from those
#      public keys, so every node's Lane 0 validator set is identical and
#      known before first boot (no chicken-and-egg).
#   3. Writes docker/.env with OMNIA_LANE0_VALIDATORS (and, if absent, a
#      fresh OMNIA_JWT_SECRET and a benchmark-friendly OMNIA_RATE_LIMIT_RPS),
#      preserving any other variables already there.
#
# docker/docker-compose.testnet.yml mounts ops/testnet-keys/nodeK into each
# node and points OMNIA_NODE_KEY_FILE at it, so pubkeys stay stable across
# restarts and match the validator set.
#
# Usage:
#   ./scripts/setup-validators.sh                 # 3 nodes, stake 1 each
#   NODES=3 STAKE=1 ./scripts/setup-validators.sh
#
# Then bring the testnet up:
#   OMNIA_JWT_SECRET=$(grep '^OMNIA_JWT_SECRET=' docker/.env | cut -d= -f2-) \
#   OMNIA_LANE0_VALIDATORS=$(grep '^OMNIA_LANE0_VALIDATORS=' docker/.env | cut -d= -f2-) \
#     docker compose -f docker/docker-compose.testnet.yml up -d --build
#   # (compose reads docker/.env automatically; the explicit vars above are
#   #  only needed if you invoke compose from another directory.)
#
# Re-running is safe: existing node keys are reused (not regenerated), so
# the validator set is stable. Delete ops/testnet-keys/ to rotate all keys.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT"

NODES="${NODES:-3}"
STAKE="${STAKE:-1}"
KEYS_DIR="ops/testnet-keys"
ENV_FILE="docker/.env"
BIN="target/release/omnia-node"

# ── 1. Ensure the node binary (for `keygen`) ────────────────────────────────

if [[ ! -x "$BIN" ]]; then
  echo "🔨 Building omnia-node (release) for keygen…"
  cargo build --release -p omnia-node
fi

# ── 2. Generate / reuse a keypair per node ──────────────────────────────────

mkdir -p "$KEYS_DIR"
declare -a ENTRIES=()

for ((i = 0; i < NODES; i++)); do
  dir="$KEYS_DIR/node$i"
  key="$dir/validator_key.bin"
  pub="$dir/validator_pubkey.txt"

  if [[ -f "$key" && -f "$pub" ]]; then
    echo "♻️  node$i — reusing existing key"
  else
    mkdir -p "$dir"
    # Unencrypted raw key: the node loads it directly via OMNIA_NODE_KEY_FILE.
    # (Production validators should use an encrypted key + the decrypt path;
    #  this is a local/dev testnet.)
    "$BIN" keygen --output-dir "$dir" >/dev/null
    echo "🔑 node$i — generated new key"
  fi

  pk="$(tr -d '[:space:]' < "$pub")"
  if [[ ${#pk} -ne 64 ]]; then
    echo "❌ node$i pubkey is not 64 hex chars: '$pk'" >&2
    exit 1
  fi
  ENTRIES+=("$pk:$STAKE")
done

# Keys are secrets — never commit them.
chmod -R go-rwx "$KEYS_DIR" 2>/dev/null || true

VALIDATORS="$(IFS=,; echo "${ENTRIES[*]}")"

# ── 3. Write docker/.env (preserve existing, update our keys) ───────────────

touch "$ENV_FILE"
tmp="$(mktemp)"
grep -v -E '^(OMNIA_LANE0_VALIDATORS|OMNIA_RATE_LIMIT_RPS)=' "$ENV_FILE" > "$tmp" || true
mv "$tmp" "$ENV_FILE"

# Generate a JWT secret only if one isn't already set (don't clobber a live one).
if ! grep -q '^OMNIA_JWT_SECRET=' "$ENV_FILE"; then
  echo "OMNIA_JWT_SECRET=$(openssl rand -hex 32)" >> "$ENV_FILE"
  echo "🔐 Generated a fresh OMNIA_JWT_SECRET in $ENV_FILE"
fi

{
  echo "OMNIA_LANE0_VALIDATORS=$VALIDATORS"
  # Benchmarks need headroom over the default 10 rps per-client limit.
  echo "OMNIA_RATE_LIMIT_RPS=${OMNIA_RATE_LIMIT_RPS:-1000}"
} >> "$ENV_FILE"
chmod 600 "$ENV_FILE"

# ── 4. Summary ──────────────────────────────────────────────────────────────

echo ""
echo "✅ Validator set ready for $NODES nodes (stake $STAKE each):"
for ((i = 0; i < NODES; i++)); do
  printf '   node%d  %s\n' "$i" "$(tr -d '[:space:]' < "$KEYS_DIR/node$i/validator_pubkey.txt")"
done
echo ""
echo "   Wrote OMNIA_LANE0_VALIDATORS + OMNIA_RATE_LIMIT_RPS to $ENV_FILE"
echo "   Keys in $KEYS_DIR/ (git-ignored — do not commit)."
echo ""
echo "🚀 Bring the testnet up (compose reads docker/.env automatically):"
echo "   docker compose -f docker/docker-compose.testnet.yml up -d --build"
echo ""
echo "🔍 Verify Lane 0 is live on each node:"
echo "   for p in 9090 9091 9092; do curl -s http://localhost:\$p/api/v1/node/info | \\"
echo "     python3 -c 'import json,sys;d=json.load(sys.stdin);print(d[\"node_id\"], d[\"lane0\"])'; done"
echo ""
echo "📈 Benchmark once all three are healthy and meshed:"
echo "   OMNIA_JWT_SECRET=\$(grep ^OMNIA_JWT_SECRET= $ENV_FILE | cut -d= -f2-) \\"
echo "     ./scripts/testnet-bench.sh --events 1000 --concurrency 16"
