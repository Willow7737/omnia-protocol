FROM rust:1.85-slim-bookworm AS builder
WORKDIR /build
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY substrate/Cargo.toml substrate/
COPY shards/Cargo.toml shards/
COPY binding/Cargo.toml binding/
COPY economics/Cargo.toml economics/
COPY zk/Cargo.toml zk/
COPY node/Cargo.toml node/
COPY fuzz/Cargo.toml fuzz/
COPY chaos-tests/Cargo.toml chaos-tests/
RUN mkdir -p substrate/src shards/src binding/src economics/src zk/src node/src && \
    echo "" > substrate/src/lib.rs && \
    echo "" > shards/src/lib.rs && \
    echo "" > binding/src/lib.rs && \
    echo "" > economics/src/lib.rs && \
    echo "" > zk/src/lib.rs && \
    echo "" > node/src/lib.rs && \
    cargo fetch
COPY . .
RUN cargo build --release -p omnia-node

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
EXPOSE 9090/tcp 9090/udp
ENV RUST_LOG=info
ENV OMNIA_NODE_ID=""
ENV OMNIA_BOOTSTRAP_NODES=""
ENV OMNIA_DATA_DIR=/app/data
HEALTHCHECK --interval=30s --timeout=10s --start-period=60s --retries=3 \
    CMD curl -f http://localhost:9090/health || exit 1
