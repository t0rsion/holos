#!/usr/bin/env python3
"""Run the registered version 0.18 relative-index study.

Usage: v18_relative_index_bench.py [--confirm]

Each entry builds a filtered four-cycle with many attached triangles. The
separator tree has nested, noncontractible interfaces. The relative arm uses
filtered cores at every node. The control retains a complete reduction at
every node. Each timed update changes one leaf edge without crossing the
filtration threshold.

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
CORPUS = HERE / "v18_relative_index_corpus.toml"
MANIFEST = HERE / "results_v18_relative_index_screen_manifest.txt"


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
    if values.get("format") != "holos-relative-index-bench-v1":
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


def ratio(numerator, denominator):
    return int(numerator) / int(denominator)


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
        [*cargo, "build", "--release", "-p", "relative-index-bench", "--locked"],
        cwd=ROOT, check=True,
    )
    binary = ROOT / "target/release/relative-index-bench"
    dirty = [
        line for line in text(["git", "status", "--porcelain"]).splitlines()
        if "benchmarks/results_v18_relative_index_" not in line
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
            "--petals", entry["petals"],
            "--modulus", entry["modulus"],
            "--reps", reps,
        ]
        values = fields(run(command).stdout.strip())
        records.append((entry, values))

    exact = all(int(values["bars"]) > 0 for _, values in records)
    local = all(
        int(values["nodes_shared"]) > int(values["nodes"]) // 2
        and int(values["relative_nodes_rebuilt"]) == 1
        and int(values["relative_nodes_composed"]) > 0
        for _, values in records
    )
    compression = all(
        int(values["relative_core_cells"]) < int(values["relative_input_cells"])
        and int(values["relative_cancellations"]) > 0
        for _, values in records
    )
    update = all(
        ratio(values["materialized_update_ns"], values["relative_update_ns"]) >= 1.25
        for _, values in records
    )
    proofs = all(
        int(values["relative_snapshot_bytes"]) > 0
        and int(values["relative_delta_bytes"]) > 0
        and int(values["relative_verify_ns"]) > 0
        for _, values in records
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
        f"locality_decision={'pass' if local else 'fail'}",
        f"compression_decision={'pass' if compression else 'fail'}",
        f"update_decision={'pass' if update else 'fail'}",
        f"proof_decision={'pass' if proofs else 'fail'}",
    ]
    raw = HERE / f"results_v18_relative_index_{table}.txt"
    markdown = HERE / f"results_v18_relative_index_{table}.md"
    raw.write_text(
        "\n".join(metadata + ["", *[values_line(values) for _, values in records]]) + "\n"
    )
    rows = [
        "| entry | vertices | nodes/shared | cells, input/core | update, relative/control ms | compile, relative/control ms | proof, snapshot/delta KiB | check, relative/control ms |",
        "|:--|--:|--:|--:|--:|--:|--:|--:|",
    ]
    for entry, values in records:
        rows.append(
            f"| {entry['id']} | {values['vertices']} | "
            f"{values['nodes']}/{values['nodes_shared']} | "
            f"{values['relative_input_cells']}/{values['relative_core_cells']} | "
            f"{int(values['relative_update_ns']) / 1e6:.3f}/"
            f"{int(values['materialized_update_ns']) / 1e6:.3f} | "
            f"{int(values['relative_compile_ns']) / 1e6:.3f}/"
            f"{int(values['materialized_compile_ns']) / 1e6:.3f} | "
            f"{int(values['relative_snapshot_bytes']) / 1024:.2f}/"
            f"{int(values['relative_delta_bytes']) / 1024:.2f} | "
            f"{int(values['relative_verify_ns']) / 1e6:.3f}/"
            f"{int(values['materialized_verify_ns']) / 1e6:.3f} |"
        )
    markdown.write_text(
        "<!-- Generated by benchmarks/v18_relative_index_bench.py. Do not edit. -->\n\n"
        f"# Version 0.18 relative-index {table}\n\n"
        + "\n".join(f"- {line}" for line in metadata)
        + "\n\n" + "\n".join(rows) + "\n"
    )
    if not args.confirm:
        MANIFEST.write_text("\n".join(expected_manifest) + "\n")
    if not (exact and local and compression and update and proofs):
        raise SystemExit("one or more registered decisions failed")
    print(markdown)


def values_line(values):
    return " ".join(f"{key}={value}" for key, value in values.items())


if __name__ == "__main__":
    main()
