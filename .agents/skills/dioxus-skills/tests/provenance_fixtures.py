#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///

# How to run: imported by `uv run tests/test_provenance.py`

from __future__ import annotations

from scripts.contract_types import JsonObject, JsonValue


SHA_A = "a" * 64
SHA_B = "b" * 64
GIT_SHA = "1" * 40


def object_at(value: JsonValue, *path: str | int) -> JsonObject:
    current = value
    for segment in path:
        if isinstance(segment, str) and isinstance(current, dict):
            current = current[segment]
        elif isinstance(segment, int) and isinstance(current, list):
            current = current[segment]
        else:
            raise AssertionError(f"invalid fixture path segment {segment}")
    if not isinstance(current, dict):
        raise AssertionError("fixture path does not identify an object")
    return current


def array_at(value: JsonValue, *path: str | int) -> list[JsonValue]:
    current = value
    for segment in path:
        if isinstance(segment, str) and isinstance(current, dict):
            current = current[segment]
        elif isinstance(segment, int) and isinstance(current, list):
            current = current[segment]
        else:
            raise AssertionError(f"invalid fixture path segment {segment}")
    if not isinstance(current, list):
        raise AssertionError("fixture path does not identify an array")
    return current


def source_pin() -> JsonObject:
    return {
        "repository": "https://github.com/DioxusLabs/dioxus.git",
        "ref": "main",
        "sha": GIT_SHA,
        "path": "packages/core/src/lib.rs",
        "locator": {"symbol": "VirtualDom"},
        "contentSha256": SHA_A,
        "capture": {
            "capturedAt": "2026-09-17T12:00:00Z",
            "tool": "git-show",
            "trust": "untrusted",
            "instructionsExecuted": False,
        },
    }


def valid_provenance() -> JsonObject:
    return {
        "schemaVersion": 1,
        "artifacts": [
            {
                "artifactPath": "skills/dioxus-core/SKILL.md",
                "artifactClass": "dioxus-authored",
                "authorship": "hand-authored",
                "revisions": [
                    {
                        "revisionId": "core-1",
                        "status": "current",
                        "artifactSha256": SHA_B,
                        "capturedAt": "2026-09-17T12:00:00Z",
                        "sourcePin": source_pin(),
                        "docs": [
                            {
                                "url": "https://dioxuslabs.com/learn/0.7/",
                                "retrievedAt": "2026-09-17T11:00:00Z",
                                "contentSha256": SHA_A,
                            }
                        ],
                    }
                ],
            },
            {
                "artifactPath": "schemas/provenance-v1.schema.json",
                "artifactClass": "repository-config",
                "authorship": "hand-authored",
                "revisions": [
                    {
                        "revisionId": "schema-1",
                        "status": "current",
                        "artifactSha256": SHA_A,
                        "capturedAt": "2026-09-17T12:00:00Z",
                        "exception": {
                            "kind": "repository-config",
                            "reason": "Repository contract, not Dioxus guidance",
                        },
                        "docs": [],
                    }
                ],
            },
        ],
    }


def valid_report() -> JsonObject:
    return {
        "schemaVersion": 1,
        "reportId": "report-20260917",
        "createdAt": "2026-09-17T12:00:00Z",
        "sourceState": {
            "repository": "https://github.com/DioxusLabs/dioxus.git",
            "ref": "main",
            "sha": GIT_SHA,
            "dirty": False,
        },
        "sourcePrecedence": [
            "local-source",
            "local-tests-examples",
            "local-agents",
            "local-architecture",
            "upstream-source",
            "public-docs",
        ],
        "operations": [
            {
                "operation": "update",
                "artifactPath": "skills/dioxus-core/SKILL.md",
                "artifactClass": "dioxus-authored",
                "beforeSha256": SHA_A,
                "afterSha256": SHA_B,
                "provenanceRevisionId": "core-1",
                "verdict": "approved",
                "blockers": [],
            }
        ],
        "conflicts": [],
        "validation": {
            "verdict": "approved",
            "checks": [{"name": "source-pins", "verdict": "pass", "detail": "exact"}],
        },
    }
