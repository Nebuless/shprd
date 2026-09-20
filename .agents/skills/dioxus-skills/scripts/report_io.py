from __future__ import annotations

import hashlib
import os
import stat
import subprocess
import secrets
from contextlib import contextmanager
from fcntl import LOCK_EX, LOCK_NB, flock
from pathlib import Path
from typing import Final, Iterator

from contract_types import (
    ContractError as ContractViolation,
    JsonObject,
    canonical_json,
)


class InspectionError(RuntimeError):
    def __init__(self, path: str, detail: str) -> None:
        super().__init__(f"{path}: {detail}")


ContractError = InspectionError

ROOT: Final = Path(__file__).resolve().parents[1]


def safe_path(path: Path) -> Path:
    if ".." in path.parts:
        raise ContractError(str(path), "unsafe path traversal")
    absolute = path.absolute()
    for part in (absolute, *absolute.parents):
        if part.is_symlink():
            raise ContractError(str(path), "unsafe symlink")
    return absolute


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


@contextmanager
def parent_descriptor(path: Path) -> Iterator[int]:
    absolute = safe_path(path)
    descriptor = os.open("/", os.O_RDONLY | os.O_DIRECTORY)
    try:
        for part in absolute.parent.parts[1:]:
            child = os.open(
                part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=descriptor
            )
            os.close(descriptor)
            descriptor = child
        yield descriptor
    finally:
        os.close(descriptor)


def read_regular(path: Path) -> bytes:
    with parent_descriptor(path) as parent:
        with os.fdopen(
            os.open(
                path.name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent
            ),
            "rb",
        ) as stream:
            info = os.fstat(stream.fileno())
            if not stat.S_ISREG(info.st_mode):
                raise ContractError(str(path), "must be a regular file")
            data = stream.read()
            after = os.fstat(stream.fileno())
            if (after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns) != (
                info.st_ino,
                info.st_size,
                info.st_mtime_ns,
                info.st_ctime_ns,
            ):
                raise ContractError(str(path), "file changed during read")
            return data


def git(source: Path, *arguments: str, check: bool = True) -> str:
    result = subprocess.run(
        [
            "git",
            "--no-optional-locks",
            "-c",
            "core.fsmonitor=false",
            "-C",
            str(source),
            *arguments,
        ],
        capture_output=True,
        text=True,
        check=check,
        timeout=120,
        env={**os.environ, "GIT_TERMINAL_PROMPT": "0", "GIT_OPTIONAL_LOCKS": "0"},
    )
    return result.stdout.strip()


def repository_identity(repository: str) -> str:
    return (
        repository.rstrip("/").removesuffix(".git")
        if "://" in repository
        else repository
    )


def state(source: Path) -> JsonObject:
    if not source.exists():
        raise ContractError(str(source), "missing source")
    safe_path(source)
    if git(source, "rev-parse", "--show-toplevel") != str(source):
        raise ContractError(str(source), "source must be Git repository root")
    return {
        "repository": git(source, "config", "--get", "remote.origin.url", check=False)
        or str(source),
        "ref": git(source, "symbolic-ref", "--short", "HEAD", check=False)
        or "DETACHED",
        "sha": git(source, "rev-parse", "HEAD"),
        "dirty": bool(git(source, "status", "--porcelain=v1", "--untracked-files=all")),
    }


def snapshot(source: Path) -> JsonObject:
    current = state(source)
    files: JsonObject = {}
    names = git(
        source, "ls-files", "-z", "--cached", "--others", "--exclude-standard"
    ).split("\0")
    for name in sorted(set(names) - {""}):
        path = safe_path(source / name)
        if path.is_file():
            files[name] = digest(read_regular(path))
        elif path.is_dir():
            files[name] = digest(canonical_json(snapshot(path)))
        else:
            files[name] = None
    return {
        "state": current,
        "files": files,
        "remotes": git(source, "remote", "-v"),
        "submodules": git(source, "submodule", "status", "--recursive"),
        "status": git(source, "status", "--porcelain=v1", "--untracked-files=all"),
        "configSha256": digest(git(source, "config", "--local", "--list").encode()),
    }


def disjoint(path: Path, roots: tuple[Path, ...]) -> None:
    safe_path(path)
    if any(
        path == root or root in path.parents or path in root.parents for root in roots
    ):
        raise ContractError(str(path), "unsafe overlapping path")


@contextmanager
def comparison_clone(
    path: Path, source: Path, target: Path
) -> Iterator[tuple[Path, str]]:
    path = safe_path(path)
    disjoint(path, (source, target, ROOT))
    path.parent.mkdir(parents=True, exist_ok=True)
    lock = safe_path(path.with_name(path.name + ".lock"))
    disjoint(lock, (source, target, ROOT))
    with (
        parent_descriptor(lock) as parent,
        os.fdopen(
            os.open(
                lock.name,
                os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW | os.O_NONBLOCK,
                0o600,
                dir_fd=parent,
            ),
            "a",
        ) as stream,
    ):
        lock_info = os.fstat(stream.fileno())
        if not stat.S_ISREG(lock_info.st_mode) or lock_info.st_nlink != 1:
            raise ContractError(str(lock), "unsafe lock")
        flock(stream, LOCK_EX | LOCK_NB)
        repository = str(state(source)["repository"])
        if path.exists():
            for candidate in path.rglob("*"):
                safe_path(candidate)
                if candidate.is_file() and candidate.stat().st_nlink != 1:
                    raise ContractError(str(candidate), "unsafe comparison hardlink")
            if (path / "objects/info/alternates").exists():
                raise ContractError(str(path), "comparison alternates forbidden")
            if git(path, "config", "--get", "remote.origin.url") != repository:
                raise ContractError(str(path), "comparison remote mismatch")
            if git(path, "rev-parse", "--is-bare-repository") != "true":
                raise ContractError(
                    str(path), "comparison must be dedicated bare clone"
                )
        else:
            subprocess.run(
                [
                    "git",
                    "clone",
                    "--bare",
                    "--no-hardlinks",
                    "--",
                    repository,
                    str(path),
                ],
                check=True,
                capture_output=True,
                timeout=120,
                env={**os.environ, "GIT_TERMINAL_PROMPT": "0"},
            )
        git(path, "fetch", "--no-tags", "--no-recurse-submodules", "origin", "HEAD")
        try:
            yield path, git(path, "rev-parse", "FETCH_HEAD")
        except ContractViolation as error:
            raise InspectionError(error.path, error.detail) from error
        if lock.stat().st_ino != lock_info.st_ino:
            raise ContractError(str(lock), "lock replaced during inspection")


def publish(path: Path, document: JsonObject) -> None:
    safe_path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with parent_descriptor(path) as parent:
        temporary = ".report-" + secrets.token_hex(16)
        try:
            with os.fdopen(
                os.open(
                    temporary,
                    os.O_CREAT | os.O_EXCL | os.O_WRONLY,
                    0o600,
                    dir_fd=parent,
                ),
                "wb",
            ) as stream:
                stream.write(canonical_json(document))
                stream.flush()
                os.fsync(stream.fileno())
            safe_path(path)
            os.replace(temporary, path.name, src_dir_fd=parent, dst_dir_fd=parent)
        finally:
            try:
                os.unlink(temporary, dir_fd=parent)
            except FileNotFoundError:
                temporary = ""
