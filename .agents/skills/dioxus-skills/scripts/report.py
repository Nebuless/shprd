#!/usr/bin/env python3
from __future__ import annotations

import argparse
import datetime as dt
import json
import subprocess
import sys
from contextlib import nullcontext
from pathlib import Path

from contract_types import ContractError, JsonObject, JsonValue, canonical_json
from provenance_contract import expect_object, expect_string
from report_contract import SOURCE_PRECEDENCE, parse_report
from report_discovery import (
    discover,
    inspect_artifact,
    source_conflicts,
    target_manifest,
)
from report_io import (
    ROOT,
    comparison_clone,
    digest,
    disjoint,
    git,
    publish,
    safe_path,
    snapshot,
    state,
)


def validate_source_state(report: JsonObject, source: Path) -> bool:
    if state(safe_path(source)) != report["sourceState"]:
        raise ContractError("source", "source state/hash mismatch")
    from report_validation import inspection

    evidence = inspection(report)
    if (
        evidence is not None
        and digest(canonical_json(snapshot(source))) != evidence["sourceSnapshotSha256"]
    ):
        raise ContractError("source", "source state/hash mismatch")
    return True


def generate(arguments: argparse.Namespace) -> JsonObject:
    source = safe_path(arguments.source)
    target = safe_path(arguments.target)
    output = safe_path(arguments.json_out)
    if not source.exists():
        raise ContractError(str(source), "missing source")
    disjoint(source, (target, ROOT))
    disjoint(output, (source,))
    if not arguments.no_upstream:
        comparison_path = safe_path(arguments.comparison)
        disjoint(
            output,
            (
                comparison_path,
                comparison_path.with_name(comparison_path.name + ".lock"),
            ),
        )
    if target in output.parents and output.relative_to(target).parts[0] != "reports":
        raise ContractError(str(output), "output inside target must use reports/")
    before = snapshot(source)
    source_state = expect_object(before["state"], "state")
    if source_state["dirty"] and not arguments.allow_dirty:
        raise ContractError("source", "source is dirty (use --allow-dirty to inspect)")
    provenance = arguments.provenance
    if provenance is None and (target / "generated/provenance.json").exists():
        provenance = target / "generated/provenance.json"
    if provenance is not None:
        disjoint(output, (safe_path(provenance),))
    artifacts = discover(target, provenance)
    paths = [expect_string(item["artifactPath"], "artifactPath") for item in artifacts]
    manifest = target_manifest(target, paths)
    operations: list[JsonValue] = []
    details: list[JsonValue] = []
    context = (
        nullcontext(None)
        if arguments.no_upstream
        else comparison_clone(arguments.comparison, source, target)
    )
    with context as upstream:
        for artifact in artifacts:
            operation, evidence = inspect_artifact(artifact, (source, target), upstream)
            operations.append(operation)
            details.append(evidence)
        comparison: JsonValue = (
            None
            if upstream is None
            else {
                "repository": git(upstream[0], "config", "--get", "remote.origin.url"),
                "sha": upstream[1],
            }
        )
        after = snapshot(source)
        if before != after:
            raise RuntimeError("source changed during inspection")
        if artifacts != discover(target, provenance):
            raise RuntimeError("provenance changed during inspection")
        if target_manifest(target, paths) != manifest:
            raise RuntimeError("target changed during inspection")
        for artifact, operation in zip(artifacts, operations):
            repeated, _ = inspect_artifact(artifact, (source, target), upstream)
            if repeated != operation:
                raise RuntimeError("target changed during inspection")
        if snapshot(source) != before:
            raise RuntimeError("source changed during inspection")
    created = arguments.created_at or dt.datetime.fromtimestamp(
        int(git(source, "show", "-s", "--format=%ct", "HEAD")), dt.UTC
    ).strftime("%Y-%m-%dT%H:%M:%SZ")
    dt.datetime.strptime(created, "%Y-%m-%dT%H:%M:%SZ")
    evidence: JsonObject = {
        "sourceSnapshotSha256": digest(canonical_json(before)),
        "remotes": before["remotes"],
        "submodules": before["submodules"],
        "comparison": comparison,
        "artifacts": details,
        "targetManifest": manifest,
    }
    blocked = any(
        expect_object(item, "operation")["verdict"] == "blocked" for item in operations
    )
    report: JsonObject = {
        "schemaVersion": 1,
        "reportId": "pending",
        "createdAt": created,
        "sourceState": source_state,
        "sourcePrecedence": list(SOURCE_PRECEDENCE),
        "operations": operations,
        "conflicts": source_conflicts(details, source_state, comparison),
        "validation": {
            "verdict": "blocked" if blocked else "approved",
            "checks": [
                {
                    "name": "inspection-v1",
                    "verdict": "pass",
                    "detail": canonical_json(evidence).decode().strip(),
                },
                {
                    "name": "source-stable",
                    "verdict": "pass",
                    "detail": "source snapshots agree",
                },
            ],
        },
    }
    report["reportId"] = "sha256:" + digest(
        canonical_json(
            {key: value for key, value in report.items() if key != "reportId"}
        )
    )
    ledger: JsonObject = {"schemaVersion": 1, "artifacts": list(artifacts)}
    parse_report(report, provenance=ledger)
    from report_validation import validate_inspection

    validate_inspection(report, target)
    publish(output, report)
    return report


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Read-only per-artifact Dioxus inspection"
    )
    parser.add_argument("--source", type=Path, default=Path("/root/repo/dioxus"))
    parser.add_argument("--target", type=Path, default=ROOT)
    parser.add_argument("--provenance", type=Path)
    parser.add_argument("--json-out", type=Path, default=Path("reports/report.json"))
    parser.add_argument(
        "--comparison",
        type=Path,
        default=Path.home() / ".cache/dioxus-skills/upstream.git",
    )
    parser.add_argument("--no-upstream", action="store_true")
    parser.add_argument("--allow-dirty", action="store_true")
    parser.add_argument("--created-at")
    arguments = parser.parse_args(sys.argv[2:] if sys.argv[1:2] == ["--"] else None)
    try:
        report = generate(arguments)
        print("Dioxus read-only report\nSource: " + str(report["sourceState"]))
        print("JSON report (complete; identical operations and evidence):")
        print(json.dumps(report, indent=2, sort_keys=True))
        return 0
    except (
        OSError,
        ValueError,
        RuntimeError,
        subprocess.SubprocessError,
        ContractError,
    ) as error:
        print(f"report failed: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
