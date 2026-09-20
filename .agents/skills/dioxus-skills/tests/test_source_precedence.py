from __future__ import annotations

import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]


class SourcePrecedenceTests(unittest.TestCase):
    def test_all_rejects_unmapped_artifact(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory)
            for name in ("skills", "templates", "generated", "reports"):
                shutil.copytree(ROOT / name, target / name)
            shutil.copy2(ROOT / "SKILL.md", target / "SKILL.md")
            (target / "skills/dioxus-core/unmapped.md").write_text("unmapped")
            result = subprocess.run(
                [
                    sys.executable,
                    str(ROOT / "scripts/validate_provenance.py"),
                    "--all",
                    "--target",
                    str(target),
                ],
                cwd=ROOT,
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertIn("missing provenance mapping", result.stderr)

    def test_all_rejects_changed_artifact(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory)
            for name in ("skills", "templates", "generated", "reports"):
                shutil.copytree(ROOT / name, target / name)
            shutil.copy2(ROOT / "SKILL.md", target / "SKILL.md")
            (target / "skills/dioxus-core/SKILL.md").write_text("changed")
            result = subprocess.run(
                [
                    sys.executable,
                    str(ROOT / "scripts/validate_provenance.py"),
                    "--all",
                    "--target",
                    str(target),
                ],
                cwd=ROOT,
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertIn("stale-target", result.stderr)

    def test_all_accepts_code_pinned_package(self) -> None:
        result = subprocess.run(
            [sys.executable, str(ROOT / "scripts/validate_provenance.py"), "--all"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_all_rejects_docs_selected_over_source(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory)
            for name in ("skills", "templates", "generated", "reports"):
                shutil.copytree(ROOT / name, target / name)
            shutil.copy2(ROOT / "SKILL.md", target / "SKILL.md")
            evidence_path = target / "reports/skill-sources.json"
            evidence = json.loads(evidence_path.read_text())
            fixture = json.loads(
                (ROOT / "tests/fixtures/source-doc-contradiction.json").read_text()
            )
            evidence["conflicts"][0].update(fixture)
            evidence_path.write_text(json.dumps(evidence))
            result = subprocess.run(
                [
                    sys.executable,
                    str(ROOT / "scripts/validate_provenance.py"),
                    "--all",
                    "--target",
                    str(target),
                ],
                cwd=ROOT,
                capture_output=True,
                text=True,
                check=False,
            )
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertIn("highest-precedence", result.stderr)

    def test_catalog_is_deterministic(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "dioxus-skills"
            target.mkdir()
            shutil.copytree(ROOT / "skills", target / "skills")
            shutil.copy2(ROOT / "SKILL.md", target / "SKILL.md")
            command = [
                "node",
                str(ROOT / "scripts/generate-index.mjs"),
                "--root",
                str(target),
            ]
            subprocess.run(command, cwd=ROOT, check=True, capture_output=True)
            first = (target / "generated/skills.json").read_bytes()
            subprocess.run(command, cwd=ROOT, check=True, capture_output=True)
            self.assertEqual(first, (target / "generated/skills.json").read_bytes())
            catalog = json.loads(first)
            self.assertEqual(
                [item["name"] for item in catalog["skills"]],
                [
                    "dioxus-core",
                    "dioxus-desktop",
                    "dioxus-fullstack",
                    "dioxus-mobile",
                    "dioxus-platform-interop",
                    "dioxus-routing",
                    "dioxus-signals",
                    "dioxus-skills",
                    "dioxus-web",
                ],
            )


if __name__ == "__main__":
    unittest.main()
