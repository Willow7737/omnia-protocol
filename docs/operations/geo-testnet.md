# Geo-Distributed Testnet Runbook

> Audience: Operators
> Context: Running the Omnia testnet across real internet distance (multiple
> regions), and benchmarking it honestly.
> Last Updated: 2026-07-19

Every benchmark recorded before this runbook ran on a **single host**
(near-zero RTT between containers). This runbook takes the same stack across
real WAN latency, which is the credibility jump that matters: it converts
"works in a lab mesh" into "works on the actual internet."

## Target topology (reference)

Three nodes, three regions, one provider (Hetzner shown; any VPS provider
works). All three are Lane 0 validators.

| Node | Region | Role | Suggested size |
|------|--------|------|----------------|
| **A** | Nuremberg, EU (`nbg1`) | bootstrap + validator + ingress + bench host | existing box |
| **B** | Ashburn, US-East (`ash`) | validator | CPX21 (3 vCPU / 4 GB) |
| **C** | Singapore (`sin`) | validator | CPX21 (3 vCPU / 4 GB) |

Expected RTTs: A↔B ~90 ms, A↔C ~170 ms, B↔C ~230 ms (the worst common
internet path — that is the point).

> **Quorum note:** three equal-stake validators means Lane 0 finality needs
> **all three** acks (> 2/3 of stake). Fine for a benchmark where all nodes
> stay up, but it has zero fault tolerance — one node down halts finality
> (propagation continues; finality resumes when it returns). Scale to 5
> validators (2 EU / 2 US / 1 Asia) for real 1-fault tolerance.

Sizing basis: a node peaked at ~160 MB RSS during a 10,000-event burst on
the 5-node single-host run — CPX21 is generous headroom.

## 1. Provision B and C

Ubuntu 24.04, Docker installed:

```bash
apt-get update && apt-get install -y docker.io docker-compose-v2 git curl
git clone https://github.com/Willow7737/omnia-protocol /opt/omnia-protocol
cd /opt/omnia-protocol && git checkout dev
```

## 2. Firewall (all three hosts)

Only two ports matter. QUIC needs UDP 4001 open **between the three node
IPs**; the HTTP API/metrics port should be reachable **only from the bench
host (A)** — do not expose /metrics to the world.

```bash
# On each host — substitute the two OTHER node IPs and A's IP:
ufw allow from <peer-ip-1> to any port 4001 proto udp
ufw allow from <peer-ip-2> to any port 4001 proto udp
ufw allow from <bench-host-ip> to any port 9090 proto tcp
ufw allow OpenSSH && ufw enable
```

(Host A already exposes 9090/9443 publicly for the wallet/dashboard via the
sslip.io reverse proxy — leave that as is; the rule above is for B and C.)

> **QUIC/MTU note:** QUIC v1 performs path-MTU discovery, but some clouds
> clamp UDP. If peers connect and then stall, test with
> `ping -M do -s 1400 <peer-ip>`; persistent fragmentation failures at
> ≤1400 indicate a clamped path — pick a different region/provider pair.

## 3. Keys and shared config (generate once, distribute)

The validator set must be **identical on every node**, so generate all keys
on host A and copy each node's key directory to its host:

```bash
# On A:
cd /opt/omnia-protocol
NODES=3 ./scripts/setup-validators.sh          # writes keys + docker/.env

scp -r ops/testnet-keys/node1 root@<B>:/opt/omnia-protocol/ops/testnet-keys/
scp -r ops/testnet-keys/node2 root@<C>:/opt/omnia-protocol/ops/testnet-keys/
scp docker/.env root@<B>:/opt/omnia-protocol/docker/.env
scp docker/.env root@<C>:/opt/omnia-protocol/docker/.env
```

Then append the per-node variables to `docker/.env` **on each host**:

```bash
# On A (bootstrap — dials nobody):
cat >> docker/.env <<'EOF'
OMNIA_NODE_ID=1
OMNIA_BOOTSTRAP_NODES=
OMNIA_KEY_DIR=../ops/testnet-keys/node0
OMNIA_TOTAL_NODES=3
EOF

# On B:
cat >> docker/.env <<'EOF'
OMNIA_NODE_ID=2
OMNIA_BOOTSTRAP_NODES=/ip4/<A-public-ip>/udp/4001/quic-v1
OMNIA_KEY_DIR=../ops/testnet-keys/node1
OMNIA_TOTAL_NODES=3
EOF

# On C (same as B but OMNIA_NODE_ID=3, OMNIA_KEY_DIR=../ops/testnet-keys/node2)
```

