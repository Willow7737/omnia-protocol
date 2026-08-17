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

Every failure here is loud. A `members` entry that resolves to nothing, or a
member with no `[[package]]` block in Cargo.lock, is an error rather than a
skip — silently under-syncing is the exact failure mode this script exists to
avoid, and it would surface far away as a `--locked` build failure.

Usage: scripts/sync-cargo-lock-version.py <version> [--check] [--lock PATH]
       --check exits non-zero if a change would be made, without writing.
"""
from __future__ import annotations

import argparse
import pathlib
import re
import sys

# SemVer 2.0.0, per semver.org's own reference expression. Cargo parses
# versions with the `semver` crate, so this must agree with it: numeric
# prerelease identifiers may not carry leading zeroes (`1.2.3-01` is invalid),
# and build metadata (`1.2.3+build.7`) is valid and must be accepted.
SEMVER = re.compile(
    r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)"
    r"(?:-((?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)"
    r"(?:\.(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*))*))?"
    r"(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$"
)

PACKAGE_BLOCK = re.compile(r"(?ms)^\[\[package\]\]\n.*?(?=^\[\[package\]\]|\Z)")
NAME_LINE = re.compile(r'^name\s*=\s*"([^"]+)"', re.M)
VERSION_LINE = re.compile(r'^version\s*=\s*"([^"]+)"', re.M)


def workspace_members(manifest: pathlib.Path) -> set[str]:
    """Return the package names of every member of the Cargo workspace.

    Members are read from `[workspace] members` in *manifest* and resolved to
    package names via each member's own Cargo.toml. Glob entries such as
    `crates/*` are expanded. Any entry that resolves to no readable manifest,
    or to one without a `name`, raises rather than being skipped.
    """
    text = manifest.read_text()
    block = re.search(r"^\[workspace\](.*?)(?=^\[|\Z)", text, re.S | re.M)
    if not block:
        sys.exit(f"error: no [workspace] table in {manifest}")

    # Only the `members` array — not every quoted string in the table. Sibling
    # keys such as `resolver = "2"` live here too, and reading those as paths
    # is how this silently resolved a member named "2".
    array = re.search(r"members\s*=\s*\[(.*?)\]", block.group(1), re.S)
    if not array:
        sys.exit(f"error: no [workspace] members array in {manifest}")

    root = manifest.parent
    names: set[str] = set()
    for entry in re.findall(r'"([^"]+)"', array.group(1)):
        candidates = sorted(root.glob(entry)) if "*" in entry else [root / entry]
        manifests = [c / "Cargo.toml" for c in candidates if (c / "Cargo.toml").is_file()]
        if not manifests:
            sys.exit(f"error: workspace member '{entry}' resolves to no Cargo.toml")
        for member in manifests:
            found = re.search(r'^\s*name\s*=\s*"([^"]+)"', member.read_text(), re.M)
            if not found:
                sys.exit(f"error: {member} has no package name")
            names.add(found.group(1))

    if not names:
        sys.exit("error: resolved no workspace member names")
    return names


def sync(text: str, members: set[str], version: str) -> tuple[str, list[str], set[str]]:
    """Rewrite the version of every workspace member's `[[package]]` block.

    Returns the updated lockfile text, the members whose version actually
    changed, and the members that were found in the lockfile at all.
    """
    stale: list[str] = []
    seen: set[str] = set()

    def fix_block(match: re.Match[str]) -> str:
        block = match.group(0)
        name = NAME_LINE.search(block)
        if not name or name.group(1) not in members:
            return block
        seen.add(name.group(1))

        def repl(m: re.Match[str]) -> str:
            if m.group(1) != version:
                stale.append(name.group(1))
            return f'version = "{version}"'

        return VERSION_LINE.sub(repl, block, count=1)

    return PACKAGE_BLOCK.sub(fix_block, text), stale, seen


def main() -> int:
    """Parse arguments, sync the lockfile, and report what changed."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version", help="target version, e.g. 0.1.96")
    parser.add_argument("--lock", default="Cargo.lock", type=pathlib.Path)
    parser.add_argument("--manifest", default="Cargo.toml", type=pathlib.Path)
    parser.add_argument(
        "--check",
        action="store_true",
        help="exit non-zero if a change would be made, without writing",
    )
    args = parser.parse_args()

    if not SEMVER.match(args.version):
        sys.exit(f"error: '{args.version}' is not a valid SemVer version")
    if not args.lock.is_file():
        sys.exit(f"error: {args.lock} not found")
    if not args.manifest.is_file():
        sys.exit(f"error: {args.manifest} not found")

    members = workspace_members(args.manifest)
    text = args.lock.read_text()
    out, stale, seen = sync(text, members, args.version)

    missing = members - seen
    if missing:
        sys.exit(
            f"error: {args.lock} has no [[package]] block for "
            f"{len(missing)} workspace member(s): {', '.join(sorted(missing))}"
        )

    if not stale:
        print(f"Cargo.lock already at {args.version} for all {len(members)} workspace members")
        return 0

    changed = sorted(set(stale))
    if args.check:
        print(f"Cargo.lock is stale for {len(changed)} member(s): {', '.join(changed)}")
        return 1

    args.lock.write_text(out)
    print(f"synced {len(changed)} workspace entries to {args.version}: {', '.join(changed)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
