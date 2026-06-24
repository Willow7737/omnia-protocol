#!/usr/bin/env python3
"""IAI-callgrind instruction-count regression gate.

Parses iai-callgrind output and compares against iai_baselines.json.
Unlike the criterion gate (wall-clock time, subject to CPU frequency
variance), iai-callgrind counts EXECUTED INSTRUCTIONS, which are
DETERMINISTIC for a given code path + compiler version + dependencies.
A regression here is a real, non-noise signal — the code path is doing
more work than before.

Usage:
    python3 scripts/check_iai_regression.py [results_file]

If results_file is omitted, reads from stdin.

Exit codes:
    0 = all benchmarks within threshold (or no baselines configured)
    1 = at least one benchmark regressed beyond threshold
    2 = no measurements could be parsed (bench crashed?)
"""

import json
import re
import sys
from pathlib import Path


# ANSI escape code pattern. iai-callgrind 0.13 outputs color codes
# (ESC [ ... m) even when stdout is piped. These must be stripped
# before parsing or the regex won't match metric values.
ANSI_ESCAPE = re.compile(r"\x1b\[[0-9;]*m|\x1b")


def strip_ansi(text: str) -> str:
    """Remove ANSI escape codes from text."""
    return ANSI_ESCAPE.sub("", text)


def parse_iai_output(text: str) -> dict[str, dict[str, int]]:
    """Parse iai-callgrind text output.

    Expected format:
        hot_path_iai::hot_path_group::bench_vector_clock_merge_100
          Instructions:          186112|N/A             (*********)
          L1 Hits:               241876|N/A             (*********)
          L2 Hits:                   11|N/A             (*********)
          RAM Hits:                 506|N/A             (*********)
          Total read+write:      242393|N/A             (*********)
          Estimated Cycles:      259641|N/A             (*********)

    Returns: {bench_name: {metric: count}}
    """
    results = {}
    current_bench = None

    for line in text.splitlines():
        # Strip ANSI escape codes — iai-callgrind 0.13 outputs color
        # codes (ESC[0m etc.) even when piped. Without this, the regex
        # won't match because ESC characters are embedded between the
        # metric name, the number, and the pipe.
        stripped = strip_ansi(line).strip()
        if not stripped:
            continue

        # Detect bench name lines — they contain "::" and no leading whitespace
        # from the indentation of metric lines. iai-callgrind prints the fully
        # qualified path: <crate>::<group>::<bench_fn>.
        # We use the last ::-separated component as the bench key so it
        # matches the keys in iai_baselines.json.
        if "::" in stripped and not stripped.startswith(("Instructions", "L1", "L2", "RAM", "Total", "Estimated")):
            # Heuristic: bench name lines don't contain ":" followed by a number
            # (metric lines do). Also, they shouldn't contain pipe characters.
            if ":" not in stripped or "|" not in stripped:
                current_bench = stripped.split("::")[-1]
                results[current_bench] = {}
                continue

        # Parse metric lines: "Instructions:          186112|N/A  (*********)"
        # or with a prior baseline:  "Instructions:     186112|185000  (+0.6%)"
        metric_match = re.match(
            r"^(Instructions|L1 Hits|L2 Hits|RAM Hits|Total read\+write|Estimated Cycles):\s+(\d+)\|",
            stripped,
        )
        if metric_match and current_bench:
            metric_name = metric_match.group(1)
            count = int(metric_match.group(2))
            results[current_bench][metric_name] = count

    return results


def check_iai_regressions(
    measured: dict[str, dict[str, int]],
    baselines: dict,
    threshold_pct: float,
) -> list[dict]:
    """Compare measured IAI counts against baselines.

    Returns a list of result dicts with keys:
        bench, metric, status (OK/REGRESSION/MISSING), baseline, measured, pct_change
    """
    results = []
    bench_defs = baselines.get("benchmarks", {})

    for bench_name, bench_baseline in bench_defs.items():
        measured_bench = measured.get(bench_name, {})
        if not measured_bench:
            results.append({
                "bench": bench_name,
                "metric": "ALL",
                "status": "MISSING",
                "message": f"No measurement found for IAI benchmark {bench_name}",
            })
            continue

        for metric_name, baseline_val in bench_baseline.items():
            # Skip non-numeric fields like "description"
            if not isinstance(baseline_val, (int, float)):
                continue
            if metric_name not in measured_bench:
                results.append({
                    "bench": bench_name,
                    "metric": metric_name,
                    "status": "MISSING",
                    "message": f"Metric {metric_name} missing for {bench_name}",
                })
                continue

            measured_val = measured_bench[metric_name]
            if baseline_val == 0:
                pct_change = 0.0
            else:
                pct_change = ((measured_val - baseline_val) / baseline_val) * 100

            # For instruction counts, ANY increase is a regression (more work).
            # The threshold provides tolerance for minor compiler-version drift.
            is_regression = pct_change > threshold_pct

            if is_regression:
                results.append({
                    "bench": bench_name,
                    "metric": metric_name,
                    "status": "REGRESSION",
                    "baseline": baseline_val,
                    "measured": measured_val,
                    "pct_change": round(pct_change, 2),
                    "message": (
                        f"REGRESSION: {bench_name}.{metric_name} increased by "
                        f"{pct_change:.2f}% (threshold: {threshold_pct}%). "
                        f"Baseline: {baseline_val}, Measured: {measured_val}. "
                        f"IAI counts are deterministic — this is a real code-path "
                        f"change, not noise."
                    ),
                })
            else:
                results.append({
                    "bench": bench_name,
                    "metric": metric_name,
                    "status": "OK",
                    "baseline": baseline_val,
                    "measured": measured_val,
                    "pct_change": round(pct_change, 2),
                })

    return results


