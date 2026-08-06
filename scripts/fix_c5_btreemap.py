#!/usr/bin/env python3
"""
Replace HashMap → BTreeMap and HashSet → BTreeSet in shard state types.

This is the C5 audit fix: HashMap iteration order is non-deterministic,
which causes state_snapshot() to produce different bytes on different
nodes for the same logical state — a consensus-safety bug.

Only touches the state.rs files in shards/src/, not the op/validator
files (which don't serialize to state snapshots).

This script is idempotent — running it twice is a no-op.
"""
import re
import sys
from pathlib import Path

STATE_FILES = [
    "shards/src/physical/state.rs",
    "shards/src/biological/state.rs",
    "shards/src/computational/state.rs",
]

REPO_ROOT = Path(__file__).resolve().parent.parent

def fix_file(rel_path: str) -> int:
    """Apply replacements to a single file. Returns number of changes."""
    path = REPO_ROOT / rel_path
    if not path.exists():
        print(f"  skip (not found): {rel_path}")
        return 0

    original = path.read_text()
    new = original

    # Replace imports
    new = new.replace("use std::collections::HashMap;", "use std::collections::BTreeMap;")
    new = new.replace("use std::collections::{HashMap, HashSet};",
                      "use std::collections::{BTreeMap, BTreeSet};")
    new = new.replace("use std::collections::HashSet;", "use std::collections::BTreeSet;")

    # Replace type usages
    new = new.replace("HashMap<", "BTreeMap<")
    new = new.replace("HashSet<", "BTreeSet<")
    new = new.replace("HashMap::new()", "BTreeMap::new()")
    new = new.replace("HashSet::new()", "BTreeSet::new()")

    if new == original:
        print(f"  no changes: {rel_path}")
        return 0

    path.write_text(new)
    changes = original.count("HashMap") + original.count("HashSet") - new.count("HashMap") - new.count("HashSet")
    print(f"  fixed ({changes} replacements): {rel_path}")
    return changes


def main() -> int:
    print("Replacing HashMap→BTreeMap, HashSet→BTreeSet in shard state files...")
    total = 0
    for f in STATE_FILES:
        total += fix_file(f)
    print(f"Done. Total replacements: {total}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
