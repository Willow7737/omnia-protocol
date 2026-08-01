#!/usr/bin/env bash
# Rotate OMNIA_JWT_SECRET for a docker-compose deployment.
#
# Usage:
#   ./scripts/rotate-jwt-secret.sh [compose-file]
#
# PASS THE COMPOSE FILE THAT ACTUALLY GOVERNS THIS HOST. The default below
# is the local multi-node stack; a geo-distributed WAN member runs
# `docker/docker-compose.wan.yml`. Rotating against the wrong file silently
# does nothing to the live node (and may try to start a second stack that
# fights the running one for port 9090).
#
# What it does:
#   1. Generates a fresh 256-bit secret.
#   2. Writes it to docker/.env (compose substitutes it into every node).
#   3. Recreates the containers (the node caches the secret at startup,
#      so a recreate is required — `restart` is not enough).
#   4. Prints the new secret ONCE so you can mirror it to Supabase
#      (Dashboard -> Edge Functions -> Secrets -> OMNIA_JWT_SECRET).
#
# Effects of rotation: every outstanding JWT (wallet sessions, web
# sessions) becomes invalid. Clients recover automatically — the wallet
# re-runs challenge/login (self-custody) or re-mints via the edge
# function (Supabase mode) on the next 401. The Supabase secret MUST be
# updated in the same sitting or Supabase-mode sign-ins will 401 until
# it is.
#
# MULTI-NODE DEPLOYMENTS: recreating the bootstrap node's container drops
# its peer connections, and peers do NOT re-dial it (see issue #411). The
# mesh silently degrades to a partition — the bootstrap node keeps serving
# HTTP 200 while finalizing nothing. After rotating on the bootstrap node
# you MUST restart the other nodes. The completion output below spells this
# out with a verification command.
set -euo pipefail
cd "$(dirname "$0")/.."

COMPOSE_FILE="${1:-docker/docker-compose.yml}"

if [ ! -f "${COMPOSE_FILE}" ]; then
    echo "error: compose file not found: ${COMPOSE_FILE}" >&2
    echo "Pass the file that governs this host, e.g." >&2
    echo "  $0 docker/docker-compose.wan.yml" >&2
    exit 1
fi

# Compose gives shell environment variables priority over docker/.env, so an
# exported OMNIA_JWT_SECRET would shadow the value we are about to write and
# the rotation would silently no-op. Refuse rather than pretend to succeed.
if [ -n "${OMNIA_JWT_SECRET:-}" ]; then
    echo "error: OMNIA_JWT_SECRET is set in this shell." >&2
    echo "Compose prefers shell env over docker/.env, so the rotation would" >&2
    echo "write a new secret that the containers then ignore. Run:" >&2
    echo >&2
    echo "  unset OMNIA_JWT_SECRET" >&2
    echo >&2
    echo "then re-run this script." >&2
    exit 1
fi

NEW_SECRET=$(openssl rand -hex 32)

touch docker/.env
grep -v '^OMNIA_JWT_SECRET=' docker/.env > docker/.env.tmp || true
mv docker/.env.tmp docker/.env
echo "OMNIA_JWT_SECRET=${NEW_SECRET}" >> docker/.env
chmod 600 docker/.env

docker compose -f "${COMPOSE_FILE}" up -d

echo
echo "Rotated. New OMNIA_JWT_SECRET (set the SAME value on Supabase now):"
echo
echo "  ${NEW_SECRET}"
echo
echo "Supabase: Dashboard -> Edge Functions -> Secrets -> OMNIA_JWT_SECRET"
echo "(or: supabase secrets set OMNIA_JWT_SECRET=${NEW_SECRET} --project-ref <ref>)"
echo
echo "----------------------------------------------------------------------"
echo "MULTI-NODE: restart the other nodes now, or the mesh stays partitioned."
echo "----------------------------------------------------------------------"
echo
echo "Recreating this container dropped its peer connections. Peers do NOT"
echo "re-dial a bootstrap node that restarts (issue #411), so they will keep"
echo "running without it and this node will finalize nothing — while still"
echo "answering HTTP 200. On EVERY other node in the mesh, run:"
echo
echo "  cd /opt/omnia-protocol && docker restart omnia-node"
echo
echo "Then verify every node reports the full peer count (n-1 peers each):"
echo
echo '  for ip in localhost <B-ip> <C-ip>; do'
echo '    printf "%-16s peers=" "$ip"'
echo '    curl -s "http://$ip:9090/api/v1/node/info" \'
echo '      | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['"'"'peers'"'"'], '"'"'lane0_finalized='"'"', d['"'"'lane0'"'"']['"'"'events_finalized'"'"'])"'
echo '  done'
echo
echo "A node reporting fewer peers than expected is partitioned: with equal"
echo "stake, 3 validators need all 3 acks for Lane 0 quorum, so one missing"
echo "peer halts finality network-wide."
