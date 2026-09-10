"""Corpus loading and study bindings."""

from __future__ import annotations

import hashlib
from pathlib import Path
import tomllib


def load_corpus(path: Path) -> dict:
    with path.open("rb") as source:
        return tomllib.load(source)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def parse_record(line: str) -> dict[str, str]:
    fields = dict(item.split("=", 1) for item in line.split())
    if fields.get("format") != "holos-public-program-bench-v1":
        raise SystemExit(f"unexpected benchmark record: {line}")
    return fields
