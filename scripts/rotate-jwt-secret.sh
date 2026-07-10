#!/usr/bin/env bash
# Rotate OMNIA_JWT_SECRET for a docker-compose deployment.
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
set -euo pipefail
cd "$(dirname "$0")/.."

COMPOSE_FILE="${1:-docker/docker-compose.yml}"
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
