#!/usr/bin/env python3
"""Regression tests for sync-cargo-lock-version.py.

Standard library only — run with:

    python3 -m unittest discover -s scripts -p 'test_*.py'

Each case here is a failure that was either found in review or would have
corrupted Cargo.lock silently, which is the whole hazard this script exists to
avoid: a lockfile that looks synced, and fails much later under
`cargo test --locked`.
"""
from __future__ import annotations

import importlib.util
import pathlib
import tempfile
import unittest

_SPEC = importlib.util.spec_from_file_location(
    "sync_cargo_lock_version",
    pathlib.Path(__file__).parent / "sync-cargo-lock-version.py",
)
sclv = importlib.util.module_from_spec(_SPEC)
assert _SPEC.loader is not None
_SPEC.loader.exec_module(sclv)


class SemVerValidation(unittest.TestCase):
    """The validator must agree with Cargo's `semver` crate."""

    def test_accepts_valid_versions(self) -> None:
        for version in ("1.2.3", "0.1.96", "1.2.3-alpha.1", "1.2.3-0",
                        "1.2.3+build.7", "1.2.3-alpha+build.7"):
            with self.subTest(version=version):
                self.assertTrue(sclv.SEMVER.fullmatch(version))

    def test_rejects_leading_zero_prerelease(self) -> None:
        # Cargo's semver crate rejects this: "invalid leading zero in
        # pre-release identifier".
        self.assertFalse(sclv.SEMVER.fullmatch("1.2.3-01"))

    def test_rejects_malformed(self) -> None:
        for version in ("1.2", "v1.2.3", "01.2.3", "", "not-a-version"):
            with self.subTest(version=version):
                self.assertFalse(sclv.SEMVER.fullmatch(version))

    def test_rejects_trailing_newline(self) -> None:
        # `$` matches before a trailing newline in Python, so `.match()` would
        # accept this and write the newline straight into Cargo.lock.
        self.assertTrue(sclv.SEMVER.match("1.2.3\n"))
        self.assertFalse(sclv.SEMVER.fullmatch("1.2.3\n"))


class SourceCollision(unittest.TestCase):
    """A dependency may share a workspace member's name."""

    LOCK = (
        '[[package]]\n'
        'name = "shared-name"\n'
        'version = "0.1.0"\n'
        'source = "registry+https://github.com/rust-lang/crates.io-index"\n'
        'checksum = "deadbeef"\n'
        '\n'
        '[[package]]\n'
        'name = "shared-name"\n'
        'version = "0.1.95"\n'
        'dependencies = []\n'
    )

    def test_sourced_block_is_left_alone(self) -> None:
        out, stale, seen = sclv.sync(self.LOCK, {"shared-name"}, "0.1.96")
        # The local (path) package is bumped...
        self.assertIn('version = "0.1.96"', out)
        self.assertEqual(out.count('version = "0.1.96"'), 1)
        # ...and the registry package keeps its own version and source.
        self.assertIn('version = "0.1.0"', out)
        self.assertIn("registry+https://github.com/rust-lang/crates.io-index", out)
        self.assertEqual(stale, ["shared-name"])
        self.assertEqual(seen, {"shared-name"})


class MissingMember(unittest.TestCase):
    """A member absent from the lockfile must not read as 'already synced'."""

    def test_absent_member_is_not_seen(self) -> None:
        lock = '[[package]]\nname = "present"\nversion = "0.1.95"\n'
        _, _, seen = sclv.sync(lock, {"present", "absent"}, "0.1.96")
        self.assertEqual({"present", "absent"} - seen, {"absent"})


class WorkspaceMemberResolution(unittest.TestCase):
    """Member names come from the `members` array, not the whole table."""

    def test_ignores_sibling_keys_like_resolver(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            (root / "crate-a").mkdir()
            (root / "crate-a" / "Cargo.toml").write_text(
                '[package]\nname = "crate-a"\nversion = "0.1.0"\n'
            )
            manifest = root / "Cargo.toml"
            # `resolver = "2"` must not be read as a member path named "2".
            manifest.write_text(
                '[workspace]\nmembers = [\n    "crate-a",\n]\nresolver = "2"\n'
            )
            self.assertEqual(sclv.workspace_members(manifest), {"crate-a"})

    def test_unresolvable_member_is_fatal(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            manifest = pathlib.Path(tmp) / "Cargo.toml"
            manifest.write_text('[workspace]\nmembers = ["nope"]\n')
            with self.assertRaises(SystemExit):
                sclv.workspace_members(manifest)


class RealWorkspace(unittest.TestCase):
    """End-to-end against the repository's own Cargo.toml / Cargo.lock."""

    def setUp(self) -> None:
        self.root = pathlib.Path(__file__).resolve().parent.parent
        if not (self.root / "Cargo.lock").is_file():
            self.skipTest("Cargo.lock not present")

    def test_only_workspace_members_change(self) -> None:
        members = sclv.workspace_members(self.root / "Cargo.toml")
        text = (self.root / "Cargo.lock").read_text()
        out, _, seen = sclv.sync(text, members, "9.9.9")

        self.assertEqual(members - seen, set(), "every member must be in the lockfile")
        changed = sum(1 for a, b in zip(text.splitlines(), out.splitlines()) if a != b)
        self.assertEqual(changed, len(members), "exactly one line per member")
        self.assertEqual(out.count('version = "9.9.9"'), len(members))

    def test_idempotent(self) -> None:
        members = sclv.workspace_members(self.root / "Cargo.toml")
        text = (self.root / "Cargo.lock").read_text()
        once, _, _ = sclv.sync(text, members, "9.9.9")
        twice, stale, _ = sclv.sync(once, members, "9.9.9")
        self.assertEqual(once, twice)
        self.assertEqual(stale, [])


if __name__ == "__main__":
    unittest.main()