def main():
    import argparse

    parser = argparse.ArgumentParser(description="IAI-callgrind instruction-count regression gate")
    parser.add_argument(
        "results_file",
        nargs="?",
        default=None,
        help="File containing iai-callgrind bench output (default: stdin)",
    )
    parser.add_argument(
        "--baselines",
        default="benches/iai_baselines.json",
        help="Path to iai_baselines.json (default: benches/iai_baselines.json)",
    )
    parser.add_argument(
        "--threshold",
        type=float,
        default=None,
        help="Override regression threshold (percentage, default: from iai_baselines.json or 2)",
    )
    args = parser.parse_args()

    # Read benchmark output
    if args.results_file:
        with open(args.results_file) as f:
            text = f.read()
        # If the primary file has no parseable results, try the stderr
        # file. iai-callgrind 0.13 may write benchmark output to stderr
        # instead of stdout (the behavior changed between versions).
        # The CI workflow redirects stderr to iai_stderr.txt.
        if not parse_iai_output(text):
            stderr_path = Path(args.results_file).parent / "iai_stderr.txt"
            if stderr_path.exists():
                with open(stderr_path) as f:
                    stderr_text = f.read()
                if parse_iai_output(stderr_text):
                    print(f"::notice::IAI results found in {stderr_path.name} instead of {args.results_file}")
                    text = stderr_text
    else:
        text = sys.stdin.read()

    # Parse results
    measured = parse_iai_output(text)
    if not measured:
        print("::error::No IAI benchmark results could be parsed from input")
        print("This may indicate the iai-callgrind run failed or the output format changed.")
        sys.exit(2)

    # Load baselines
    baselines_path = Path(args.baselines)
    if not baselines_path.exists():
        print(f"::warning::IAI baselines file not found at {baselines_path}")
        print("IAI benchmarks ran but no baselines are configured for comparison.")
        print(f"Measured {len(measured)} benchmarks. To enable the gate, run:")
        print(f"  python3 scripts/update_iai_baselines.py {args.results_file or '<input>'}")
        print(f"and commit the resulting {baselines_path}.")
        # Not a failure — just informational. The first run after adding IAI
        # will always hit this path.
        sys.exit(0)

    with open(baselines_path) as f:
        baselines = json.load(f)

    threshold = args.threshold if args.threshold is not None else baselines.get("threshold_pct", 2)

    # Check regressions
    results = check_iai_regressions(measured, baselines, threshold)

    # Print report
    print(f"\n{'='*70}")
    print(f"  IAI-Callgrind Regression Gate (threshold: {threshold}%)")
    print(f"  Baseline version: {baselines.get('version', 'unknown')}")
    print(f"  IAI counts are DETERMINISTIC — no noise tolerance needed.")
    print(f"{'='*70}\n")

    ok_count = 0
    regression_count = 0
    missing_count = 0

    for r in results:
        status = r["status"]
        if status == "OK":
            ok_count += 1
            pct = r["pct_change"]
            if pct > 0.1:
                arrow = "↑"
            elif pct < -0.1:
                arrow = "↓"
            else:
                arrow = "→"
            print(
                f"  ✅ {r['bench']}.{r['metric']}: "
                f"{r['measured']:,} (baseline: {r['baseline']:,}, {arrow} {pct:+.2f}%)"
            )
        elif status == "REGRESSION":
            regression_count += 1
            print(f"  ❌ {r['message']}")
        elif status == "MISSING":
            missing_count += 1
            print(f"  ⚠️  {r['message']}")

    print(f"\n{'='*70}")
    print(f"  Summary: {ok_count} passed, {regression_count} regressions, {missing_count} missing")
    print(f"{'='*70}\n")

    if regression_count > 0:
        print(
            "::error::IAI regression gate FAILED — instruction counts increased "
            "beyond threshold. This is a REAL regression (IAI is deterministic, "
            "not statistical noise). Investigate the code paths or regenerate "
            "baselines if the increase is intentional (e.g., new feature)."
        )
        sys.exit(1)

    if missing_count > 0:
        total = ok_count + regression_count + missing_count
        if total > 0 and missing_count * 2 > total:
            print(
                f"::error::IAI gate FAILED — {missing_count}/{total} metrics missing "
                f"(>50%). Cannot produce a meaningful verdict."
            )
            sys.exit(1)
        else:
            print(f"::warning::Some IAI metrics missing ({missing_count}/{total})")

    print("IAI regression gate PASSED ✅ (deterministic instruction counts stable)")
    sys.exit(0)


if __name__ == "__main__":
    main()
