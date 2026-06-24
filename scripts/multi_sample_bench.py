#!/usr/bin/env python3
"""Multi-sample benchmark runner with statistical significance testing.

Runs the criterion benchmark suite N times (default 5), collects the
measurements, computes a 95% confidence interval via the bootstrap
method, and compares against baselines.json. This addresses the
mentor's critique that single-sample point comparisons cannot
distinguish "the code is faster" from "the runner was faster today."

The statistical model:
  - Each benchmark has a true mean latency μ (unknown).
  - Each run produces a noisy measurement X_i ~ N(μ, σ²) where σ²
    captures runner variance (CPU frequency, scheduler noise, etc.).
  - We collect N samples and compute the sample mean X̄ and a 95%
    bootstrap CI [lo, hi].
  - Decision rule (lower_is_better, e.g. latency):
      REGRESSION if baseline < lo AND (X̄ - baseline) / baseline > threshold
      i.e., the entire CI is ABOVE the baseline by more than threshold%.
  - Decision rule (higher_is_better, e.g. throughput):
      REGRESSION if baseline > hi AND (baseline - X̄) / baseline > threshold
      i.e., the entire CI is BELOW the baseline by more than threshold%.
  - If the CI overlaps the baseline, the result is INCONCLUSIVE — we
    cannot say with 95% confidence that there was a change. The gate
    PASSES (no regression detected) but emits a ::warning::.

Usage:
    python3 scripts/multi_sample_bench.py --runs 5 --bench throughput

This script WRAPS cargo bench — it does not parse pre-existing output.
It runs the benches itself, so it must be invoked in CI instead of the
raw `cargo bench` command when multi-sample gating is desired.
"""

import argparse
import json
import re
import subprocess
import sys
import statistics
from pathlib import Path


# ANSI escape code pattern. Both criterion and iai-callgrind output
# color codes even when piped. These must be stripped before parsing.
ANSI_ESCAPE = re.compile(r"\x1b\[[0-9;]*m|\x1b")


def strip_ansi(text: str) -> str:
    """Remove ANSI escape codes from text."""
    return ANSI_ESCAPE.sub("", text)


def parse_time_value(s: str) -> float:
    """Parse a time string like '17.610 µs' into nanoseconds."""
    s = s.strip().replace("µs", "us").replace("μs", "us")
    match = re.match(r"^([\d.]+)\s*(ns|us|ms|s)$", s)
    if not match:
        raise ValueError(f"Cannot parse time value: {s!r}")
    value = float(match.group(1))
    unit = match.group(2)
    multipliers = {"ns": 1, "us": 1000, "ms": 1_000_000, "s": 1_000_000_000}
    return value * multipliers[unit]


def parse_criterion_run(text: str) -> dict[str, float]:
    """Parse a single criterion run's output.

    Returns {bench_name: median_ns} or {bench_name: throughput} for
    throughput benchmarks (key suffixed with __thrpt).
    """
    results = {}
    current_bench = None

    for line in text.splitlines():
        stripped = strip_ansi(line).strip()
        if not stripped:
            continue

        # Time line
        time_match = re.match(r"^time:\s+\[([^\]]+)\]", stripped)
        if time_match and current_bench:
            values_str = time_match.group(1).strip()
            pairs = re.findall(r"([\d.]+)\s*(µs|μs|us|ns|ms|s)", values_str)
            if len(pairs) >= 2:
                median_ns = parse_time_value(f"{pairs[1][0]} {pairs[1][1]}")
                results[current_bench] = median_ns
            elif len(pairs) == 1:
                median_ns = parse_time_value(f"{pairs[0][0]} {pairs[0][1]}")
                results[current_bench] = median_ns
            continue

        # Throughput line
        thrpt_match = re.match(r"^thrpt:\s+\[([^\]]+)\]", stripped)
        if thrpt_match and current_bench:
            values_str = thrpt_match.group(1).strip()
            # Matches "12.583 Melem/s" or "12583 elem/s" or "12.583 Kitems/s"
            # Group 1: number, Group 2: optional prefix (K/M/G/T), Group 3: unit
            pairs = re.findall(r"([\d.]+)\s*([KMGT])?(?:i)?(?:elem|items)/s", values_str)
            if len(pairs) >= 2:
                median_val = float(pairs[1][0])
                unit_prefix = pairs[1][1] or ""
                if unit_prefix == "K":
                    median_val *= 1000
                elif unit_prefix == "M":
                    median_val *= 1_000_000
                elif unit_prefix == "G":
                    median_val *= 1_000_000_000
                elif unit_prefix == "T":
                    median_val *= 1_000_000_000_000
                results[current_bench + "__thrpt"] = median_val
            elif len(pairs) == 1:
                median_val = float(pairs[0][0])
                unit_prefix = pairs[0][1] or ""
                if unit_prefix == "K":
                    median_val *= 1000
                elif unit_prefix == "M":
                    median_val *= 1_000_000
                elif unit_prefix == "G":
                    median_val *= 1_000_000_000
                elif unit_prefix == "T":
                    median_val *= 1_000_000_000_000
                results[current_bench + "__thrpt"] = median_val
            continue

        # Bench name line
        if re.match(r"^[\w/]+$", stripped):
            current_bench = stripped
            continue

    return results


