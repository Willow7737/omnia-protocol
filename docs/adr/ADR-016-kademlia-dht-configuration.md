# ADR-016: Kademlia DHT Configuration

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

Use Kademlia DHT with protocol `/omnia/kad/1.0.0`, periodic bootstrap interval of 5 minutes, and in-memory store for DHT records. Combine with AutoNAT for NAT type detection, Relay client for NAT traversal (libp2p relay v2), and DCutr for direct connection upgrade after relay.

## Context

The initial networking stack relied on static bootstrap peers for peer discovery. New nodes could only connect to explicitly configured bootstrap nodes, which created several problems:

1. **Single points of failure**: If all bootstrap nodes go offline, no new nodes can join the network.
2. **No self-healing topology**: The network couldn't recover from peer churn or partition events.
3. **Limited scalability**: Every node needed manual configuration of peer addresses.
4. **NAT traversal**: Nodes behind NAT (most home/office connections) couldn't accept incoming connections, limiting network connectivity.

A distributed hash table (DHT) provides dynamic peer discovery where nodes can find each other without centralized coordination. Kademlia is the de facto standard DHT in P2P networks, used by IPFS, Ethereum (discv5), and many others.

## Alternatives Considered

### Static Peer Lists Only

Keep the existing approach of manually configured bootstrap peers. Simple and requires no additional protocol, but creates single points of failure and doesn't scale beyond small networks.

### mDNS-Only Discovery

Use multicast DNS for local network discovery. Works well for development and LAN testing but cannot discover peers across the internet. Not suitable for production deployment.

### Custom Discovery Protocol

Build a custom peer discovery protocol. Allows tailoring to specific needs but is a significant development effort, hard to get right, and lacks battle-testing that Kademlia has.

## Consequences

### Positive

- Dynamic peer discovery — nodes find each other without manual configuration
- Self-healing network topology — routing tables repair automatically after peer churn
- Periodic bootstrap (5 min) ensures routing table freshness
- AutoNAT detects NAT type, enabling appropriate connection strategies
- Relay v2 allows nodes behind NAT to receive incoming connections
- DCutr upgrades relay connections to direct connections when possible, reducing latency
- TCP transport fallback alongside QUIC for compatibility
- Configurable via `NetworkConfig` with `relay_servers`, `dht_protocol`, and feature flags

### Negative

- Additional network traffic for DHT maintenance (bootstrap queries, routing table updates)
- Kademlia requires at least one reachable bootstrap peer for initial join
- Relay connections add latency compared to direct connections
- Memory store for DHT records means records are lost on node restart (acceptable for peer discovery)

### Trade-offs

- Chose Kademlia over mDNS for wide-area discovery capability
- Periodic bootstrap interval of 5 minutes balances freshness with network overhead
- Memory store is sufficient for peer discovery; persistent store not needed
- AutoNAT + Relay + DCutr provides maximum NAT traversal compatibility

---

🔙 **Back**: [ADR Index](./) | 🔄 **Related**: [ADR Index](../reference/adr-index.md)
🚀 **Next**: [ADR Index](../reference/adr-index.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)
