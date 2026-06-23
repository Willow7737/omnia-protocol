# Self-Hosted Benchmark Runner Setup

This guide covers setting up a dedicated physical machine as a GitHub Actions self-hosted runner for the Omnia Protocol benchmark regression gate.

## Why a self-hosted runner?

GitHub Actions shared runners (`ubuntu-latest`) have **±20–30% inter-run CPU frequency variance** because they're Azure VMs with heterogeneous CPU generations (Intel vs AMD, 2.7–3.8 GHz clock range). This noise makes it impossible to detect real performance regressions smaller than ~25% — which is exactly the range where most real regressions live.

A self-hosted runner on dedicated hardware reduces variance to **<3%**, enabling:
- Tight regression thresholds (5% instead of 25%)
- Multi-sample statistical significance testing that actually works
- Deterministic IAI-callgrind instruction counts (with ASLR disabled)

## Hardware requirements

| Component | Minimum | Recommended |
|-----------|---------|-------------|
| CPU | 4 physical cores | 8+ physical cores (Intel Xeon or AMD EPYC) |
| RAM | 16 GiB | 32 GiB (ZK benchmarks need ~8 GiB) |
| Disk | 50 GiB SSD | 100 GiB NVMe SSD |
| Network | Broadband | Broadband (no high bandwidth needed) |
| OS | Ubuntu 22.04 LTS | Ubuntu 24.04 LTS |

**Bare metal is strongly preferred over VMs.** VM hypervisors introduce scheduler jitter that re-introduces the variance we're trying to eliminate. If you must use a VM, pin vCPUs to physical cores and disable CPU overcommit.

## One-time setup

### 1. Install the base OS

Install Ubuntu 22.04+ LTS Server. During installation:
- Create a user named `github-runner`
- Enable SSH
- Do NOT install a desktop environment (waste of resources)

### 2. Disable hyperthreading (for deterministic cache behavior)

Hyperthreading shares L1/L2 cache between two logical cores, which introduces non-deterministic cache eviction patterns. Disable it in BIOS, or at boot time:

```bash
# Add to /etc/default/grub:
sudo sed -i 's/GRUB_CMDLINE_LINUX_DEFAULT=""/GRUB_CMDLINE_LINUX_DEFAULT="nosmt"/' /etc/default/grub
sudo update-grub
sudo reboot
```

After reboot, verify:
```bash
lscpu | grep Thread
# Should show: Thread(s) per core: 1
```

### 3. Pin CPU governor to 'performance'

