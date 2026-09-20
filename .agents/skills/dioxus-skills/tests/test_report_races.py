from __future__ import annotations

import argparse
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from report import generate
from report_io import snapshot

ROOT = Path(__file__).resolve().parents[1]
SOURCE = Path(
    os.environ["DIOXUS_SOURCE"]
    if os.environ.get("CI")
    else os.environ.get("DIOXUS_SOURCE", "/root/repo/dioxus")
)


class RaceTest(unittest.TestCase):
    def test_source_race_rejected_before_publication(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / "target"
            target.mkdir()
            source = SOURCE
            initial = snapshot(source)
            changed = {**initial, "status": "M packages/core/src/lib.rs"}
            arguments = argparse.Namespace(
                source=source,
                target=target,
                json_out=root / "report.json",
                provenance=None,
                no_upstream=True,
                allow_dirty=False,
                comparison=root / "comparison",
                created_at=None,
            )
            with patch("report.snapshot", side_effect=[initial, changed]):
                with self.assertRaisesRegex(RuntimeError, "source changed"):
                    generate(arguments)
            self.assertFalse(arguments.json_out.exists())

    def test_network_failure_preserves_existing_report(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "source"
            subprocess.run(
                [
                    "git",
                    "clone",
                    "--quiet",
                    "--bare",
                    "--no-hardlinks",
                    str(SOURCE),
                    str(root / "bare"),
                ],
                check=True,
            )
            subprocess.run(
                ["git", "clone", "--quiet", str(root / "bare"), str(source)], check=True
            )
            subprocess.run(
                [
                    "git",
                    "-C",
                    str(source),
                    "remote",
                    "set-url",
                    "origin",
                    str(root / "missing"),
                ],
                check=True,
            )
            target = root / "target"
            target.mkdir()
            output = root / "report.json"
            output.write_text("retained")
            result = subprocess.run(
                [
                    sys.executable,
                    str(ROOT / "scripts/report.py"),
                    "--source",
                    str(source),
                    "--target",
                    str(target),
                    "--comparison",
                    str(root / "comparison"),
                    "--json-out",
                    str(output),
                ],
                capture_output=True,
                text=True,
                timeout=140,
            )
            self.assertEqual(result.returncode, 2, result.stderr)
            self.assertEqual(output.read_text(), "retained")


if __name__ == "__main__":
    unittest.main()
