from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from tests.test_apply import ApplyTest, ROOT
from report_io import digest


class LedgerApplyTest(unittest.TestCase):
    setUp = ApplyTest.setUp
    generate = ApplyTest.generate
    invoke = ApplyTest.invoke

    def test_update_when_ledger_includes_repository_config_noops(self) -> None:
        ledger = json.loads((ROOT / "generated/provenance.json").read_bytes())
        retained = {}
        for artifact in ledger["artifacts"]:
            if artifact["artifactClass"] != "repository-config":
                continue
            path = artifact["artifactPath"]
            if path == "SKILL.md":
                artifact["artifactPath"] = path = "docs/router.md"
            content = b"retained configuration\n"
            destination = self.target / path
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(content)
            for revision in artifact["revisions"]:
                revision["artifactSha256"] = digest(content)
            self.ledger["artifacts"].append(artifact)
            retained[path] = content
        (self.target / "generated/provenance.json").write_text(json.dumps(self.ledger))
        subprocess.run(["git", "-C", str(self.target), "add", "."], check=True)
        subprocess.run(
            [
                "git",
                "-C",
                str(self.target),
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-qm",
                "ledger fixture",
            ],
            check=True,
        )
        self.generate()

        result = self.invoke("--confirm")

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.artifact.read_bytes(), b"new\n")
        for path, content in retained.items():
            self.assertEqual((self.target / path).read_bytes(), content)
        self.assertEqual(self.invoke("--confirm").returncode, 0)

    def test_real_full_ledger_update_preserves_non_authored_files(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "dioxus-skills"
            shutil.copytree(
                ROOT,
                target,
                ignore=shutil.ignore_patterns(
                    ".git",
                    ".codegraph",
                    ".qlty",
                    ".ruff_cache",
                    ".omo",
                    "__pycache__",
                ),
            )
            source = Path(
                os.environ["DIOXUS_SOURCE"]
                if os.environ.get("CI")
                else os.environ.get("DIOXUS_SOURCE", "/root/repo/dioxus")
            )
            mapping = target / "generated/provenance.json"
            subprocess.run(
                ["mise", "trust", str(target / "mise.toml")],
                check=True,
                capture_output=True,
            )
            ledger = json.loads(mapping.read_bytes())
            artifact = next(
                item
                for item in ledger["artifacts"]
                if item["artifactPath"]
                == "skills/dioxus-core/references/source-authority.md"
            )
            previous = artifact["revisions"][-1]
            content = (
                target / artifact["artifactPath"]
            ).read_bytes() + b"\nFixture-reviewed update.\n"
            successor = {
                **previous,
                "revisionId": "fixture-update",
                "artifactSha256": digest(content),
            }
            previous.update(status="superseded", supersededBy="fixture-update")
            artifact["revisions"].append(successor)
            mapping.write_text(json.dumps(ledger))
            payloads = target / "generated/apply-blobs"
            payloads.mkdir(exist_ok=True)
            (payloads / digest(content)).write_bytes(content)
            retained = {
                item["artifactPath"]: (target / item["artifactPath"]).read_bytes()
                for item in ledger["artifacts"]
                if item["artifactClass"] != "dioxus-authored"
            }
            subprocess.run(["git", "init", "-q", str(target)], check=True)
            subprocess.run(["git", "-C", str(target), "add", "."], check=True)
            subprocess.run(
                [
                    "git",
                    "-C",
                    str(target),
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.com",
                    "commit",
                    "-qm",
                    "real ledger fixture",
                ],
                check=True,
            )
            output = Path(directory) / "report.json"
            report = subprocess.run(
                [
                    sys.executable,
                    str(ROOT / "scripts/report.py"),
                    "--source",
                    str(source),
                    "--target",
                    str(target),
                    "--no-upstream",
                    "--json-out",
                    str(output),
                ],
                capture_output=True,
                text=True,
            )
            self.assertEqual(report.returncode, 0, report.stderr)
            document = json.loads(output.read_bytes())
            self.assertEqual(len(document["operations"]), len(ledger["artifacts"]))
            self.assertEqual(document["validation"]["verdict"], "approved")
            validated = subprocess.run(
                [
                    sys.executable,
                    str(ROOT / "scripts/validate_provenance.py"),
                    "--report",
                    str(output),
                    "--target",
                    str(target),
                    "--source",
                    str(source),
                ],
                capture_output=True,
                text=True,
            )
            self.assertEqual(validated.returncode, 0, validated.stderr)
            checked = subprocess.run(
                [
                    "node",
                    str(ROOT / "scripts/validate-skills.mjs"),
                    "--root",
                    str(target),
                ],
                capture_output=True,
                text=True,
            )
            self.assertEqual(checked.returncode, 0, checked.stderr)

            applied = subprocess.run(
                [
                    sys.executable,
                    str(ROOT / "scripts/apply.py"),
                    "--report",
                    str(output),
                    "--confirm",
                ],
                cwd=target,
                env={**os.environ, "DIOXUS_SOURCE": str(source)},
                capture_output=True,
                text=True,
            )

            self.assertEqual(applied.returncode, 0, applied.stderr)
            self.assertEqual((target / artifact["artifactPath"]).read_bytes(), content)
            for path, original in retained.items():
                self.assertEqual((target / path).read_bytes(), original)
            changed = subprocess.check_output(
                ["git", "-C", str(target), "diff", "--name-only"], text=True
            ).splitlines()
            self.assertEqual(changed, [artifact["artifactPath"]])