def bootstrap_ci(samples: list[float], confidence: float = 0.95, iterations: int = 10000) -> tuple[float, float]:
    """Compute a bootstrap confidence interval for the mean.

    The bootstrap is distribution-free (no normality assumption) and
    works well for small sample sizes (N >= 5). We resample with
    replacement `iterations` times, compute the mean of each resample,
    and return the (lo, hi) percentile bounds.
    """
    import random

    if len(samples) < 2:
        return (samples[0], samples[0]) if samples else (0, 0)

    n = len(samples)
    means = []
    for _ in range(iterations):
        resample = [random.choice(samples) for _ in range(n)]
        means.append(statistics.mean(resample))

    means.sort()
    alpha = 1 - confidence
    lo_idx = int(alpha / 2 * iterations)
    hi_idx = int((1 - alpha / 2) * iterations)
    return (means[lo_idx], means[hi_idx])


def run_bench_multiple_times(
    bench_name: str,
    runs: int,
    features: str | None = None,
    extra_args: list[str] | None = None,
) -> list[dict[str, float]]:
    """Run `cargo bench` N times, returning parsed results for each run."""
    all_runs = []
    cmd = ["cargo", "bench", "-p", "omnia-benches", "--bench", bench_name]
    if features:
        cmd.extend(["--features", features])
    if extra_args:
        cmd.extend(extra_args)

    for i in range(runs):
        print(f"\n--- Run {i+1}/{runs}: {' '.join(cmd)} ---", file=sys.stderr)
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=600)
        if result.returncode != 0:
            print(f"::warning::Run {i+1} failed (exit {result.returncode}): {result.stderr[:500]}", file=sys.stderr)
            continue
        parsed = parse_criterion_run(result.stdout)
        if parsed:
            all_runs.append(parsed)
            print(f"  Parsed {len(parsed)} benchmarks from run {i+1}", file=sys.stderr)

    return all_runs


def aggregate_samples(
    all_runs: list[dict[str, float]],
) -> dict[str, dict]:
    """Aggregate multi-run samples per benchmark.

    Returns {bench_name: {samples: [...], mean: float, ci_lo: float, ci_hi: float, n: int}}
    """
    # Collect samples per benchmark
    per_bench_samples: dict[str, list[float]] = {}
    for run in all_runs:
        for bench, value in run.items():
            per_bench_samples.setdefault(bench, []).append(value)

    aggregated = {}
    for bench, samples in per_bench_samples.items():
        if not samples:
            continue
        mean = statistics.mean(samples)
        if len(samples) >= 2:
            ci_lo, ci_hi = bootstrap_ci(samples)
        else:
            ci_lo = ci_hi = mean
        stdev = statistics.stdev(samples) if len(samples) >= 2 else 0
        aggregated[bench] = {
            "samples": samples,
            "mean": mean,
            "ci_lo": ci_lo,
            "ci_hi": ci_hi,
            "stdev": stdev,
            "n": len(samples),
        }

    return aggregated


