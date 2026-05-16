#!/bin/bash
# apply-fixes.sh — Legacy migration tool for Omnia Protocol
#
# This script copies pre-built fix files from the repository root (substrate/)
# into the target substrate directory. It was created during early development
# when critical fixes were applied as patches to a reference implementation.
# In normal workflows this script is NOT needed — the substrate/ directory
# already contains the correct, up-to-date source code. It is retained for
# historical reference and for edge cases where a fresh clone needs to be
# patched from the bundled fix files.
#
# Usage: ./apply-fixes.sh [repo-root-path]

set -e

REPO_ROOT="${1:-.}"
SUBSTRATE_DIR="$REPO_ROOT/substrate"

if [ ! -d "$SUBSTRATE_DIR" ]; then
    echo "Error: $SUBSTRATE_DIR not found. Please run from repo root or pass repo path."
    exit 1
fi

echo "Applying Fix 1: Real bincode serialization..."
cp substrate/Cargo.toml "$SUBSTRATE_DIR/Cargo.toml"
cp substrate/src/event.rs "$SUBSTRATE_DIR/src/event.rs"

echo "Applying Fix 2: Ed25519 cryptographic signatures..."
cp substrate/src/crypto.rs "$SUBSTRATE_DIR/src/crypto.rs"

echo "Applying Fix 3: Consensus witness logic fix..."
cp substrate/src/consensus.rs "$SUBSTRATE_DIR/src/consensus.rs"

echo "Applying Fix 4: Async gossip with libp2p..."
cp substrate/src/network.rs "$SUBSTRATE_DIR/src/network.rs"
cp substrate/src/gossip.rs "$SUBSTRATE_DIR/src/gossip.rs"
cp substrate/src/lib.rs "$SUBSTRATE_DIR/src/lib.rs"

echo "Applying Fix 5: Throughput benchmark..."
mkdir -p "$SUBSTRATE_DIR/benches"
cp substrate/benches/throughput.rs "$SUBSTRATE_DIR/benches/throughput.rs"

echo "Applying Fix 6: Property-based tests..."
mkdir -p "$SUBSTRATE_DIR/tests"
cp substrate/tests/property_tests.rs "$SUBSTRATE_DIR/tests/property_tests.rs"

echo ""
echo "All fixes applied. Next steps:"
echo "  cd $SUBSTRATE_DIR"
echo "  cargo check"
echo "  cargo test"
echo "  cargo clippy"
echo "  cargo bench"
