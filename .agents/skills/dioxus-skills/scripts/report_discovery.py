from __future__ import annotations

import difflib
import json
import subprocess
from pathlib import Path

from contract_types import JsonObject, JsonValue
from provenance_contract import (
    expect_list,
    expect_object,
    expect_path,
    expect_string,
    parse_provenance,
)
from report_io import digest, git, read_regular, repository_identity, safe_path


def blob(repository: Path, revision: str, path: str) -> bytes | None:
    entry = git(repository, "ls-tree", revision, "--", path)
    if not entry:
        return None
    if not entry.startswith(("100644 ", "100755 ")):
        raise RuntimeError(f"{path}: unsafe source object type")
    return subprocess.run(
        [
            "git",
            "--no-optional-locks",
            "-C",
            str(repository),
            "cat-file",
            "blob",
            f"{revision}:{path}",
        ],
        check=True,
        capture_output=True,
        timeout=30,
    ).stdout


def source_diff(before: bytes | None, after: bytes | None) -> str:
    return "".join(
        difflib.unified_diff(
            (before or b"").decode("utf-8", errors="replace").splitlines(keepends=True),
            (after or b"").decode("utf-8", errors="replace").splitlines(keepends=True),
            fromfile="pinned",
            tofile="observed",
        )
    )


def discover(target: Path, provenance: Path | None) -> list[JsonObject]:
    artifacts: list[JsonObject] = []
    if provenance is not None:
        from validate_provenance import reject_duplicate_keys

        raw = expect_object(
            json.loads(
                read_regular(provenance), object_pairs_hook=reject_duplicate_keys
            ),
            "provenance",
        )
        ledger = parse_provenance(raw)
        artifacts = [
            expect_object(item, "artifact")
            for item in expect_list(ledger["artifacts"], "artifacts")
        ]
    known = {expect_string(item["artifactPath"], "artifactPath") for item in artifacts}
    candidates: list[Path] = []
    for root in ("skills", "references", "assets", "templates"):
        directory = safe_path(target / root)
        if directory.exists():
            for candidate in sorted(directory.rglob("*")):
                safe_path(candidate)
                if candidate.is_file():
                    candidates.append(candidate)
    for candidate in candidates:
        if candidate.exists() and candidate.relative_to(target).as_posix() not in known:
            artifacts.append(
                {
                    "artifactPath": candidate.relative_to(target).as_posix(),
                    "artifactClass": "dioxus-authored",
                    "revisions": [],
                }
            )
    return sorted(artifacts, key=lambda item: str(item["artifactPath"]))


def inspect_artifact(
    artifact: JsonObject, roots: tuple[Path, Path], upstream: tuple[Path, str] | None
) -> tuple[JsonObject, JsonObject]:
    source, target = roots
    path = expect_path(artifact["artifactPath"], "artifactPath")
    candidate = safe_path(target / path)
    current_hash = digest(read_regular(candidate)) if candidate.exists() else None
    revisions = [
        expect_object(item, "revision")
        for item in expect_list(artifact["revisions"], "revisions")
    ]
    current = next((item for item in revisions if item["status"] == "current"), None)
    blockers: list[JsonValue] = []
    evidence: JsonObject = {"artifactPath": path, "targetSha256": current_hash}
    if current is None:
        blockers.append(
            {
                "code": "ambiguous-mapping",
                "detail": "artifact has no provenance mapping",
            }
        )
        desired_hash = current_hash or digest(b"")
        revision_id = "unmapped"
    else:
        desired_hash = expect_string(current["artifactSha256"], "artifactSha256")
        revision_id = expect_string(current["revisionId"], "revisionId")
        evidence["revision"] = current
        predecessors = {
            item["artifactSha256"]
            for item in revisions
            if item.get("supersededBy") == revision_id
        }
        if (
            current_hash is not None
            and current_hash != desired_hash
            and current_hash not in predecessors
        ):
            blockers.append(
                {
                    "code": "stale-target",
                    "detail": "target differs from current provenance revision",
                }
            )
        if "sourcePin" in current:
            pin = expect_object(current["sourcePin"], "sourcePin")
            identity = git(
                source, "config", "--get", "remote.origin.url", check=False
            ) or str(source)
            if repository_identity(
                expect_string(pin["repository"], "repository")
            ) != repository_identity(identity):
                blockers.append(
                    {
                        "code": "source-conflict",
                        "detail": "source repository differs from provenance",
                    }
                )
            source_path = expect_path(pin["path"], "sourcePin.path")
            pinned = blob(source, expect_string(pin["sha"], "sha"), source_path)
            local_path = safe_path(source / source_path)
            local = read_regular(local_path) if local_path.exists() else None
            remote = blob(upstream[0], upstream[1], source_path) if upstream else None
            evidence.update(
                {
                    "sourcePath": source_path,
                    "pinnedSha256": digest(pinned) if pinned is not None else None,
                    "localSha256": digest(local) if local is not None else None,
                    "upstreamSha256": digest(remote) if remote is not None else None,
                    "localDiff": source_diff(pinned, local),
                    "upstreamDiff": source_diff(local, remote) if upstream else None,
                }
            )
            if pinned is None or local is None:
                blockers.append({"code": "source-deleted", "detail": source_path})
            elif digest(pinned) != pin["contentSha256"]:
                blockers.append(
                    {"code": "hash-mismatch", "detail": "pinned source hash mismatch"}
                )
            elif pinned != local:
                blockers.append(
                    {
                        "code": "stale-source",
                        "detail": "local source differs from pinned bytes; guidance needs review",
                    }
                )
            locator = expect_object(pin["locator"], "locator")
            if local is not None:
                if "symbol" in locator:
                    symbol = expect_string(locator["symbol"], "symbol")
                    if symbol.encode() not in local:
                        blockers.append(
                            {
                                "code": "ambiguous-mapping",
                                "detail": "source symbol absent",
                            }
                        )
                else:
                    lines = expect_object(locator["lines"], "lines")
                    if int(str(lines["end"])) > len(local.splitlines()):
                        blockers.append(
                            {
                                "code": "ambiguous-mapping",
                                "detail": "source line range absent",
                            }
                        )
            if upstream and remote != local:
                blockers.append(
                    {
                        "code": "source-conflict",
                        "detail": "upstream differs; local source retains precedence",
                    }
                )
    operation: JsonObject = {
        "operation": "add"
        if current_hash is None
        else ("noop" if current_hash == desired_hash else "update"),
        "artifactPath": path,
        "artifactClass": artifact["artifactClass"],
        "beforeSha256": current_hash,
        "afterSha256": desired_hash,
        "provenanceRevisionId": revision_id,
        "verdict": "blocked" if blockers else "approved",
        "blockers": blockers,
    }
    return operation, evidence


