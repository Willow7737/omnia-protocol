#!/usr/bin/env python3
"""Update IAI baselines from a benchmark run.

Parses iai-callgrind output and writes a new iai_baselines.json file.
Use this when the hot-path code has intentionally changed (e.g., new
feature, refactor) and the instruction-count baselines need to be
refreshed.

Usage:
    python3 scripts/update_iai_baselines.py [results_file]

If results_file is omitted, reads from stdin.

The script PRESERVES the existing baselines.json structure (version,
threshold, descriptions) and only updates the numeric values. New
benchmarks are added with a placeholder description.
"""

import json
import sys
from pathlib import Path

# Reuse the parser from the gate script
sys.path.insert(0, str(Path(__file__).parent))
from check_iai_regression import parse_iai_output  # noqa: E402


def main():
    import argparse

    parser = argparse.ArgumentParser(description="Update IAI baselines from benchmark output")
    parser.add_argument(
        "results_file",
        nargs="?",
        default=None,
        help="File containing iai-callgrind bench output (default: stdin)",
    )
    parser.add_argument(
        "--output",
        default="benches/iai_baselines.json",
        help="Output path (default: benches/iai_baselines.json)",
    )
    parser.add_argument(
        "--description",
        default=None,
        help="Optional commit/message describing why baselines are being updated",
    )
    args = parser.parse_args()

    if args.results_file:
        with open(args.results_file) as f:
            text = f.read()
    else:
        text = sys.stdin.read()

    measured = parse_iai_output(text)
    if not measured:
        print("::error::No IAI benchmark results could be parsed from input")
        sys.exit(2)

    # Load existing baselines to preserve structure
    output_path = Path(args.output)
    if output_path.exists():
        with open(output_path) as f:
            baselines = json.load(f)
        print(f"Updating existing baselines at {output_path}")
    else:
        baselines = {
            "_comment": "IAI-callgrind instruction-count baselines (auto-generated).",
            "_methodology": "Regenerate with: python3 scripts/update_iai_baselines.py <iai_output>",
            "version": "0.1.68",
            "threshold_pct": 2,
            "metrics": ["Instructions", "L1 Hits", "L2 Hits", "RAM Hits", "Total read+write", "Estimated Cycles"],
            "benchmarks": {},
        }
        print(f"Creating new baselines file at {output_path}")

    # Update benchmark values
    existing_benches = baselines.setdefault("benchmarks", {})
    for bench_name, metrics in measured.items():
        if bench_name not in existing_benches:
            existing_benches[bench_name] = {
                "description": f"{bench_name} (auto-added — update description manually)"
            }
            print(f"  + Added new benchmark: {bench_name}")
        for metric_name, value in metrics.items():
            old = existing_benches[bench_name].get(metric_name)
            if old is not None and old != value:
                pct = ((value - old) / old) * 100 if old else 0
                print(f"  ~ {bench_name}.{metric_name}: {old:,} -> {value:,} ({pct:+.2f}%)")
            existing_benches[bench_name][metric_name] = value

    if args.description:
        baselines["_last_update_reason"] = args.description

    # Write back with sorted keys for stable diffs
    with open(output_path, "w") as f:
        json.dump(baselines, f, indent=2)
        f.write("\n")

    print(f"\nWrote {len(measured)} benchmark baselines to {output_path}")
    print("Commit this file to update the IAI regression gate.")


if __name__ == "__main__":
    main()
