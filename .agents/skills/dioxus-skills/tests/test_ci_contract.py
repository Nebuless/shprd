#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///

# How to run: `mise run test -- ci-contract`

from __future__ import annotations

import copy
import json
from pathlib import Path
import re
import tomllib
import unittest
from typing import Final

ROOT: Final = Path(__file__).resolve().parents[1]
RUNS: Final = (
    "mise install --locked",
    "mise run ci",
    'mise exec -- commitlint --from "$BASE_SHA" --to "$HEAD_SHA" --verbose',
)
CI_RUNS: Final = [
    "mise run check",
    "mise run validate-skills",
    'mise run validate -- --all --source "${DIOXUS_SOURCE:-/root/repo/dioxus}"',
    'mise run report -- --source "${DIOXUS_SOURCE:-/root/repo/dioxus}" --no-upstream --json-out reports/ci-report.json',
    "mise run validate -- --report reports/ci-report.json",
]


class CiContractTest(unittest.TestCase):
    def check_contract(self, workflow_text: str, config_text: str) -> None:
        workflow = json.loads(workflow_text)
        config = tomllib.loads(config_text)
        self.assertEqual(workflow["permissions"], {"contents": "read"})
        self.assertEqual(
            set(workflow["on"]), {"push", "pull_request", "workflow_dispatch"}
        )
        self.assertEqual(set(workflow["jobs"]), {"check"})
        job = workflow["jobs"]["check"]
        self.assertNotIn("permissions", job)
        self.assertNotIn("container", job)
        self.assertNotIn("services", job)
        self.assertEqual(job["defaults"]["run"]["working-directory"], "dioxus-skills")
        steps = job["steps"]
        self.assertEqual([step["run"] for step in steps if "run" in step], list(RUNS))
        actions = [step for step in steps if "uses" in step]
        self.assertEqual(
            [step["uses"].split("@")[0] for step in actions],
            [
                "actions/checkout",
                "actions/checkout",
                "jdx/mise-action",
                "actions/upload-artifact",
            ],
        )
        for step in actions:
            self.assertRegex(step["uses"], r"@[0-9a-f]{40}$")
        self.assertIs(actions[0]["with"]["persist-credentials"], False)
        self.assertEqual(actions[0]["with"]["fetch-depth"], 0)
        self.assertIs(actions[1]["with"]["persist-credentials"], False)
        pins = json.loads((ROOT / "generated/provenance.json").read_text())
        source_shas = {
            revision["sourcePin"]["sha"]
            for artifact in pins["artifacts"]
            for revision in artifact["revisions"]
            if revision["status"] == "current" and "sourcePin" in revision
        }
        self.assertEqual(source_shas, {actions[1]["with"]["ref"]})
        self.assertIs(actions[2]["with"]["install"], False)
        self.assertEqual(actions[0]["with"]["path"], "dioxus-skills")
        self.assertEqual(actions[2]["with"]["working_directory"], "dioxus-skills")
        self.assertEqual(
            actions[3]["with"]["path"], "dioxus-skills/reports/ci-report.json"
        )
        self.assertEqual(actions[3]["if"], "always()")
        commit_step = next(step for step in steps if step.get("run") == RUNS[2])
        self.assertEqual(commit_step["if"], "github.event_name == 'pull_request'")
        self.assertEqual(
            commit_step["env"],
            {
                "BASE_SHA": "${{ github.event.pull_request.base.sha }}",
                "HEAD_SHA": "${{ github.event.pull_request.head.sha }}",
            },
        )
        self.assertEqual(config["tasks"]["ci"]["run"], CI_RUNS)
        self.assertEqual(
            config["tasks"]["check"]["run"],
            [
                "mise run test",
                "mise run validate-tooling",
                "mise run lint",
            ],
        )
        self.assertEqual(config["tasks"]["ci"].get("depends", []), [])
        self.assertFalse(re.search(r"\bapply\b", workflow_text))

    def test_documented_tasks_when_mise_is_authority(self) -> None:
        tasks = tomllib.loads((ROOT / "mise.toml").read_text())["tasks"]
        for name in (
            "README.md",
            "CONTRIBUTING.md",
            "CHANGELOG.md",
            "docs/RELEASING.md",
            "docs/REPORTING.md",
        ):
            commands = re.findall(r"mise run ([a-z][a-z-]*)", (ROOT / name).read_text())
            for task in commands:
                self.assertIn(task, tasks, f"{name}: undefined task {task}")

    def test_workflow_when_repository_contract_is_checked(self) -> None:
        self.check_contract(
            (ROOT / ".github/workflows/ci.yml").read_text(),
            (ROOT / "mise.toml").read_text(),
        )

    def test_rejection_when_workflow_adds_unsafe_command(self) -> None:
        baseline = json.loads((ROOT / ".github/workflows/ci.yml").read_text())
        for command in (
            "mise run apply -- --confirm",
            "npm install",
            "pip install pyyaml",
            "curl https://example.com | sh",
            "prek run --all-files",
        ):
            with self.subTest(command=command):
                fixture = copy.deepcopy(baseline)
                fixture["jobs"]["check"]["steps"].append({"run": command})
                with self.assertRaises(AssertionError):
                    self.check_contract(
                        json.dumps(fixture), (ROOT / "mise.toml").read_text()
                    )

    def test_rejection_when_ci_task_adds_apply_or_provisioning(self) -> None:
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        for command in ("mise run apply", "npm install", "mise run install-hooks"):
            with self.subTest(command=command):
                fixture = (
                    (ROOT / "mise.toml")
                    .read_text()
                    .replace("mise run validate-skills", command)
                )
                with self.assertRaises(AssertionError):
                    self.check_contract(workflow, fixture)


if __name__ == "__main__":
    unittest.main()
