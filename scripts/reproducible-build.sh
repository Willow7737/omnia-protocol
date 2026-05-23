#!/usr/bin/env bash
set -euo pipefail

# Reproducible build script for Omnia Protocol
#
# This script produces a deterministic binary by controlling build
# inputs that normally introduce non-determinism:
# - SOURCE_DATE_EPOCH: Pins the build timestamp to the git commit time
# - TZ / LC_ALL: Forces UTC timezone and C locale
# - RUSTFLAGS: Strips build-ids from the linker output
#
# Note: Rust builds are NOT currently fully deterministic due to
# LLVM parallelism and symbol ordering. This script provides a
# foundation for reproducibility and tracks progress toward
# fully deterministic builds.

export SOURCE_DATE_EPOCH=$(git log -1 --pretty=%ct)
export TZ=UTC
export LC_ALL=C

echo "=== Omnia Protocol — Reproducible Build ==="
echo "SOURCE_DATE_EPOCH: $SOURCE_DATE_EPOCH"
echo "Commit: $(git rev-parse HEAD)"
echo "Date: $(date -u -d @$SOURCE_DATE_EPOCH 2>/dev/null || date -u -r $SOURCE_DATE_EPOCH 2>/dev/null || echo 'unknown')"
echo ""

# Strip any build-ids and timestamps from the binary
RUSTFLAGS="-C link-arg=-Wl,--build-id=none" \
cargo build --release --locked --target x86_64-unknown-linux-gnu

# Compute and display the hash of the resulting binary
echo ""
echo "=== Build Complete ==="
sha256sum target/x86_64-unknown-linux-gnu/release/omnia-node 2>/dev/null || echo "Binary not found (may need to build the node crate)"
echo ""
echo "SOURCE_DATE_EPOCH: $SOURCE_DATE_EPOCH"
echo "Commit: $(git rev-parse HEAD)"
