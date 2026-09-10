"""Collect provenance for the external degree-Rips comparison."""

from __future__ import annotations

import hashlib
import os
import shutil
import subprocess
from argparse import Namespace
from pathlib import Path
from typing import Any


def collect(options: Namespace) -> dict[str, Any]:
    """Collect repository, input, binary, interpreter, and CPU provenance."""

    root = Path(options.root).resolve()
    status = _git(root, "status", "--porcelain", "--untracked-files=all")
    commit = _git(root, "rev-parse", "HEAD")
    interpreter = _resolve_executable(options.multipers_python)
    requirements = Path(options.requirements).resolve()
    return {
        "root": str(root),
        "commit": commit or "unknown",
        "tree_state": "clean" if status == "" else "dirty" if status else "unknown",
        "tree_entries": 0
        if status == ""
        else len(status.splitlines())
        if status
        else None,
        "input_sha256": _sha256(options.input),
        "holos_sha256": _sha256(options.holos),
        "external_python": interpreter,
        "external_environment": str(Path(interpreter).absolute().parent.parent),
        "external_python_sha256": _sha256(Path(interpreter)),
        "external_python_version": _version(interpreter),
        "child_sha256": _sha256(options.child),
        "requirements": _relative(root, requirements),
        "requirements_sha256": _sha256(requirements),
        "requested_affinity": options.affinity or "unconfigured",
        "effective_affinity": _effective_affinity(),
        "allow_dirty": os.environ.get("ALLOW_DIRTY") == "1",
        "recreate_commands": [
            "uv venv --python 3.12.12 /tmp/holos-multipers-validation",
            (
                "uv pip install --python /tmp/holos-multipers-validation/bin/python "
                f"--no-cache -r {_relative(root, requirements)}"
            ),
        ],
    }


def _git(root: Path, *arguments: str) -> str | None:
    try:
        completed = subprocess.run(
            ["git", "-C", str(root), *arguments],
            text=True,
            capture_output=True,
            check=False,
            timeout=10,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    if completed.returncode != 0:
        return None
    return completed.stdout.strip()


def _sha256(path: Path) -> str:
    if not path.is_file():
        return "missing"
    digest = hashlib.sha256()
    try:
        with path.open("rb") as stream:
            for block in iter(lambda: stream.read(1 << 20), b""):
                digest.update(block)
    except OSError:
        return "unreadable"
    return digest.hexdigest()


def _resolve_executable(path: Path) -> str:
    candidate = path.expanduser()
    if candidate.is_file():
        return str(candidate.absolute())
    located = shutil.which(str(path))
    return str(Path(located).absolute()) if located else str(path)


def _relative(root: Path, candidate: Path) -> str:
    try:
        return candidate.relative_to(root).as_posix()
    except ValueError:
        return candidate.name


def _version(executable: str) -> str:
    try:
        completed = subprocess.run(
            [executable, "--version"],
            text=True,
            capture_output=True,
            check=False,
            timeout=10,
        )
    except (OSError, subprocess.SubprocessError):
        return "unavailable"
    output = (completed.stdout + "\n" + completed.stderr).strip()
    return output if completed.returncode == 0 and output else "unavailable"


def _effective_affinity() -> str:
    getter = getattr(os, "sched_getaffinity", None)
    if getter is None:
        return "unavailable"
    try:
        return _format_cpus(sorted(getter(0)))
    except OSError:
        return "unavailable"


def _format_cpus(cpus: list[int]) -> str:
    if not cpus:
        return "empty"
    ranges = []
    start = previous = cpus[0]
    for cpu in cpus[1:]:
        if cpu == previous + 1:
            previous = cpu
            continue
        ranges.append(str(start) if start == previous else f"{start}-{previous}")
        start = previous = cpu
    ranges.append(str(start) if start == previous else f"{start}-{previous}")
    return ",".join(ranges)
