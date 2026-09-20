from __future__ import annotations

import copy
import json
import subprocess
import sys
import unittest
from unittest.mock import patch

from tests.test_report_verifier import VerifierTest, ROOT
from contract_types import canonical_json
from report_io import digest, snapshot


class ApplyTest(unittest.TestCase):
    generate = VerifierTest.generate

    def setUp(self) -> None:
        VerifierTest.setUp(self)
        self.artifact = self.target / "references/guide.md"
        old = self.target / "skills/dioxus-core/SKILL.md"
        old.unlink()
        self.artifact.parent.mkdir()
        self.artifact.write_bytes(b"old\n")
        artifact = self.ledger["artifacts"][0]
        artifact["artifactPath"] = "references/guide.md"
        self.revision["artifactSha256"] = digest(b"old\n")
        successor = copy.deepcopy(self.revision)
        successor.update({"revisionId": "core-2", "artifactSha256": digest(b"new\n")})
        self.revision.update({"status": "superseded", "supersededBy": "core-2"})
        artifact["revisions"].append(successor)
        generated = self.target / "generated"
        generated.mkdir()
        (generated / "provenance.json").write_text(json.dumps(self.ledger))
        payloads = generated / "apply-blobs"
        payloads.mkdir()
        (payloads / digest(b"new\n")).write_bytes(b"new\n")
        (self.target / "mise.toml").write_text(
            '[tasks.apply]\nrun = "python ' + str(ROOT / "scripts/apply.py") + '"\n'
        )
        subprocess.run(["git", "init", "-q", str(self.target)], check=True)
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
                "fixture",
            ],
            check=True,
        )

    def invoke(self, *args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [
                sys.executable,
                str(ROOT / "scripts/apply.py"),
                "--report",
                str(self.output),
                *args,
            ],
            cwd=self.target,
            capture_output=True,
            text=True,
        )

    def resign(self, report: dict) -> None:
        report["reportId"] = "sha256:" + digest(
            canonical_json(
                {key: value for key, value in report.items() if key != "reportId"}
            )
        )
        self.output.write_text(json.dumps(report))

    def test_apply_and_rerun_when_confirmed(self) -> None:
        self.generate()
        source_before = snapshot(self.source)
        result = self.invoke("--confirm")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.artifact.read_bytes(), b"new\n")
        before = snapshot(self.target)
        repeated = self.invoke("--confirm")
        self.assertEqual(repeated.returncode, 0, repeated.stderr)
        self.assertEqual(snapshot(self.target), before)
        self.assertEqual(snapshot(self.source), source_before)

    def test_rejections_leave_target_unchanged(self) -> None:
        for scenario in (
            "confirm",
            "dirty",
            "source",
            "target",
            "invalid",
            "traversal",
            "unexpected",
            "ambiguous",
            "payload",
            "symlink",
            "extra",
        ):
            with self.subTest(scenario=scenario):
                report = self.generate()
                if scenario == "dirty":
                    (self.target / "dirty").write_text("dirty")
                elif scenario == "source":
                    (self.source / "source.rs").write_text("drift\n")
                elif scenario == "target":
                    self.artifact.write_bytes(b"drift\n")
                elif scenario == "invalid":
                    report["reportId"] = "invalid"
                    self.output.write_text(json.dumps(report))
                elif scenario in ("traversal", "unexpected"):
                    report["operations"][0]["artifactPath"] = (
                        "../escape" if scenario == "traversal" else "scripts/evil.py"
                    )
                    self.resign(report)
                elif scenario == "ambiguous":
                    report["validation"]["verdict"] = "blocked"
                    self.resign(report)
                elif scenario == "payload":
                    (
                        self.target / "generated/apply-blobs" / digest(b"new\n")
                    ).write_bytes(b"bad")
                elif scenario == "symlink":
                    self.artifact.unlink()
                    self.artifact.symlink_to(self.source / "source.rs")
                elif scenario == "extra":
                    (self.artifact.parent / "extra.md").write_text("extra")
                before = (
                    snapshot(self.target)
                    if scenario != "symlink"
                    else self.artifact.readlink()
                )
                result = self.invoke(*(() if scenario == "confirm" else ("--confirm",)))
                self.assertNotEqual(result.returncode, 0, result.stdout)
                self.assertEqual(
                    snapshot(self.target)
                    if scenario != "symlink"
                    else self.artifact.readlink(),
                    before,
                )
                if self.artifact.is_symlink():
                    self.artifact.unlink()
                if (self.artifact.parent / "extra.md").exists():
                    (self.artifact.parent / "extra.md").unlink()
                if (self.target / "dirty").exists():
                    (self.target / "dirty").unlink()
                (self.source / "source.rs").write_text("source\n")
                self.artifact.write_bytes(b"old\n")
                (self.target / "generated/apply-blobs" / digest(b"new\n")).write_bytes(
                    b"new\n"
                )

    def test_rollback_when_post_validation_fails(self) -> None:
        self.generate()
        from apply import apply_report

        before = snapshot(self.target)
        with patch(
            "apply.validate_output", side_effect=RuntimeError("injected write failure")
        ):
            with self.assertRaises(RuntimeError):
                apply_report(self.output, self.target, self.source)
        self.assertEqual(snapshot(self.target), before)

    def test_addition_rollback_when_second_write_fails(self) -> None:
        addition = copy.deepcopy(self.ledger["artifacts"][0])
        addition["artifactPath"] = "references/new/nested.md"
        addition["revisions"] = [addition["revisions"][-1]]
        self.ledger["artifacts"].append(addition)
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
                "addition fixture",
            ],
            check=True,
        )
        self.generate()
        from apply import apply_report
        import os

        sync = os.fsync
        calls = 0

        def fail_second(descriptor: int) -> None:
            nonlocal calls
            calls += 1
            if calls == 2:
                raise OSError("injected second write failure")
            sync(descriptor)

        before = snapshot(self.target)
        with patch("apply.os.fsync", side_effect=fail_second):
            with self.assertRaises(OSError):
                apply_report(self.output, self.target, self.source)
        self.assertEqual(snapshot(self.target), before)
        self.assertFalse((self.target / "references/new").exists())
        self.assertEqual(self.invoke("--confirm").returncode, 0)
        self.assertEqual(self.invoke("--confirm").returncode, 0)

    def test_rollback_when_write_fails_after_bytes_written(self) -> None:
        self.generate()
        from apply import apply_report
        import os

        sync = os.fsync
        calls = 0

        def fail_once(descriptor: int) -> None:
            nonlocal calls
            calls += 1
            if calls == 1:
                raise OSError("injected write failure")
            sync(descriptor)

        before = snapshot(self.target)
        with patch("apply.os.fsync", side_effect=fail_once):
            with self.assertRaises(OSError):
                apply_report(self.output, self.target, self.source)
        self.assertEqual(snapshot(self.target), before)

    def test_rollback_when_source_drifts_after_writes(self) -> None:
        self.generate()
        from apply import apply_report

        before = snapshot(self.target)
        with patch(
            "apply.validate_output",
            side_effect=lambda *_: (self.source / "source.rs").write_text("drift"),
        ):
            with self.assertRaises(RuntimeError):
                apply_report(self.output, self.target, self.source)
        self.assertEqual(snapshot(self.target), before)
