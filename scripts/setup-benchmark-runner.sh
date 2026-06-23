#!/usr/bin/env bash
# Setup script for the self-hosted benchmark runner.
#
# This script is IDEMPOTENT — safe to run on every CI job. It:
#   1. Pins the CPU governor to 'performance' (eliminates frequency
#      scaling variance, the #1 source of benchmark noise on shared
#      runners).
#   2. Disables ASLR (address space layout randomization) so memory
#      layout is deterministic across runs — improves IAI-callgrind
#      reproducibility and reduces allocator-state variance.
#   3. Installs required system packages (valgrind, gnuplot, jq).
#   4. Verifies the Rust toolchain is present.
#
# This script assumes the runner is a Linux x86_64 machine with sudo
# access. For ARM64 runners, adjust the apt packages as needed.
#
# For one-time runner setup (NOT per-job), see
# docs/operations/self-hosted-runner-setup.md which covers:
#   - Registering the runner with the 'benchmark' label
#   - Pinning the runner process to specific CPU cores via systemd
#   - Disabling hyperthreading for deterministic cache behavior
#   - Setting up a cron job to keep the runner updated
set -euo pipefail

echo "=== Self-hosted benchmark runner setup ==="
echo "Hostname: $(hostname)"
echo "Kernel: $(uname -r)"
echo "CPU: $(grep 'model name' /proc/cpuinfo | head -1 | cut -d: -f2 | xargs)"
echo "CPU cores: $(nproc)"
echo ""

# ── 1. Pin CPU governor to 'performance' ─────────────────────────
# This eliminates dynamic frequency scaling (the #1 source of inter-run
# variance on cloud VMs). On a physical self-hosted runner, this should
# already be set in BIOS, but we enforce it here as defense-in-depth.
echo "--- CPU governor ---"
if [ -w /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor ]; then
    for cpu in /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor; do
        echo performance | sudo tee "$cpu" > /dev/null 2>&1 || true
    done
    actual=$(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor 2>/dev/null || echo "unreadable")
    echo "  Governor: $actual"
    if [ "$actual" != "performance" ]; then
        echo "  ⚠️  Governor is NOT 'performance' — benchmark numbers will have frequency-scaling variance."
        echo "     This usually means the runner is a VM that restricts governor changes."
        echo "     For true determinism, use a bare-metal runner."
    fi
else
    echo "  scaling_governor not available (VM or container) — cannot pin governor."
fi
echo ""

# ── 2. Disable ASLR ──────────────────────────────────────────────
# ASLR randomizes the memory layout of processes, which means the
# allocator's free-list state varies across runs. This is the #2 source
# of variance for allocator-heavy benchmarks (Groth16 proof generation).
# Disabling it improves reproducibility.
#
# NOTE: Disabling ASLR reduces security. Only do this on a dedicated
# benchmark runner that does NOT run untrusted code. The runner should
# be isolated from production workloads.
echo "--- ASLR ---"
current_aslr=$(cat /proc/sys/kernel/randomize_va_space 2>/dev/null || echo "unreadable")
echo "  Current ASLR setting: $current_aslr (0=off, 1=partial, 2=full)"
if [ "$current_aslr" != "0" ]; then
    echo "  Attempting to disable ASLR (requires sudo)..."
    echo 0 | sudo tee /proc/sys/kernel/randomize_va_space > /dev/null 2>&1 || true
    new_aslr=$(cat /proc/sys/kernel/randomize_va_space 2>/dev/null || echo "unreadable")
    echo "  New ASLR setting: $new_aslr"
    if [ "$new_aslr" != "0" ]; then
        echo "  ⚠️  Could not disable ASLR — allocator-state variance will persist."
        echo "     For permanent disable, add 'kernel.randomize_va_space=0' to /etc/sysctl.conf"
    fi
fi
echo ""

# ── 3. Install required packages ─────────────────────────────────
echo "--- System packages ---"
needed_pkgs=()
command -v valgrind >/dev/null 2>&1 || needed_pkgs+=(valgrind)
command -v gnuplot >/dev/null 2>&1 || needed_pkgs+=(gnuplot)
command -v jq >/dev/null 2>&1 || needed_pkgs+=(jq)

if [ ${#needed_pkgs[@]} -gt 0 ]; then
    echo "  Installing: ${needed_pkgs[*]}"
    sudo apt-get update -qq
    sudo apt-get install -y -qq "${needed_pkgs[@]}"
else
    echo "  All required packages already installed."
fi
echo ""

# ── 4. Verify Rust toolchain ─────────────────────────────────────
echo "--- Rust toolchain ---"
if command -v cargo >/dev/null 2>&1; then
    echo "  cargo: $(cargo --version)"
    echo "  rustc: $(rustc --version)"
else
    echo "  ⚠️  cargo not found — the dtolnay/rust-toolchain action will install it."
fi
echo ""

# ── 5. Disk space check ──────────────────────────────────────────
echo "--- Disk space ---"
df -h . | tail -1
echo ""

# ── 6. CPU topology summary (for affinity planning) ──────────────
echo "--- CPU topology ---"
if command -v lscpu >/dev/null 2>&1; then
    lscpu | grep -E '^(Architecture|CPU\(s\)|Thread|Core|Socket|Model name)' || true
fi
echo ""

echo "=== Setup complete ==="
echo ""
echo "For one-time runner setup (systemd CPU pinning, hyperthreading"
echo "disable, etc.), see docs/operations/self-hosted-runner-setup.md"
