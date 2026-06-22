#!/usr/bin/env python3
"""Benchmark regression gate checker.

Parses criterion benchmark output and compares against baselines.json.
Exits with code 1 if any benchmark regresses beyond the configured threshold.

Usage:
    python3 scripts/check_benchmark_regression.py [--baselines PATH] [--threshold PCT] [results_file]

If results_file is omitted, reads from stdin.
"""

import json
import re
import sys
import os
from pathlib import Path


def parse_time_value(s: str) -> float:
    """Parse a time string like '17.610 µs' or '1.73 ms' or '389.30 ns' into nanoseconds."""
    s = s.strip()
    # Handle Unicode micro sign and Greek mu
    s = s.replace("µs", "us").replace("μs", "us")

    match = re.match(r"^([\d.]+)\s*(ns|us|ms|s)$", s)
    if not match:
        raise ValueError(f"Cannot parse time value: {s!r}")

    value = float(match.group(1))
    unit = match.group(2)

    multipliers = {"ns": 1, "us": 1000, "ms": 1_000_000, "s": 1_000_000_000}
    return value * multipliers[unit]


def parse_criterion_output(text: str) -> dict[str, float]:
    """Parse criterion text output and return {bench_name: median_ns}.

    Criterion output lines look like:
        throughput/create_and_sign
                                time:   [17.454 µs 17.610 µs 17.796 µs]

    The middle value in the brackets is the median (point estimate).
    """
    results = {}
    current_bench = None

    for line in text.splitlines():
        line_stripped = line.strip()
        if not line_stripped:
            continue

        # ── Parse time line FIRST (before benchmark name detection) ───
        # Criterion outputs two formats depending on terminal width:
        #   Format A (wide terminal): bench name and time on separate lines
        #     finality_latency/creation_to_finality_mean
        #                         time:   [24.736 µs 24.788 µs 24.841 µs]
        #   Format B (narrow terminal): bench name and time on same line
        #     zk_proof_gen/1_tx_batch time:   [3.0882 ms 3.0902 ms 3.0924 ms]
        #
        # The regexes below handle both formats. Format A matches
        # "^time:..." and uses current_bench. Format B matches
        # "<bench_name> time:..." and extracts the bench name from the line.
        time_match = re.match(r"^time:\s+\[([^\]]+)\]", line_stripped)
        if time_match and current_bench:
            values_str = time_match.group(1).strip()
            try:
                # Extract all number-unit pairs from the bracket content
                # Handles: "17.454 µs 17.610 µs 17.796 µs" or "17.454 17.610 17.796 µs"
                pairs = re.findall(r"([\d.]+)\s*(µs|μs|us|ns|ms|s)", values_str)
                if len(pairs) >= 2:
                    # pairs[1] is the median (point estimate)
                    median_ns = parse_time_value(f"{pairs[1][0]} {pairs[1][1]}")
                    results[current_bench] = median_ns
                elif len(pairs) == 1:
                    # Only one value-unit pair — use it
                    median_ns = parse_time_value(f"{pairs[0][0]} {pairs[0][1]}")
                    results[current_bench] = median_ns
                else:
                    # Fallback: try splitting by whitespace and taking the middle
                    parts = values_str.split()
                    if len(parts) >= 3:
                        median_ns = parse_time_value(f"{parts[1]} {parts[2]}")
                        results[current_bench] = median_ns
            except (ValueError, IndexError):
                pass
            continue

        # Format B: "<bench_name> time:   [...]" (bench name and time on same line)
        # This happens when Criterion detects a narrow terminal width.
        inline_time_match = re.match(r"^([\w/]+)\s+time:\s+\[([^\]]+)\]", line_stripped)
        if inline_time_match:
            inline_bench = inline_time_match.group(1)
            values_str = inline_time_match.group(2).strip()
            try:
                pairs = re.findall(r"([\d.]+)\s*(µs|μs|us|ns|ms|s)", values_str)
                if len(pairs) >= 2:
                    median_ns = parse_time_value(f"{pairs[1][0]} {pairs[1][1]}")
                    results[inline_bench] = median_ns
                elif len(pairs) == 1:
                    median_ns = parse_time_value(f"{pairs[0][0]} {pairs[0][1]}")
                    results[inline_bench] = median_ns
                else:
                    parts = values_str.split()
                    if len(parts) >= 3:
                        median_ns = parse_time_value(f"{parts[1]} {parts[2]}")
                        results[inline_bench] = median_ns
            except (ValueError, IndexError):
                pass
            # Update current_bench too, in case the next line is a thrpt: line
            current_bench = inline_bench
            continue

        # ── Parse throughput line ──────────────────────────────────────
        # "thrpt:   [7.1900 Kelem/s 7.2500 Kelem/s 7.3100 Kelem/s]"
        thrpt_match = re.match(r"^thrpt:\s+\[([^\]]+)\]", line_stripped)
        if thrpt_match and current_bench:
            values_str = thrpt_match.group(1).strip()
            try:
                # Extract number-unit pairs for throughput
                pairs = re.findall(r"([\d.]+)\s*([KMGT]?elem/s|elements/s)", values_str)
                if len(pairs) >= 2:
                    median_val = float(pairs[1][0])
                    unit_prefix = pairs[1][1][0]  # K, M, G, T, or 'e' (for elem/s)
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
                    unit_prefix = pairs[0][1][0]
                    if unit_prefix == "K":
                        median_val *= 1000
                    elif unit_prefix == "M":
                        median_val *= 1_000_000
                    elif unit_prefix == "G":
                        median_val *= 1_000_000_000
                    elif unit_prefix == "T":
                        median_val *= 1_000_000_000_000
                    results[current_bench + "__thrpt"] = median_val
            except (ValueError, IndexError):
                pass
            continue

        # ── Detect benchmark name ─────────────────────────────────────
        # Criterion prints the bench name on its own line (no leading whitespace)
        # Skip lines that are clearly not benchmark names
        if line_stripped.startswith(("time:", "thrpt:", "[", "change:", "Found", "Smallest")):
            continue

        # Check if the line looks like a benchmark ID FIRST (before the
        # keyword filter). This prevents false negatives when a benchmark
        # name contains a word that is also a Criterion statistics keyword.
        # For example, "finality_latency/creation_to_finality_mean" contains
        # "mean" as a substring, which would cause the keyword filter to
        # skip it — leaving current_bench=None when the time: line arrives.
        # The bench-name pattern is strict (word chars + slashes only, no
        # spaces), so it won't match Criterion statistics lines like
        # "mean: 24.788 µs" or "Performing bootstrap analysis".
        if re.match(r"^[\w/]+$", line_stripped):
            current_bench = line_stripped
            continue

        # If the line doesn't look like a bench name, check if it contains
        # Criterion statistics keywords. If so, reset current_bench (the
        # next time: line will be ignored until a new bench name appears).
        if any(kw in line_stripped.lower() for kw in [
            "warming up", "collecting", "performing", "found", "outliers",
            "mean", "median", "madvise", "meas", "sample", "bootstrapped",
            "estimate", "lower", "upper", "slope",
        ]):
            current_bench = None
            continue

    return results


