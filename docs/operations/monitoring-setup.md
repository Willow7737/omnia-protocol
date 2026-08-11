# Monitoring the WAN Testnet (Prometheus → Grafana Cloud)

> Audience: Operators
> Context: Standing up metrics + alerting for the geo-distributed testnet.
> Last Updated: 2026-08-02

## Why this exists

A four-day network partition went completely unnoticed in July 2026. The
alert that would have caught it (`PeerCountDrop`, in
[`monitoring/grafana/alerts/omnia-alerts.yml`](../../monitoring/grafana/alerts/omnia-alerts.yml))
had existed for months and was correct — it was simply never evaluated
against real data:

- The Prometheus configs targeted Docker bridge hostnames on port 8080
  (`omnia-node-1:8080`), which only exist in the single-host stacks. WAN
  members run `docker-compose.wan.yml` with host networking, one container
  named `omnia-node` per host, metrics on **9090**. Prometheus was scraping
  nothing.
- Grafana had been in `Exited (1)` for two weeks, so nobody was looking.

Both halves have to work. Metrics with no alerting means nobody notices;
alerting on metrics that were never collected means the same.

## Architecture, and why Grafana runs in the cloud

```
node A (Nuremberg)  ─┐
node B (Ashburn)    ─┤
node C (Singapore)  ─┼─► Prometheus on host A ──remote_write──► Grafana Cloud
node D (Helsinki)   ─┤        (scrape :9090)                  (dashboards + alerts)
node E (Falkenstein)─┘
```

Prometheus runs on **host A** because it is the only host the other members
allow through their firewalls on 9090.

**Do not run Grafana on host A.** A Grafana there cannot alert you that host
A is down, which is the single most important thing to be told. Alert
evaluation belongs in Grafana Cloud, fed by `remote_write`, so it survives
the loss of the box being monitored.

## Setup

### 1. Grafana Cloud credentials

In your Grafana Cloud instance: **Connections → Add new connection →
Prometheus → "From my local Prometheus server" → "Send metrics from a
single Prometheus instance" → "Directly"**, then generate an access-policy
token (the `set:alloy-data-write` preset includes the required
`metrics:write` scope).

Note the **url** and **username**; the generated `password` is the token.

Prefer **no expiry** on the token. If it lapses, metrics stop and alerting
goes silent — and the thing that would have told you is the alerting. If you
do set an expiry, put a calendar reminder well before it.

### 2. On host A

```bash
cd /opt/omnia-protocol

# Token — `read -s` keeps it out of shell history and scrollback
read -s -p "token: " T && printf '%s' "$T" > docker/monitoring/grafana-cloud-token && unset T

# REQUIRED: the Prometheus image runs as `nobody` (uid 65534)
chown 65534:65534 docker/monitoring/grafana-cloud-token
chmod 644 docker/monitoring/grafana-cloud-token
```

Fill in the `remote_write` url and username in
`docker/monitoring/prometheus-wan.yml` (keep `password_file` — do not inline
the token), then:

```bash
docker compose -f docker/docker-compose.monitoring.yml up -d
```

### 3. Verify — both halves, separately

Scraping:

```bash
curl -s localhost:9095/api/v1/targets \
  | python3 -c "import sys,json; [print(t['labels'].get('node','?'), t['scrapeUrl'], t['health']) for t in json.load(sys.stdin)['data']['activeTargets']]"
```

Expect one line per node, all `up`.

Shipping (this is the step people skip):

```bash
curl -s localhost:9095/metrics | grep -v '^#' \
  | grep -E 'remote_storage_(bytes_total|samples_failed_total|samples_pending)'
```

- `bytes_total` **> 0 and climbing** — data is actually reaching Grafana Cloud
- `samples_failed_total` **0**
- `samples_pending` **small / falling** — a large growing value means the
  queue is backing up and nothing is being delivered

Then query `omnia_node_peers_connected` in Grafana Cloud → Explore. One series
per node — five on the current mesh.

### 4. Alert rules

**Alerting → Alert rules → + New alert rule**, using the `grafanacloud-…-prom`
data source:

| Alert | Query (Code mode) | Condition | Pending |
|---|---|---|---|
| Node lost peers | `max_over_time(omnia_node_peers_connected[10m])` | `IS BELOW <n-1>` | 5m |
| Finality stalled | `rate(omnia_node_events_finalized_total[5m])` | `IS BELOW 0.001` | 10m |

