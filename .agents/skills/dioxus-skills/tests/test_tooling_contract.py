#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///

# How to run: `uv run tests/test_tooling_contract.py`

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import tomllib
import unittest
from pathlib import Path
from shutil import copytree, which
from typing import Final


ROOT: Final = Path(__file__).resolve().parents[1]
MISE_EXEC: Final = ("mise", "exec", "--")


class ToolingContractTest(unittest.TestCase):
    def test_toml_configs_parse(self) -> None:
        for path in (
            ROOT / "mise.toml",
            ROOT / "prek.toml",
            ROOT / ".qlty/qlty.toml",
            ROOT / "cliff.toml",
        ):
            with self.subTest(path=path):
                tomllib.loads(path.read_text(encoding="utf-8"))

    def test_malformed_prek_config_fails(self) -> None:
        with tempfile.NamedTemporaryFile(mode="w", suffix=".toml") as config:
            config.write('repos = "not a list"\n')
            config.flush()
            result = subprocess.run(
                [*MISE_EXEC, "prek", "validate-config", config.name],
                cwd=ROOT,
                check=False,
                capture_output=True,
                text=True,
            )

        self.assertNotEqual(
            result.returncode, 0, "malformed Prek config reported success"
        )

    def test_commitlint_accepts_policy_and_rejects_malformed_header(self) -> None:
        valid = subprocess.run(
            [*MISE_EXEC, "commitlint"],
            cwd=ROOT,
            input="build(tooling): add quality contracts\n",
            check=False,
            capture_output=True,
            text=True,
        )
        invalid = subprocess.run(
            [*MISE_EXEC, "commitlint"],
            cwd=ROOT,
            input="bad message\n",
            check=False,
            capture_output=True,
            text=True,
        )

        self.assertEqual(valid.returncode, 0, valid.stderr)
        self.assertNotEqual(
            invalid.returncode, 0, "malformed commit message reported success"
        )

    def test_git_cliff_is_deterministic_for_fixed_input(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            repository = Path(temporary_directory)
            subprocess.run(["git", "init", "--quiet", str(repository)], check=True)
            subprocess.run(
                ["git", "-C", str(repository), "config", "user.name", "Fixture"],
                check=True,
            )
            subprocess.run(
                [
                    "git",
                    "-C",
                    str(repository),
                    "config",
                    "user.email",
                    "fixture@example.com",
                ],
                check=True,
            )
            fixture = repository / "fixture.txt"
            fixture.write_text("base\n", encoding="utf-8")
            subprocess.run(
                ["git", "-C", str(repository), "add", "fixture.txt"], check=True
            )
            subprocess.run(
                [
                    "git",
                    "-C",
                    str(repository),
                    "commit",
                    "--quiet",
                    "-m",
                    "chore: baseline",
                ],
                env={
                    **os.environ,
                    "GIT_AUTHOR_DATE": "2026-01-01T00:00:00Z",
                    "GIT_COMMITTER_DATE": "2026-01-01T00:00:00Z",
                },
                check=True,
            )
            command = [
                *MISE_EXEC,
                "git-cliff",
                "--config",
                str(ROOT / "cliff.toml"),
                "--workdir",
                str(repository),
                "--with-commit",
                "feat(skills): add fixture",
            ]
            first = subprocess.run(
                command, cwd=ROOT, check=True, capture_output=True, text=True
            ).stdout
            second = subprocess.run(
                command, cwd=ROOT, check=True, capture_output=True, text=True
            ).stdout

        self.assertEqual(first, second)
        self.assertIn("### Features", first)

    @unittest.skipIf(
        os.environ.get("DIOXUS_SKILLS_HOOK_SMOKE") == "1",
        "avoid recursive pre-push smoke",
    )
    def test_installed_hooks_use_mise_managed_tools(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            repository = Path(temporary_directory) / "hook-fixture"
            copytree(
                ROOT,
                repository,
                ignore=lambda _directory, names: {
                    name
                    for name in names
                    if name in {".git", ".mise", ".omo", ".codegraph", "__pycache__"}
                },
            )
            subprocess.run(["git", "init", "--quiet", str(repository)], check=True)
            mise_path = which("mise")
            self.assertIsNotNone(mise_path)
            hook_environment = {
                "HOME": str(Path.home()),
                "DIOXUS_SKILLS_HOOK_SMOKE": "1",
                "MISE_TRUSTED_CONFIG_PATHS": str(repository),
                "PATH": f"{Path(mise_path).parent}:/usr/bin:/bin",
                "PYTHONDONTWRITEBYTECODE": "1",
            }
            install = subprocess.run(
                [
                    *MISE_EXEC,
                    "prek",
                    "install",
                    "--hook-type",
                    "pre-commit",
                    "--hook-type",
                    "commit-msg",
                    "--hook-type",
                    "pre-push",
                ],
                cwd=repository,
                env=hook_environment,
                check=False,
                capture_output=True,
                text=True,
            )
            self.assertEqual(install.returncode, 0, install.stderr)
            valid_message = repository / "valid-message"
            valid_message.write_text(
                "build(tooling): validate hooks\n", encoding="utf-8"
            )
            invalid_message = repository / "invalid-message"
            invalid_message.write_text("bad message\n", encoding="utf-8")
            probe = repository / "hook-probe.md"
            probe.write_text("# Hook probe\n", encoding="utf-8")
            subprocess.run(
                ["git", "-C", str(repository), "add", "hook-probe.md"], check=True
            )

            valid = subprocess.run(
                [str(repository / ".git/hooks/commit-msg"), str(valid_message)],
                cwd=repository,
                env=hook_environment,
                check=False,
                capture_output=True,
                text=True,
            )
            invalid = subprocess.run(
                [str(repository / ".git/hooks/commit-msg"), str(invalid_message)],
                cwd=repository,
                env=hook_environment,
                check=False,
                capture_output=True,
                text=True,
            )
            pre_commit = subprocess.run(
                [str(repository / ".git/hooks/pre-commit")],
                cwd=repository,
                env=hook_environment,
                check=False,
                capture_output=True,
                text=True,
            )
            pre_push = subprocess.run(
                [
                    str(repository / ".git/hooks/pre-push"),
                    "origin",
                    "https://example.invalid/repository.git",
                ],
                cwd=repository,
                env=hook_environment,
                check=False,
                capture_output=True,
                text=True,
            )

        self.assertEqual(valid.returncode, 0, valid.stderr)
        self.assertNotEqual(invalid.returncode, 0, "invalid commit message passed")
        self.assertEqual(pre_commit.returncode, 0, pre_commit.stderr)
        self.assertIn("qlty check", pre_commit.stdout)
        self.assertEqual(pre_push.returncode, 0, pre_push.stderr)


if __name__ == "__main__":
    unittest.main(argv=[sys.argv[0]])