def check_regressions_with_significance(
    aggregated: dict[str, dict],
    baselines: dict,
    threshold_pct: float,
) -> list[dict]:
    """Check regressions using CI-overlap significance test.

    Decision rules (see module docstring):
      - lower_is_better: REGRESSION if baseline < ci_lo AND mean > baseline * (1 + threshold/100)
      - higher_is_better: REGRESSION if baseline > ci_hi AND mean < baseline * (1 - threshold/100)
      - INCONCLUSIVE if CI overlaps baseline (cannot reject null hypothesis of no change)
    """
    results = []
    bench_defs = baselines.get("benchmarks", {})

    for key, definition in bench_defs.items():
        source_bench = definition.get("source_bench", key)
        direction = definition.get("direction", "lower_is_better")
        unit = definition.get("unit", "ns")
        baseline_val = definition.get("baseline")
        bench_threshold = definition.get("threshold_pct", threshold_pct)
        gated = definition.get("gated", True)

        if baseline_val is None:
            continue

        # Find the measured value. For throughput benchmarks, check the
        # __thrpt suffixed key first.
        measured_key = source_bench
        if unit == "events_per_sec":
            thrpt_key = source_bench + "__thrpt"
            if thrpt_key in aggregated:
                measured_key = thrpt_key

        # If neither the source_bench nor its __thrpt variant is in the
        # measured results, SKIP this baseline entirely. This happens
        # when running a single bench file (e.g., --bench throughput)
        # that only produces some benchmark groups. We should NOT report
        # these as MISSING — they're simply not applicable to this run.
        if measured_key not in aggregated:
            continue

        agg = aggregated[measured_key]
        mean = agg["mean"]
        ci_lo = agg["ci_lo"]
        ci_hi = agg["ci_hi"]
        n = agg["n"]
        stdev = agg["stdev"]

        if baseline_val == 0:
            pct_change = 0
        else:
            pct_change = ((mean - baseline_val) / baseline_val) * 100

        if not gated:
            results.append({
                "key": key,
                "status": "SKIP",
                "baseline": baseline_val,
                "mean": mean,
                "ci_lo": ci_lo,
                "ci_hi": ci_hi,
                "pct_change": round(pct_change, 2),
                "n": n,
                "stdev": stdev,
                "direction": direction,
                "unit": unit,
            })
            continue

        # Significance test: does the CI overlap the baseline?
        ci_overlaps_baseline = (ci_lo <= baseline_val <= ci_hi)

        if direction == "higher_is_better":
            # Throughput: regression if mean dropped by > threshold%
            is_regression = pct_change < -bench_threshold
            # But only call it a regression if the CI doesn't overlap the
            # baseline (i.e., we're confident the drop is real)
            if is_regression and ci_overlaps_baseline:
                status = "INCONCLUSIVE"
            elif is_regression:
                status = "REGRESSION"
            else:
                status = "OK"
        else:
            # Latency: regression if mean climbed by > threshold%
            is_regression = pct_change > bench_threshold
            if is_regression and ci_overlaps_baseline:
                status = "INCONCLUSIVE"
            elif is_regression:
                status = "REGRESSION"
            else:
                status = "OK"

        results.append({
            "key": key,
            "status": status,
            "baseline": baseline_val,
            "mean": mean,
            "ci_lo": ci_lo,
            "ci_hi": ci_hi,
            "pct_change": round(pct_change, 2),
            "n": n,
            "stdev": stdev,
            "direction": direction,
            "unit": unit,
            "threshold": bench_threshold,
        })

    return results


def format_ns(ns: float) -> str:
    if ns >= 1_000_000_000:
        return f"{ns / 1_000_000_000:.2f} s"
    elif ns >= 1_000_000:
        return f"{ns / 1_000_000:.2f} ms"
    elif ns >= 1_000:
        return f"{ns / 1_000:.2f} µs"
    else:
        return f"{ns:.2f} ns"


