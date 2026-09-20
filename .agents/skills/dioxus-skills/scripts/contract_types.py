from __future__ import annotations

import json
from dataclasses import dataclass
from typing import TypeAlias


JsonValue: TypeAlias = (
    None | bool | int | float | str | list["JsonValue"] | dict[str, "JsonValue"]
)
JsonObject: TypeAlias = dict[str, JsonValue]


@dataclass(frozen=True, slots=True)
class ContractError(Exception):
    path: str
    detail: str

    def __str__(self) -> str:
        return f"{self.path}: {self.detail}"


def fail(path: str, detail: str) -> None:
    raise ContractError(path, detail)


def canonical_json(value: JsonValue) -> bytes:
    return (
        json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
        + "\n"
    ).encode("utf-8")