Keys are secrets: `chmod -R go-rwx ops/testnet-keys` and remember the
containers run as **uid 1000** — `setup-validators.sh` chowns for you when
run as root, but after `scp` re-check on B and C:
`chown -R 1000:1000 ops/testnet-keys`. (An unreadable key silently degrades
the node to a non-validator identity and finality never happens — the
classic failure, see the operational note in
[benchmark-gates.md](../reference/benchmark-gates.md).)

## 4. Bring the nodes up

Start **A first** (it is the bootstrap), then B and C:

```bash
# Each host:
cd /opt/omnia-protocol
docker compose -f docker/docker-compose.wan.yml up -d --build
```

`docker-compose.wan.yml` uses **host networking** deliberately: the node
binds the host's real UDP 4001 and TCP 9090, so there is no Docker NAT
between peers — inbound QUIC dials and libp2p's observed-address exchange
work without port-mapping games.

Verify each node, then the mesh:

```bash
# On each host:
curl -s http://localhost:9090/api/v1/node/info | python3 -m json.tool
# expect: "lane0": {...} present and this_node_is_validator=true in logs

# From A — peers must be 2 on every node before benchmarking:
for ip in localhost <B> <C>; do printf "%s peers=" "$ip"; \
  curl -s "http://$ip:9090/metrics" | awk '/^omnia_node_peers_connected /{print $2}'; done
```

## 5. Capture the RTT matrix (record it with the results)

```bash
# On A:
for ip in <B> <C>; do printf "A->%s  " "$ip"; ping -c 5 -q "$ip" | tail -1; done
# On B:
printf "B->C  "; ping -c 5 -q <C> | tail -1
```

Record the matrix (min/avg/max) in `benchmark-gates.md` next to the run —
"3 nodes across EU / US-East / Asia, RTTs 90/170/230 ms" is the sentence
that makes the numbers credible.

## 6. The benchmark ladder

Run from A. Climb; don't jump straight to 10k — each rung either passes
(record it) or fails in a way that tells you exactly which assumption broke:

```bash
OMNIA_JWT_SECRET=$(grep ^OMNIA_JWT_SECRET= docker/.env | cut -d= -f2-) \
  ./scripts/testnet-bench.sh \
  --nodes http://localhost:9090,http://<B>:9090,http://<C>:9090 \
  --events 1000  --concurrency 16 --timeout 300     # rung 1
# then: --events 5000  --concurrency 64 --timeout 600   rung 2
# then: --events 10000 --concurrency 64 --timeout 900   rung 3
```

The script shows a live progress line during submission and propagation, so
a slow WAN tail is visibly moving. Convergence times are measured on A's
clock only (the script polls every node from one host), so **cross-node
clock skew cannot skew the numbers** — only ~0.1–0.2 s of polling RTT
noise. NTP on the hosts is good hygiene for log correlation, nothing more.

What the rungs test, in order:
1. **1k** — mesh formation + steady gossip at WAN RTT (should look like the
   single-host run plus RTT).
2. **5k** — burst overload + anti-entropy repair across 90–230 ms paths
   (repair chains one batch per round; each chain round-trip now costs real
   RTT, so expect the repair tail to stretch roughly with RTT).
3. **10k** — the full self-healing story under the worst path. This is the
   headline if it converges.

If a rung fails, the failure localizes the assumption: no mesh → §2
firewall / bootstrap addr; propagation stalls at a rate-limiter-shaped
ceiling with repair active → read the repair logs exactly as in the
2026-07-19 diagnosis arc in
[benchmark-gates.md](../reference/benchmark-gates.md); finality zero with
propagation fine → §3 key permissions or a validator-set mismatch.

## 7. Record the results

Add a row to `benchmark-gates.md` with: date, topology (regions + instance
types + RTT matrix), events/concurrency, per-node convergence, finalized
totals, and the report JSON filename (`bench-results/…json`). Single-host
numbers carry an asterisk; these don't — say so.

## Troubleshooting quick table

| Symptom | Cause | Fix |
|---|---|---|
| `peers=0` on B/C | UDP 4001 blocked, or wrong bootstrap addr | §2 firewall; `OMNIA_BOOTSTRAP_NODES` must be `/ip4/<A>/udp/4001/quic-v1` |
| `this_node_is_validator=false` in logs | key dir unreadable by uid 1000 | `chown -R 1000:1000 ops/testnet-keys` and restart |
| finality stuck at 0, propagation fine | `OMNIA_LANE0_VALIDATORS` differs between hosts | re-copy `docker/.env` from A; every node must have the identical list |
| peers connect then stall | UDP MTU clamped on the path | `ping -M do -s 1400` test; try another region pair |
| propagation wedges at a flat % | repair-path regression | compare against the 2026-07-19 diagnosis arc in benchmark-gates.md before touching code |