def load_baselines(path: str) -> dict:
    """Load baselines.json."""
    with open(path) as f:
        return json.load(f)


def check_regressions(
    measured: dict[str, float],
    baselines: dict,
    threshold_pct: float,
) -> list[dict]:
    """Check each measured benchmark against its baseline.

    Returns a list of per-benchmark reports. Each report has a "status" of:
      - "OK"        — measured, within threshold (passes gate)
      - "REGRESSION"— measured, exceeds threshold (fails gate)
      - "MISSING"   — no measurement found for this benchmark
      - "SKIP"      — measured, but "gated": false in baselines.json;
                      reported informatively but does NOT fail CI

    An empty regression-count means the gate passes. SKIP and MISSING
    do not count as regressions.
    """
    regressions = []
    bench_defs = baselines.get("benchmarks", {})

    for key, definition in bench_defs.items():
        source_bench = definition.get("source_bench", "")
        direction = definition.get("direction", "lower_is_better")
        baseline_val = definition["baseline"]
        unit = definition.get("unit", "ns")
        # "gated" defaults to true for backward compatibility. When false,
        # the benchmark is reported (SKIP status) but does not fail CI.
        gated = definition.get("gated", True)
        # Per-benchmark threshold override: if "threshold_pct" is set on the
        # benchmark definition, it takes precedence over the global threshold.
        # This allows noise-tolerant gating for benchmarks with high runner
        # variance (e.g., consensus_throughput on shared CI runners) while
        # keeping tight thresholds for stable benchmarks.
        bench_threshold = definition.get("threshold_pct", threshold_pct)

        # Find the measured value — exact match only.
        # The previous partial-match fallback (matching on the last path
        # component) caused false regressions: e.g., source_bench
        # "dag_insert/insert_latency/0" matched against measured key
        # "consensus_realistic_workload/sequential_finalization_hashmap/0"
        # because both end in "/0", producing a 399% false regression.
        #
        # If the exact source_bench key is not in the measured dict, the
        # benchmark is reported as MISSING (not matched against a wrong
        # benchmark). This is the correct behavior — a missing benchmark
        # should not trigger a false regression.
        measured_val = None

        if unit == "events_per_sec":
            # For throughput, prefer the __thrpt suffixed key (events/sec)
            thrpt_key = source_bench + "__thrpt"
            if thrpt_key in measured:
                measured_val = measured[thrpt_key]
            elif source_bench in measured:
                measured_val = measured[source_bench]
        else:
            # For latency, use the time measurement — exact match only
            if source_bench in measured:
                measured_val = measured[source_bench]

        if measured_val is None:
            regressions.append({
                "key": key,
                "status": "MISSING",
                "message": f"No measurement found for {key} (source_bench: {source_bench})",
            })
            continue

        # Calculate regression percentage.
        #
        # Sign convention (fixed 2026-06-19 per mentor review):
        #   pct_change = ((measured - baseline) / baseline) * 100
        #
        #   - POSITIVE pct_change  →  measured went UP
        #   - NEGATIVE pct_change  →  measured went DOWN
        #
        # Display interpretation (intuitive, direction-agnostic):
        #   - higher_is_better (throughput): +74.1% = improvement, -10% = regression
        #   - lower_is_better  (latency):    +5.9% = regression, -10% = improvement
        #
        # The regression *threshold check* is direction-aware:
        #   - higher_is_better: regression when measured dropped by > threshold%
        #     (i.e., pct_change < -threshold)
        #   - lower_is_better:  regression when measured climbed by > threshold%
        #     (i.e., pct_change > threshold)
        if baseline_val == 0:
            pct_change = 0
        else:
            pct_change = ((measured_val - baseline_val) / baseline_val) * 100

        if direction == "higher_is_better":
            # Throughput: regression means measured is LOWER than baseline.
            is_regression = pct_change < -bench_threshold
        else:
            # Latency: regression means measured is HIGHER than baseline.
            is_regression = pct_change > bench_threshold

        # If the benchmark is excluded from gating ("gated": false), report
        # it as SKIP regardless of whether it would have been a regression.
        # The measurement is still displayed for observability, but it does
        # NOT count toward the regression total and does NOT fail CI.
        if not gated:
            regressions.append({
                "key": key,
                "status": "SKIP",
                "baseline": baseline_val,
                "measured": measured_val,
                "pct_change": round(pct_change, 2),
                "direction": direction,
                "unit": unit,
                "description": definition.get("description", ""),
                "gated_short": definition.get("gated_short", ""),
                "gated_reason": definition.get("gated_reason", ""),
            })
            continue

        if is_regression:
            regressions.append({
                "key": key,
                "status": "REGRESSION",
                "baseline": baseline_val,
                "measured": measured_val,
                "pct_change": round(pct_change, 2),
                "direction": direction,
                "unit": unit,
                "description": definition.get("description", ""),
                "message": (
                    f"REGRESSION: {key} ({definition.get('description', '')}) "
                    f"changed by {pct_change:.1f}% (threshold: {bench_threshold}%). "
                    f"Baseline: {baseline_val} {unit}, Measured: {measured_val:.0f} {unit}"
                ),
            })
        else:
            regressions.append({
                "key": key,
                "status": "OK",
                "baseline": baseline_val,
                "measured": measured_val,
                "pct_change": round(pct_change, 2),
                "direction": direction,
                "unit": unit,
                "description": definition.get("description", ""),
            })

    return regressions


