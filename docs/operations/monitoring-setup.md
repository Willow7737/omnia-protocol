# Monitoring the WAN Testnet (Prometheus → Grafana Cloud)

> Audience: Operators
> Context: Standing up metrics + alerting for the geo-distributed testnet.
> Last Updated: 2026-08-01

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
node A (Nuremberg) ─┐
node B (Ashburn)   ─┼─► Prometheus on host A ──remote_write──► Grafana Cloud
node C (Singapore) ─┘        (scrape :9090)                  (dashboards + alerts)
```

Prometheus runs on **host A** because it is the only host permitted through
B's and C's firewalls on 9090.

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

Then query `omnia_node_peers_connected` in Grafana Cloud → Explore. Three
series, one per node.

### 4. Alert rules

**Alerting → Alert rules → + New alert rule**, using the `grafanacloud-…-prom`
data source:

| Alert | Query (Code mode) | Condition | Pending |
|---|---|---|---|
| Node lost peers | `omnia_node_peers_connected` | `IS BELOW 2` | 3m |
| Finality stalled | `rate(omnia_node_events_finalized_total[5m])` | `IS BELOW 0.001` | 10m |

Attach a contact point (**Alerting → Contact points**) — an alert with no
contact point notifies nobody.

**Test it for real.** Stop `omnia-node` on host B and *wait past the pending
period* (4+ minutes for a 3m rule) before starting it again. Stopping and
starting in one go proves nothing. An untested alert is a hope, not a
control.

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
