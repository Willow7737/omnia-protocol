#!/usr/bin/env bash
set -euo pipefail

CORPUS_DIR="$(dirname "$0")/../fuzz/fuzz_targets/corpus"

echo "Generating fuzz corpus seeds..."

# Event deserialization seeds
mkdir -p "$CORPUS_DIR/event_deserialization"
python3 -c "
import struct, os
creator = b'\x01' * 32
sequence = struct.pack('<Q', 1)
payload = b'test'
payload_len = struct.pack('<Q', len(payload))
with open('$CORPUS_DIR/event_deserialization/seed1.bin', 'wb') as f:
    f.write(creator + sequence + payload_len + payload)
"

# Snapshot deserialization seeds
mkdir -p "$CORPUS_DIR/snapshot_deserialization"
python3 -c "
import struct
version = struct.pack('<I', 1)
height = struct.pack('<Q', 0)
event_count = struct.pack('<Q', 0)
with open('$CORPUS_DIR/snapshot_deserialization/seed1.bin', 'wb') as f:
    f.write(version + height + event_count + b'\x00' * 64)
"

# Vector clock seeds
mkdir -p "$CORPUS_DIR/vector_clock_merge"
python3 -c "
with open('$CORPUS_DIR/vector_clock_merge/seed1.bin', 'wb') as f:
    f.write(b'\x00' * 16)
"

# Rate limiter seeds
mkdir -p "$CORPUS_DIR/rate_limiter"
python3 -c "
with open('$CORPUS_DIR/rate_limiter/seed1.bin', 'wb') as f:
    f.write(bytes([0, 1, 0, 0, 1, 0, 0, 0, 1, 0] * 10))
"

# Gossip message seeds
mkdir -p "$CORPUS_DIR/gossip_message"
python3 -c "
import struct
with open('$CORPUS_DIR/gossip_message/seed1.bin', 'wb') as f:
    f.write(struct.pack('<I', 1) + b'\x00' * 32 + struct.pack('<Q', 0))
"

# ZK proof deserialization seeds
mkdir -p "$CORPUS_DIR/zk_proof_deserialization"
python3 -c "
with open('$CORPUS_DIR/zk_proof_deserialization/seed1.bin', 'wb') as f:
    f.write(b'\x00' * 128)
"

# Consensus state transition seeds
mkdir -p "$CORPUS_DIR/consensus_state_transition"
python3 -c "
import struct
with open('$CORPUS_DIR/consensus_state_transition/seed1.bin', 'wb') as f:
    f.write(struct.pack('<Q', 0) + struct.pack('<Q', 1) + b'\x01' * 32)
"

echo "Corpus seeds generated in $CORPUS_DIR"
