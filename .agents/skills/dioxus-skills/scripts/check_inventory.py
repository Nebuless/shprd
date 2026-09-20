#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///

# How to run: `uv run scripts/check_inventory.py --root . --policy inventory.toml`

from __future__ import annotations

import argparse
from fnmatch import fnmatchcase
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Final


VALID_KINDS: Final = frozenset(
    {
        "authored",
        "cache",
        "fixture",
        "generated",
        "report",
        "vendored",
        "vendor-provenance",
        "workflow-state",
    }
)


@dataclass(frozen=True, slots=True)
class Rule:
    name: str
    owner: str
    kind: str
    paths: tuple[str, ...]
    exclude: tuple[str, ...]


@dataclass(frozen=True, slots=True)
class PolicyError(Exception):
    detail: str

    def __str__(self) -> str:
        return self.detail


def parse_string(value: str | None, field: str) -> str:
    if isinstance(value, str) and value:
        return value
    raise PolicyError(f"{field} must be a non-empty string")


def parse_paths(
    value: list[str] | None, field: str, *, required: bool
) -> tuple[str, ...]:
    if value is None and not required:
        return ()
    if (
        not isinstance(value, list)
        or not value
        or not all(isinstance(item, str) and item for item in value)
    ):
        raise PolicyError(f"{field} must be a non-empty string array")
    parsed = tuple(value)
    if any(path.startswith("/") or ".." in Path(path).parts for path in parsed):
        raise PolicyError(f"{field} contains unsafe path")
    return parsed


def load_policy(path: Path) -> tuple[Rule, ...]:
    try:
        raw = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, tomllib.TOMLDecodeError) as error:
        raise PolicyError(str(error)) from error
    if raw.get("version") != 1:
        raise PolicyError("version must equal 1")
    raw_rules = raw.get("rules")
    if not isinstance(raw_rules, list) or not raw_rules:
        raise PolicyError("rules must be a non-empty array")
    rules: list[Rule] = []
    for index, raw_rule in enumerate(raw_rules):
        if not isinstance(raw_rule, dict):
            raise PolicyError(f"rules[{index}] must be a table")
        kind = parse_string(raw_rule.get("kind"), f"rules[{index}].kind")
        if kind not in VALID_KINDS:
            raise PolicyError(f"rules[{index}].kind is unsupported")
        rules.append(
            Rule(
                name=parse_string(raw_rule.get("name"), f"rules[{index}].name"),
                owner=parse_string(raw_rule.get("owner"), f"rules[{index}].owner"),
                kind=kind,
                paths=parse_paths(
                    raw_rule.get("paths"), f"rules[{index}].paths", required=True
                ),
                exclude=parse_paths(
                    raw_rule.get("exclude"), f"rules[{index}].exclude", required=False
                ),
            )
        )
    return tuple(rules)


def path_matches(path: str, pattern: str) -> bool:
    return (
        (path == pattern.rstrip("/") or path.startswith(pattern))
        if pattern.endswith("/")
        else fnmatchcase(path, pattern)
    )


def classify(path: str, rules: tuple[Rule, ...]) -> tuple[Rule, ...]:
    return tuple(
        rule
        for rule in rules
        if any(path_matches(path, pattern) for pattern in rule.paths)
        and not any(path_matches(path, pattern) for pattern in rule.exclude)
    )


def repository_files(root: Path) -> tuple[str, ...]:
    return tuple(
        sorted(
            path.relative_to(root).as_posix()
            for path in root.rglob("*")
            if (path.is_file() or path.is_symlink())
            and ".git" not in path.relative_to(root).parts
        )
    )


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Verify repository ownership inventory"
    )
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--policy", type=Path, default=Path("inventory.toml"))
    arguments = parser.parse_args()
    try:
        rules = load_policy(arguments.policy)
    except PolicyError as error:
        print(f"invalid inventory policy: {error}", file=sys.stderr)
        return 2
    failures: list[str] = []
    counts: dict[str, int] = {rule.name: 0 for rule in rules}
    for path in repository_files(arguments.root.resolve()):
        matches = classify(path, rules)
        if len(matches) != 1:
            label = "unclassified" if not matches else "multiply classified"
            failures.append(f"{label}: {path}")
            continue
        counts[matches[0].name] += 1
    if failures:
        print("\n".join(failures), file=sys.stderr)
        return 1
    print(
        f"inventory ok: {sum(counts.values())} files across {sum(count > 0 for count in counts.values())} active rules"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
