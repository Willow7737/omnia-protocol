#!/usr/bin/env python3
"""Sync workspace-member versions in Cargo.lock to a target version.

Why this exists: release-please cannot do it. Its generic TOML updater is
driven by a single JSONPath, and for Cargo.lock neither available form is safe.
`$.package[*].version` rewrites every third-party dependency (766 of them in
this lockfile), while filter expressions such as
`$.package[?(@.name=="omnia-node")].version` silently match nothing, because
the bundled jsonpath-plus runs without script evaluation. The silent one is the
more dangerous: it leaves the lockfile stale with no error, and `cargo test
--locked` in ci.yml then fails on the release PR.

Workspace members are resolved from Cargo.toml rather than matched on an
"omnia-" name prefix, so a third-party crate sharing that prefix can never be
rewritten by accident.

Usage: scripts/sync-cargo-lock-version.py <version> [--check] [--lock PATH]
       --check exits non-zero if a change would be made, without writing.
"""
from __future__ import annotations
import argparse, pathlib, re, sys

SEMVER = re.compile(r"^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$")


def workspace_members(manifest: pathlib.Path) -> set[str]:
    block = re.search(r"^\[workspace\](.*?)(?=^\[|\Z)", manifest.read_text(), re.S | re.M)
    if not block:
        sys.exit(f"error: no [workspace] table in {manifest}")
    names: set[str] = set()
    for rel in re.findall(r'"([^"]+)"', block.group(1)):
        member = manifest.parent / rel / "Cargo.toml"
        if member.is_file():
            found = re.search(r'^\s*name\s*=\s*"([^"]+)"', member.read_text(), re.M)
            if found:
                names.add(found.group(1))
    if not names:
        sys.exit("error: resolved no workspace member names")
    return names


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("version")
    ap.add_argument("--lock", default="Cargo.lock", type=pathlib.Path)
    ap.add_argument("--manifest", default="Cargo.toml", type=pathlib.Path)
    ap.add_argument("--check", action="store_true")
    args = ap.parse_args()

    if not SEMVER.match(args.version):
        sys.exit(f"error: '{args.version}' is not a semver version")
    if not args.lock.is_file():
        sys.exit(f"error: {args.lock} not found")

    members = workspace_members(args.manifest)
    text = args.lock.read_text()
    stale: list[str] = []

    def fix_block(match: re.Match[str]) -> str:
        block = match.group(0)
        name = re.search(r'^name\s*=\s*"([^"]+)"', block, re.M)
        if not name or name.group(1) not in members:
            return block
        def repl(m: re.Match[str]) -> str:
            if m.group(1) != args.version:
                stale.append(name.group(1))
            return f'version = "{args.version}"'
        return re.sub(r'^version\s*=\s*"([^"]+)"', repl, block, count=1, flags=re.M)

    out = re.sub(r"(?ms)^\[\[package\]\]\n.*?(?=^\[\[package\]\]|\Z)", fix_block, text)

    if not stale:
        print(f"Cargo.lock already at {args.version} for all {len(members)} workspace members")
        return 0
    if args.check:
        print(f"Cargo.lock is stale for {len(stale)} member(s): {', '.join(sorted(set(stale)))}")
        return 1
    args.lock.write_text(out)
    print(f"synced {len(stale)} workspace entries to {args.version}: {', '.join(sorted(set(stale)))}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
