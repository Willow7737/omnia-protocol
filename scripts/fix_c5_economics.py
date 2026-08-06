#!/usr/bin/env python3
"""
C5 fix for the omnia-economics crate.

Replace HashMap → BTreeMap and HashSet → BTreeSet in EconomicsState and
related types. These are serialized via to_bytes() / from_bytes() which
feed into state_snapshot() — non-deterministic iteration order would
break consensus state_root agreement.

Idempotent — safe to re-run.
"""
import re
import sys
from pathlib import Path

FILES = [
    "economics/src/governance.rs",
    "economics/src/economics_shard.rs",
    "economics/src/quota.rs",
    "economics/src/time_lock.rs",
]

REPO_ROOT = Path(__file__).resolve().parent.parent


def fix_file(rel_path: str) -> int:
    path = REPO_ROOT / rel_path
    if not path.exists():
        print(f"  skip (not found): {rel_path}")
        return 0

    original = path.read_text()
    new = original

    # Replace imports
    new = new.replace("use std::collections::{HashMap, HashSet};",
                      "use std::collections::{BTreeMap, BTreeSet};")
    new = new.replace("use std::collections::HashMap;", "use std::collections::BTreeMap;")
    new = re.sub(r"use std::collections::\{HashMap\};", "use std::collections::BTreeMap;", new)
    new = re.sub(r"use std::collections::\{HashSet\};", "use std::collections::BTreeSet;", new)

    # Replace type usages (only standalone, not in comments)
    new = new.replace("HashMap<", "BTreeMap<")
    new = new.replace("HashSet<", "BTreeSet<")
    new = new.replace("HashMap::new()", "BTreeMap::new()")
    new = new.replace("HashSet::new()", "BTreeSet::new()")

    if new == original:
        print(f"  no changes: {rel_path}")
        return 0

    path.write_text(new)
    print(f"  fixed: {rel_path}")
    return 1


def main() -> int:
    print("Fixing C5 (HashMap→BTreeMap) in omnia-economics crate...")
    for f in FILES:
        fix_file(f)
    print("Done.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
