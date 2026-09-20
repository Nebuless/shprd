from __future__ import annotations

import copy
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from tests.provenance_fixtures import array_at, object_at, valid_provenance

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from contract_types import ContractError, JsonObject
from report_contract import parse_report
from report_io import digest
from report_validation import validate_inspection

ROOT = Path(__file__).resolve().parents[1]


class VerifierTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.source = self.root / "source"
        subprocess.run(["git", "init", "-q", str(self.source)], check=True)
        (self.source / "source.rs").write_text("source\n")
        subprocess.run(["git", "-C", str(self.source), "add", "."], check=True)
        subprocess.run(
            [
                "git",
                "-C",
                str(self.source),
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-qm",
                "fixture",
            ],
            check=True,
        )
        self.target = self.root / "target"
        self.artifact = self.target / "skills/dioxus-core/SKILL.md"
        self.artifact.parent.mkdir(parents=True)
        self.artifact.write_text("guidance\n")
        self.ledger = valid_provenance()
        self.ledger["artifacts"] = array_at(self.ledger, "artifacts")[:1]
        self.revision = object_at(self.ledger, "artifacts", 0, "revisions", 0)
        self.revision["artifactSha256"] = digest(self.artifact.read_bytes())
        pin = object_at(self.revision, "sourcePin")
        pin.update(
            {
                "repository": str(self.source),
                "path": "source.rs",
                "sha": subprocess.check_output(
                    ["git", "-C", str(self.source), "rev-parse", "HEAD"], text=True
                ).strip(),
                "contentSha256": digest(b"source\n"),
                "locator": {"lines": {"start": 1, "end": 1}},
            }
        )
        self.mapping = self.root / "provenance.json"
        self.output = self.root / "report.json"

    def generate(self) -> JsonObject:
        self.mapping.write_text(json.dumps(self.ledger))
        result = subprocess.run(
            [
                sys.executable,
                str(ROOT / "scripts/report.py"),
                "--source",
                str(self.source),
                "--target",
                str(self.target),
                "--provenance",
                str(self.mapping),
                "--no-upstream",
                "--json-out",
                str(self.output),
            ],
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(self.output.read_bytes())

    def test_missing_inspection_rejected_when_report_id_is_downgraded(self) -> None:
        report = self.generate()
        report["reportId"] = "legacy"
        report["validation"]["checks"] = report["validation"]["checks"][1:]
        with self.assertRaises(ContractError):
            validate_inspection(report, self.target)

    def test_target_addition_rejected_when_report_was_already_generated(self) -> None:
        report = self.generate()
        (self.artifact.parent / "extra.md").write_text("extra")
        with self.assertRaises(ContractError):
            validate_inspection(report, self.target)

    def test_legacy_contract_requires_explicit_mode(self) -> None:
        report = self.generate()
        report["reportId"] = "legacy"
        report["validation"]["checks"] = report["validation"]["checks"][1:]
        with self.assertRaises(ContractError):
            parse_report(report)
        parse_report(report, legacy_only=True)
        with self.assertRaises(ContractError):
            validate_inspection(report)

    def test_noop_approved_when_successor_already_installed(self) -> None:
        predecessor = copy.deepcopy(self.revision)
        predecessor.update(
            {
                "revisionId": "core-0",
                "status": "superseded",
                "supersededBy": "core-1",
                "artifactSha256": digest(b"old"),
            }
        )
        array_at(self.ledger, "artifacts", 0, "revisions").insert(0, predecessor)
        report = self.generate()
        self.assertEqual(report["operations"][0]["verdict"], "approved")

    def test_target_removal_rejected_when_report_was_already_generated(self) -> None:
        report = self.generate()
        self.artifact.unlink()
        with self.assertRaises(ContractError):
            validate_inspection(report, self.target)

    def test_new_symlink_rejected_when_report_was_already_generated(self) -> None:
        report = self.generate()
        (self.artifact.parent / "extra").symlink_to(
            self.source, target_is_directory=True
        )
        with self.assertRaises((ContractError, RuntimeError)):
            validate_inspection(report, self.target)

    def test_update_approved_when_target_matches_supersession_predecessor(self) -> None:
        successor = copy.deepcopy(self.revision)
        successor.update(
            {"revisionId": "core-2", "artifactSha256": digest(b"new guidance\n")}
        )
        self.revision.update({"status": "superseded", "supersededBy": "core-2"})
        array_at(self.ledger, "artifacts", 0, "revisions").append(successor)
        report = self.generate()
        self.assertEqual(
            (report["operations"][0]["operation"], report["operations"][0]["verdict"]),
            ("update", "approved"),
        )

    def test_structured_conflict_recorded_when_source_identity_differs(self) -> None:
        object_at(self.revision, "sourcePin")["repository"] = (
            "https://example.com/other.git"
        )
        report = self.generate()
        self.assertEqual(len(report["conflicts"]), 1)
        self.assertEqual(report["conflicts"][0]["status"], "ambiguous")
        self.assertEqual(len(report["conflicts"][0]["candidates"]), 2)


if __name__ == "__main__":
    unittest.main()
