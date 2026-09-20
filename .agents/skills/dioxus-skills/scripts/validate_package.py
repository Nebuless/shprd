from __future__ import annotations

import json
from pathlib import Path

from contract_types import ContractError
from provenance_contract import (
    expect_list,
    expect_object,
    expect_path,
    expect_string,
    exact_keys,
    parse_provenance,
)
from report_contract import validate_conflict
from report_discovery import blob, discover, inspect_artifact
from report_io import digest, git, read_regular, safe_path


def validate_package(target: Path, source: Path) -> None:
    from validate_provenance import reject_duplicate_keys

    target, source = safe_path(target), safe_path(source)
    ledger_path = target / "generated/provenance.json"
    ledger = parse_provenance(
        expect_object(
            json.loads(
                read_regular(ledger_path),
                object_pairs_hook=reject_duplicate_keys,
            ),
            "provenance",
        )
    )
    evidence = expect_object(
        json.loads(
            read_regular(target / "reports/skill-sources.json"),
            object_pairs_hook=reject_duplicate_keys,
        ),
        "skill-sources",
    )
    exact_keys(evidence, {"schemaVersion", "conflicts"}, set(), "skill-sources")
    if evidence["schemaVersion"] != 1:
        raise ContractError("skill-sources", "schemaVersion must equal 1")
    conflicts = expect_list(evidence["conflicts"], "conflicts")
    if not conflicts:
        raise ContractError("conflicts", "source/doc review evidence required")
    for conflict in conflicts:
        if validate_conflict(conflict, "conflict"):
            raise ContractError(
                "conflict", "ambiguous source/doc contradiction blocks validation"
            )
    head = git(source, "rev-parse", "HEAD")
    mapped = {
        expect_string(expect_object(item, "artifact")["artifactPath"], "artifactPath")
        for item in expect_list(ledger["artifacts"], "artifacts")
    }
    if "SKILL.md" not in mapped:
        raise ContractError("SKILL.md", "missing router provenance mapping")
    for artifact in discover(target, ledger_path):
        artifact_path = expect_string(artifact["artifactPath"], "artifactPath")
        if artifact_path not in mapped:
            raise ContractError(artifact_path, "missing provenance mapping")
        operation, detail = inspect_artifact(artifact, (source, target), None)
        if operation["verdict"] != "approved":
            raise ContractError(artifact_path, str(operation["blockers"]))
        revision = expect_object(detail["revision"], "revision")
        if detail["targetSha256"] != revision["artifactSha256"]:
            raise ContractError(
                artifact_path, "artifact content differs from current revision"
            )
        if "sourcePin" in revision:
            pin = expect_object(revision["sourcePin"], "sourcePin")
            if pin["sha"] != head:
                raise ContractError(artifact_path, "source pin must match local HEAD")
            docs = expect_list(revision["docs"], "docs")
            if not docs:
                raise ContractError(artifact_path, "explanatory docs required")
            reference = f"{pin['repository']}@{pin['sha']}:{pin['path']}"
            relevant = [
                expect_object(item, "conflict")
                for item in conflicts
                if expect_object(item, "conflict")["topic"] == artifact_path
            ]
            if len(relevant) != 1 or relevant[0].get("selectedReference") != reference:
                raise ContractError(
                    artifact_path, "source selection missing or differs from pin"
                )
            candidates = [
                expect_object(item, "candidate")
                for item in expect_list(relevant[0]["candidates"], "candidates")
            ]
            local = [
                item for item in candidates if item["sourceKind"] == "local-source"
            ]
            if len(local) != 1 or local[0]["contentSha256"] != pin["contentSha256"]:
                raise ContractError(
                    artifact_path, "source evidence hash differs from pin"
                )
            for doc_value in docs:
                doc = expect_object(doc_value, "doc")
                if not any(
                    item["sourceKind"] == "public-docs"
                    and item["reference"] == doc["url"]
                    and item["contentSha256"] == doc["contentSha256"]
                    for item in candidates
                ):
                    raise ContractError(
                        artifact_path, "docs evidence differs from provenance"
                    )
                prefix = f"https://github.com/DioxusLabs/dioxus/blob/{head}/"
                url = expect_string(doc["url"], "url")
                if not url.startswith(prefix):
                    raise ContractError(
                        artifact_path,
                        "docs must identify captured Markdown at pinned SHA",
                    )
                doc_path = expect_path(url.removeprefix(prefix), "docs path")
                captured = blob(source, head, doc_path)
                if captured is None or digest(captured) != doc["contentSha256"]:
                    raise ContractError(artifact_path, "docs blob hash mismatch")
