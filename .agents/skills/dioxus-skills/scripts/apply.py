#!/usr/bin/env python3
# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///
# How to run: mise run apply -- --report PATH --confirm
from __future__ import annotations

import argparse
import json
import os
import stat
import subprocess
import sys
from contextlib import ExitStack
from dataclasses import dataclass
from fcntl import LOCK_EX, LOCK_NB, flock
from pathlib import Path

from contract_types import ContractError, JsonObject
from provenance_contract import expect_list, expect_object, expect_string
from report import validate_source_state
from report_contract import parse_report
from report_discovery import blob, discover, inspect_artifact, target_manifest
from report_io import (
    ROOT,
    digest,
    disjoint,
    git,
    parent_descriptor,
    read_regular,
    safe_path,
    state,
    snapshot,
)
from report_validation import inspection, validate_inspection
from validate_provenance import reject_duplicate_keys


@dataclass(frozen=True, slots=True)
class Write:
    path: Path
    content: bytes
    before: bytes | None


def load_report(path: Path) -> JsonObject:
    report = parse_report(
        expect_object(
            json.loads(
                read_regular(safe_path(path)), object_pairs_hook=reject_duplicate_keys
            ),
            "report",
        )
    )
    validate_inspection(report)
    validation = expect_object(report["validation"], "validation")
    if validation["verdict"] != "approved" or any(
        expect_object(check, "check")["verdict"] != "pass"
        for check in expect_list(validation["checks"], "checks")
    ):
        raise ContractError("report", "all validation checks must pass")
    return report


def validate_output(target: Path, expected: JsonObject) -> None:
    if target_manifest(target, list(expected)) != expected:
        raise ContractError("target", "post-apply hash validation failed")
    if (target / "SKILL.md").exists():
        subprocess.run(
            ["node", str(ROOT / "scripts/validate-skills.mjs"), "--root", str(target)],
            check=True,
            capture_output=True,
            timeout=120,
        )


def apply_report(report_path: Path, target: Path, source: Path) -> str:
    target, source = safe_path(target), safe_path(source)
    disjoint(target, (source, Path("/root/repo/dioxus")))
    report = load_report(report_path)
    if state(source)["dirty"]:
        raise ContractError("source", "clean source required")
    validate_source_state(report, source)
    if git(target, "rev-parse", "--show-toplevel") != str(target):
        raise ContractError("target", "repository root required")
    git_directory = safe_path(Path(git(target, "rev-parse", "--absolute-git-dir")))
    disjoint(git_directory, (source, Path("/root/repo/dioxus")))
    with ExitStack() as stack:
        lock = os.open(git_directory, os.O_RDONLY | os.O_DIRECTORY)
        stack.callback(os.close, lock)
        flock(lock, LOCK_EX | LOCK_NB)
        try:
            return transact(report, (target, source), stack)
        except ContractError as error:
            raise RuntimeError(str(error)) from None


