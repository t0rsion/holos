#!/usr/bin/env python3
"""Run the registered version 0.22 coverage study.

Usage: v22_coverage_bench.py [--confirm]

Each entry builds fenced communication states with redundant center sensors,
harmless candidate sensors, and a bounded failure model. Independent entries
separate state-action components. Coupled entries share every action across
all states. The study compares certified synthesis with flat subset search
and checks each proof in the separate crate.

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
CORPUS = HERE / "v22_coverage_corpus.toml"
MANIFEST = HERE / "results_v22_coverage_screen_manifest.txt"


def run(command):
    return subprocess.run(
        [str(item) for item in command],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=True,
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
    if values.get("format") != "holos-coverage-bench-v1":
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


def values_line(values):
    return " ".join(f"{key}={value}" for key, value in values.items())


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
        [*cargo, "build", "--release", "-p", "coverage-bench", "--locked"],
        cwd=ROOT,
        check=True,
    )
    binary = ROOT / "target/release/coverage-bench"
    dirty = [
        line
        for line in text(["git", "status", "--porcelain"]).splitlines()
        if "benchmarks/results_v22_coverage_" not in line
    ]
    if dirty and os.environ.get("ALLOW_DIRTY") != "1":
        raise SystemExit("worktree is dirty; commit first or set ALLOW_DIRTY=1")
    commit = text(["git", "rev-parse", "HEAD"])
    binary_hash = sha256(binary)
    with open(CORPUS, "rb") as source:
        corpus = tomllib.load(source)
    corpus_hash = sha256(CORPUS)
    expected_manifest = [
        f"commit={commit}",
        f"binary_sha256={binary_hash}",
        f"corpus_sha256={corpus_hash}",
        *[
            "entry="
            + entry["id"]
            + ":"
            + hashlib.sha256(repr(sorted(entry.items())).encode()).hexdigest()
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
            "taskset",
            "-c",
            affinity,
            binary,
            "--fixture",
            entry["fixture"],
            "--states",
            entry["states"],
            "--redundant",
            entry["redundant"],
            "--distractors",
            entry["distractors"],
            "--failures",
            entry["failures"],
            "--modulus",
            entry["modulus"],
            "--reps",
            reps,
        ]
        values = fields(run(command).stdout.strip())
        records.append((entry, command, values))

    exact = all(
        values["status"] == "optimal"
        and values["optimal_cost"] == values["lower_bound"]
        for _, _, values in records
    )
    proof = all(
        int(values["proof_checks"]) < int(values["oracle_calls"])
        and int(values["proof_nodes"]) > 0
        for _, _, values in records
    )
    search = all(
        int(values["oracle_calls"]) < int(values["flat_subsets"])
        for _, _, values in records
    )
    bounds = all(
        int(values["artifact_bytes"]) < 1 << 20
        and int(values["check_ns"]) < int(values["build_ns"])
        for _, _, values in records
    )
    components = all(
        int(values["components"])
        == (entry["states"] if entry["fixture"] == "independent" else 1)
        for entry, _, values in records
    )
    failures = all(int(values["failure_checks"]) > 0 for _, _, values in records)
    speed = all(
        int(values["build_ns"]) < int(values["flat_ns"])
        for entry, _, values in records
        if entry["fixture"] == "independent"
    )
    fields_seen = {int(values["modulus"]) for _, _, values in records}
    field_coverage = fields_seen == ({3, 5} if args.confirm else {2, 3, 5})
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
        f"binary_sha256={binary_hash}",
        f"source_dirty={'yes' if dirty else 'no'}",
        f"exact_decision={'pass' if exact else 'fail'}",
        f"proof_decision={'pass' if proof else 'fail'}",
        f"search_decision={'pass' if search else 'fail'}",
        f"bounds_decision={'pass' if bounds else 'fail'}",
        f"component_decision={'pass' if components else 'fail'}",
        f"failure_decision={'pass' if failures else 'fail'}",
        f"speed_decision={('pass' if speed else 'fail') if args.confirm else 'not_applicable'}",
        f"field_decision={'pass' if field_coverage else 'fail'}",
    ]
    raw = HERE / f"results_v22_coverage_{table}.txt"
    markdown = HERE / f"results_v22_coverage_{table}.md"
    raw.write_text(
        "\n".join(metadata + ["", *[values_line(values) for _, _, values in records]])
        + "\n"
    )
    rows = [
        "| entry | fixture | field | states/components | failures | activations/candidates | producer/frontier/proof/flat calls | artifact KiB | producer/checker/flat ms |",
        "|:--|:--|:--|--:|--:|--:|--:|--:|--:|",
    ]
    for entry, _, values in records:
        rows.append(
            f"| {entry['id']} | {values['fixture']} | Z/{values['modulus']} | "
            f"{values['states']}/{values['components']} | "
            f"{values['failure_budget']} | "
            f"{values['activations']}/{values['candidates']} | "
            f"{values['oracle_calls']}/{values['frontier_calls']}/"
            f"{values['proof_checks']}/{values['flat_subsets']} | "
            f"{int(values['artifact_bytes']) / 1024:.2f} | "
            f"{int(values['build_ns']) / 1e6:.3f}/"
            f"{int(values['check_ns']) / 1e6:.3f}/"
            f"{int(values['flat_ns']) / 1e6:.3f} |"
        )
    markdown.write_text(
        "<!-- Generated by benchmarks/v22_coverage_bench.py. Do not edit. -->\n\n"
        f"# Version 0.22 coverage {table}\n\n"
        + "\n".join(f"- {line}" for line in metadata)
        + "\n\n"
        + "\n".join(rows)
        + "\n"
    )
    passed = exact and proof and search and bounds and components and failures and field_coverage
    if args.confirm:
        passed = passed and speed
    if not passed:
        raise SystemExit("one or more registered decisions failed")
    if not args.confirm:
        MANIFEST.write_text("\n".join(expected_manifest) + "\n")
    print(markdown)


if __name__ == "__main__":
    main()
