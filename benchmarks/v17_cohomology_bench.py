#!/usr/bin/env python3
"""Run the registered version 0.17 exact-class study.

Usage: v17_cohomology_bench.py [--confirm]

Each entry constructs a cross-polytope sphere in H1, H2, or H3. The study
checks its fixed-scale rank, the relation after a filling edge, the exact
affine crossing, a minimum finite-candidate intervention, independent replay,
and the rank from full persistence.

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
CORPUS = HERE / "v17_cohomology_corpus.toml"
MANIFEST = HERE / "results_v17_cohomology_screen_manifest.txt"


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
    if values.get("format") != "holos-cohomology-bench-v2":
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
        [*cargo, "build", "--release", "-p", "cohomology-bench", "--locked"],
        cwd=ROOT, check=True,
    )
    binary = ROOT / "target/release/cohomology-bench"
    dirty = [
        line for line in text(["git", "status", "--porcelain"]).splitlines()
        if "benchmarks/results_v17_cohomology_" not in line
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
            "--dimension", entry["dimension"],
            "--modulus", entry["modulus"],
            "--reps", reps,
        ]
        values = fields(run(command).stdout.strip())
        records.append((entry, command, values))

    rank = all(
        values["rank"] == values["persistence_rank"] == "1"
        and values["filled_rank"] == values["relation_rank"] == "0"
        and values["identity_isomorphism"] == "true"
        for _, _, values in records
    )
    events = all(
        values["threshold_events"] == values["event_before_rank"] == "1"
        and values["event_after_rank"] == "0"
        for _, _, values in records
    )
    interventions = all(
        values["intervention_status"] == "optimal"
        and values["edits"] == "1"
        and int(values["oracle_calls"]) > 0
        for _, _, values in records
    )
    proofs = all(int(values["proof_bytes"]) < 4096 for _, _, values in records)
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
        f"rank_decision={'pass' if rank else 'fail'}",
        f"event_decision={'pass' if events else 'fail'}",
        f"intervention_decision={'pass' if interventions else 'fail'}",
        f"proof_size_decision={'pass' if proofs else 'fail'}",
    ]
    raw = HERE / f"results_v17_cohomology_{table}.txt"
    markdown = HERE / f"results_v17_cohomology_{table}.md"
    raw.write_text(
        "\n".join(metadata + ["", *[values_line(values) for _, _, values in records]])
        + "\n"
    )
    rows = [
        "| entry | dimension | field | vertices/edges | proof bytes | space/relation us | event/intervention us | checker/persistence us |",
        "|:--|--:|:--|--:|--:|--:|--:|--:|",
    ]
    for entry, _, values in records:
        rows.append(
            f"| {entry['id']} | {values['dimension']} | Z/{values['modulus']} | "
            f"{values['vertices']}/{values['active_edges']} | {values['proof_bytes']} | "
            f"{int(values['cohomology_ns']) / 1e3:.3f}/{int(values['relation_ns']) / 1e3:.3f} | "
            f"{int(values['kinetic_ns']) / 1e3:.3f}/{int(values['intervention_ns']) / 1e3:.3f} | "
            f"{int(values['checker_ns']) / 1e3:.3f}/{int(values['persistence_ns']) / 1e3:.3f} |"
        )
    markdown.write_text(
        "<!-- Generated by benchmarks/v17_cohomology_bench.py. Do not edit. -->\n\n"
        f"# Version 0.17 exact-class {table}\n\n"
        + "\n".join(f"- {line}" for line in metadata)
        + "\n\n" + "\n".join(rows) + "\n"
    )
    if not args.confirm:
        MANIFEST.write_text("\n".join(expected_manifest) + "\n")
    if not (rank and events and interventions and proofs):
        raise SystemExit("one or more registered decisions failed")
    print(markdown)


def values_line(values):
    return " ".join(f"{key}={value}" for key, value in values.items())


if __name__ == "__main__":
    main()
