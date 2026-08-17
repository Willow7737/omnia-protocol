# Financial Layer Rollout — Runbook

> Audience: Operators
> Context: Promoting `dev` → `main` (PR #469) and rolling the resulting build
> across the standing 5-node geo mesh.
> Companion to [geo-testnet.md](./geo-testnet.md), which covers standing up a
> node from nothing. This covers upgrading nodes that are already running.

This release is different from previous ones in two ways, and both change the
procedure:

1. It ships the financial layer — asset registry, treasury, supply
   accounting, fee burn. Those subsystems are inert unless
   `OMNIA_MINT_AUTHORITY` is set, and the endpoints that expose them are
   **unauthenticated**.
2. The mesh is five nodes now, not three. Lane 0 finality needs 4-of-5 acks,
   so one node can be down without stalling the network. This is the first
   release that can be rolled node-by-node instead of coordinated.

## 0. Pre-flight

Everything here is verifiable before you touch a host. Do not skip to §2.

### 0.1 Wire protocol is unchanged

```bash
grep 'pub const PROTOCOL_VERSION' substrate/src/lib.rs   # expect "4.0.0"
curl -s https://78.47.43.136.sslip.io/api/v1/node/info | grep protocol_version
```

Both must read `4.0.0`. Old and new nodes peer across the roll only because
this is unchanged — if a future release bumps it, the node-by-node procedure
in §2 is invalid and every node must restart together.

### 0.2 The version must actually move

Merging #469 does **not** bump the version. `release-please` runs on push to
`main` and opens a *Release PR*; the bump to `Cargo.toml` and the `v…` tag
land only when that second PR is merged.

Merge #469, wait for the Release PR, merge it too, then confirm:

```bash
git fetch --tags origin && git tag --sort=-v:refname | head -1   # expect > v0.1.95
git show origin/main:Cargo.toml | grep -m1 '^version'
```

If you build from `main` after #469 but before the Release PR, every node
reports `0.1.95` — the version they already report. You will have no way to
tell an upgraded node from a stale one, on any of the five hosts. That is the
whole reason this step is a gate.

### 0.3 Decide the mint authority

`OMNIA_MINT_AUTHORITY` is an Ed25519 public key (64 hex chars) and is a
**shared genesis parameter** — byte-identical on all five nodes, like
`OMNIA_LANE0_VALIDATORS`. It is not the node's own key.

Unset does not fall back to anything. Minting is disabled, and
`/api/v1/supply` and `/api/v1/treasury/*` answer zeros. Those routes serve
without a JWT, so on the public ingress an unset value renders as a complete,
working-looking, entirely zero economy — indistinguishable to a visitor from
a broken deploy.

Set it deliberately, in `docker/.env` on **every** host:

```bash
grep OMNIA_MINT_AUTHORITY docker/.env      # same 64 hex chars on all five
```

Or decide minting stays off for this release and leave it empty everywhere —
that is a legitimate choice, but make it a choice.

> Until the fix in this branch, `docker-compose.wan.yml` did not pass
> `OMNIA_MINT_AUTHORITY` into the container at all. Setting it on the host
> had no effect. Confirm you are deploying a compose file that forwards it:
> `grep MINT_AUTHORITY docker/docker-compose.wan.yml`.

## 1. Roll order

| Order | Node | Host | Why this position |
|:-----:|:-----|:-----|:------------------|
| 1 | **E** | `46.224.103.217` Falkenstein | Plain validator, newest, least load-bearing |
| 2 | **D** | `46.62.218.24` Helsinki | Plain validator |
| 3 | **C** | `5.223.85.30` Singapore | Plain validator, highest RTT |
| 4 | **B** | `178.156.163.211` Ashburn | Plain validator |
| 5 | **A** | `78.47.43.136` Nuremberg | **Last.** Bootstrap + public ingress + bench host + Bitcoin testnet4 |

One node at a time. With 4-of-5 quorum the mesh keeps finalising throughout;
take two down at once and it stalls.

Node A goes last because it is the bootstrap peer, the only public HTTP
surface, and the dashboard's endpoint. Restarting it is the only step in this
list a user can see.

## 2. Per-node procedure

On each host, in the order above:

```bash
cd ~/omnia-protocol
git fetch origin main && git checkout main && git pull --ff-only origin main

# confirm you are on the tagged release, not just main's tip
git describe --tags --exact-match 2>/dev/null || echo "WARNING: not on a tag"

docker compose -f docker/docker-compose.wan.yml up -d --build
```

Then verify **before moving to the next node**:

```bash
curl -s http://localhost:9090/api/v1/node/info | python3 -m json.tool
```

Check all four:

- `version` — the new version from §0.2, not `0.1.95`
- `protocol_version` — `4.0.0`
- `peers` — climbs back to `4`; give it ~30 s
- `/health` — `alive`

If `peers` does not return to 4 within a couple of minutes, **stop**. Do not
roll the next node. See §4.

## 3. Post-deploy verification

From anywhere, against the public ingress — no token, which is the point:

```bash
curl -s https://78.47.43.136.sslip.io/api/v1/supply
curl -s https://78.47.43.136.sslip.io/api/v1/treasury/status
curl -s https://78.47.43.136.sslip.io/api/v1/fees/burn-policy
curl -s https://78.47.43.136.sslip.io/api/v1/bridge/health
```

All four must answer without a JWT. A `401` means the routes regressed behind
the auth layer; a `404` means node A is still on the old build.

Then confirm the per-account routes did **not** open up:

```bash
curl -s -o /dev/null -w '%{http_code}\n' \
  https://78.47.43.136.sslip.io/api/v1/financial/balance/deadbeef   # expect 401
```

### The real smoke test

The mesh has been idle since the v0.1.95 rollout — Lane 0 counters and
`events_submitted_total` all read zero. Zero counters after this deploy prove
nothing, because they read zero before it too.

Submit one authenticated event and watch `acks_accepted` move on more than
one node. That is the first evidence the release actually works, as opposed
to merely starting.

## 4. If a node fails to rejoin

The mesh tolerates exactly one node down. A failed upgrade is not an
emergency — it is a budget you have now spent.

1. **Do not roll another node.**
2. Check logs: `docker compose -f docker/docker-compose.wan.yml logs --tail=100`
3. Most likely causes, in order:
   - `OMNIA_LANE0_VALIDATORS` mismatch — acks rejected as "unknown validator"
   - `OMNIA_MINT_AUTHORITY` set on some hosts but not others
   - `OMNIA_JWT_SECRET` drift between hosts
   - Key dir not mounted (`OMNIA_KEY_DIR`)
4. To roll back one node, check out the previous tag and rebuild:

```bash
git checkout v0.1.95
docker compose -f docker/docker-compose.wan.yml up -d --build
```

Rolling back a single node is safe precisely because the wire protocol is
unchanged (§0.1) — a `v0.1.95` node peers with the new ones.

## 5. After the rollout

The public docs describe the mesh as idle, with zero Lane 0 counters, because
that is what it has been. If this deploy is followed by real traffic, those
claims go stale in the good direction and should be updated:

- `README.md` — the live-network block and the standing-network bullet
- `omnia-protocol-interface/README.md` — the "what the dashboard will show" block
- `omnia-web` — FAQ and roadmap entries
- `Omnia-Wallet/README.md` — the testnet description

Each one currently says the counters read zero. Do not leave that in place
once it stops being true; the reason it is written down at all is that the
previous claim — "Lane 0 finalizing events" — outlived its accuracy and sat
there next to an endpoint anyone could curl.