This disables dynamic frequency scaling (the #1 source of variance):

```bash
# Install cpupower
sudo apt-get update && sudo apt-get install -y linux-tools-common linux-tools-generic

# Set governor to performance on all cores
sudo cpupower frequency-set -g performance

# Make it persistent across reboots
echo 'GOVERNOR=performance' | sudo tee /etc/default/cpupower
sudo systemctl enable cpupower

# Verify
cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor
# Should output: performance
```

### 4. Disable ASLR (for IAI-callgrind determinism)

ASLR randomizes process memory layout, which means the allocator's free-list state varies across runs. Disabling it makes IAI instruction counts perfectly reproducible and reduces allocator-state variance for Groth16 benchmarks.

**⚠️ Security warning:** Disabling ASLR reduces security. Only do this on a dedicated benchmark runner that does NOT run untrusted code. The runner should be isolated from production workloads.

```bash
# Disable ASLR immediately
echo 0 | sudo tee /proc/sys/kernel/randomize_va_space

# Make it persistent across reboots
echo 'kernel.randomize_va_space=0' | sudo tee -a /etc/sysctl.d/99-benchmark.conf
sudo sysctl -p /etc/sysctl.d/99-benchmark.conf

# Verify
cat /proc/sys/kernel/randomize_va_space
# Should output: 0
```

### 5. Install required packages

```bash
sudo apt-get update
sudo apt-get install -y \
    build-essential \
    pkg-config \
    libssl-dev \
    valgrind \
    gnuplot \
    jq \
    curl \
    git
```

### 6. Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.91.0
source "$HOME/.cargo/env"
rustc --version  # Should output: rustc 1.91.0
```

### 7. Register the runner with GitHub

Follow GitHub's official guide: [Adding self-hosted runners](https://docs.github.com/en/actions/hosting-your-own-runners/managing-self-hosted-runners-with-github-actions/adding-self-hosted-runners)

Key steps:
1. Go to **Settings → Actions → Runners → New self-hosted runner**
2. Choose **Linux x64**
3. Run the config commands provided by GitHub
4. **Important:** When prompted for labels, enter: `benchmark`
5. Install as a systemd service:
   ```bash
   sudo ./svc.sh install github-runner
   sudo ./svc.sh start
   ```

### 8. Pin the runner process to specific CPU cores (optional but recommended)

For maximum determinism, pin the runner process to specific physical cores so it doesn't get scheduled onto different cores with different cache states:

```bash
# Edit the systemd service
sudo systemctl edit github-runner.actions.githubusercontent.com.<repo>.runner.service

# Add:
[Service]
CPUAffinity=0-3  # Pin to cores 0-3 (adjust based on your core count)
Nice=-10         # Higher priority
```

Then restart:
```bash
sudo systemctl daemon-reload
sudo systemctl restart github-runner.actions.githubusercontent.com.<repo>.runner.service
```

## Verification

After setup, verify the runner is online and properly labeled:

```bash
# On the runner machine:
cd actions-runner
./config.sh --url https://github.com/Willow7737/omnia-protocol --labels benchmark
sudo systemctl status github-runner.actions.githubusercontent.com.Willow7737_omnia-protocol.runner.service

# From your local machine (with gh CLI):
gh api repos/Willow7737/omnia-protocol/actions/runners --jq '.runners[] | select(.labels[].name=="benchmark") | {id, name, status, busy}'
```

The output should show your runner with `status: "online"`.

## Triggering the self-hosted benchmark workflow

The self-hosted workflow (`.github/workflows/bench-self-hosted.yml`) runs:
- Automatically on pushes to `main` (pre-merge validation)
- Manually via workflow_dispatch

To trigger manually:
1. Go to **Actions → Benchmark Regression Gate (Self-Hosted)**
2. Click **Run workflow**
3. Configure:
   - **runs**: Number of multi-sample runs (default: 10)
   - **threshold**: Regression threshold % (default: 5 — tight because self-hosted variance is <3%)

## Maintenance

### Updating the runner

```bash
cd actions-runner
sudo ./svc.sh stop
./config.sh --url https://github.com/Willow7737/omnia-protocol --labels benchmark
sudo ./svc.sh start
```

### Cleaning up old benchmark artifacts

Benchmark runs accumulate disk usage. Clean up periodically:

```bash
# Remove old target/ directories older than 7 days
find /home/github-runner/work -name "target" -type d -mtime +7 -exec rm -rf {} + 2>/dev/null || true

# Remove old Cargo registry cache (forces re-download but frees space)
rm -rf /home/github-runner/.cargo/registry/cache
```

Add this as a weekly cron job:
```bash
echo "0 3 * * 0 find /home/github-runner/work -name 'target' -type d -mtime +7 -exec rm -rf {} + 2>/dev/null" | crontab -
```

### Monitoring runner health

Set up a simple health check that alerts if the runner goes offline:

```bash
# Add to crontab:
*/5 * * * * systemctl is-active --quiet github-runner.actions.githubusercontent.com.Willow7737_omnia-protocol.runner.service || echo "Runner down!" | mail -s "Omnia benchmark runner offline" alerts@example.com
```

## Cost considerations

A bare-metal server with the specs above costs approximately:
- **Hetzner:** €30–50/month (AX41-NVMe, AMD Ryzen 5)
- **OVH:** $40–60/month (Advance-1, Intel Xeon)
- **AWS EC2 dedicated host:** $100–200/month (overkill for this use case)
- **Local hardware:** One-time ~$500–800 (used workstation)

The Hetzner and OVH options are the best value for this workload. Both offer bare-metal servers (not VMs), which is critical for variance reduction.

## Troubleshooting

### "No runner with label 'benchmark' is online"

The self-hosted workflow will queue indefinitely if no runner is available. Check:
1. Is the runner service running? `sudo systemctl status github-runner*`
2. Is the runner labeled `benchmark`? `./config.sh --labels benchmark`
3. Is the runner idle? (Check GitHub UI: Settings → Actions → Runners)

### Benchmarks are still noisy despite self-hosted runner

Check:
1. Is hyperthreading disabled? `lscpu | grep Thread` (should show 1)
2. Is the CPU governor set to 'performance'? `cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor`
3. Is ASLR disabled? `cat /proc/sys/kernel/randomize_va_space` (should show 0)
4. Is the runner pinned to specific cores? `cat /proc/$(pgrep -f actions-runner)/status | grep Cpus_allowed`
5. Are there other processes competing for CPU? `top -bn1 | head -20`

### IAI baselines show N/A

The IAI gate uses a committed baseline file (`benches/iai_baselines.json`), not a cache. If you see N/A, the benchmark output format may have changed. Run:
```bash
python3 scripts/update_iai_baselines.py iai_output.txt
```
And commit the updated file.
