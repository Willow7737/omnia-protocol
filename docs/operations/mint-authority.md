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

So the holder of the matching **private** key can mint. The only ceiling on that
path is `total_supply.checked_add` — arithmetic overflow. The treasury bucket
caps in `/api/v1/treasury/status` constrain treasury allocation, not this.

Two consequences follow, and both are the reason this document exists:

1. **The private key is a supply-control key.** Treat it like one. It should not
   live on a validator host, and it should not be the same key a node uses for
   its own identity — see `node/src/main.rs`, which logs a warning when it
   detects exactly that.
2. **The public key is a shared genesis parameter.** Byte-identical on all five
   hosts, like `OMNIA_LANE0_VALIDATORS`. Hosts that disagree disagree about who
   may create supply, and per
   [financial-layer-rollout.md](./financial-layer-rollout.md) §4 a partial set
   is a known cause of a node failing to rejoin.

Leaving it unset is a legitimate choice — minting is disabled, transfers still
work, `/api/v1/supply` answers zeros. It is a *choice*, not a default to drift
into: those routes serve unauthenticated, so on the public ingress an unset
value reads as a complete, working-looking, entirely zero economy.

## 1. Generate the keypair

Generate **off the validator hosts**, on a machine you control. Any host with
the repo and a Rust toolchain will do:

```bash
mkdir -p ~/omnia-mint && chmod 700 ~/omnia-mint
cargo run -p omnia-node -- keygen \
  --output-dir ~/omnia-mint \
  --passphrase "$(read -rsp 'passphrase: ' p && echo "$p")"
```

Or, without a toolchain, from the published image. Note the path — it is
`<owner>/<repo>/omnia-node`, because `release.yml` builds the tag from the full
repository slug — and the `--user`, because the image runs as `USER omnia` and
would otherwise be unable to write to a directory your account owns:

```bash
docker run --rm -it --user "$(id -u):$(id -g)" -v ~/omnia-mint:/out \
  ghcr.io/willow7737/omnia-protocol/omnia-node:latest \
  keygen --output-dir /out --passphrase 'CHOOSE-A-REAL-ONE'
```

It writes two files and prints the public key:

| File | Contents |
|:--|:--|
| `validator_pubkey.txt` | the 64 hex characters — this is what goes in `.env` |
| `validator_key.enc` | the private key, AES-256-GCM, mode `0600` |

Without `--passphrase` the private key is written as `validator_key.bin`,
unencrypted, and the command says so in a box. Do not use that here.

> The output filenames are `validator_*`, the same prefix the per-node validator
> keys use. Generate into a scratch directory as above — never into a node's
> `OMNIA_KEY_DIR` — so there is no chance of confusing this key with a node
> identity later. (The node's own key is `node_key.bin`, so nothing is
> overwritten; the risk is to your ability to tell them apart, which matters
> more for this key than for any other.)

Then:

- Move `validator_key.enc` to wherever you keep supply-control material. Not a
  validator host, not the deployment repo, not `docker/.env`.
- Keep the passphrase separately from the file. A passphrase stored beside the
  key it protects is an unencrypted key with extra steps.
- Record the public key somewhere you can compare against later. §4 depends on
  having something to compare *to*.

`docs/governance/treasury-multisig-policy.md` §2 separates issuance authority
from node operation and §4 requires 3-of-5 multisig with hardware wallets for
treasury keys. A single Ed25519 key is not that, and the protocol's
`mint_authority` field cannot express a threshold today. This procedure gets the
separation right; the threshold remains open, and is a pre-mainnet gate rather
than something this runbook can close.

## 2. Distribute the public key

The value must be identical on all five hosts *before* any host restarts. A host
restarted with a new value while others still hold the old one is the split in
§4 of the rollout runbook.

Either drive it from the validator console — Fleet → **Shared configuration**,
which writes to every host and refuses to restart anything unless all five come
back byte-identical — or, by hand, on each host.

