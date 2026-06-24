# Operations Documentation

> 🎯 Audience: Operators
> 🔗 Context: Index for all operational guides, runbooks, and deployment procedures
> 📅 Last Updated: 2026-05-20

## Operations Documents

| Document                                                         | Description                                                         |
| ---------------------------------------------------------------- | ------------------------------------------------------------------- |
| [validator-setup.md](validator-setup.md)                         | Validator setup guide — key generation, node configuration, startup |
| [monitoring.md](monitoring.md)                                   | Monitoring setup — Grafana, Prometheus, alert rules                 |
| [deployment.md](deployment.md)                                   | Deployment procedures — Docker Compose, Kubernetes/Helm             |
| [runbook.md](runbook.md)                                         | Operations runbook — key rotation, slashing, partition recovery     |
| [feature-flags.md](feature-flags.md)                             | Feature flag reference                                              |
| [cli-and-api.md](cli-and-api.md)                                 | CLI subcommands and REST API reference                              |
| [self-hosted-runner-setup.md](self-hosted-runner-setup.md)       | Self-hosted GitHub Actions runner setup guide                       |

## Quick Reference

```bash
# Start a node
omnia-node --node-id 1 --http-port 8080

# Check health
curl http://localhost:8080/healthz

# Check readiness
curl http://localhost:8080/readyz

# View metrics
curl http://localhost:8080/metrics

# Docker 5-node testnet
docker compose -f docker/docker-compose.yml up -d
```

---

🔙 **Back**: [docs/](../) | 🔄 **Related**: [building/](../building/)
🚀 **Next**: [validator-setup.md](./validator-setup.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)
