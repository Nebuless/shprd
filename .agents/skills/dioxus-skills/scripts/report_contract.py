from __future__ import annotations

from typing import Final

try:
    from .provenance_contract import (
        ARTIFACT_CLASSES,
        GIT_SHA,
        SHA256,
        TIMESTAMP,
        JsonObject,
        JsonValue,
        exact_keys,
        expect_enum,
        expect_list,
        expect_object,
        expect_path,
        expect_pattern,
        expect_string,
        CLASS_ROOTS,
        fail,
    )
except ImportError:
    from provenance_contract import (
        ARTIFACT_CLASSES,
        GIT_SHA,
        SHA256,
        TIMESTAMP,
        JsonObject,
        JsonValue,
        exact_keys,
        expect_enum,
        expect_list,
        expect_object,
        expect_path,
        expect_pattern,
        expect_string,
        CLASS_ROOTS,
        fail,
    )


SOURCE_PRECEDENCE: Final = (
    "local-source",
    "local-tests-examples",
    "local-agents",
    "local-architecture",
    "upstream-source",
    "public-docs",
)
BLOCKER_CODES: Final = frozenset(
    {
        "ambiguous-mapping",
        "source-deleted",
        "source-conflict",
        "stale-source",
        "stale-target",
        "unapproved-class",
        "unsafe-path",
        "hash-mismatch",
    }
)


def validate_report_operation(
    value: JsonValue, path: str, provenance: JsonObject | None = None
) -> None:
    operation = expect_object(value, path)
    exact_keys(
        operation,
        {
            "operation",
            "artifactPath",
            "artifactClass",
            "beforeSha256",
            "afterSha256",
            "provenanceRevisionId",
            "verdict",
            "blockers",
        },
        set(),
        path,
    )
    kind = expect_enum(
        operation["operation"], {"add", "update", "noop"}, f"{path}.operation"
    )
    artifact_class = expect_enum(
        operation["artifactClass"], ARTIFACT_CLASSES, f"{path}.artifactClass"
    )
    artifact_path = expect_path(operation["artifactPath"], f"{path}.artifactPath")
    if not any(
        artifact_path == root or artifact_path.startswith(root)
        for root in CLASS_ROOTS[artifact_class]
    ):
        fail(f"{path}.artifactPath", "is not approved for artifactClass")
    if kind == "add":
        if operation["beforeSha256"] is not None:
            fail(f"{path}.beforeSha256", "must be null for add")
    else:
        expect_pattern(operation["beforeSha256"], SHA256, f"{path}.beforeSha256")
    expect_pattern(operation["afterSha256"], SHA256, f"{path}.afterSha256")
    if provenance is not None:
        matches = [
            a for a in provenance["artifacts"] if a["artifactPath"] == artifact_path
        ]
        if len(matches) != 1:
            fail(f"{path}.artifactPath", "must resolve to provenance artifact")
        revisions = matches[0]["revisions"]
        linked = [
            r for r in revisions if r["revisionId"] == operation["provenanceRevisionId"]
        ]
        if (
            not linked
            and operation["provenanceRevisionId"] == "unmapped"
            and not revisions
            and operation["verdict"] == "blocked"
        ):
            linked = [
                {"revisionId": "unmapped", "artifactSha256": operation["afterSha256"]}
            ]
            revisions = linked
        if len(linked) != 1 or linked[0]["artifactSha256"] != operation["afterSha256"]:
            fail(f"{path}.provenanceRevisionId", "must link afterSha256 to provenance")
        current = revisions[
            [r["revisionId"] for r in revisions].index(
                operation["provenanceRevisionId"]
            )
        ]
        predecessor = next(
            (r for r in revisions if r.get("supersededBy") == current["revisionId"]),
            None,
        )
        expected_before = (
            None
            if kind == "add"
            else (
                predecessor["artifactSha256"]
                if predecessor and kind == "update"
                else current["artifactSha256"]
            )
        )
        if (
            operation["verdict"] == "approved"
            and operation["beforeSha256"] != expected_before
        ):
            fail(f"{path}.beforeSha256", "must link to provenance predecessor")
    expect_string(operation["provenanceRevisionId"], f"{path}.provenanceRevisionId")
    verdict = expect_enum(
        operation["verdict"], {"approved", "blocked"}, f"{path}.verdict"
    )
    blockers = expect_list(operation["blockers"], f"{path}.blockers")
    for index, blocker_value in enumerate(blockers):
        blocker_path = f"{path}.blockers[{index}]"
        blocker = expect_object(blocker_value, blocker_path)
        exact_keys(blocker, {"code", "detail"}, set(), blocker_path)
        expect_enum(blocker["code"], BLOCKER_CODES, f"{blocker_path}.code")
        expect_string(blocker["detail"], f"{blocker_path}.detail")
    if (verdict == "blocked") != bool(blockers):
        fail(path, "blocked verdict must agree with blockers")


