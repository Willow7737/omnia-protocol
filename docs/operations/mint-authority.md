# Setting the Mint Authority — Runbook

> Audience: Operators
> Scope: Choosing, distributing and verifying `OMNIA_MINT_AUTHORITY` across the
> standing 5-node mesh.
> Expands [financial-layer-rollout.md](./financial-layer-rollout.md) §0.3, which
> says to decide this and does not say how.

## What you are actually deciding

`OMNIA_MINT_AUTHORITY` is a 64-hex-character Ed25519 **public** key. Minting is
authorised in `shards/src/financial/state.rs` by comparing the *event creator's*
public key against it:

```rust
FinancialOp::Mint { to, amount } => {
    let minter = event.creator_pubkey;
    match self.mint_authority {
        Some(auth) if auth == minter => {}
        Some(_) => return Err(/* not the mint authority */),
        None    => return Err(/* minting is disabled */),
    }
    // …increments the balance and total_supply
}
```

`event.creator_pubkey` is the raw 32-byte Ed25519 key that signed the event
(`omnia-primitives/src/event.rs`), so the comparison is coherent: the holder of
the matching **private** key can mint. The only ceiling on that path is
`total_supply.checked_add` — arithmetic overflow. The treasury bucket caps in
`/api/v1/treasury/status` constrain treasury *allocation*, not this.

Two consequences follow:

1. **The private key is a supply-control key.** Treat it like one.
2. **The public key is a shared genesis parameter.** Byte-identical on all five
   hosts, like `OMNIA_LANE0_VALIDATORS`. Hosts that disagree disagree about who
   may create supply, and per
   [financial-layer-rollout.md](./financial-layer-rollout.md) §4 a partial set
   is a known cause of a node failing to rejoin.

Leaving it unset is a legitimate choice — minting is disabled, transfers still
work, `/api/v1/supply` answers zeros. It is a *choice*, not a default to drift
into: those routes serve unauthenticated, so on the public ingress an unset
value reads as a complete, working-looking, entirely zero economy.

## 0. Which keys can actually mint today

**Read this before generating anything.** It constrains the choice more than the
security argument does.

Minting requires a committed event whose `creator_pubkey` is the configured
authority. Today there is exactly one way to get an event onto the chain through
the API, and it does not let you choose the signing key:

| Path | What it does |
|:--|:--|
| `POST /api/v1/events` | Signs with **this node's own persistent keypair** (`node/src/api/events.rs`, `state.keypair`). The request body carries `payload` and `event_type` only — there is no field for a caller-supplied signature. |
| `POST /api/v1/shards/financial/operations` | Returns **501 Not Implemented** — `handle_generic_shard_op` in `node/src/api/shards.rs`. The financial shard has no operations endpoint. |
| `POST /api/v1/shards/economics/operations` | Real, but its `mint` is `EconomicsOp::MintUbc` on the *economics* shard — different shard, different state — and the code notes it applies to local state only. |

So:

- **The validator key of a node you can submit through** can mint. Send a
  `FinancialOp::Mint` payload to that node's `/api/v1/events`; the node signs as
  itself, `creator_pubkey` matches, the mint is authorised.
- **A fresh, dedicated, offline key cannot mint** — not because it is refused,
  but because no API path will ever sign an event with it. Configuring one
  enables a check that nothing can satisfy: minting stays impossible, and the
  node logs `Financial shard mint authority configured` exactly as if it had
  worked.

A dedicated offline authority is the right destination — it is what
`docs/governance/treasury-multisig-policy.md` §2 asks for, and `node/src/main.rs`
warns when the authority equals the node's own key for good reason: whoever
compromises that host gets the money. But it needs an endpoint that accepts an
externally-signed `FinancialOp::Mint`, and that endpoint does not exist yet.
Until it does, the honest options are:

1. **Leave it unset** and say so publicly. Minting is off, which is what an
   unfunded testnet actually is.
2. **Use the bootstrap node's validator key**, accepting that node A's key is
   now also the mint key, and that node A is the public ingress. Workable for a
   testnet; not a mainnet posture.

