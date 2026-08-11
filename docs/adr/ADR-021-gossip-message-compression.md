# ADR-021: Gossip Message Compression

> 🎯 Audience: Architects
> 🔗 Context: Part of the adr documentation section
> 📅 Last Updated: 2026-08-11

## Status

Accepted

## Date

2026-05-19

## Version

1.0.0

## Decision

Use Snappy compression for gossip messages exceeding 256 bytes, with a compression flag byte prefix (0x00 = uncompressed, 0x01 = Snappy) for backward compatibility. Messages at or below 256 bytes are sent uncompressed, as the compression overhead would exceed the size savings.

## Context

Network bandwidth is a scarce resource in P2P networks. GossipSub propagates every message to multiple peers (mesh fanout), amplifying the bandwidth cost of each message. As the Omnia network grows and message volume increases, bandwidth efficiency becomes critical for:

1. **Node accessibility**: Nodes on bandwidth-limited connections (home internet, mobile) should be able to participate.
2. **Network scalability**: Total bandwidth consumption scales linearly with message size × peer count.
3. **Consensus performance**: Smaller messages propagate faster, reducing latency in the consensus loop.

The consensus and event messages in Omnia frequently exceed 256 bytes (event payloads, signatures, vector clocks, shard operation data), making compression beneficial for most messages while keeping the overhead negligible for small control messages.

## Alternatives Considered

### No Compression

Send all messages uncompressed. Simplest approach with zero CPU overhead, but wastes bandwidth on large messages. Not sustainable as the network scales.

### LZ4 Compression

LZ4 offers faster compression/decompression than Snappy with slightly lower compression ratios. However, Snappy has broader ecosystem support in Rust (the `snap` crate is mature and well-tested), and the performance difference is negligible at Omnia's message sizes.

### Zstandard (zstd) Compression

Zstandard provides the best compression ratios, especially at higher compression levels. However, it has higher CPU overhead and more complex configuration (compression level selection). For gossip messages where latency matters more than maximum compression, Snappy's speed is preferable.

## Consequences

### Positive

- Reduced bandwidth consumption for messages >256 bytes (typically 40-60% size reduction)
- Compression flag byte prefix ensures backward compatibility — old nodes can detect and skip compressed messages
- Snappy compression is fast (microseconds for typical message sizes), adding negligible latency
- `serialize_compressed()` and `deserialize_compressed()` provide clean API with automatic threshold
- 256-byte threshold avoids compression overhead for small messages where savings would be minimal
- No configuration needed — compression is automatic and transparent

### Negative

- Small CPU overhead for compression/decompression (mitigated by Snappy's speed)
- 1-byte overhead (flag byte) for all messages, even uncompressed ones
- Backward compatibility requires old nodes to handle the flag byte gracefully (skip unknown messages)
- Compression ratio varies by message content — some messages may not compress well

### Trade-offs

- Chose Snappy over LZ4 for broader ecosystem support and mature Rust implementation
- Chose Snappy over zstd for lower latency at the cost of slightly worse compression
- 256-byte threshold balances compression benefit against overhead
- Flag byte prefix trades 1 byte per message for clean backward compatibility without version negotiation

---

🔙 **Back**: [ADR Index](./) | 🔄 **Related**: [ADR Index](../reference/adr-index.md)
🚀 **Next**: [ADR Index](../reference/adr-index.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)
