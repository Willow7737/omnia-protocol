# Geo-Distributed Testnet Runbook

> Audience: Operators
> Context: Running the Omnia testnet across real internet distance (multiple
> regions), and benchmarking it honestly.
> Last Updated: 2026-08-08

Every benchmark recorded before this runbook ran on a **single host**
(near-zero RTT between containers). This runbook takes the same stack across
real WAN latency, which is the credibility jump that matters: it converts
"works in a lab mesh" into "works on the actual internet."

## Target topology (reference)

Five nodes, three regions (EU-central, US-east, AP-southeast), two continents.
All five are Lane 0 validators.

| Node | IP | Region | vCPU | Role |
|------|----|--------|:----:|------|
| **A** | 78.47.43.136 | Nuremberg, DE (`nbg1`) | 3 | bootstrap + validator + ingress + bench host + Bitcoin testnet4 |
| **B** | 178.156.163.211 | Ashburn, US-East (`ash`) | 3 | validator |
| **C** | 5.223.85.30 | Singapore (`sin`) | 4 | validator |
| **D** | 46.62.218.24 | Helsinki, FI (`hel1`) | 4 | validator |
| **E** | 46.224.103.217 | Falkenstein, DE (`fsn1`) | 4 | validator |

Sizing: CPX21 (3–4 vCPU / 4 GB) is ample for validator nodes — see the footprint note below. Node A runs a larger instance (CPX31, 16 GB / 300 GB SSD) to accommodate Bitcoin Core's testnet4 IBD and full unpruned chain with `txindex=1`.

Expected RTTs: A↔B ~90 ms, A↔C ~170 ms, B↔C ~230 ms (the worst common
internet path — that is the point).

> **Quorum note:** five equal-stake validators means Lane 0 finality needs
> **four of five** acks (> 3/5 of stake). This provides 1-fault tolerance —
> one node down still achieves quorum.

Sizing basis: a node peaked at ~160 MB RSS during a 10,000-event burst on
the 5-node single-host run — CPX21 is generous headroom. Measured in
production on 2026-08-01, a live node idled at **30 MB RSS with 0.02% CPU**,
so the constraint is fault tolerance, not resources.

## Expanding the validator set (3 → 5)

Read this whole section before starting. Expansion is not additive: every
node's `OMNIA_LANE0_VALIDATORS` must change, so **all** nodes restart, not
just the new ones.

### Why 5

| Validators | Quorum (>2/3 stake) | Faults tolerated |
|---|---|---|
| 3 | all 3 | **0** — any node down halts finality |
| 5 | 4 of 5 | **1** |
| 7 | 5 of 7 | **2** |

Three is a benchmark topology. Five is the smallest set that survives losing
a node, which is the point of running more than one.

**Correlated failure is the limit, not node count.** Five validators tolerate
one failure, so *any two simultaneous* losses halt finality. Placement
therefore matters as much as the number: the current set is 3 EU / 1 US / 1
Asia, with A (`nbg1`) and E (`fsn1`) both in Germany on the same provider.
A single German or Hetzner-EU incident is one event that can take two nodes
and stop finality network-wide. That is an accepted trade for now — it is
still strictly better than the previous zero-fault-tolerance topology — but
it means the next expansion should add capacity *outside* the EU (and ideally
outside Hetzner) rather than more of the same region.

### Procedure

**1. Generate keys for the new nodes — on host A only.**

The validator set must be byte-identical everywhere, so all keys come from
one place. `setup-validators.sh` reuses existing keys and only creates what
is missing, so nodes 0–2 keep their identities:

```bash
cd /opt/omnia-protocol
NODES=5 ./scripts/setup-validators.sh
```

This rewrites `OMNIA_LANE0_VALIDATORS` in `docker/.env` with all five
pubkeys. It preserves an existing `OMNIA_JWT_SECRET` rather than clobbering
it — confirm that, because a changed secret invalidates every live wallet
session.

**2. Provision D and E** as in §1–2 above (Docker, clone, firewall).

Firewall is the step most likely to be done incompletely: five nodes means
**ten pairs**, and the existing hosts each need rules for the two new IPs.
On every host, allow UDP 4001 from the other four, and TCP 9090 from host A
only:

```bash
# Run on EACH host, omitting its own IP:
for ip in 78.47.43.136 178.156.163.211 5.223.85.30 46.62.218.24 46.224.103.217; do
  ufw allow from "$ip" to any port 4001 proto udp
done
ufw allow from 78.47.43.136 to any port 9090 proto tcp   # skip on A itself
```

A missing rule does not announce itself — the mesh simply forms with fewer
peers than it should, and `peers` sits at 3 instead of 4 on the affected
nodes. Step 5 is what catches it.

**3. Distribute keys and the shared config.**