Choose 1 or 2 deliberately. Do not configure a fresh offline key expecting it to
work — it will look configured and mint nothing.

## 1. Generate the keypair

Only needed for a dedicated authority, which cannot mint yet — see §0. For
option 2 the key already exists: it is the node's `validator_pubkey`, readable
from `/api/v1/node/info`.

Generate **off the validator hosts**, on a machine you control.

The passphrase goes through the environment, never the command line.
`--passphrase` places it in `argv`, which any local user can read from `ps` or
`/proc/<pid>/cmdline`; the CLI accepts `OMNIA_KEYGEN_PASSPHRASE` for exactly
this reason (`node/src/config.rs`). A `VAR=value command` prefix puts the value
in the child's environment, not in anyone's `argv`:

```bash
mkdir -p ~/omnia-mint && chmod 700 ~/omnia-mint

IFS= read -rsp 'keygen passphrase: ' MINT_PASS && echo
[ -n "$MINT_PASS" ] || { echo "empty passphrase — refusing" >&2; return 1 2>/dev/null || exit 1; }
OMNIA_KEYGEN_PASSPHRASE="$MINT_PASS" \
  cargo run -p omnia-node -- keygen --output-dir ~/omnia-mint
unset MINT_PASS
```

The empty check is load-bearing. clap treats an environment variable set to the
empty string as *provided*, not absent, so `OMNIA_KEYGEN_PASSPHRASE=` yields
`Some("")` and `run_keygen` takes the **encrypted** branch with an empty
passphrase. You get a `validator_key.enc` protected by nothing, and the warning
box that a plaintext `validator_key.bin` would have printed never appears.

Or from the published image. Note the tag path — `<owner>/<repo>/omnia-node`,
because `release.yml` builds it from the full repository slug — and the `--user`,
because the image runs as `USER omnia` and could not otherwise write to a
directory your account owns. `-e VAR` with no `=` forwards the value from your
environment without placing it in the command line:

```bash
IFS= read -rsp 'keygen passphrase: ' OMNIA_KEYGEN_PASSPHRASE && echo
[ -n "$OMNIA_KEYGEN_PASSPHRASE" ] || { echo "empty passphrase — refusing" >&2; exit 1; }
export OMNIA_KEYGEN_PASSPHRASE
docker run --rm -i --user "$(id -u):$(id -g)" \
  -e OMNIA_KEYGEN_PASSPHRASE -v ~/omnia-mint:/out \
  ghcr.io/willow7737/omnia-protocol/omnia-node:latest \
  keygen --output-dir /out
unset OMNIA_KEYGEN_PASSPHRASE
```

It writes two files and prints the public key:

| File | Contents |
|:--|:--|
| `validator_pubkey.txt` | the 64 hex characters — this is what goes in `.env` |
| `validator_key.enc` | the private key, AES-256-GCM, mode `0600` |

Without a passphrase the private key is written as `validator_key.bin`,
unencrypted, and the command says so in a box. Do not use that here.

> The output filenames are `validator_*`, the same prefix the per-node validator
> keys use. Generate into a scratch directory as above — never into a node's
> `OMNIA_KEY_DIR` — so there is no chance of confusing this key with a node
> identity later. (The node's own key is `node_key.bin`, so nothing is
> overwritten; the risk is to your ability to tell them apart.)

Then move `validator_key.enc` to wherever you keep supply-control material —
not a validator host, not the deployment repository, not `docker/.env` — and
keep the passphrase somewhere else again. A passphrase stored beside the key it
protects is an unencrypted key with extra steps.

## 2. Distribute the public key

The value must be identical on all five hosts *before* any host restarts. A host
restarted with a new value while others still hold the old one is the split in
§4 of the rollout runbook.

Either drive it from the validator console — Fleet → **Shared configuration**,
which writes to every host and reads each file back to confirm before you
restart anything — or by hand as below.

### 2.1 Establish the deployment path once

This repository disagrees with itself about where the checkout lives:
`geo-testnet.md` says `/opt/omnia-protocol` (eighteen times), and so does the
console's fact gatherer, while `financial-layer-rollout.md` §2 says
`~/omnia-protocol`. As root those are different directories, and editing `.env`
in the wrong one changes nothing while appearing to succeed. Resolve it once, on
each host, and use `DEPLOY_DIR` everywhere afterwards:

