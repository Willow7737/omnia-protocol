# Multi-Node Testnet Benchmark (ADR-025 Stage 2)

> 🎯 Audience: Operators
> 🔗 Context: Measures real multi-node throughput and propagation for the [ADR-025](../adr/ADR-025-two-lane-consensus.md) rollout
> 📅 Last Updated: 2026-07-10

Stage 2 of ADR-025 requires **honest multi-node numbers**: the causal-graph
hot path does ~7,675 events/s in-process, but the only multi-node figure on
record (~77 events/s) comes from an in-process 3-node *simulation*. This
runbook produces real numbers from a real topology.

## What the benchmark measures

`scripts/testnet-bench.sh` drives authenticated write load against ONE node
and watches every node's Prometheus metrics until the events appear in all
of their DAGs:

1. **Submission throughput** — accepted `POST /api/v1/events` per second
   (includes HTTP + JSON + node-side signing; NOT comparable to the
   in-process numbers in [benchmark-gates.md](../reference/benchmark-gates.md)).
2. **Propagation completeness** — per node, the growth of
   `omnia_dag_events_total` relative to its pre-run baseline, as a
   percentage of accepted events.
3. **Convergence time** — seconds from the start of submission until each
   node's DAG contains all submitted events.
4. **Steady-state signals** — `omnia_node_events_finalized_total`,
   `omnia_node_peers_connected`, `omnia_node_memory_rss_bytes` per node.

The per-node metrics are sampled once per consensus round (1 s) by the
node's background loop, so convergence times have ±1–2 s of sampling
granularity — fine for the second-scale answers Stage 2 needs.

## Prerequisites

- A running testnet built from `dev` at or after ADR-025 Stage 1
  (PR #276), so the gossip path includes the integrated components and the
  nodes export live metrics.
- `OMNIA_JWT_SECRET` — the same secret the nodes run with.
- **Raise the API rate limit for the run.** The node defaults to
  `OMNIA_RATE_LIMIT_RPS=10` (burst 20) per client, which throttles any
  meaningful load test to ~10 ev/s. Both compose files pass the variable
  through:

  ```bash
  OMNIA_JWT_SECRET=<secret> OMNIA_RATE_LIMIT_RPS=1000 \
    docker compose -f docker/docker-compose.testnet.yml up -d --build
  ```

## Running

```bash
# Local 3-node compose testnet (defaults match its port mappings):
OMNIA_JWT_SECRET=<secret> ./scripts/testnet-bench.sh

# Real hosts:
OMNIA_JWT_SECRET=<secret> ./scripts/testnet-bench.sh \
  --nodes https://node-a.example,https://node-b.example,https://node-c.example \
  --events 1000 --concurrency 16 --timeout 180
```

Events are submitted to the **first** URL; propagation is measured on all
of them. The script prints a summary table and writes a JSON report to
`bench-results/` (git-ignored territory — copy numbers into docs, don't
commit raw reports).

Example (single dev-build node, 2026-07-10 — pipeline verification, not a
performance claim):

```
📊 Summary
  node                             dag Δ   prop %  converged (s)
  http://localhost:8080               200    100.0           7.78

  Submit: 200 events in 5.75s → 34.8 ev/s
```

## Recording results

Copy the summary into `docs/reference/benchmark-gates.md` under a dated
heading, always with:

- topology (hosts, regions/RTTs, container CPU/memory limits),
- build profile (release vs dev) and version/commit,
- `OMNIA_RATE_LIMIT_RPS` and the script's `--events/--concurrency`,
- the JSON report values, including partial-propagation percentages if the
  run timed out.

These become the **pre-Lane-0 baseline** that ADR-025 Stage 3 must beat.

## Interpreting results

- **prop % < 100 with converged nodes present** — gossip is propagating but
  slowly; check `omnia_node_peers_connected` on the lagging node and the
  gossip logs for rate-limiting (`max_events_per_second` in gossip config).
- **prop % = 0 on non-target nodes** — the nodes never meshed: verify
  bootstrap addresses and that UDP/QUIC ports are reachable between hosts.
- **429s during submission** — raise `OMNIA_RATE_LIMIT_RPS` (see above).
- **finalized_total stays 0** — expected on topologies without a validator
  quorum; finality metrics become meaningful once the validator set is
  established (Stage 2+ with staked validators).

---

🔙 **Back**: [operations/](./) | 🔄 **Related**: [../reference/benchmark-gates.md](../reference/benchmark-gates.md), [../adr/ADR-025-two-lane-consensus.md](../adr/ADR-025-two-lane-consensus.md)