**The peer threshold is `node_count - 1`** — in a healthy mesh every node
sees every other. It is `2` for the 3-node topology and `4` for 5 nodes.
**Update it whenever the validator set changes.** A stale `< 2` on a 5-node
mesh does not error or look broken; it simply keeps reporting healthy until
three of five nodes are gone.

**Use `max_over_time`, not the raw gauge.** `omnia_node_peers_connected`
flaps: peer-level connectivity is derived from connection-level libp2p events
without consulting `num_established`, so closing one of several connections
to a peer drops it from the count until the next connection is established
(#422). Measured on the live 5-node mesh, a node read 3 for ~64 seconds and
recovered untouched. A raw `< 4` pages on every one of those. `max_over_time`
asks whether the node reached full connectivity *anywhere* in the window — a
transient dip does, a genuinely dead link never does. Detection costs one
window; false positives go to zero.

Note the gauge can also over-report. A half-open connection is counted by the
side that did not observe the close, so a broken link may show as `4` on one
end and `3` on the other. The alert catches it from the dropping side only.

The pending period must be **>= the evaluation group's interval** — the
interval belongs to the group, not the rule. A 1m group interval with a 3m
pending period gives detection in ~3-4 minutes; leaving the group at 3m
pushes worst-case notification out to ~6 minutes.

Under **Configure no data and error handling**, set **no data → Alerting**.
The `< 2` condition catches a *degraded* node, but a node whose host dies
stops producing the series entirely — there is no value left to compare, so
without this the rule lands in "No Data" rather than firing. Leave the
execution-error state as **Error**: it still notifies, but keeps a Grafana
or datasource fault distinguishable from a real node fault. If you conflate
them you will learn to distrust the alert.

Attach a contact point (**Alerting → Contact points**) — an alert with no
contact point notifies nobody. Send a test through it before relying on it.

**Test it for real.** Stop `omnia-node` on host B and *wait past the pending
period* (4+ minutes for a 3m rule) before starting it again. Stopping and
starting in one go proves nothing. An untested alert is a hope, not a
control.

**Check your spam folder on the first fire.** Verified 2026-08-01: the alert
fired correctly and delivered in 30s — straight into Gmail's spam folder. An
alert nobody sees is not much better than no alert. Whitelist the sender
(Gmail: open the message -> Filter messages like these -> Create filter ->
"Never send it to Spam"), and consider a second contact point that does not
depend on email deliverability at all — Grafana Cloud supports Discord,
Slack, Telegram, and generic webhooks.

### Verified behaviour (2026-08-01)

Stopping `omnia-node` on host B produced **two** firing instances, one each
for A and C — the per-node labels split them, which is correct and confirms
the labelling works. Each notification carried the templated summary
("Omnia node C (sin-ap-southeast) has 1 peer(s), expected 4"), the
diagnosis, and the runbook link.

Note that a dead node is reported *by its neighbours*, not by itself: its
own series disappears and that alert instance goes stale after
`missing series evaluations` intervals, while the surviving nodes each drop
below 2 peers and fire. Total loss of scraping (host A or Prometheus down)
is what the no-data → Alerting setting covers.

## Gotchas that cost real time

**The token permission trap.** If the token file is not readable by uid
65534, Prometheus logs

```
unable to read basic auth password file /etc/prometheus/grafana-cloud-token: permission denied
```

at **WARN** level and retries forever. The container stays up, scraping
keeps working, `/metrics` looks healthy, and nothing ever reaches Grafana
Cloud. Nothing surfaces as an error. `bytes_total: 0` is the tell.

**`host.docker.internal` is unreliable.** With `extra_hosts: host-gateway`
it resolves on some Docker/Ubuntu combinations and silently yields no target
on others. Use host A's explicit IP instead.

**Prometheus publishes on 9095, not 9090.** The Omnia node already owns
9090 on the host.

**Stale targets linger for ~5 minutes.** After changing a target and
restarting, the old series still answers instant queries until the lookback
window passes. Check `/api/v1/targets` for what is actually configured
rather than inferring from an `up` query.

**Don't copy Grafana's generated `scrape_configs`.** The snippet it produces
includes its own `localhost:9090` job, which inside the container scrapes
Prometheus itself. Take only the `remote_write` block.

## Related

- #411 — peers never re-dial a restarted bootstrap node (the partition this
  monitoring is meant to catch)
- #412 — restart ordering and peer-health checks in the runbooks