```bash
for d in /opt/omnia-protocol "$HOME/omnia-protocol"; do
  [ -f "$d/docker/.env" ] && DEPLOY_DIR="$d" && break
done
[ -n "${DEPLOY_DIR:-}" ] || { echo "no deployment checkout found" >&2; exit 1; }
echo "using $DEPLOY_DIR"
```

### 2.2 Set the value

`PUBKEY` must be assigned and checked before the file is touched. Left unset it
expands to nothing, silently clears the authority, and then compares equal
everywhere — the agreement check in §3 would report five-way agreement on an
empty value:

```bash
IFS= read -rp 'mint authority public key (64 hex): ' PUBKEY
PUBKEY="${PUBKEY#0x}"
printf '%s' "$PUBKEY" | grep -Eq '^[0-9a-fA-F]{64}$' \
  || { echo "not 64 hex characters — refusing to write" >&2; exit 1; }
```

Back the file up **outside the checkout**. `docker/.env` also holds
`OMNIA_JWT_SECRET`, so a `.bak` beside it is a second copy of the fleet's shared
secret sitting in a git working tree, where `git stash -u` and stray `rsync`
calls can reach it:

```bash
install -d -m 0700 /root/omnia-env-backups
install -m 0600 "$DEPLOY_DIR/docker/.env" "/root/omnia-env-backups/env.$(date +%s)"
```

Then rewrite. Dropping every existing line and appending one collapses
duplicates rather than adding to them, and is idempotent — running it twice
leaves exactly one assignment. A plain `sed -i s///` rewrites each duplicate in
place and leaves the count unchanged, which then fails the check below with no
repair step:

```bash
set -euo pipefail
cd "$DEPLOY_DIR"
umask 077
tmp=$(mktemp docker/.env.tmp.XXXXXX)
trap 'rm -f "$tmp"' EXIT

awk '!/^OMNIA_MINT_AUTHORITY=/' docker/.env > "$tmp"
printf 'OMNIA_MINT_AUTHORITY=%s\n' "$PUBKEY" >> "$tmp"
chmod --reference=docker/.env "$tmp" 2>/dev/null || chmod 600 "$tmp"

# Validate before the file is replaced, not after.
count=$(awk '/^OMNIA_MINT_AUTHORITY=/{n++} END {print n+0}' "$tmp")
[ "$count" -eq 1 ] || { echo "expected exactly one assignment, got $count" >&2; exit 1; }
grep -q '^OMNIA_JWT_SECRET=' "$tmp" || { echo "JWT secret missing from rewrite" >&2; exit 1; }

mv "$tmp" docker/.env
trap - EXIT
```

Three details matter here.

`awk`, not `grep -v`: under `set -e`, `grep` exits 1 when it matches nothing, so
a `.env` containing only this one key would abort the script mid-rewrite.

The check runs against the temporary file **before** `mv`, and aborts rather
than printing. The earlier version replaced `.env` first and then printed a
count nobody was required to read — so a partial read (`grep` failing on an
unreadable file) installed a truncated `.env` over a good one, taking
`OMNIA_JWT_SECRET` with it. The `trap` removes the temporary file on any exit
path.

The `OMNIA_JWT_SECRET` assertion is a cheap canary for exactly that truncation.

A duplicated key is not a syntax error and does not warn: compose reads the last
occurrence, so a stray earlier line is silently ignored on one host and silently
authoritative on another if the order differs.

## 3. Confirm agreement before restarting anything

The loop must fail closed. An `ssh` that fails yields an empty value, and five
empty values group into a single line with a count of five — reporting perfect
agreement on nothing read at all. A host counts only on a successful exit status
*and* a well-formed key:

```bash
set -uo pipefail
HOSTS="78.47.43.136 178.156.163.211 5.223.85.30 46.62.218.24 46.224.103.217"
REMOTE='d=/opt/omnia-protocol; [ -f "$d/docker/.env" ] || d=$HOME/omnia-protocol;
        grep "^OMNIA_MINT_AUTHORITY=" "$d/docker/.env" | tail -1 | cut -d= -f2-'

# The key you intended to distribute — not merely whatever the hosts agree on.
EXPECT=$(printf '%s' "${PUBKEY:?set PUBKEY to the intended authority}" \
         | tr 'A-F' 'a-f')

ok=0
for h in $HOSTS; do
  if ! v=$(ssh -o BatchMode=yes root@"$h" "$REMOTE" 2>/dev/null); then
    echo "FAIL $h — could not read"; continue
  fi
  v=$(printf '%s' "$v" | tr -d '[:space:]' | tr 'A-F' 'a-f')
  if ! printf '%s' "$v" | grep -Eq '^[0-9a-f]{64}$'; then
    echo "FAIL $h — unset or malformed: '$v'"; continue
  fi
  if [ "$v" != "$EXPECT" ]; then
    echo "FAIL $h — holds a different key: $v"; continue
  fi
  echo "ok   $h"; ok=$((ok+1))
done

if [ "$ok" -eq 5 ]; then
  echo "AGREED on all 5, and on the intended key"
else
  echo "NOT AGREED — do not restart" >&2
  exit 1
fi
```

Two things this does that the obvious version does not.

**It compares against `PUBKEY`, not just against itself.** Five hosts agreeing
is not the same as five hosts holding the key you meant to distribute — if §2
silently edited the wrong checkout (see 2.1), all five still agree, on the *old*
value, and a self-comparison reports success. The expected key is the input, not
the majority.

**It exits nonzero when it fails.** Printing `NOT AGREED` and returning 0 means
a wrapper script, or a `&&`-chained paste, proceeds to the restart anyway.

Hex case is normalised on both sides, so a host written `0xAB…` and one written
`ab…` are not reported as a divergence they are not.

Doing the distribution and the restart as separate steps is the entire point: a
fleet that holds a new value but has not restarted is simply a fleet running the
old value, which is a safe place to stand.

## 4. Restart, in order

Same order as every other roll: **E, D, C, B, A** — bootstrap last, because it
seeds the mesh and peers do not re-dial a bootstrap that restarts. One node at a
time; 4-of-5 quorum keeps finality alive throughout, and taking two down at once
stalls it.

```bash
cd "$DEPLOY_DIR"
docker compose -f docker/docker-compose.wan.yml up -d
```

No `--build` is needed — nothing about the code changed, only the environment.
Confirm the node came back before moving on:

```bash
curl -s http://localhost:9090/api/v1/node/info | python3 -m json.tool
```

`peers` must climb back to 4. If it does not within a couple of minutes, stop
and see [financial-layer-rollout.md](./financial-layer-rollout.md) §4.

## 5. Verify it took effect

**The node logged the authority it loaded.** `node/src/main.rs` logs at startup
in every case:

```bash
docker compose -f docker/docker-compose.wan.yml logs --tail=200 \
  | grep -i 'mint authority\|minting on the financial shard'
```

Expect `Financial shard mint authority configured` with your key. If you see
`No mint_authority configured … minting on the financial shard is DISABLED`, the
variable did not reach the container — check that the compose file forwards it
(`grep MINT_AUTHORITY docker/docker-compose.wan.yml`, expect the
`${OMNIA_MINT_AUTHORITY:-}` line).

**That log line is not proof that minting works.** It reports what was parsed
out of the environment, nothing more, and it appears identically for an
authority that no reachable key can sign for.

### Actually minting

Possible only under option 2 in §0 — the authority is the validator key of the
node you submit through. The payload is a postcard-serialised `ShardPayload`
(`shards/src/payload.rs`) carrying `ShardOp::Financial(FinancialOp::Mint)`,
hex-encoded, sent to that node's `/api/v1/events`, which signs it with the
node's own key. `route_event` decodes the payload and dispatches on the variant,
so the `shard_id` field and the enum variant must agree.

Two inputs have no API to look them up, so decide them before you start:

* **`nonce`** — `route_event` keeps `last_nonces` per `creator_pubkey`, defaults
  it to 0, and requires `last_nonce < nonce <= last_nonce + NONCE_GAP_LIMIT`.
  Nothing exposes the current value: `/api/v1/financial/balance/*` reports the
  *transfer* nonce, which is a different counter, and substituting it will be
  rejected as a replay. On a node whose key has submitted no shard payloads,
  start at `1` and increment; otherwise raise it until the submission stops
  returning `Replay detected`, which names the last nonce it saw.
* **`to`** — the recipient `AccountId`, and **`amount`**, in base units.

Build the hex with the workspace's own types rather than by hand. The encoding
is postcard, which is not self-describing, so a hand-rolled byte string decodes
as something else rather than failing:

```rust
// A sketch, not a runnable binary: fill in to/amount/nonce and build it inside
// the workspace so the types come from shards/src/payload.rs.
let payload = ShardPayload {
    shard_id: ShardId::financial(),
    operation: ShardOp::Financial(FinancialOp::Mint { to, amount }),
    nonce,
};
println!("{}", hex::encode(payload.to_bytes()?));
```

Submit it. The token goes in a file, not on the command line — `-H @file` is
curl's documented form, and it keeps the bearer token out of `argv` where `ps`
would show it, exactly as the passphrase does in §1:

```bash
set -euo pipefail
: "${HEX:?set HEX to the payload printed above}"
printf '%s' "$HEX" | grep -Eq '^[0-9a-fA-F]+$' || { echo "HEX is not hex" >&2; exit 1; }

umask 077
hdr=$(mktemp); trap 'rm -f "$hdr"' EXIT
IFS= read -rsp 'API token: ' TOKEN && echo
[ -n "$TOKEN" ] || { echo "empty token" >&2; exit 1; }
printf 'Authorization: Bearer %s\n' "$TOKEN" > "$hdr"
unset TOKEN

code=$(curl --show-error --silent -o /tmp/mint.out -w '%{http_code}' \
  -X POST http://localhost:9090/api/v1/events \
  -H @"$hdr" -H 'content-type: application/json' \
  -d "{\"payload\":\"$HEX\",\"event_type\":\"generic\"}")

cat /tmp/mint.out; echo
[ "$code" = "201" ] || { echo "submission failed: HTTP $code" >&2; exit 1; }
```

`201 Created` answers with:

```json
{"event_id": "<64 hex characters>", "status": "submitted"}
```

`400` means the hex payload would not parse, `413` that it exceeded
`MAX_PAYLOAD_SIZE`, `500` that the node has no persistent keypair configured.
The explicit `%{http_code}` check is used in preference to `--fail-with-body`,
which needs curl 7.76.0 or newer and is not on every host.

**`201` is not evidence that anything was minted.** It says the event was signed
and submitted. The authority check runs later, when the committed event is
applied to the financial shard — a mint the authority does not cover is rejected
there, well after the API has answered `submitted`, and the rejection appears in
the node log rather than in the HTTP response. Supply is the only thing that
distinguishes the two:

```bash
curl -s https://78.47.43.136.sslip.io/api/v1/supply
```

Until that number changes, all you have verified is that a node started. A wrong
key — or a dedicated offline key, per §0 — produces a node that starts perfectly
and rejects every mint with `Unauthorized: caller is not the mint authority`,
which from outside looks exactly like a network nobody has minted on yet.

> **Known gap.** No endpoint accepts an externally-signed `FinancialOp::Mint`,
> which is what forces the choice in §0. Adding one — a financial-shard
> operations route taking a caller-supplied `creator_pubkey` and signature,
> verified with `verify_strict`, in the shape `node/src/api/economics.rs`
> already uses for signed transfers — is the change that would make a dedicated
> offline mint authority usable. It should land before mainnet.

## 6. Afterwards

`README.md` and the dashboards describe the economy as zero because it has been.
Once supply is non-zero that stops being true, and the reason it is written down
at all is that the previous claim outlived its accuracy while sitting next to an
endpoint anyone could curl. See
[financial-layer-rollout.md](./financial-layer-rollout.md) §5 for the file list.