def format_ns(ns: float) -> str:
    """Format nanoseconds into a human-readable string."""
    if ns >= 1_000_000_000:
        return f"{ns / 1_000_000_000:.2f} s"
    elif ns >= 1_000_000:
        return f"{ns / 1_000_000:.2f} ms"
    elif ns >= 1_000:
        return f"{ns / 1_000:.2f} µs"
    else:
        return f"{ns:.2f} ns"


def main():
    import argparse

    parser = argparse.ArgumentParser(description="Benchmark regression gate checker")
    parser.add_argument(
        "results_file",
        nargs="?",
        default=None,
        help="File containing criterion bench output (default: stdin)",
    )
    parser.add_argument(
        "--baselines",
        default="benches/baselines.json",
        help="Path to baselines.json (default: benches/baselines.json)",
    )
    parser.add_argument(
        "--threshold",
        type=float,
        default=None,
        help="Override regression threshold (percentage, default: from baselines.json or 20)",
    )
    args = parser.parse_args()

    # Read benchmark output
    if args.results_file:
        with open(args.results_file) as f:
            text = f.read()
    else:
        text = sys.stdin.read()

    # Parse results
    measured = parse_criterion_output(text)
    if not measured:
        print("::warning::No benchmark results could be parsed from input")
        # Still try to load baselines and report missing
        baselines = load_baselines(args.baselines)
        threshold = args.threshold or baselines.get("threshold_pct", 20)
        bench_defs = baselines.get("benchmarks", {})
        print(f"\nParsed 0 benchmark measurements from output.")
        print(f"Expected {len(bench_defs)} benchmarks from baselines.json.")
        print("This may indicate the benchmark run failed or output format changed.")
        sys.exit(1)

    # Load baselines
    baselines = load_baselines(args.baselines)
    threshold = args.threshold or baselines.get("threshold_pct", 20)

    # Check regressions
    results = check_regressions(measured, baselines, threshold)

    # Print report
    print(f"\n{'='*70}")
    print(f"  Benchmark Regression Gate Report (threshold: {threshold}%)")
    print(f"  Baseline version: {baselines.get('version', 'unknown')}")
    print(f"{'='*70}\n")

    ok_count = 0
    regression_count = 0
    missing_count = 0
    skip_count = 0

    for r in results:
        status = r["status"]
        if status == "OK":
            ok_count += 1
            pct = r["pct_change"]
            direction = r["direction"]
            unit = r["unit"]
            if unit == "events_per_sec":
                baseline_str = f"{r['baseline']:.0f} ops/s"
                measured_str = f"{r['measured']:.0f} ops/s"
            else:
                baseline_str = format_ns(r["baseline"])
                measured_str = format_ns(r["measured"])
            # Arrow shows the direction of change (measured vs baseline),
            # independent of whether that direction is good or bad:
            #   ↑ = measured went UP (higher than baseline)
            #   ↓ = measured went DOWN (lower than baseline)
            #   → = no change (within 0.1%)
            # The pct_change sign already encodes good/bad via the
            # direction-aware convention documented above.
            if pct > 0.1:
                arrow = "↑"
            elif pct < -0.1:
                arrow = "↓"
            else:
                arrow = "→"
            print(f"  ✅ {r['key']}: {measured_str} (baseline: {baseline_str}, {arrow} {pct:+.1f}%)")
        elif status == "REGRESSION":
            regression_count += 1
            print(f"  ❌ {r['key']}: {r['message']}")
        elif status == "MISSING":
            missing_count += 1
            print(f"  ⚠️  {r['key']}: {r['message']}")
        elif status == "SKIP":
            skip_count += 1
            pct = r["pct_change"]
            unit = r["unit"]
            if unit == "events_per_sec":
                baseline_str = f"{r['baseline']:.0f} ops/s"
                measured_str = f"{r['measured']:.0f} ops/s"
            else:
                baseline_str = format_ns(r["baseline"])
                measured_str = format_ns(r["measured"])
            if pct > 0.1:
                arrow = "↑"
            elif pct < -0.1:
                arrow = "↓"
            else:
                arrow = "→"
            # SKIP benchmarks are displayed for observability but do NOT
            # count toward the regression total and do NOT fail CI.
            # Prefer gated_short for a readable one-line summary; the full
            # gated_reason is in baselines.json for details.
            short_reason = r.get("gated_short") or r.get("gated_reason", "") or "excluded from gate"
            print(f"  ⏭️  {r['key']}: {measured_str} (baseline: {baseline_str}, {arrow} {pct:+.1f}%) — UNGATED: {short_reason}")

    print(f"\n{'='*70}")
    print(f"  Summary: {ok_count} passed, {regression_count} regressions, {missing_count} missing, {skip_count} ungated")
    print(f"{'='*70}\n")

    if regression_count > 0:
        print("::error::Benchmark regression gate FAILED — regressions exceed threshold")
        sys.exit(1)

    if missing_count > 0:
        print("::warning::Some benchmarks were not measured — results may be incomplete")

    if skip_count > 0:
        print(f"::notice::{skip_count} benchmark(s) excluded from gate (see baselines.json 'gated': false) — reported informatively only")

    print("Benchmark regression gate PASSED ✅")
    sys.exit(0)


if __name__ == "__main__":
    main()
