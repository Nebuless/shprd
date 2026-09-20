from __future__ import annotations

import hashlib
import re
from pathlib import PurePosixPath
from pathlib import Path
from typing import Final, Mapping

try:
    from .contract_types import JsonObject, JsonValue, fail
except ImportError:
    from contract_types import JsonObject, JsonValue, fail

SHA256: Final = re.compile(r"^[0-9a-f]{64}$")
GIT_SHA: Final = re.compile(r"^[0-9a-f]{40}$")
TIMESTAMP: Final = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$")
ARTIFACT_CLASSES: Final = frozenset(
    {
        "dioxus-authored",
        "repository-config",
        "report",
        "fixture",
        "generated",
        "vendor-guidance",
        "vendor-provenance",
    }
)
CLASS_ROOTS: Final = {
    "dioxus-authored": ("skills/", "references/", "templates/", "assets/"),
    "repository-config": (
        "schemas/",
        "scripts/",
        "docs/",
        "mise.toml",
        "inventory.toml",
        "templates/",
        "SKILL.md",
    ),
    "report": ("reports/", ".omo/evidence/"),
    "fixture": ("fixtures/", "tests/fixtures/"),
    "generated": ("generated/",),
    "vendor-guidance": (".agents/skills/",),
    "vendor-provenance": ("skills-lock.json",),
}


def expect_object(value: JsonValue, path: str) -> JsonObject:
    if not isinstance(value, dict):
        fail(path, "must be an object")
    return value


def expect_list(value: JsonValue, path: str) -> list[JsonValue]:
    if not isinstance(value, list):
        fail(path, "must be an array")
    return value


def exact_keys(
    value: JsonObject, required: set[str], optional: set[str], path: str
) -> None:
    missing = sorted(required - value.keys())
    unknown = sorted(value.keys() - required - optional)
    if missing:
        fail(path, f"missing field {missing[0]}")
    if unknown:
        fail(path, f"unknown field {unknown[0]}")


def expect_string(value: JsonValue, path: str) -> str:
    if not isinstance(value, str) or not value:
        fail(path, "must be a non-empty string")
    return value


def expect_enum(value: JsonValue, allowed: frozenset[str] | set[str], path: str) -> str:
    parsed = expect_string(value, path)
    if parsed not in allowed:
        fail(path, f"must be one of {', '.join(sorted(allowed))}")
    return parsed


def expect_pattern(value: JsonValue, pattern: re.Pattern[str], path: str) -> str:
    parsed = expect_string(value, path)
    if pattern.fullmatch(parsed) is None:
        fail(path, "has invalid format")
    return parsed


def expect_path(value: JsonValue, path: str) -> str:
    parsed = expect_string(value, path)
    parts = PurePosixPath(parsed).parts
    if (
        parsed.startswith("/")
        or ".." in parts
        or "." in parts
        or "//" in parsed
        or "\\" in parsed
    ):
        fail(path, "must be a safe relative path")
    return parsed


def validate_docs(value: JsonValue, path: str) -> None:
    for index, item in enumerate(expect_list(value, path)):
        item_path = f"{path}[{index}]"
        doc = expect_object(item, item_path)
        exact_keys(doc, {"url", "retrievedAt", "contentSha256"}, set(), item_path)
        if not expect_string(doc["url"], f"{item_path}.url").startswith("https://"):
            fail(f"{item_path}.url", "must use https")
        expect_pattern(doc["retrievedAt"], TIMESTAMP, f"{item_path}.retrievedAt")
        expect_pattern(doc["contentSha256"], SHA256, f"{item_path}.contentSha256")


def validate_source_pin(value: JsonValue, path: str) -> None:
    pin = expect_object(value, path)
    exact_keys(
        pin,
        {"repository", "ref", "sha", "path", "locator", "contentSha256", "capture"},
        set(),
        path,
    )
    expect_string(pin["repository"], f"{path}.repository")
    expect_string(pin["ref"], f"{path}.ref")
    expect_pattern(pin["sha"], GIT_SHA, f"{path}.sha")
    expect_path(pin["path"], f"{path}.path")
    expect_pattern(pin["contentSha256"], SHA256, f"{path}.contentSha256")
    locator = expect_object(pin["locator"], f"{path}.locator")
    if set(locator) == {"symbol"}:
        expect_string(locator["symbol"], f"{path}.locator.symbol")
    elif set(locator) == {"lines"}:
        lines = expect_object(locator["lines"], f"{path}.locator.lines")
        exact_keys(lines, {"start", "end"}, set(), f"{path}.locator.lines")
        start, end = lines["start"], lines["end"]
        if (
            isinstance(start, bool)
            or not isinstance(start, int)
            or isinstance(end, bool)
            or not isinstance(end, int)
            or start < 1
            or end < start
        ):
            fail(f"{path}.locator.lines", "must be an ordered positive range")
    else:
        fail(f"{path}.locator", "must contain exactly symbol or lines")
    capture = expect_object(pin["capture"], f"{path}.capture")
    exact_keys(
        capture,
        {"capturedAt", "tool", "trust", "instructionsExecuted"},
        set(),
        f"{path}.capture",
    )
    expect_pattern(capture["capturedAt"], TIMESTAMP, f"{path}.capture.capturedAt")
    expect_string(capture["tool"], f"{path}.capture.tool")
    if capture["trust"] != "untrusted" or capture["instructionsExecuted"] is not False:
        fail(f"{path}.capture", "untrusted source instructions must not be executed")


