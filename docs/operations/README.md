# Operations Documentation

> 🎯 Audience: Operators
> 🔗 Context: Index for all operational guides, runbooks, and deployment procedures
> 📅 Last Updated: 2026-08-11

## Operations Documents

| Document                                                         | Description                                                         |
| ---------------------------------------------------------------- | ------------------------------------------------------------------- |
| [validator-setup.md](validator-setup.md)                         | Validator setup guide — key generation, external operator onboarding, node configuration, startup |
| [monitoring.md](monitoring.md)                                   | Monitoring setup — Grafana, Prometheus, alert rules                 |
| [deployment.md](deployment.md)                                   | Deployment procedures — Docker Compose, Kubernetes/Helm             |
| [runbook.md](runbook.md)                                         | Operations runbook — key rotation, slashing, partition recovery     |
| [financial-layer-rollout.md](financial-layer-rollout.md)         | Rolling the financial-layer release across the standing 5-node mesh |
| [feature-flags.md](feature-flags.md)                             | Feature flag reference                                              |
| [cli-and-api.md](cli-and-api.md)                                 | CLI subcommands and REST API reference                              |
| [self-hosted-runner-setup.md](self-hosted-runner-setup.md)       | Self-hosted GitHub Actions runner setup guide                       |

## Quick Reference

```bash
# Start a single external validator from sample config
omnia-node --config config/external-validator.toml

# Check health
curl http://localhost:8080/healthz

# Check readiness (peers present + not syncing; idle networks can be ready)
curl http://localhost:8080/readyz

# View metrics
curl http://localhost:8080/metrics

# Docker 5-node testnet
docker compose -f docker/docker-compose.yml up -d
```

---

🔙 **Back**: [docs/](../) | 🔄 **Related**: [building/](../building/)
🚀 **Next**: [validator-setup.md](./validator-setup.md) | 📜 **Source of Truth**: [Restructuring Blueprint](../reference/blueprint-reference.md)
