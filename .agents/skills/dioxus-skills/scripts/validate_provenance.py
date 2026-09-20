#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///

# How to run: `uv run scripts/validate_provenance.py FILE [--kind provenance|report]`

from __future__ import annotations

import argparse
import json
import sys
import subprocess
from pathlib import Path

from contract_types import ContractError, JsonObject, JsonValue
from provenance_contract import parse_provenance
from report_contract import parse_report


def reject_duplicate_keys(pairs: list[tuple[str, JsonValue]]) -> JsonObject:
    result: JsonObject = {}
    for key, value in pairs:
        if key in result:
            raise ContractError("$", f"duplicate JSON key {key}")
        result[key] = value
    return result


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate Dioxus Skills provenance contracts"
    )
    parser.add_argument("file", type=Path, nargs="?")
    parser.add_argument("--report", dest="report_file", type=Path)
    parser.add_argument(
        "--kind", choices=("provenance", "report"), default="provenance"
    )
    parser.add_argument("--source", type=Path)
    parser.add_argument("--target", type=Path)
    parser.add_argument(
        "--all",
        action="store_true",
        help="Validate package mappings, live source pins and source/doc precedence",
    )
    parser.add_argument(
        "--legacy-only",
        action="store_true",
        help="Read legacy schema only; never validates inspection or apply preconditions",
    )
    arguments = parser.parse_args(sys.argv[2:] if sys.argv[1:2] == ["--"] else None)
    if arguments.report_file:
        arguments.file = arguments.report_file
        arguments.kind = "report"
    if arguments.all and (
        arguments.file or arguments.report_file or arguments.legacy_only
    ):
        parser.error("--all cannot be combined with file/report/legacy validation")
    if arguments.file is None and not arguments.all:
        parser.error("a report/provenance file is required")
    if arguments.legacy_only and (
        arguments.kind != "report" or arguments.source or arguments.target
    ):
        parser.error(
            "--legacy-only requires report kind and forbids live source/target checks"
        )
    try:
        if arguments.all:
            from validate_package import validate_package

            validate_package(
                arguments.target or Path.cwd(),
                arguments.source or Path("/root/repo/dioxus"),
            )
            print("valid package: provenance, live source pins, source/doc precedence")
            return 0
        raw = json.loads(
            arguments.file.read_text(encoding="utf-8"),
            object_pairs_hook=reject_duplicate_keys,
        )
        if not isinstance(raw, dict):
            raise ContractError("$", "must be an object")
        if arguments.kind == "provenance":
            parse_provenance(raw)
        else:
            parse_report(raw, legacy_only=arguments.legacy_only)
            from report_validation import validate_inspection

            if not arguments.legacy_only:
                validate_inspection(raw, arguments.target)
            if arguments.source:
                from report import validate_source_state

                validate_source_state(raw, arguments.source)
    except (
        OSError,
        UnicodeError,
        json.JSONDecodeError,
        ContractError,
        RuntimeError,
        subprocess.CalledProcessError,
    ) as error:
        print(f"invalid {arguments.kind}: {error}", file=sys.stderr)
        return 1
    print(
        f"{'legacy schema only; not inspection/apply valid' if arguments.legacy_only else 'valid ' + arguments.kind}: {arguments.file}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
