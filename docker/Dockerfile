FROM rust:1.91-slim-bookworm AS builder
WORKDIR /build
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*
# Copy workspace root manifest and lockfile
COPY Cargo.toml Cargo.lock ./
# Copy only the production crate manifests (10 crates)
COPY omnia-primitives/Cargo.toml omnia-primitives/
COPY omnia-consensus/Cargo.toml omnia-consensus/
COPY omnia-crypto/Cargo.toml omnia-crypto/
COPY omnia-network/Cargo.toml omnia-network/
COPY omnia-adapters/Cargo.toml omnia-adapters/
COPY shards/Cargo.toml shards/
COPY binding/Cargo.toml binding/
COPY economics/Cargo.toml economics/
COPY node/Cargo.toml node/
COPY substrate/Cargo.toml substrate/
# CI-only crates (benches, fuzz, chaos-tests, tests) are excluded by .dockerignore.
# 1) Strip them from workspace members
# 2) Create stub source files for the remaining 10 crates
# 3) Remove the lockfile — cargo will regenerate it for the reduced workspace
# 4) Pre-fetch all dependencies
RUN sed -i '/"chaos-tests"/d;/"fuzz"/d;/"benches"/d;/"tests"/d' Cargo.toml && \
    mkdir -p omnia-primitives/src omnia-consensus/src omnia-crypto/src \
    omnia-network/src omnia-adapters/src omnia-adapters/examples \
    shards/src binding/src \
    economics/src node/src substrate/src && \
    echo "" > omnia-primitives/src/lib.rs && \
    echo "" > omnia-consensus/src/lib.rs && \
    echo "" > omnia-crypto/src/lib.rs && \
    echo "" > omnia-network/src/lib.rs && \
    echo "" > omnia-adapters/src/lib.rs && \
    echo "fn main() {}" > omnia-adapters/examples/bitcoin_regtest_e2e.rs && \
    echo "" > shards/src/lib.rs && \
    echo "" > binding/src/lib.rs && \
    echo "" > economics/src/lib.rs && \
    echo "" > node/src/lib.rs && \
    echo "" > substrate/src/lib.rs && \
    rm Cargo.lock && \
    cargo fetch
# Copy all real source code (CI-only dirs excluded by .dockerignore)
COPY . .
# Re-strip CI-only members since COPY . . restores the original Cargo.toml.
# Remove the original lockfile's CI-only crate entries so cargo build works.
RUN sed -i '/"chaos-tests"/d;/"fuzz"/d;/"benches"/d;/"tests"/d' Cargo.toml && \
    rm Cargo.lock
# Production profile: --no-default-features --features full (excludes swagger-ui to meet ≤12MB target)
RUN cargo build --release -p omnia-node --no-default-features --features full

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*
RUN useradd -m -u 1000 -s /bin/bash omnia
WORKDIR /app
COPY --from=builder /build/target/release/omnia-node /app/omnia-node
RUN mkdir -p /app/data && chown -R omnia:omnia /app
USER omnia
EXPOSE 8080/tcp 4001/udp
ENV RUST_LOG=info
ENV OMNIA_NODE_ID=""
ENV OMNIA_BOOTSTRAP_NODES=""
ENV OMNIA_DATA_DIR=/app/data
HEALTHCHECK --interval=30s --timeout=10s --start-period=60s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

ENTRYPOINT ["/app/omnia-node"]
