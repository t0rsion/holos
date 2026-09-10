"""Corpus loading and immutable study bindings."""

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


def entry_binding(entry: dict) -> str:
    encoded = repr(sorted(entry.items())).encode()
    return hashlib.sha256(encoded).hexdigest()


def benchmark_command(
    binary: Path,
    entry: dict,
    repetitions: int,
    artifact_prefix: Path | None = None,
) -> list[str]:
    command = [
        str(binary),
        "--atoms",
        str(entry["atoms"]),
        "--atom-vertices",
        str(entry["atom_vertices"]),
        "--seed",
        str(entry["seed"]),
        "--steps",
        str(entry["steps"]),
        "--reps",
        str(repetitions),
        "--modulus",
        str(entry["modulus"]),
    ]
    if artifact_prefix is not None:
        command.extend(("--artifact-prefix", str(artifact_prefix)))
    return command


def parse_record(line: str) -> dict[str, str]:
    fields = dict(item.split("=", 1) for item in line.split())
    if fields.get("format") != "holos-program-bench-v2":
        raise SystemExit(f"unexpected benchmark record: {line}")
    return fields