> **Check the path first.** This repository disagrees with itself about where
> the deployment checkout lives: `geo-testnet.md` says `/opt/omnia-protocol`
> (eighteen times), and so does the validator console's fact gatherer, while
> `financial-layer-rollout.md` §2 says `~/omnia-protocol`. Connected as root
> those are `/opt/omnia-protocol` and `/root/omnia-protocol` — different
> directories, and editing `.env` in the wrong one changes nothing while
> appearing to succeed. Run `ls -d /opt/omnia-protocol ~/omnia-protocol` on one
> host and use whichever exists. The commands below assume `/opt`.

```bash
cd /opt/omnia-protocol
cp docker/.env docker/.env.bak.$(date +%s)

# replace the line if present, append it if not — never both
if grep -q '^OMNIA_MINT_AUTHORITY=' docker/.env; then
  sed -i "s|^OMNIA_MINT_AUTHORITY=.*|OMNIA_MINT_AUTHORITY=$PUBKEY|" docker/.env
else
  printf 'OMNIA_MINT_AUTHORITY=%s\n' "$PUBKEY" >> docker/.env
fi
```

A duplicated key is not a syntax error and does not warn: the compose file reads
the last occurrence, so a stray earlier line is silently ignored on one host and
silently authoritative on another if the order differs. Check before restarting:

```bash
grep -c '^OMNIA_MINT_AUTHORITY=' docker/.env   # expect exactly 1
```

## 3. Confirm agreement before restarting anything

From your workstation, across all five:

```bash
for h in 78.47.43.136 178.156.163.211 5.223.85.30 46.62.218.24 46.224.103.217; do
  printf '%-16s ' "$h"
  ssh root@"$h" "grep '^OMNIA_MINT_AUTHORITY=' /opt/omnia-protocol/docker/.env | cut -d= -f2-"
done | sort -k2 | uniq -c -f1
```

One line, count 5. Anything else — stop and fix it before §4. This is the whole
point of doing the distribution as a separate step from the restart.

## 4. Restart, in order

Same order as every other roll: **E, D, C, B, A** — bootstrap last, because it
seeds the mesh and peers do not re-dial a bootstrap that restarts. One node at a
time; 4-of-5 quorum keeps finality alive throughout, and taking two down at once
stalls it.

```bash
cd /opt/omnia-protocol
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

Setting the variable is not evidence that the node read it. Two checks, and the
second is the one that matters.

**The node logged the authority it loaded.** `node/src/main.rs` logs at startup
in every case — configured, configured-and-equal-to-this-node's-own-key, or
absent:

```bash
docker compose -f docker/docker-compose.wan.yml logs --tail=200 \
  | grep -i 'mint authority\|minting on the financial shard'
```

Expect `Financial shard mint authority configured` with your key. If you instead
see `No mint_authority configured … minting on the financial shard is DISABLED`,
the variable did not reach the container — check that the compose file forwards
it (`grep MINT_AUTHORITY docker/docker-compose.wan.yml`, expect the
`${OMNIA_MINT_AUTHORITY:-}` line).

**Mint something.** Zero counters prove nothing here; they read zero before the
change too. Submit one signed `Mint` event with the private key from §1 and
watch `/api/v1/supply` move:

```bash
curl -s https://78.47.43.136.sslip.io/api/v1/supply
```

Until that number moves, all you have verified is that a node started. A wrong
key produces a node that starts perfectly and rejects every mint with
`Unauthorized: caller is not the mint authority` — which looks, from the
outside, exactly like a network nobody has minted on yet.

## 6. Afterwards

`README.md` and the dashboards describe the economy as zero because it has been.
Once supply is non-zero that stops being true, and the reason it is written down
at all is that the previous claim outlived its accuracy while sitting next to an
endpoint anyone could curl. See [financial-layer-rollout.md](./financial-layer-rollout.md) §5
for the list of files.