def transact(report: JsonObject, roots: tuple[Path, Path], stack: ExitStack) -> str:
    target, source = roots
    evidence = inspection(report)
    if evidence is None:
        raise ContractError("report", "inspection required")
    operations = [
        expect_object(item, "operation")
        for item in expect_list(report["operations"], "operations")
    ]
    mapping = safe_path(target / "generated/provenance.json")
    artifacts = discover(target, mapping)
    ledger: JsonObject = {"schemaVersion": 1, "artifacts": list(artifacts)}
    parse_report(report, provenance=ledger)
    if [item["artifactPath"] for item in artifacts] != [
        item["artifactPath"] for item in operations
    ]:
        raise ContractError("target", "unexpected artifact paths")
    expected: JsonObject = {}
    writes: list[Write] = []
    details = [
        expect_object(item, "detail")
        for item in expect_list(evidence["artifacts"], "artifacts")
    ]
    for operation, artifact, detail in zip(operations, artifacts, details):
        path = expect_string(operation["artifactPath"], "artifactPath")
        if operation["verdict"] != "approved" or (
            operation["artifactClass"] != "dioxus-authored"
            and operation["operation"] != "noop"
        ):
            raise ContractError(path, "only approved authored artifacts allowed")
        observed, live_detail = inspect_artifact(artifact, (source, target), None)
        if live_detail.get("revision") != detail.get("revision"):
            raise ContractError(path, "report revision differs from live provenance")
        if (
            observed["verdict"] != "approved"
            or observed["afterSha256"] != operation["afterSha256"]
        ):
            raise ContractError(path, "live provenance differs")
        expected[path] = operation["afterSha256"]
        if operation["artifactClass"] != "dioxus-authored":
            if observed["operation"] != "noop":
                raise ContractError(path, "non-authored artifact must remain unchanged")
            continue
        candidate = safe_path(target / path)
        before = read_regular(candidate) if candidate.exists() else None
        if before is not None and candidate.stat().st_nlink != 1:
            raise ContractError(path, "hardlink forbidden")
        payload = (
            before
            if before is not None and digest(before) == operation["afterSha256"]
            else read_regular(
                safe_path(
                    target
                    / "generated/apply-blobs"
                    / expect_string(operation["afterSha256"], "hash")
                )
            )
        )
        if digest(payload) != operation["afterSha256"]:
            raise ContractError(path, "payload hash mismatch")
        writes.append(Write(candidate, payload, before))
    actual = target_manifest(target, list(expected))
    complete = actual == expected
    dirty = bool(git(target, "status", "--porcelain=v1", "--untracked-files=all"))
    if dirty:
        changed = set(
            filter(
                None,
                git(target, "diff", "HEAD", "--name-only", "-z", "--no-renames").split(
                    "\0"
                ),
            )
        )
        untracked = set(
            filter(
                None,
                git(target, "ls-files", "--others", "--exclude-standard", "-z").split(
                    "\0"
                ),
            )
        )
        intended = {
            str(item["artifactPath"])
            for item in operations
            if item["beforeSha256"] != item["afterSha256"]
        }
        if (
            not complete
            or changed | untracked != intended
            or git(target, "diff", "--cached", "--name-only")
        ):
            raise ContractError(
                "target", "clean target required (except exact completed rerun)"
            )
        for operation in operations:
            original = blob(target, "HEAD", str(operation["artifactPath"]))
            if (digest(original) if original is not None else None) != operation[
                "beforeSha256"
            ]:
                raise ContractError("target", "rerun baseline mismatch")
        if git(target, "diff", "--summary"):
            raise ContractError("target", "mode changes forbidden")
    if complete:
        validate_output(target, expected)
        validate_source_state(report, source)
        return "already applied; no changes"
    validate_inspection(report, target)
    original_mapping = read_regular(mapping)
    baseline = snapshot(target)
    validate_source_state(report, source)
    created: list[Path] = []
    opened: list[tuple[Write, int, int]] = []
    touched: list[tuple[Write, int, int]] = []
    succeeded = False
    try:
        if snapshot(target) != baseline:
            raise ContractError("target", "target changed before transaction")
        for write in writes:
            if write.before == write.content:
                continue
            missing = []
            parent = write.path.parent
            while not parent.exists():
                missing.append(parent)
                parent = parent.parent
            for directory in reversed(missing):
                safe_path(directory).mkdir()
                created.append(directory)
            parent_fd = stack.enter_context(parent_descriptor(write.path))
            flags = os.O_RDWR | os.O_NOFOLLOW | os.O_NONBLOCK
            if write.before is None:
                flags |= os.O_CREAT | os.O_EXCL
            descriptor = os.open(write.path.name, flags, 0o644, dir_fd=parent_fd)
            stream = stack.enter_context(os.fdopen(descriptor, "r+b"))
            if write.before is None:
                touched.append((write, descriptor, parent_fd))
            info = os.fstat(descriptor)
            if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
                raise ContractError(str(write.path), "unsafe write destination")
            opened.append((write, descriptor, parent_fd))
            if write.before is not None and stream.read() != write.before:
                raise ContractError(str(write.path), "target changed before write")
        validate_source_state(report, source)
        if read_regular(mapping) != original_mapping:
            raise ContractError("provenance", "mapping changed")
        current = snapshot(target)
        baseline_files = expect_object(baseline["files"], "files")
        current_files = expect_object(current["files"], "files")
        for write in writes:
            name = write.path.relative_to(target).as_posix()
            if write.before is None:
                current_files.pop(name, None)
        if (
            current_files != baseline_files
            or current["configSha256"] != baseline["configSha256"]
        ):
            raise ContractError("target", "target drift during staging")
        for write, descriptor, parent_fd in opened:
            if write.before is not None:
                os.lseek(descriptor, 0, os.SEEK_SET)
                with os.fdopen(os.dup(descriptor), "rb") as original:
                    if original.read() != write.before:
                        raise ContractError(
                            str(write.path), "target changed before write"
                        )
            if write.before is not None:
                touched.append((write, descriptor, parent_fd))
            os.lseek(descriptor, 0, os.SEEK_SET)
            with os.fdopen(os.dup(descriptor), "wb") as output:
                output.write(write.content)
                output.flush()
                os.ftruncate(descriptor, len(write.content))
                os.fsync(descriptor)
        validate_output(target, expected)
        validate_source_state(report, source)
        after = snapshot(target)
        after_files = expect_object(after["files"], "files")
        if (
            after_files != {**baseline_files, **expected}
            or after["configSha256"] != baseline["configSha256"]
            or state(target)["sha"] != expect_object(baseline["state"], "state")["sha"]
        ):
            raise ContractError("target", "unexpected post-apply drift")
        if git(target, "diff", "--cached", "--name-only"):
            raise ContractError("target", "index changed during apply")
        succeeded = True
    finally:
        if not succeeded:
            for write, descriptor, parent_fd in reversed(touched):
                if write.before is None:
                    os.unlink(write.path.name, dir_fd=parent_fd)
                else:
                    os.lseek(descriptor, 0, os.SEEK_SET)
                    with os.fdopen(os.dup(descriptor), "wb") as output:
                        output.write(write.before)
                        output.flush()
                        os.ftruncate(descriptor, len(write.before))
                        os.fsync(descriptor)
            for directory in reversed(created):
                directory.rmdir()
    return f"applied {len(opened)} authored artifacts; post-validation passed"


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Explicit guarded authored-artifact apply"
    )
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--confirm", action="store_true", required=True)
    arguments = parser.parse_args(sys.argv[2:] if sys.argv[1:2] == ["--"] else None)
    try:
        report = load_report(arguments.report)
        repository = expect_string(
            expect_object(report["sourceState"], "sourceState")["repository"],
            "repository",
        )
        source = (
            Path(repository)
            if Path(repository).is_absolute()
            else Path(os.environ.get("DIOXUS_SOURCE", "/root/repo/dioxus"))
        )
        print(apply_report(arguments.report, Path.cwd(), source))
        return 0
    except (
        OSError,
        ValueError,
        RuntimeError,
        ContractError,
        subprocess.SubprocessError,
    ) as error:
        print(f"apply failed: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
