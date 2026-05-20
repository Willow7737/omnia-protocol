FROM rust:1.88-slim-bookworm AS builder
WORKDIR /build
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
# Core 9-crate layout (post-restructuring)
COPY omnia-primitives/Cargo.toml omnia-primitives/
COPY omnia-consensus/Cargo.toml omnia-consensus/
COPY omnia-crypto/Cargo.toml omnia-crypto/
COPY omnia-network/Cargo.toml omnia-network/
COPY omnia-adapters/Cargo.toml omnia-adapters/
COPY shards/Cargo.toml shards/
COPY binding/Cargo.toml binding/
COPY economics/Cargo.toml economics/
COPY node/Cargo.toml node/
# Deprecated facade (still in workspace)
COPY substrate/Cargo.toml substrate/
# CI-only crates
COPY benches/Cargo.toml benches/
COPY fuzz/Cargo.toml fuzz/
COPY chaos-tests/Cargo.toml chaos-tests/
RUN mkdir -p omnia-primitives/src omnia-consensus/src omnia-crypto/src \
    omnia-network/src omnia-adapters/src shards/src binding/src \
    economics/src node/src substrate/src benches/src fuzz/src \
    chaos-tests/src && \
    echo "" > omnia-primitives/src/lib.rs && \
    echo "" > omnia-consensus/src/lib.rs && \
    echo "" > omnia-crypto/src/lib.rs && \
    echo "" > omnia-network/src/lib.rs && \
    echo "" > omnia-adapters/src/lib.rs && \
    echo "" > shards/src/lib.rs && \
    echo "" > binding/src/lib.rs && \
    echo "" > economics/src/lib.rs && \
    echo "" > node/src/lib.rs && \
    echo "" > substrate/src/lib.rs && \
    echo "" > benches/src/lib.rs && \
    echo "" > chaos-tests/src/lib.rs && \
    cargo fetch
COPY . .
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
EXPOSE 8080/tcp 4001/tcp 4001/udp
ENV RUST_LOG=info
ENV OMNIA_NODE_ID=""
ENV OMNIA_BOOTSTRAP_NODES=""
ENV OMNIA_DATA_DIR=/app/data
HEALTHCHECK --interval=30s --timeout=10s --start-period=60s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1
