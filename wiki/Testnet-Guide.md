# Testnet Guide

The Omnia testnet is a real QUIC/gossipsub mesh of Lane 0 validator nodes —
the same code path as production, no simulation.

## Topologies in the repo

| Compose file | Shape | Use for |
|---|---|---|
| `docker/docker-compose.testnet.yml` | 3 nodes on one host (bootstrap + 2) | Quick local testnet |
| `docker/docker-compose.yml` + `docker-compose.lane0.yml` | 5-node worker mesh + validator overlay | The full single-host benchmark topology |
| `docker/docker-compose.wan.yml` | 1 node per host, host networking | **Geo-distributed** testnet across real regions |

## Standing up a validator testnet (single host)

```bash
git clone https://github.com/Willow7737/omnia-protocol && cd omnia-protocol
NODES=5 ./scripts/setup-validators.sh        # keys + OMNIA_LANE0_VALIDATORS + .env
docker compose -f docker/docker-compose.yml -f docker/docker-compose.lane0.yml up -d --build
```

`setup-validators.sh` generates a persistent Ed25519 keypair per node and
writes the identical validator set into `docker/.env` — every node must
agree on `OMNIA_LANE0_VALIDATORS` byte-for-byte or acks are rejected as
"unknown validator."

**The classic gotcha:** containers run as uid 1000. If validator keys are
root-owned with `go-rwx`, the node silently falls back to an ephemeral
identity (`this_node_is_validator=false` in logs) and finality stays at
zero while propagation looks healthy. The script chowns for you when run as
root; re-check after copying keys between machines.

## Benchmarking

```bash
OMNIA_JWT_SECRET=$(grep ^OMNIA_JWT_SECRET= docker/.env | cut -d= -f2-) \
  ./scripts/testnet-bench.sh \
  --nodes http://localhost:9090,http://localhost:9091,http://localhost:9092,http://localhost:9093,http://localhost:9094 \
  --events 10000 --concurrency 64 --timeout 600
```

The script submits load to the first node, then measures propagation on
**all** listed nodes via Prometheus metrics, with a live progress line
(elapsed timer, slowest-node bar, per-node percentages) so long repair
tails are visibly moving. It writes a JSON report to `bench-results/`.

What the metrics mean:

- `omnia_dag_events_total` — events inserted into this node's DAG
  (propagation).
- `omnia_node_events_finalized_total` — events with a Lane 0 quorum
  certificate (BFT finality). Trails propagation by moments; certificates
  are grow-only, so it never decreases.
- `omnia_node_peers_connected` — mesh health; N-1 on a full mesh.

## Going geo-distributed

The full operator runbook — provisioning, firewall rules, key
distribution, RTT-matrix capture, the 1k→5k→10k benchmark ladder, and a
troubleshooting table — is
[`docs/operations/geo-testnet.md`](https://github.com/Willow7737/omnia-protocol/blob/main/docs/operations/geo-testnet.md).

## Troubleshooting (short version)

| Symptom | First check |
|---|---|
| `peers=0` | UDP 4001 reachability; `OMNIA_BOOTSTRAP_NODES` multiaddr |
| Finality 0, propagation fine | Key permissions (uid 1000) or validator-set mismatch |
| Propagation stuck at a flat % | Read the anti-entropy repair logs; compare with the diagnosis arc in [`benchmark-gates.md`](https://github.com/Willow7737/omnia-protocol/blob/main/docs/reference/benchmark-gates.md) |
| HTTP 429 during benchmarks | Raise `OMNIA_RATE_LIMIT_RPS` (default 10) |
