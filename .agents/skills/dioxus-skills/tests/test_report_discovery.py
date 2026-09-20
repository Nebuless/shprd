from __future__ import annotations

import hashlib
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from tests.provenance_fixtures import valid_provenance, object_at, array_at

ROOT = Path(__file__).resolve().parents[1]


class DiscoveryTest(unittest.TestCase):
    def test_report_discovers_each_mapping_and_is_deterministic(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "target"
            target.mkdir()
            artifact = target / "skills/dioxus-core/SKILL.md"
            artifact.parent.mkdir(parents=True)
            artifact.write_text("guidance\n")
            ledger = valid_provenance()
            ledger["artifacts"] = array_at(ledger, "artifacts")[:1]
            revision = object_at(ledger, "artifacts", 0, "revisions", 0)
            revision["artifactSha256"] = hashlib.sha256(
                artifact.read_bytes()
            ).hexdigest()
            source = Path(
                os.environ["DIOXUS_SOURCE"]
                if os.environ.get("CI")
                else os.environ.get("DIOXUS_SOURCE", "/root/repo/dioxus")
            )
            pin = object_at(revision, "sourcePin")
            pin["sha"] = subprocess.check_output(
                ["git", "-C", str(source), "rev-parse", "HEAD"], text=True
            ).strip()
            pin["contentSha256"] = hashlib.sha256(
                (source / "packages/core/src/lib.rs").read_bytes()
            ).hexdigest()
            pin["locator"] = {"lines": {"start": 1, "end": 1}}
            mapping = target / "provenance.json"
            mapping.write_text(json.dumps(ledger))
            output = Path(directory) / "report.json"
            command = [
                sys.executable,
                str(ROOT / "scripts/report.py"),
                "--source",
                str(source),
                "--target",
                str(target),
                "--provenance",
                str(mapping),
                "--no-upstream",
                "--json-out",
                str(output),
            ]
            first = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(first.returncode, 0, first.stderr)
            before = output.read_bytes()
            document = json.loads(before)
            self.assertEqual(len(document["operations"]), 1)
            self.assertEqual(document["operations"][0]["operation"], "noop")
            self.assertEqual(document["operations"][0]["verdict"], "approved")
            second = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(second.returncode, 0, second.stderr)
            self.assertEqual(before, output.read_bytes())
            self.assertEqual(first.stdout, second.stdout)
            validator = [
                sys.executable,
                str(ROOT / "scripts/validate_provenance.py"),
                "--report",
                str(output),
                "--source",
                str(source),
                "--target",
                str(target),
            ]
            checked = subprocess.run(validator, capture_output=True, text=True)
            self.assertEqual(checked.returncode, 0, checked.stderr)
            comparison = Path(directory) / "comparison.git"
            subprocess.run(
                [
                    "git",
                    "clone",
                    "--quiet",
                    "--bare",
                    "--no-hardlinks",
                    str(source),
                    str(comparison),
                ],
                check=True,
            )
            subprocess.run(
                [
                    "git",
                    "-C",
                    str(comparison),
                    "config",
                    "remote.origin.url",
                    str(source),
                ],
                check=True,
            )
            upstream_source = Path(directory) / "upstream-source"
            subprocess.run(
                [
                    "git",
                    "clone",
                    "--quiet",
                    "--no-hardlinks",
                    str(comparison),
                    str(upstream_source),
                ],
                check=True,
            )
            subprocess.run(
                [
                    "git",
                    "-C",
                    str(upstream_source),
                    "config",
                    "remote.origin.url",
                    str(source),
                ],
                check=True,
            )
            upstream_command = command.copy()
            upstream_command[upstream_command.index(str(source))] = str(upstream_source)
            upstream_command.remove("--no-upstream")
            upstream_command.extend(["--comparison", str(comparison)])
            compared = subprocess.run(upstream_command, capture_output=True, text=True)
            self.assertEqual(compared.returncode, 0, compared.stderr)
            evidence = json.loads(
                json.loads(output.read_bytes())["validation"]["checks"][0]["detail"]
            )
            self.assertEqual(
                evidence["artifacts"][0]["localSha256"],
                evidence["artifacts"][0]["upstreamSha256"],
            )
            self.assertEqual(evidence["artifacts"][0]["upstreamDiff"], "")
            (upstream_source / "packages/core/src/lib.rs").write_text(
                "changed source\n"
            )
            drifted = subprocess.run(
                upstream_command + ["--allow-dirty"], capture_output=True, text=True
            )
            self.assertEqual(drifted.returncode, 0, drifted.stderr)
            drift = json.loads(output.read_bytes())
            codes = {item["code"] for item in drift["operations"][0]["blockers"]}
            self.assertIn("stale-source", codes)
            self.assertIn("source-conflict", codes)
            self.assertTrue(drift["conflicts"])
            self.assertIn(
                "resolved-by-precedence",
                {item["status"] for item in drift["conflicts"]},
            )
            evidence = json.loads(drift["validation"]["checks"][0]["detail"])
            self.assertIn("+changed source", evidence["artifacts"][0]["localDiff"])
            (upstream_source / "packages/core/src/lib.rs").unlink()
            deleted = subprocess.run(
                upstream_command + ["--allow-dirty"], capture_output=True, text=True
            )
            self.assertEqual(deleted.returncode, 0, deleted.stderr)
            codes = {
                item["code"]
                for item in json.loads(output.read_bytes())["operations"][0]["blockers"]
            }
            self.assertIn("source-deleted", codes)
            artifact.unlink()
            added = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(added.returncode, 0, added.stderr)
            self.assertEqual(
                json.loads(output.read_bytes())["operations"][0]["operation"], "add"
            )
            artifact.write_text("changed guidance\n")
            updated = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(updated.returncode, 0, updated.stderr)
            self.assertEqual(
                json.loads(output.read_bytes())["operations"][0]["operation"], "update"
            )
            artifact.write_text("guidance\n")
            pin["contentSha256"] = "0" * 64
            mapping.write_text(json.dumps(ledger))
            rejected = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(rejected.returncode, 0, rejected.stderr)
            blocked = json.loads(output.read_bytes())["operations"][0]
            self.assertEqual(blocked["blockers"][0]["code"], "hash-mismatch")
            pin["path"] = "../outside"
            mapping.write_text(json.dumps(ledger))
            rejected = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(rejected.returncode, 2)


if __name__ == "__main__":
    unittest.main()
