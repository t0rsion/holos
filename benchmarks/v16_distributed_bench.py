#!/usr/bin/env python3
"""Run the registered version 0.16 durable-composition study.

Usage: v16_distributed_bench.py [--confirm]

Each entry stores ordered relative-interface shards, composes them through a
noncontractible four-cycle, removes the final manifest, and resumes from the
durable fold prefix. The checker then reloads proof objects by content id.

Environment:
  CARGO          Cargo command, including an optional toolchain.
  REPS           Timed repetitions per arm. The default and minimum are 5.
  INDEX_AFFINITY CPUs for the study. The default is ``0-3,12-15``.
  ALLOW_DIRTY    Set to 1 to record a dirty source tree.
"""

from __future__ import annotations

import argparse
import hashlib
import os
from pathlib import Path
import platform
import subprocess
import tomllib


ROOT = Path(__file__).resolve().parent.parent
HERE = ROOT / "benchmarks"
CORPUS = HERE / "v16_distributed_corpus.toml"
MANIFEST = HERE / "results_v16_distributed_screen_manifest.txt"


def run(command):
    return subprocess.run(
        [str(item) for item in command], cwd=ROOT, text=True,
        capture_output=True, check=True,
    )


def text(command):
    return run(command).stdout.strip()


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as source:
        for block in iter(lambda: source.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def fields(line):
    values = dict(item.split("=", 1) for item in line.split())
    if values.get("format") != "holos-distributed-bench-v1":
        raise SystemExit(f"unexpected benchmark record: {line}")
    return values


def cpu_model():
    try:
        for line in Path("/proc/cpuinfo").read_text().splitlines():
            if line.startswith("model name"):
                return line.split(":", 1)[1].strip()
    except OSError:
        pass
    return platform.processor() or "unknown"


def main():
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--confirm", action="store_true")
    parser.add_argument("-h", "--help", action="store_true")
    args = parser.parse_args()
    if args.help:
        print(__doc__)
        return

    reps = int(os.environ.get("REPS", "5"))
    if reps < 5:
        raise SystemExit("REPS must be at least 5")
    cargo = os.environ.get("CARGO", "cargo").split()
    affinity = os.environ.get("INDEX_AFFINITY", "0-3,12-15")
    subprocess.run(
        [*cargo, "build", "--release", "-p", "distributed-bench", "--locked"],
        cwd=ROOT, check=True,
    )
    binary = ROOT / "target/release/distributed-bench"
    dirty = [
        line for line in text(["git", "status", "--porcelain"]).splitlines()
        if "benchmarks/results_v16_distributed_" not in line
    ]
    if dirty and os.environ.get("ALLOW_DIRTY") != "1":
        raise SystemExit("worktree is dirty; commit first or set ALLOW_DIRTY=1")
    commit = text(["git", "rev-parse", "HEAD"])
    with open(CORPUS, "rb") as source:
        corpus = tomllib.load(source)
    corpus_hash = sha256(CORPUS)
    expected_manifest = [
        f"commit={commit}",
        f"corpus_sha256={corpus_hash}",
        *[
            f"entry={entry['id']}:{hashlib.sha256(repr(sorted(entry.items())).encode()).hexdigest()}"
            for entry in corpus["screen"]
        ],
    ]
    if args.confirm and (
        not MANIFEST.is_file()
        or MANIFEST.read_text().splitlines() != expected_manifest
    ):
        raise SystemExit("confirmation needs the matching complete screen manifest")

    table = "confirm" if args.confirm else "screen"
    records = []
    for entry in corpus[table]:
        command = [
            "taskset", "-c", affinity, binary,
            "--shards", entry["shards"],
            "--gadgets", entry["gadgets"],
            "--modulus", entry["modulus"],
            "--reps", reps,
        ]
        values = fields(run(command).stdout.strip())
        records.append((entry, command, values))

    exact = all(int(values["h1_bars"]) == 1 for _, _, values in records)
    recovery = all(
        int(values["folds_reused"]) == int(values["shards"])
        and int(values["recovery_ns"]) < int(values["clean_ns"])
        for _, _, values in records
    )
    bounded = all(
        int(values["peak_artifact_bytes"]) < int(values["total_shard_bytes"])
        for _, _, values in records
    )
    compression = all(
        int(values["shard_core_cells"]) < int(values["shard_input_cells"])
        for _, _, values in records
    )
    metadata = [
        f"commit={commit}",
        f"corpus={CORPUS.name}",
        f"corpus_version={corpus['meta']['version']}",
        f"corpus_sha256={corpus_hash}",
        f"set={table}",
        f"repetitions={reps}",
        f"affinity={affinity}",
        f"cpu={cpu_model()}",
        f"platform={platform.platform()}",
        f"python={platform.python_version()}",
        f"cargo={text([*cargo, '--version'])}",
        f"binary_sha256={sha256(binary)}",
        f"source_dirty={'yes' if dirty else 'no'}",
        f"exact_decision={'pass' if exact else 'fail'}",
        f"recovery_decision={'pass' if recovery else 'fail'}",
        f"bounded_memory_decision={'pass' if bounded else 'fail'}",
        f"compression_decision={'pass' if compression else 'fail'}",
    ]
    raw = HERE / f"results_v16_distributed_{table}.txt"
    markdown = HERE / f"results_v16_distributed_{table}.md"
    raw.write_text(
        "\n".join(metadata + ["", *[values_line(values) for _, _, values in records]])
        + "\n"
    )
    rows = [
        "| entry | shards | vertices | field | shard cells, input/core | peak/total shard bytes | clean/recovery ms | check/control ms |",
        "|:--|--:|--:|:--|--:|--:|--:|--:|",
    ]
    for entry, _, values in records:
        rows.append(
            f"| {entry['id']} | {values['shards']} | {values['vertices']} | "
            f"Z/{values['modulus']} | "
            f"{values['shard_input_cells']}/{values['shard_core_cells']} | "
            f"{values['peak_artifact_bytes']}/{values['total_shard_bytes']} | "
            f"{int(values['clean_ns']) / 1e6:.3f}/{int(values['recovery_ns']) / 1e6:.3f} | "
            f"{int(values['check_ns']) / 1e6:.3f}/{int(values['materialized_ns']) / 1e6:.3f} |"
        )
    markdown.write_text(
        "<!-- Generated by benchmarks/v16_distributed_bench.py. Do not edit. -->\n\n"
        f"# Version 0.16 durable-composition {table}\n\n"
        + "\n".join(f"- {line}" for line in metadata)
        + "\n\n" + "\n".join(rows) + "\n"
    )
    if not args.confirm:
        MANIFEST.write_text("\n".join(expected_manifest) + "\n")
    if not (exact and recovery and bounded and compression):
        raise SystemExit("one or more registered decisions failed")
    print(markdown)


def values_line(values):
    return " ".join(f"{key}={value}" for key, value in values.items())


if __name__ == "__main__":
    main()
