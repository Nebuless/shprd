#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///

# How to run: `uv run tests/test_inventory.py`

from __future__ import annotations

import subprocess
import sys
import tempfile
import tomllib
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "check_inventory.py"


class InventoryTest(unittest.TestCase):
    def test_runtime_files_are_not_authored(self) -> None:
        sys.path.insert(0, str(ROOT / "scripts"))
        from check_inventory import classify, load_policy

        rules = load_policy(ROOT / "inventory.toml")
        for path in (
            "scripts/__pycache__/sample.pyc",
            "tests/sample.pyc",
            ".qlty/sources/default",
            ".qlty/logs/run.log",
            ".omo/run/state.json",
        ):
            with self.subTest(path=path):
                matches = classify(path, rules)
                self.assertEqual(len(matches), 1)
                self.assertIn(matches[0].kind, ("cache", "workflow-state"))

    def run_inventory(
        self, root: Path, policy: Path
    ) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, str(SCRIPT), "--root", str(root), "--policy", str(policy)],
            check=False,
            capture_output=True,
            text=True,
        )

    def test_real_repository_is_fully_classified(self) -> None:
        # Given the repository and its checked-in ownership policy
        # When inventory validation runs
        result = self.run_inventory(ROOT, ROOT / "inventory.toml")

        # Then every current path has exactly one owner
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_vendor_guidance_and_provenance_have_distinct_kinds(self) -> None:
        # Given the checked-in ownership policy
        policy = tomllib.loads((ROOT / "inventory.toml").read_text(encoding="utf-8"))

        # When rules are indexed by name
        kinds = {rule["name"]: rule["kind"] for rule in policy["rules"]}

        # Then vendored content and its repository-owned lock stay distinct
        self.assertEqual(kinds["vendored-writing-guidance"], "vendored")
        self.assertEqual(kinds["vendor-provenance"], "vendor-provenance")

    def test_unclassified_file_fails(self) -> None:
        # Given a valid policy that owns only README.md
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            policy = root / "inventory.toml"
            policy.write_text(
                'version = 1\n[[rules]]\nname = "docs"\nowner = "maintainers"\nkind = "authored"\npaths = ["README.md", "inventory.toml"]\n',
                encoding="utf-8",
            )
            (root / "README.md").write_text("owned\n", encoding="utf-8")
            (root / "surprise.txt").write_text("unowned\n", encoding="utf-8")

            # When inventory validation runs
            result = self.run_inventory(root, policy)

        # Then it rejects the unclassified path
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unclassified: surprise.txt", result.stderr)

    def test_malformed_policy_fails(self) -> None:
        # Given malformed TOML
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            policy = root / "inventory.toml"
            policy.write_text("version = [\n", encoding="utf-8")

            # When inventory validation runs
            result = self.run_inventory(root, policy)

        # Then parsing fails closed
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("invalid inventory policy", result.stderr)

    def test_multiply_classified_file_fails(self) -> None:
        # Given two ownership rules covering the same path
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            policy = root / "inventory.toml"
            policy.write_text(
                'version = 1\n[[rules]]\nname = "first"\nowner = "one"\nkind = "authored"\npaths = ["inventory.toml"]\n[[rules]]\nname = "second"\nowner = "two"\nkind = "generated"\npaths = ["inventory.toml"]\n',
                encoding="utf-8",
            )

            # When inventory validation runs
            result = self.run_inventory(root, policy)

        # Then duplicate ownership fails instead of choosing silently
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("multiply classified: inventory.toml", result.stderr)

    def test_stale_cache_is_classified_without_becoming_authored(self) -> None:
        # Given arbitrary stale state under a cache root
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            policy = root / "inventory.toml"
            policy.write_text(
                'version = 1\n[[rules]]\nname = "policy"\nowner = "maintainers"\nkind = "authored"\npaths = ["inventory.toml"]\n[[rules]]\nname = "cache"\nowner = "local tooling"\nkind = "cache"\npaths = [".codegraph/"]\n',
                encoding="utf-8",
            )
            cache = root / ".codegraph"
            cache.mkdir()
            (cache / "stale.db").write_bytes(b"stale")

            # When inventory validation runs
            result = self.run_inventory(root, policy)

        # Then stale state remains cache-owned and validation succeeds
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main(argv=[sys.argv[0]])
