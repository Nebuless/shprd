from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import unittest
from fcntl import LOCK_EX, LOCK_NB, flock
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCE = Path(
    os.environ["DIOXUS_SOURCE"]
    if os.environ.get("CI")
    else os.environ.get("DIOXUS_SOURCE", "/root/repo/dioxus")
)


class SafetyTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.target = self.root / "target"
        self.target.mkdir()
        artifact = self.target / "skills/test/SKILL.md"
        artifact.parent.mkdir(parents=True)
        artifact.write_text("unmapped guidance\n")
        self.output = self.root / "report.json"

    def run_report(self, *arguments: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [
                sys.executable,
                str(ROOT / "scripts/report.py"),
                "--source",
                str(SOURCE),
                "--target",
                str(self.target),
                "--json-out",
                str(self.output),
                *arguments,
            ],
            capture_output=True,
            text=True,
            timeout=140,
        )

    def test_unmapped_artifact_is_blocked_and_validates_offline(self) -> None:
        result = self.run_report("--no-upstream")
        self.assertEqual(result.returncode, 0, result.stderr)
        document = json.loads(self.output.read_bytes())
        self.assertEqual(document["validation"]["verdict"], "blocked")
        validated = subprocess.run(
            [
                sys.executable,
                str(ROOT / "scripts/validate_provenance.py"),
                "--report",
                str(self.output),
                "--target",
                str(self.target),
            ],
            capture_output=True,
            text=True,
        )
        self.assertEqual(validated.returncode, 0, validated.stderr)

    def test_symlink_and_traversal_output_rejected(self) -> None:
        victim = self.root / "victim"
        victim.write_text("unchanged")
        self.output.symlink_to(victim)
        result = self.run_report("--no-upstream")
        self.assertEqual(result.returncode, 2)
        self.assertEqual(victim.read_text(), "unchanged")
        result = self.run_report(
            "--no-upstream", "--json-out", str(self.root / "target/../bad")
        )
        self.assertEqual(result.returncode, 2)

    def test_source_output_and_overlapping_comparison_rejected(self) -> None:
        for options in (
            ("--no-upstream", "--json-out", str(SOURCE / "unsafe.json")),
            ("--comparison", str(SOURCE)),
            ("--comparison", str(self.root)),
        ):
            with self.subTest(options=options):
                self.assertEqual(self.run_report(*options).returncode, 2)

    def test_symlink_artifact_rejected(self) -> None:
        artifact = self.target / "skills/test/SKILL.md"
        artifact.unlink()
        artifact.symlink_to(SOURCE / "Cargo.toml")
        self.assertEqual(self.run_report("--no-upstream").returncode, 2)

    def test_busy_and_symlink_lock_rejected(self) -> None:
        comparison = self.root / "upstream.git"
        lock = self.root / "upstream.git.lock"
        with lock.open("w") as stream:
            flock(stream, LOCK_EX | LOCK_NB)
            self.assertEqual(
                self.run_report("--comparison", str(comparison)).returncode, 2
            )
        lock.unlink()
        lock.symlink_to(self.target / "skills/test/SKILL.md")
        self.assertEqual(self.run_report("--comparison", str(comparison)).returncode, 2)

    def test_tampered_sha_and_target_drift_fail_validation(self) -> None:
        self.assertEqual(self.run_report("--no-upstream").returncode, 0)
        command = [
            sys.executable,
            str(ROOT / "scripts/validate_provenance.py"),
            "--report",
            str(self.output),
        ]
        original = self.output.read_bytes()
        document = json.loads(original)
        document["sourceState"]["sha"] = "0" * 40
        self.output.write_text(json.dumps(document))
        self.assertNotEqual(subprocess.run(command, capture_output=True).returncode, 0)
        self.output.write_bytes(original)
        (self.target / "skills/test/SKILL.md").write_text("drift")
        self.assertNotEqual(
            subprocess.run(
                command + ["--target", str(self.target)], capture_output=True
            ).returncode,
            0,
        )

    def test_invalid_timestamp_fails_without_output(self) -> None:
        self.assertEqual(
            self.run_report("--no-upstream", "--created-at", "invalid").returncode, 2
        )
        self.assertFalse(self.output.exists())


if __name__ == "__main__":
    unittest.main()