```bash
# On A — copy each node's own key dir, plus the shared validator set
scp -r ops/testnet-keys/node3 root@46.62.218.24:/opt/omnia-protocol/ops/testnet-keys/
scp -r ops/testnet-keys/node4 root@46.224.103.217:/opt/omnia-protocol/ops/testnet-keys/
for h in 178.156.163.211 5.223.85.30 46.62.218.24 46.224.103.217; do
  scp docker/.env root@$h:/opt/omnia-protocol/docker/.env
done
```

Then on **every** host: `chown -R 1000:1000 ops/testnet-keys`. The container
runs as uid 1000; an unreadable key does not fail loudly — the node
silently falls back to an ephemeral identity, logs
`this_node_is_validator=false`, has every ack rejected as "unknown
validator", and finality never happens.

Append the per-node variables on D and E (`OMNIA_NODE_ID=4` / `5`,
`OMNIA_KEY_DIR=../ops/testnet-keys/node3` / `node4`, `OMNIA_TOTAL_NODES=5`,
and `OMNIA_BOOTSTRAP_NODES` listing the other four).

**4. Put every node on the same commit, then restart in order.**

New nodes get a fresh clone. The existing nodes are running whatever image
they were last built from, which may be months behind. **Rebuild them too** —
Lane 0 acks are encoded with postcard, which is not self-describing, so a
field added to `SignedAck` in the interim makes the two builds mutually
unintelligible.

**Nodes run `main`** (§1). Before deploying, confirm the change you mean to
ship has actually reached `main` — a PR merged into `dev` has not, and
rebuilding then produces the binary already running. That failure is silent:
the build succeeds, containers restart, and nothing changes. Check with
`git log origin/main --oneline -1` before touching any host.

```bash
# On EVERY host, new and old alike:
cd /opt/omnia-protocol
git fetch origin && git checkout -B main origin/main
git log -1 --format='%h %s'   # same hash on all five before building
docker compose -f docker/docker-compose.wan.yml build
```

Build everywhere *before* restarting anything — a Rust release build takes
3–7 minutes and you do not want nodes bouncing one at a time across that
window. Then:

```bash
# On A first:
docker compose -f docker/docker-compose.wan.yml up -d
# Then B, C, D, E:
docker compose -f docker/docker-compose.wan.yml up -d
```