def main():
    parser = argparse.ArgumentParser(
        description="Multi-sample benchmark runner with statistical significance testing"
    )
    parser.add_argument(
        "--runs",
        type=int,
        default=5,
        help="Number of times to run each benchmark (default: 5)",
    )
    parser.add_argument(
        "--bench",
        default="throughput",
        help="Criterion bench to run (default: throughput)",
    )
    parser.add_argument(
        "--features",
        default=None,
        help="Cargo features to enable (e.g., 'full' for ZK benches)",
    )
    parser.add_argument(
        "--baselines",
        default="benches/baselines.json",
        help="Path to baselines.json",
    )
    parser.add_argument(
        "--threshold",
        type=float,
        default=None,
        help="Override regression threshold (percentage)",
    )
    parser.add_argument(
        "--output",
        default="multi_sample_results.txt",
        help="Output file for the aggregated results",
    )
    parser.add_argument(
        "--extra-args",
        nargs="*",
        default=None,
        help="Extra arguments to pass to cargo bench (e.g., bench filter)",
    )
    parser.add_argument(
        "--filter-prefix",
        default=None,
        help="Only check benchmarks whose key starts with this prefix "
        "(e.g., 'event_' or 'graph_'). Use when running a single bench "
        "file to avoid false 'missing' reports for benchmarks that "
        "weren't run.",
    )
    args = parser.parse_args()

    # Load baselines
    baselines_path = Path(args.baselines)
    if not baselines_path.exists():
        print(f"::error::Baselines file not found: {baselines_path}")
        sys.exit(2)
    with open(baselines_path) as f:
        baselines = json.load(f)
    threshold = args.threshold if args.threshold is not None else baselines.get("threshold_pct", 10)

    # Apply prefix filter to baselines before checking. This prevents
    # false "missing" reports when running a single bench file (e.g.,
    # --bench throughput only produces event_creation/, graph_insertion/,
    # etc. — it should not be checked against consensus_throughput or
    # finality_latency baselines from baseline_bench.rs).
    if args.filter_prefix:
        original_count = len(baselines.get("benchmarks", {}))
        baselines["benchmarks"] = {
            k: v for k, v in baselines.get("benchmarks", {}).items()
            if k.startswith(args.filter_prefix)
        }
        filtered_count = len(baselines["benchmarks"])
        print(f"  Filter: only checking benchmarks starting with '{args.filter_prefix}' "
              f"({filtered_count}/{original_count} baselines)")

    print(f"\n{'='*70}")
    print(f"  Multi-Sample Benchmark Gate (N={args.runs} runs, 95% bootstrap CI)")
    print(f"  Threshold: {threshold}% (per-benchmark overrides apply)")
    print(f"{'='*70}\n")

    # Run benchmarks N times
    all_runs = run_bench_multiple_times(
        args.bench,
        args.runs,
        features=args.features,
        extra_args=args.extra_args,
    )

    if not all_runs:
        print("::error::No benchmark runs produced parseable output")
        sys.exit(2)

    print(f"\nCollected {len(all_runs)} successful runs out of {args.runs} attempts.\n")

    # Aggregate samples
    aggregated = aggregate_samples(all_runs)

    # Check regressions with significance test
    results = check_regressions_with_significance(aggregated, baselines, threshold)

    # Print report
    ok_count = 0
    regression_count = 0
    missing_count = 0
    inconclusive_count = 0
    skip_count = 0

    report_lines = []

    for r in results:
        status = r["status"]
        if status == "OK":
            ok_count += 1
            pct = r["pct_change"]
            unit = r["unit"]
            if unit == "events_per_sec":
                baseline_str = f"{r['baseline']:.0f} ops/s"
                mean_str = f"{r['mean']:.0f} ops/s"
                ci_str = f"[{r['ci_lo']:.0f}, {r['ci_hi']:.0f}]"
            else:
                baseline_str = format_ns(r["baseline"])
                mean_str = format_ns(r["mean"])
                ci_str = f"[{format_ns(r['ci_lo'])}, {format_ns(r['ci_hi'])}]"
            cv = (r["stdev"] / r["mean"] * 100) if r["mean"] else 0
            if pct > 0.1:
                arrow = "↑"
            elif pct < -0.1:
                arrow = "↓"
            else:
                arrow = "→"
            line = (
                f"  ✅ {r['key']}: {mean_str} (baseline: {baseline_str}, {arrow} {pct:+.1f}%, "
                f"CI: {ci_str}, CV: {cv:.1f}%, N={r['n']})"
            )
            print(line)
            report_lines.append(line)
        elif status == "REGRESSION":
            regression_count += 1
            unit = r["unit"]
            if unit == "events_per_sec":
                baseline_str = f"{r['baseline']:.0f} ops/s"
                mean_str = f"{r['mean']:.0f} ops/s"
                ci_str = f"[{r['ci_lo']:.0f}, {r['ci_hi']:.0f}]"
            else:
                baseline_str = format_ns(r["baseline"])
                mean_str = format_ns(r["mean"])
                ci_str = f"[{format_ns(r['ci_lo'])}, {format_ns(r['ci_hi'])}]"
            line = (
                f"  ❌ {r['key']}: REGRESSION — mean {mean_str} vs baseline {baseline_str} "
                f"({r['pct_change']:+.1f}%, threshold {r['threshold']}%). "
                f"95% CI {ci_str} does NOT overlap baseline — this is a statistically "
                f"significant regression (N={r['n']})."
            )
            print(line)
            report_lines.append(line)
        elif status == "INCONCLUSIVE":
            inconclusive_count += 1
            unit = r["unit"]
            if unit == "events_per_sec":
                baseline_str = f"{r['baseline']:.0f} ops/s"
                mean_str = f"{r['mean']:.0f} ops/s"
                ci_str = f"[{r['ci_lo']:.0f}, {r['ci_hi']:.0f}]"
            else:
                baseline_str = format_ns(r["baseline"])
                mean_str = format_ns(r["mean"])
                ci_str = f"[{format_ns(r['ci_lo'])}, {format_ns(r['ci_hi'])}]"
            cv = (r["stdev"] / r["mean"] * 100) if r["mean"] else 0
            line = (
                f"  ❓ {r['key']}: INCONCLUSIVE — mean {mean_str} ({r['pct_change']:+.1f}%) "
                f"exceeds {r['threshold']}% threshold BUT 95% CI {ci_str} overlaps baseline "
                f"{baseline_str}. Cannot reject the null hypothesis of no change. "
                f"Runner variance (CV: {cv:.1f}%) is too high — consider more runs or a "
                f"self-hosted runner. Gate PASSES but treat the number with suspicion. (N={r['n']})"
            )
            print(line)
            report_lines.append(line)
        elif status == "MISSING":
            missing_count += 1
            line = f"  ⚠️  {r['key']}: {r['message']}"
            print(line)
            report_lines.append(line)
        elif status == "SKIP":
            skip_count += 1
            line = f"  ⏭️  {r['key']}: UNGATED (see baselines.json)"
            print(line)
            report_lines.append(line)

    print(f"\n{'='*70}")
    print(
        f"  Summary: {ok_count} passed, {regression_count} regressions, "
        f"{inconclusive_count} inconclusive, {missing_count} missing, {skip_count} ungated"
    )
    print(f"{'='*70}\n")

    # Write report to file
    with open(args.output, "w") as f:
        f.write(f"Multi-Sample Benchmark Gate Report (N={args.runs}, 95% bootstrap CI)\n")
        f.write(f"Threshold: {threshold}%\n")
        f.write(f"Runs collected: {len(all_runs)}/{args.runs}\n\n")
        for line in report_lines:
            f.write(line + "\n")
        f.write(f"\nSummary: {ok_count} passed, {regression_count} regressions, ")
        f.write(f"{inconclusive_count} inconclusive, {missing_count} missing, {skip_count} ungated\n")

    # Exit decision
    if regression_count > 0:
        print(
            "::error::Multi-sample gate FAILED — statistically significant "
            "regressions detected (95% CI does not overlap baseline)."
        )
        sys.exit(1)

    if inconclusive_count > 0:
        print(
            f"::warning::{inconclusive_count} benchmark(s) INCONCLUSIVE — mean exceeds "
            f"threshold but CI overlaps baseline. Runner variance is too high. "
            f"Consider: (1) increasing --runs to 10+, (2) using a self-hosted runner, "
            f"(3) relying on the IAI gate (deterministic instruction counts) instead."
        )

    if missing_count > 0:
        total = ok_count + regression_count + missing_count + inconclusive_count + skip_count
        if total > 0 and missing_count * 2 > total:
            print(f"::error::Gate FAILED — {missing_count}/{total} benchmarks missing (>50%)")
            sys.exit(1)

    print("Multi-sample benchmark gate PASSED ✅")
    sys.exit(0)


if __name__ == "__main__":
    main()
