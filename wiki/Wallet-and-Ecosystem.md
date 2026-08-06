# Wallet & Ecosystem

Omnia is not just a protocol repo — it ships with a working client
ecosystem, all live against the public testnet node.

## 📱 Omnia Wallet (mobile, v1 shipped)

**Repo:** [Willow7737/Omnia-Wallet](https://github.com/Willow7737/Omnia-Wallet)
(Flutter, Android/iOS)

- **Dual-mode auth** — true self-custody (an Ed25519 key generated and
  stored on-device signs a challenge; the key never leaves the phone) or
  convenience sign-in (Google/GitHub/email via Supabase, which mints node
  JWTs through an edge function).
- **Self-sovereign identity** — your DID is derived from your public key
  (`did:omnia:` + `sha256(pubkey)[..32]`), identically on device and node,
  pinned by a cross-repo test vector.
- **The full loop works today:** create wallet → challenge/login →
  registered DID with a 1,000 UBC monthly quota → send → history, with
  per-transaction detail including **Lane 0 finality status** and signing
  provenance.
- Plus: governance voting, QR send/receive with request-amounts, address
  book, biometric app lock, in-app notifications, and a team news feed.

## 🖥️ Web dashboard

**Repo:** [Willow7737/omnia-protocol-interface](https://github.com/Willow7737/omnia-protocol-interface)
(Next.js + Supabase) — node stats, events, balances, governance in the
browser.

## 🌐 Website

**Repo:** [Willow7737/omnia-web](https://github.com/Willow7737/omnia-web)

## The node API the clients speak

Public REST API on the testnet node (Swagger UI at the node root):

| Endpoint | Purpose |
|---|---|
| `POST /api/v1/auth/challenge` → `POST /api/v1/auth/login` | Self-custody login: sign a single-use nonce (`"omnia-auth:" + nonce`), get a JWT |
| `POST /api/v1/auth/register` | Idempotent DID registration for externally-minted JWTs |
| `GET /api/v1/economics/balance/:did` | Balance, monthly quota, epoch |
| `POST /api/v1/economics/transfer` | UBC transfer (emits an on-graph event with Lane 0 finality) |
| `GET /api/v1/economics/transfers` | History with `event_id` + `lane0_final` |
| `GET /api/v1/node/info` | Node identity, Lane 0 stats, peers |

Security properties worth knowing: nonces are single-use with a short TTL,
signed messages are domain-separated (`omnia-auth:`), signatures verified
with `verify_strict`, and the node derives your DID from the *verified*
key — never from request bodies.

## A note on UBC

UBC (Universal Basic Compute) is **soulbound protocol capacity, not a
speculative token**. Every registered DID receives a monthly quota. There
was no token sale and the entire protocol is CC0 public domain.