A must go first because it is the bootstrap node, and peers do **not**
re-dial a bootstrap that restarts (#411). Restarting A after the others
would orphan it — exactly the failure that went unnoticed for four days in
July 2026.

> **Back up host A's live monitoring files outside the repo before the
> checkout.** Two of them do not survive it:
>
> - `docker/monitoring/prometheus-wan.yml` — the live copy carries the real
>   `remote_write` url and username; the committed version leaves that block
>   commented out. Silently stops shipping if overwritten.
> - `docker/monitoring/grafana-cloud-token` — **`git stash -u` takes this
>   too.** It is gitignored on current `main`, but `.gitignore` is itself
>   part of what you are upgrading, so on the old commit the rule does not
>   exist yet and the file is merely untracked. `-u` stashes untracked files;
>   only `-a` includes ignored ones. Losing it leaves a bind-mount pointing
>   at nothing, Docker creates a *directory* in its place, and Prometheus
>   fails to start with `not a directory`.
>
> ```bash
> mkdir -p /root/monitoring-backup
> cp -a docker/monitoring/prometheus-wan.yml \
>       docker/monitoring/grafana-cloud-token /root/monitoring-backup/
> ```
>
> Recover from the stash with `git checkout stash@{0}^3 -- <path>` (untracked
> files live in the stash's *third* parent, not its tree). Afterwards confirm
> `grep -c '^remote_write' docker/monitoring/prometheus-wan.yml` returns 1,
> and re-apply `chown 65534:65534` + `chmod 644` to the token before starting
> Prometheus.

**5. Verify — every node must report 4 peers.**

```bash
for ip in localhost 178.156.163.211 5.223.85.30 46.62.218.24 46.224.103.217; do
  printf "%-16s " "$ip"
  curl -s "http://$ip:9090/api/v1/node/info"     | python3 -c "import sys,json; d=json.load(sys.stdin); print('peers=', d['peers'], 'lane0_finalized=', d['lane0']['events_finalized'])"
done
```

Anything below `peers=4` is a partial mesh. Do not move on until all five
agree — a node with a different validator set will reject acks it should
accept, and the symptom (finality stalls) looks nothing like the cause.

`peers` alone is not sufficient. Also confirm every node converges on the
same ack and finality counts, because version skew produces a full peer
count and zero finality:

```bash
for ip in localhost 178.156.163.211 5.223.85.30 46.62.218.24 46.224.103.217; do
  printf "%-16s " "$ip"
  curl -s "http://$ip:9090/api/v1/node/info" | python3 -c "import sys,json; d=json.load(sys.stdin); l=d['lane0']; print('validators=', l['validator_count'], 'acks_ok=', l['acks_accepted'], 'acks_rej=', l['acks_rejected'], 'final=', l['events_finalized'])"
done
```

`validators=5` on every host is the direct check for the failure mode in the
troubleshooting table below: a host whose `OMNIA_LANE0_VALIDATORS` differs
reports a different size here, and that is visible immediately instead of
being inferred from stalled finality. `validator_count` is `null` when Lane 0
is disabled on that node — which is itself a finding on a validator host.

Note that this is **not** the same number as `count` from
`GET /api/v1/validators`. That endpoint reports the `validator_candidates`
staking registry, where only the genesis node is currently registered, so it
answers `1` on the five-node mesh. It carries `lane0_validator_count`
alongside for exactly this reason. Do not build a dashboard on `count` and
call it the validator count.

Certificates are grow-only sets of the same acks, so healthy nodes converge.
Counts that cluster into groups — 36/36/14 for one build and 24/24 for
another — mean acks are not crossing between the groups.

**`acks_rejected` staying at 0 does not clear the network.** A batch whose
wire format the receiver cannot parse fails in `decode_ack_batch`, which is
upstream of the counter: it logs `Lane 0 ack batch rejected` at WARN and
drops the message. Check the log, not the counter:

```bash
docker logs omnia-node 2>&1 | grep -ci "ack batch rejected"   # must be 0
```

**6. Update monitoring — this is easy to forget and silently degrades it.**

Add D and E to `docker/monitoring/prometheus-wan.yml` and restart
`omnia-prometheus`, then **change the alert threshold from `< 2` to `< 4`**
in Grafana Cloud.

Leaving it at `< 2` on a 5-node mesh means the alert only fires once a node
is down to 1 peer — i.e. after **three of five** nodes are already gone. The
rule keeps evaluating and reporting healthy through the exact degradation it
exists to catch. See [monitoring-setup.md](./monitoring-setup.md).

## 1. Provision B and C

Ubuntu 24.04, Docker installed:

```bash
apt-get update && apt-get install -y docker.io docker-compose-v2 git curl
git clone https://github.com/Willow7737/omnia-protocol /opt/omnia-protocol
cd /opt/omnia-protocol && git checkout main
```

**Nodes run `main`, never `dev`.** `dev` is the integration branch: changes
land there to be exercised by CI and can be red at any moment. Deploying from
it puts unvalidated code on validators. The series is
`feature branch → dev → CI green → merge to main → deploy`.

## 2. Firewall (all five hosts)

Only two ports matter. QUIC needs UDP 4001 open **between all five node
IPs**; the HTTP API/metrics port should be reachable **only from the bench
host (A)** — do not expose /metrics to the world.

```bash
# On each host — substitute the four OTHER node IPs and A's IP:
ufw allow from <peer-ip-1> to any port 4001 proto udp
ufw allow from <peer-ip-2> to any port 4001 proto udp
ufw allow from <peer-ip-3> to any port 4001 proto udp
ufw allow from <peer-ip-4> to any port 4001 proto udp
ufw allow from <bench-host-ip> to any port 9090 proto tcp
ufw allow OpenSSH && ufw enable
```

(Host A already exposes 9090/9443 publicly for the wallet/dashboard via the
sslip.io reverse proxy — leave that as is; the rule above is for B–E.)

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
NODES=5 ./scripts/setup-validators.sh          # writes keys + docker/.env

scp -r ops/testnet-keys/node1 root@<B>:/opt/omnia-protocol/ops/testnet-keys/
scp -r ops/testnet-keys/node2 root@<C>:/opt/omnia-protocol/ops/testnet-keys/
scp -r ops/testnet-keys/node3 root@<D>:/opt/omnia-protocol/ops/testnet-keys/
scp -r ops/testnet-keys/node4 root@<E>:/opt/omnia-protocol/ops/testnet-keys/
scp docker/.env root@<B>:/opt/omnia-protocol/docker/.env
scp docker/.env root@<C>:/opt/omnia-protocol/docker/.env
scp docker/.env root@<D>:/opt/omnia-protocol/docker/.env
scp docker/.env root@<E>:/opt/omnia-protocol/docker/.env
```

Then append the per-node variables to `docker/.env` **on each host**:

```bash
# On A (bootstrap — dials nobody):
cat >> docker/.env <<'EOF'
OMNIA_NODE_ID=1
OMNIA_BOOTSTRAP_NODES=
OMNIA_KEY_DIR=../ops/testnet-keys/node0
OMNIA_TOTAL_NODES=5
EOF

# On B:
cat >> docker/.env <<'EOF'
OMNIA_NODE_ID=2
OMNIA_BOOTSTRAP_NODES=/ip4/<A-public-ip>/udp/4001/quic-v1
OMNIA_KEY_DIR=../ops/testnet-keys/node1
OMNIA_TOTAL_NODES=5
EOF

# On C (same as B but OMNIA_NODE_ID=3, OMNIA_KEY_DIR=../ops/testnet-keys/node2)
# On D (same as B but OMNIA_NODE_ID=4, OMNIA_KEY_DIR=../ops/testnet-keys/node3)
# On E (same as B but OMNIA_NODE_ID=5, OMNIA_KEY_DIR=../ops/testnet-keys/node4)
```

Keys are secrets: `chmod -R go-rwx ops/testnet-keys` and remember the
containers run as **uid 1000** — `setup-validators.sh` chowns for you when
run as root, but after `scp` re-check on B–E:
`chown -R 1000:1000 ops/testnet-keys`. (An unreadable key silently degrades
the node to a non-validator identity and finality never happens — the
classic failure, see the operational note in
[benchmark-gates.md](../reference/benchmark-gates.md).)

## 4. Bring the nodes up

Start **A first** (it is the bootstrap), then B, C, D, and E:

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

# From A — every node must report (node_count - 1) peers before benchmarking.
# That is 4 on the current 5-node mesh. Fewer means a partial mesh, and Lane 0
# quorum needs 4 of 5 acks, so one absent peer is enough to halt finality.
for ip in localhost <B> <C> <D> <E>; do printf "%s peers=" "$ip"; \
  curl -s "http://$ip:9090/metrics" | awk '/^omnia_node_peers_connected /{print $2}'; done
```

## 5. Capture the RTT matrix (record it with the results)

```bash
# On A:
for ip in <B> <C> <D> <E>; do printf "A->%s  " "$ip"; ping -c 5 -q "$ip" | tail -1; done
# On B:
for ip in <C> <D> <E>; do printf "B->%s  " "$ip"; ping -c 5 -q "$ip" | tail -1; done
# On C:
for ip in <D> <E>; do printf "C->%s  " "$ip"; ping -c 5 -q "$ip" | tail -1; done
```

Record the matrix (min/avg/max) in `benchmark-gates.md` next to the run —
"5 nodes across EU-central / US-East / AP-southeast, RTTs 90/170/230 ms" is
the sentence that makes the numbers credible.

## 6. The benchmark ladder

Run from A. Climb; don't jump straight to 10k — each rung either passes
(record it) or fails in a way that tells you exactly which assumption broke:

```bash
OMNIA_JWT_SECRET=$(grep ^OMNIA_JWT_SECRET= docker/.env | cut -d= -f2-) \
  ./scripts/testnet-bench.sh \
  --nodes http://localhost:9090,http://<B>:9090,http://<C>:9090,http://<D>:9090,http://<E>:9090 \
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
| bitcoind won't start after config edit | `addnode=` outside `[testnet4]` section, or pruned blocks with `txindex=1` | Network-specific settings must be under `[testnet4]` header; `txindex=1` requires `prune=0`. If pruned, restart with `-reindex` |

## 8. Bitcoin testnet4 on Node A

Node A (nbg1) also runs a Bitcoin Core v28+ instance for settlement layer validation. The Bitcoin Settlement Adapter uses testnet4 (testnet3 was removed in Core v28/v30).

### bitcoind configuration

```bash
# Node A only — ~/.bitcoin/bitcoin.conf
rpcuser=<your-rpc-user>
rpcpassword=<your-rpc-password>
server=1
txindex=1
prune=0
maxconnections=25

[testnet4]
rpcport=48332
addnode=testnet4.nodelete.org:48333
addnode=testnet4-services.arcblaze.com:48333
addnode=seed.testnet4.bitcoin.sprovoost.nl:48333
addnode=testnet4.bitcoin.jonasschnelli.ch:48333
```

**Critical notes:**
- `txindex=1` and `prune=0` are **mandatory** — `fetch_finality` calls `gettransaction` which requires the full unpruned chain with transaction index.
- All `addnode=` entries must be inside the `[testnet4]` section. Entries outside it will be rejected and bitcoind will not start.
- If the node was previously running with pruning enabled, adding `txindex=1` requires a full reindex: `bitcoind -testnet4 -daemon -reindex`.
- Testnet4 uses port 48332 for RPC and 48333 for P2P. Data directory is `~/.bitcoin/testnet4/`.

### Verify Bitcoin settlement

```bash
# Check peer count (should be 8+)
bitcoin-cli -testnet4 -rpcuser=<user> -rpcpassword=<pass> getconnectioncount

# Check sync progress
bitcoin-cli -testnet4 -rpcuser=<user> -rpcpassword=<pass> getblockchaininfo | python3 -m json.tool | head -15

# Run finality verification (from repo root)
cd /opt/omnia-protocol
cargo run --example bitcoin_testnet4_finality_verify --features bitcoin-live
```