def validate_revision(
    value: JsonValue, artifact_class: str, path: str
) -> tuple[str, str, str | None]:
    revision = expect_object(value, path)
    exact_keys(
        revision,
        {"revisionId", "status", "artifactSha256", "capturedAt", "docs"},
        {"sourcePin", "exception", "supersededBy"},
        path,
    )
    revision_id = expect_string(revision["revisionId"], f"{path}.revisionId")
    status = expect_enum(
        revision["status"], {"current", "superseded"}, f"{path}.status"
    )
    expect_pattern(revision["artifactSha256"], SHA256, f"{path}.artifactSha256")
    expect_pattern(revision["capturedAt"], TIMESTAMP, f"{path}.capturedAt")
    validate_docs(revision["docs"], f"{path}.docs")
    superseded_by = None
    if status == "superseded":
        superseded_by = expect_string(
            revision.get("supersededBy"), f"{path}.supersededBy"
        )
    elif "supersededBy" in revision:
        fail(
            f"{path}.supersededBy",
            "only superseded revisions may reference a successor",
        )
    if artifact_class == "dioxus-authored":
        if "exception" in revision or "sourcePin" not in revision:
            fail(
                path,
                "dioxus-authored revision requires sourcePin and forbids exception",
            )
        validate_source_pin(revision["sourcePin"], f"{path}.sourcePin")
    else:
        if "sourcePin" in revision or "exception" not in revision:
            fail(path, "non-Dioxus revision requires exception and forbids sourcePin")
        exception = expect_object(revision["exception"], f"{path}.exception")
        exact_keys(exception, {"kind", "reason"}, set(), f"{path}.exception")
        if exception["kind"] != artifact_class:
            fail(f"{path}.exception.kind", "must match artifactClass")
        expect_string(exception["reason"], f"{path}.exception.reason")
    return revision_id, status, superseded_by


def _content_sha256(value: bytes | str) -> str:
    data = value.encode("utf-8") if isinstance(value, str) else value
    return hashlib.sha256(data).hexdigest()


def _available_content(
    path: str,
    content_root: Path | None,
    content_files: Mapping[str, bytes | str] | None,
) -> bytes | str | None:
    if content_files is not None and path in content_files:
        return content_files[path]
    if content_root is not None:
        candidate = content_root / path
        if candidate.is_file():
            return candidate.read_bytes()
    return None


def parse_provenance(
    raw: JsonObject,
    *,
    content_root: Path | None = None,
    content_files: Mapping[str, bytes | str] | None = None,
) -> JsonObject:
    exact_keys(raw, {"schemaVersion", "artifacts"}, set(), "$")
    if raw["schemaVersion"] != 1:
        fail("$.schemaVersion", "must equal 1")
    artifacts = expect_list(raw["artifacts"], "$.artifacts")
    if not artifacts:
        fail("$.artifacts", "must not be empty")
    seen_paths: set[str] = set()
    for index, item in enumerate(artifacts):
        path = f"$.artifacts[{index}]"
        artifact = expect_object(item, path)
        exact_keys(
            artifact,
            {"artifactPath", "artifactClass", "authorship", "revisions"},
            set(),
            path,
        )
        artifact_path = expect_path(artifact["artifactPath"], f"{path}.artifactPath")
        if artifact_path in seen_paths:
            fail(
                f"{path}.artifactPath",
                "duplicate artifactPath creates ambiguous mapping",
            )
        seen_paths.add(artifact_path)
        artifact_class = expect_enum(
            artifact["artifactClass"], ARTIFACT_CLASSES, f"{path}.artifactClass"
        )
        if not any(
            artifact_path == root or artifact_path.startswith(root)
            for root in CLASS_ROOTS[artifact_class]
        ):
            fail(
                f"{path}.artifactPath",
                f"is not approved for artifactClass {artifact_class}",
            )
        authorship = expect_enum(
            artifact["authorship"], {"hand-authored", "generated"}, f"{path}.authorship"
        )
        if (artifact_class == "generated") != (authorship == "generated"):
            fail(f"{path}.authorship", "must agree with generated artifactClass")
        revisions = expect_list(artifact["revisions"], f"{path}.revisions")
        if not revisions:
            fail(f"{path}.revisions", "must not be empty")
        identities: set[str] = set()
        current = 0
        links: list[tuple[str, str]] = []
        for revision_index, revision in enumerate(revisions):
            revision_id, status, successor = validate_revision(
                revision, artifact_class, f"{path}.revisions[{revision_index}]"
            )
            if revision_id in identities:
                fail(f"{path}.revisions[{revision_index}].revisionId", "must be unique")
            identities.add(revision_id)
            current += status == "current"
            if successor is not None:
                links.append((revision_id, successor))
        if current != 1:
            fail(f"{path}.revisions", "must contain exactly one current revision")
        revision_map = {item["revisionId"]: item for item in revisions}
        for revision in revisions:
            available = _available_content(artifact_path, content_root, content_files)
            if available is not None and revision["artifactSha256"] != _content_sha256(
                available
            ):
                fail(
                    f"{path}.revisions",
                    "artifactSha256 does not match supplied content",
                )
        successors = dict(links)
        for revision_id in identities:
            seen: set[str] = set()
            cursor = revision_id
            while cursor in successors:
                if cursor in seen:
                    fail(f"{path}.revisions", "supersession graph must be acyclic")
                seen.add(cursor)
                cursor = successors[cursor]
            if cursor not in revision_map:
                fail(
                    f"{path}.revisions",
                    "supersededBy must reference a retained revision",
                )
            if revision_map[cursor]["status"] != "current":
                fail(
                    f"{path}.revisions",
                    "every supersession chain must end at current leaf",
                )
        for previous, successor in links:
            if successor not in identities or [
                item["revisionId"] for item in revisions
            ].index(successor) <= [item["revisionId"] for item in revisions].index(
                previous
            ):
                fail(
                    f"{path}.revisions",
                    "supersededBy must reference a later retained revision",
                )
    return raw