def target_manifest(target: Path, paths: list[str]) -> JsonObject:
    discovered = {
        expect_string(item["artifactPath"], "artifactPath")
        for item in discover(target, None)
    }
    manifest: JsonObject = {}
    for path in sorted(discovered | set(paths)):
        candidate = safe_path(target / path)
        manifest[path] = digest(read_regular(candidate)) if candidate.exists() else None
    return manifest


def source_conflicts(
    details: list[JsonValue], source_state: JsonObject, comparison: JsonValue
) -> list[JsonValue]:
    conflicts: list[JsonValue] = []
    for value in details:
        detail = expect_object(value, "artifact")
        if "sourcePath" not in detail:
            continue
        revision = expect_object(detail["revision"], "revision")
        pin = expect_object(revision["sourcePin"], "sourcePin")
        local_reference = (
            f"{source_state['repository']}@{source_state['sha']}:{detail['sourcePath']}"
        )
        local: JsonObject = {
            "sourceKind": "local-source",
            "reference": local_reference,
            "contentSha256": detail["localSha256"] or digest(b""),
            "claim": "observed local source"
            if detail["localSha256"]
            else "local source absent",
        }
        if repository_identity(
            expect_string(pin["repository"], "repository")
        ) != repository_identity(
            expect_string(source_state["repository"], "repository")
        ):
            conflicts.append(
                {
                    "conflictId": f"{detail['artifactPath']}:repository",
                    "topic": detail["artifactPath"],
                    "status": "ambiguous",
                    "candidates": [
                        local,
                        {
                            "sourceKind": "local-source",
                            "reference": f"{pin['repository']}@{pin['sha']}:{pin['path']}",
                            "contentSha256": pin["contentSha256"],
                            "claim": "provenance repository identity",
                        },
                    ],
                }
            )
        if comparison is not None and detail["upstreamSha256"] != detail["localSha256"]:
            upstream = expect_object(comparison, "comparison")
            conflicts.append(
                {
                    "conflictId": f"{detail['artifactPath']}:upstream",
                    "topic": detail["artifactPath"],
                    "status": "resolved-by-precedence",
                    "selectedReference": local_reference,
                    "candidates": [
                        local,
                        {
                            "sourceKind": "upstream-source",
                            "reference": f"{upstream['repository']}@{upstream['sha']}:{detail['sourcePath']}",
                            "contentSha256": detail["upstreamSha256"] or digest(b""),
                            "claim": "observed upstream source"
                            if detail["upstreamSha256"]
                            else "upstream source absent",
                        },
                    ],
                }
            )
    return conflicts
