"""Shared paths and provenance helpers for the BATS comparison."""

from __future__ import annotations

import hashlib
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HERE = ROOT / "benchmarks" / "v10_external"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def text(command: list[str]) -> str:
    return subprocess.run(
        command, cwd=ROOT, text=True, capture_output=True, check=True
    ).stdout.strip()


def parse_record(output: str) -> dict[str, str]:
    for line in reversed(output.splitlines()):
        if line.startswith("format=holos-bats-warm-v1 "):
            return dict(item.split("=", 1) for item in line.split())
    raise ValueError("BATS did not print a v1 record")


def source_changes() -> list[str]:
    ignored = (
        "benchmarks/results_v10_external.",
        "target/",
    )
    return [
        line
        for line in text(["git", "status", "--porcelain"]).splitlines()
        if not any(line[3:].startswith(prefix) for prefix in ignored)
    ]


def revision(path: Path) -> str:
    return text(["git", "-C", str(path), "rev-parse", "HEAD"])
