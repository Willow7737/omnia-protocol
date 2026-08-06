# Getting Started

Three paths, fastest first.

## 1. Just talk to the live testnet (30 seconds)

No install — the public multi-node testnet is running now:

```bash
curl https://78.47.43.136.sslip.io/api/v1/node/info
```

Swagger UI for the full REST API is served at the same host. For the wallet
experience, see [Wallet & Ecosystem](Wallet-and-Ecosystem).

## 2. Run a local node (5 minutes with Docker)

```bash
git clone https://github.com/Willow7737/omnia-protocol
cd omnia-protocol
docker compose -f docker/docker-compose.testnet.yml up -d --build
curl -s http://localhost:9090/api/v1/node/info | python3 -m json.tool
```

That starts a 3-node local testnet (bootstrap + 2 peers) with health
checks. To make the nodes **Lane 0 validators** with real BFT finality:

```bash
NODES=3 ./scripts/setup-validators.sh   # generates keys + docker/.env
docker compose -f docker/docker-compose.testnet.yml up -d --build
```

Then benchmark it (live progress bars included):

```bash
OMNIA_JWT_SECRET=$(grep ^OMNIA_JWT_SECRET= docker/.env | cut -d= -f2-) \
  ./scripts/testnet-bench.sh --events 1000 --concurrency 16
```

## 3. Build from source

Requirements: Rust ≥ 1.91 (MSRV), `protobuf-compiler`, `pkg-config`,
`libssl-dev`.

```bash
git clone https://github.com/Willow7737/omnia-protocol
cd omnia-protocol
cargo build --release -p omnia-node --no-default-features --features full
cargo test --workspace          # 1,300+ tests
./target/release/omnia-node --help
```

Feature flags matter — `full` enables ZK proving (arkworks), `light` is the
minimal node. The matrix is documented in
[`docs/building/feature-matrix.md`](https://github.com/Willow7737/omnia-protocol/blob/main/docs/building/feature-matrix.md).

## Key environment variables

| Variable | Purpose |
|---|---|
| `OMNIA_JWT_SECRET` | HMAC secret for API JWTs (required) |
| `OMNIA_LANE0_VALIDATORS` | Comma list of `<hex-pubkey>:<stake>` — enables Lane 0 finality; identical on every node |
| `OMNIA_NODE_KEY_FILE` | Persistent Ed25519 validator key (raw 32 bytes) |
| `OMNIA_BOOTSTRAP_NODES` | Multiaddr(s) to dial on startup, e.g. `/ip4/1.2.3.4/udp/4001/quic-v1` |
| `OMNIA_HTTP_PORT` | REST API + metrics port |
| `OMNIA_RATE_LIMIT_RPS` | Per-client API rate limit (raise for benchmarks) |

Full deployment reference:
[`docs/operations/`](https://github.com/Willow7737/omnia-protocol/tree/main/docs/operations).

## Where next

- Join or reproduce the real network: [Testnet Guide](Testnet-Guide)
- Understand what you just ran: [Architecture Overview](Architecture-Overview)
- Contribute: [`CONTRIBUTING.md`](https://github.com/Willow7737/omnia-protocol/blob/main/CONTRIBUTING.md)
