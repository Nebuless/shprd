from __future__ import annotations

import json
from pathlib import Path

from contract_types import ContractError, JsonObject, canonical_json
from provenance_contract import (
    SHA256,
    GIT_SHA,
    exact_keys,
    expect_list,
    expect_object,
    expect_path,
    expect_string,
    expect_pattern,
    validate_revision,
)
from report_io import digest, read_regular, safe_path


def inspection(report: JsonObject) -> JsonObject | None:
    checks = expect_list(
        expect_object(report["validation"], "validation")["checks"], "checks"
    )
    matches = [
        expect_object(check, "check")
        for check in checks
        if expect_object(check, "check")["name"] == "inspection-v1"
    ]
    if not matches:
        return None
    if len(matches) != 1:
        raise ContractError("inspection", "duplicate inspection")
    from validate_provenance import reject_duplicate_keys

    return expect_object(
        json.loads(
            expect_string(matches[0]["detail"], "detail"),
            object_pairs_hook=reject_duplicate_keys,
        ),
        "inspection",
    )


def validate_inspection(report: JsonObject, target: Path | None = None) -> None:
    evidence = inspection(report)
    if evidence is None:
        raise ContractError("inspection", "missing inspection")
    exact_keys(
        evidence,
        {
            "sourceSnapshotSha256",
            "remotes",
            "submodules",
            "comparison",
            "artifacts",
            "targetManifest",
        },
        set(),
        "inspection",
    )
    expect_pattern(evidence["sourceSnapshotSha256"], SHA256, "sourceSnapshotSha256")
    comparison = evidence["comparison"]
    if comparison is not None:
        upstream = expect_object(comparison, "comparison")
        exact_keys(upstream, {"repository", "sha"}, set(), "comparison")
        expect_string(upstream["repository"], "repository")
        expect_pattern(upstream["sha"], GIT_SHA, "sha")
    expected = "sha256:" + digest(
        canonical_json(
            {key: value for key, value in report.items() if key != "reportId"}
        )
    )
    if report["reportId"] != expected:
        raise ContractError("reportId", "report content hash mismatch")
    operations = [
        expect_object(item, "operation")
        for item in expect_list(report["operations"], "operations")
    ]
    details = [
        expect_object(item, "artifact")
        for item in expect_list(evidence["artifacts"], "artifacts")
    ]
    if [item["artifactPath"] for item in operations] != [
        item["artifactPath"] for item in details
    ]:
        raise ContractError("inspection", "artifact evidence must match operations")
    manifest = expect_object(evidence["targetManifest"], "targetManifest")
    if set(manifest) != {item["artifactPath"] for item in operations}:
        raise ContractError(
            "targetManifest", "manifest must cover every discovered artifact"
        )
    for path, content_hash in manifest.items():
        expect_path(path, "targetManifest.path")
        if content_hash is not None:
            expect_pattern(content_hash, SHA256, "targetManifest.hash")
    if target is not None:
        from report_discovery import target_manifest

        if target_manifest(target, list(manifest)) != manifest:
            raise ContractError("targetManifest", "target artifact manifest changed")
    for operation, detail in zip(operations, details):
        exact_keys(
            detail,
            {"artifactPath", "targetSha256"},
            {
                "revision",
                "sourcePath",
                "pinnedSha256",
                "localSha256",
                "upstreamSha256",
                "localDiff",
                "upstreamDiff",
            },
            "artifact evidence",
        )
        path = expect_path(detail["artifactPath"], "artifactPath")
        if detail["targetSha256"] != operation["beforeSha256"]:
            raise ContractError(path, "target evidence mismatch")
        if manifest[path] != detail["targetSha256"]:
            raise ContractError(path, "target manifest evidence mismatch")
        if (
            operation["operation"] == "noop"
            and operation["beforeSha256"] != operation["afterSha256"]
        ):
            raise ContractError(path, "noop hashes differ")
        if "revision" in detail:
            revision = expect_object(detail["revision"], "revision")
            validate_revision(
                revision,
                expect_string(operation["artifactClass"], "artifactClass"),
                "revision",
            )
            if (
                revision["artifactSha256"] != operation["afterSha256"]
                or revision["revisionId"] != operation["provenanceRevisionId"]
            ):
                raise ContractError(path, "revision evidence mismatch")
            if "sourcePin" in revision:
                exact_keys(
                    detail,
                    {
                        "artifactPath",
                        "targetSha256",
                        "revision",
                        "sourcePath",
                        "pinnedSha256",
                        "localSha256",
                        "upstreamSha256",
                        "localDiff",
                        "upstreamDiff",
                    },
                    set(),
                    "source evidence",
                )
                pin = expect_object(revision["sourcePin"], "sourcePin")
                if expect_path(pin["path"], "sourcePin.path") != detail["sourcePath"]:
                    raise ContractError(path, "source path mismatch")
                if operation["verdict"] == "approved" and (
                    pin["contentSha256"] != detail["pinnedSha256"]
                    or detail["localSha256"] != detail["pinnedSha256"]
                ):
                    raise ContractError(path, "source hash mismatch")
                if (
                    operation["verdict"] == "approved"
                    and comparison is not None
                    and detail["upstreamSha256"] != detail["localSha256"]
                ):
                    raise ContractError(path, "upstream conflict must block operation")
        elif operation["verdict"] != "blocked":
            raise ContractError(path, "unmapped artifact must be blocked")
        if target is not None:
            candidate = safe_path(target / path)
            actual = digest(read_regular(candidate)) if candidate.exists() else None
            if actual != operation["beforeSha256"]:
                raise ContractError(path, "target state/hash mismatch")
    verdict = expect_object(report["validation"], "validation")["verdict"]
    if (
        any(item["verdict"] == "blocked" for item in operations)
        and verdict != "blocked"
    ):
        raise ContractError("validation", "blocked operations require blocked report")