def validate_conflict(value: JsonValue, path: str) -> bool:
    conflict = expect_object(value, path)
    exact_keys(
        conflict,
        {"conflictId", "topic", "status", "candidates"},
        {"selectedReference"},
        path,
    )
    for key in ("conflictId", "topic"):
        expect_string(conflict[key], f"{path}.{key}")
    status = expect_enum(
        conflict["status"], {"ambiguous", "resolved-by-precedence"}, f"{path}.status"
    )
    candidates = expect_list(conflict["candidates"], f"{path}.candidates")
    if len(candidates) < 2:
        fail(f"{path}.candidates", "must contain at least two candidates")
    for index, candidate_value in enumerate(candidates):
        candidate_path = f"{path}.candidates[{index}]"
        candidate = expect_object(candidate_value, candidate_path)
        exact_keys(
            candidate,
            {"sourceKind", "reference", "contentSha256", "claim"},
            set(),
            candidate_path,
        )
        expect_enum(
            candidate["sourceKind"],
            frozenset(SOURCE_PRECEDENCE),
            f"{candidate_path}.sourceKind",
        )
        expect_string(candidate["reference"], f"{candidate_path}.reference")
        expect_pattern(
            candidate["contentSha256"], SHA256, f"{candidate_path}.contentSha256"
        )
        expect_string(candidate["claim"], f"{candidate_path}.claim")
    if status == "ambiguous":
        if "selectedReference" in conflict:
            fail(f"{path}.selectedReference", "must be absent for ambiguity")
        return True
    selected = expect_string(
        conflict.get("selectedReference"), f"{path}.selectedReference"
    )
    references = {
        expect_object(candidate, path)["reference"] for candidate in candidates
    }
    if selected not in references:
        fail(f"{path}.selectedReference", "must select a candidate")
    ranks = {
        SOURCE_PRECEDENCE.index(expect_object(candidate, path)["sourceKind"])
        for candidate in candidates
    }
    selected_candidate = next(
        expect_object(candidate, path)
        for candidate in candidates
        if expect_object(candidate, path)["reference"] == selected
    )
    selected_rank = SOURCE_PRECEDENCE.index(selected_candidate["sourceKind"])
    if selected_rank != min(ranks):
        fail(f"{path}.selectedReference", "must select highest-precedence source")
    selected_rank_count = sum(
        SOURCE_PRECEDENCE.index(expect_object(candidate, path)["sourceKind"])
        == selected_rank
        for candidate in candidates
    )
    if selected_rank_count != 1:
        fail(path, "same-precedence conflict must remain ambiguous")
    return False


def parse_report(
    raw: JsonObject, *, provenance: JsonObject | None = None, legacy_only: bool = False
) -> JsonObject:
    required = {
        "schemaVersion",
        "reportId",
        "createdAt",
        "sourceState",
        "sourcePrecedence",
        "operations",
        "conflicts",
        "validation",
    }
    exact_keys(raw, required, set(), "$")
    if raw["schemaVersion"] != 1:
        fail("$.schemaVersion", "must equal 1")
    expect_string(raw["reportId"], "$.reportId")
    expect_pattern(raw["createdAt"], TIMESTAMP, "$.createdAt")
    source = expect_object(raw["sourceState"], "$.sourceState")
    exact_keys(source, {"repository", "ref", "sha", "dirty"}, set(), "$.sourceState")
    expect_string(source["repository"], "$.sourceState.repository")
    expect_string(source["ref"], "$.sourceState.ref")
    expect_pattern(source["sha"], GIT_SHA, "$.sourceState.sha")
    if not isinstance(source["dirty"], bool):
        fail("$.sourceState.dirty", "must be boolean")
    precedence = expect_list(raw["sourcePrecedence"], "$.sourcePrecedence")
    if tuple(precedence) != SOURCE_PRECEDENCE:
        fail("$.sourcePrecedence", "must match code-first precedence")
    operation_paths: set[str] = set()
    for index, operation in enumerate(expect_list(raw["operations"], "$.operations")):
        operation_path = f"$.operations[{index}]"
        validate_report_operation(operation, operation_path, provenance)
        artifact_path = expect_object(operation, operation_path)["artifactPath"]
        if not isinstance(artifact_path, str) or artifact_path in operation_paths:
            fail(
                f"{operation_path}.artifactPath",
                "duplicate operation creates ambiguous mapping",
            )
        operation_paths.add(artifact_path)
    has_ambiguity = any(
        validate_conflict(conflict, f"$.conflicts[{index}]")
        for index, conflict in enumerate(expect_list(raw["conflicts"], "$.conflicts"))
    )
    validation = expect_object(raw["validation"], "$.validation")
    exact_keys(validation, {"verdict", "checks"}, set(), "$.validation")
    verdict = expect_enum(
        validation["verdict"],
        {"approved", "failed", "blocked"},
        "$.validation.verdict",
    )
    checks = expect_list(validation["checks"], "$.validation.checks")
    if not checks:
        fail("$.validation.checks", "must not be empty")
    for index, check_value in enumerate(checks):
        path = f"$.validation.checks[{index}]"
        check = expect_object(check_value, path)
        exact_keys(check, {"name", "verdict", "detail"}, set(), path)
        expect_string(check["name"], f"{path}.name")
        expect_enum(check["verdict"], {"pass", "fail", "blocked"}, f"{path}.verdict")
        expect_string(check["detail"], f"{path}.detail")
    if has_ambiguity and verdict != "blocked":
        fail("$.validation.verdict", "ambiguous conflict blocks apply")
    inspections = [
        expect_object(check, "check")
        for check in checks
        if expect_object(check, "check")["name"] == "inspection-v1"
    ]
    if legacy_only:
        if inspections or str(raw["reportId"]).startswith("sha256:"):
            fail("$", "legacy-only mode rejects inspection reports")
    else:
        identifier = expect_string(raw["reportId"], "reportId")
        if not identifier.startswith("sha256:"):
            fail("reportId", "inspection report requires sha256: identifier")
        expect_pattern(identifier[7:], SHA256, "reportId")
        if len(inspections) != 1 or inspections[0]["verdict"] != "pass":
            fail("inspection", "exactly one passing inspection-v1 is required")
    return raw
