from __future__ import annotations
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REPORT = ROOT / "scripts/report.py"
VALIDATE = ROOT / "scripts/validate_provenance.py"


class ReportSliceTest(unittest.TestCase):
    def run_report(self, source, *extra):
        return subprocess.run(
            [
                sys.executable,
                str(REPORT),
                "--source",
                str(source),
                "--target",
                str(source.parent / "target"),
                *extra,
            ],
            cwd=ROOT,
            text=True,
            capture_output=True,
        )

    def test_missing_source_fails(self):
        r = self.run_report(ROOT / "does-not-exist")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("missing source", r.stderr)

    def test_dirty_source_policy_fails_and_does_not_edit(self):
        with tempfile.TemporaryDirectory() as d:
            s = Path(d) / "src"
            subprocess.run(["git", "init", "-q", str(s)], check=True)
            subprocess.run(
                ["git", "-C", str(s), "config", "user.email", "x@y"], check=True
            )
            subprocess.run(
                ["git", "-C", str(s), "config", "user.name", "x"], check=True
            )
            (s / "x").write_text("x")
            subprocess.run(["git", "-C", str(s), "add", "x"], check=True)
            subprocess.run(["git", "-C", str(s), "commit", "-qm", "x"], check=True)
            (s / "y").write_text("dirty")
            r = self.run_report(s)
            self.assertNotEqual(r.returncode, 0)
            self.assertIn("dirty", r.stderr)

    def test_malformed_and_traversal_reports_fail_offline(self):
        with tempfile.TemporaryDirectory() as d:
            bad = Path(d) / "bad.json"
            bad.write_text("{}")
            r = subprocess.run(
                [sys.executable, str(VALIDATE), str(bad), "--kind", "report"],
                cwd=ROOT,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(r.returncode, 0)
            bad.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "reportId": "x",
                        "createdAt": "2026-01-01T00:00:00Z",
                        "sourceState": {
                            "repository": "x",
                            "ref": "HEAD",
                            "sha": "0" * 40,
                            "dirty": False,
                        },
                        "sourcePrecedence": [
                            "local-source",
                            "local-tests-examples",
                            "local-agents",
                            "local-architecture",
                            "upstream-source",
                            "public-docs",
                        ],
                        "operations": [
                            {
                                "operation": "add",
                                "artifactPath": "../escape",
                                "artifactClass": "dioxus-authored",
                                "beforeSha256": None,
                                "afterSha256": "0" * 64,
                                "provenanceRevisionId": "x",
                                "verdict": "approved",
                                "blockers": [],
                            }
                        ],
                        "conflicts": [],
                        "validation": {
                            "verdict": "approved",
                            "checks": [{"name": "x", "verdict": "pass", "detail": "x"}],
                        },
                    }
                )
            )
            r = subprocess.run(
                [sys.executable, str(VALIDATE), str(bad), "--kind", "report"],
                cwd=ROOT,
                text=True,
                capture_output=True,
            )
            self.assertNotEqual(r.returncode, 0)
            self.assertIn("safe", r.stderr)

    def test_fixture_report_is_human_and_json(self):
        with tempfile.TemporaryDirectory() as d:
            s = Path(d) / "src"
            subprocess.run(["git", "init", "-q", str(s)], check=True)
            subprocess.run(
                ["git", "-C", str(s), "config", "user.email", "x@y"], check=True
            )
            subprocess.run(
                ["git", "-C", str(s), "config", "user.name", "x"], check=True
            )
            (s / "x").write_text("x")
            subprocess.run(["git", "-C", str(s), "add", "x"], check=True)
            subprocess.run(["git", "-C", str(s), "commit", "-qm", "x"], check=True)
            out = Path(d) / "report.json"
            r = self.run_report(s, "--no-upstream", "--json-out", str(out))
            self.assertEqual(r.returncode, 0, r.stderr)
            self.assertEqual(json.loads(out.read_text())["schemaVersion"], 1)
            self.assertIn("Source:", r.stdout)


if __name__ == "__main__":
    unittest.main()
