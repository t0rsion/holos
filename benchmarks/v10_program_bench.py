#!/usr/bin/env python3
"""Run the registered version 0.10 persistence-program study.

Usage: v10_program_bench.py [--confirm]

The screen must finish before confirmation. Each invocation measures two
counterbalanced pairs over the same cumulative trajectories. Accepted program
evaluation is compared with exact diagram recomputation. Checked one-atom
repair is compared with exact diagram and canonical-class recomputation.
Every pair must agree bit for bit before a timing record is printed.

Environment:
  CARGO             Cargo command, including an optional toolchain. The
                    default is ``cargo``.
  REPS              Timed repetitions per arm. The default and minimum are 5.
  PROGRAM_AFFINITY  CPUs for the study. The default is ``0-3,12-15``.
  ALLOW_DIRTY       Set to 1 to record a dirty source tree. Such a record is
                    void for release claims.
"""

from __future__ import annotations

import argparse
import hashlib
import os
from pathlib import Path
import platform
import shlex
import statistics
import subprocess
import tomllib


ROOT = Path(__file__).resolve().parent.parent
HERE = ROOT / "benchmarks"
CORPUS = HERE / "v10_program_corpus.toml"
MANIFEST = HERE / "results_v10_program_screen_manifest.txt"


def run(command, **kwargs):
    return subprocess.run(
        [str(item) for item in command],
        text=True,
        capture_output=True,
        check=True,
        **kwargs,
    )


def text(command, **kwargs):
    return run(command, **kwargs).stdout.strip()


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as source:
        for block in iter(lambda: source.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def cpu_model():
    try:
        for line in Path("/proc/cpuinfo").read_text().splitlines():
            if line.startswith("model name"):
                return line.split(":", 1)[1].strip()
    except OSError:
        pass
    return platform.processor() or "unknown"


def command(binary, entry, reps):
    return [
        binary,
        "--atoms", entry["atoms"],
        "--atom-vertices", entry["atom_vertices"],
        "--seed", entry["seed"],
        "--steps", entry["steps"],
        "--reps", reps,
        "--modulus", entry["modulus"],
    ]


def fields(line):
    values = {}
    for item in line.split():
        key, value = item.split("=", 1)
        values[key] = value
    if values.get("format") != "holos-program-bench-v1":
        raise SystemExit(f"unexpected benchmark record: {line}")
    return values


def entry_binding(entry):
    return hashlib.sha256(repr(sorted(entry.items())).encode()).hexdigest()


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
    affinity = os.environ.get("PROGRAM_AFFINITY", "0-3,12-15")
    cargo = os.environ.get("CARGO", "cargo").split()
    subprocess.run(
        [*cargo, "build", "--release", "-p", "program-bench", "--locked"],
        cwd=ROOT,
        check=True,
    )
    binary = ROOT / "target/release/program-bench"
    dirty_lines = text(["git", "status", "--porcelain"], cwd=ROOT).splitlines()
    source_dirty = [
        line for line in dirty_lines
        if "benchmarks/results_v10_program_" not in line
    ]
    if source_dirty and os.environ.get("ALLOW_DIRTY") != "1":
        raise SystemExit("worktree is dirty; commit first or set ALLOW_DIRTY=1")
    commit = text(["git", "rev-parse", "HEAD"], cwd=ROOT)

    with open(CORPUS, "rb") as source:
        corpus = tomllib.load(source)
    corpus_hash = sha256(CORPUS)
    binary_hash = sha256(binary)
    expected_manifest = [
        f"commit={commit}",
        f"corpus_sha256={corpus_hash}",
        f"binary_sha256={binary_hash}",
        *[
            f"entry={entry['id']}:{entry_binding(entry)}"
            for entry in corpus["screen"]
        ],
    ]
    if args.confirm:
        if not MANIFEST.is_file() or MANIFEST.read_text().splitlines() != expected_manifest:
            raise SystemExit("confirmation needs the matching complete screen manifest")

    table = "confirm" if args.confirm else "screen"
    records = []
    for entry in corpus[table]:
        benchmark = command(binary, entry, reps)
        invocation = ["taskset", "-c", affinity, *benchmark]
        result = run(invocation, cwd=ROOT)
        records.append((entry, benchmark, fields(result.stdout.strip())))

    accepted_pass = all(
        float(record["evaluate_speedup"]) >= 2.5
        and int(record["break_even_steps"]) <= entry["steps"]
        for entry, _, record in records
    )
    repair_median = statistics.median(
        float(record["repair_speedup"]) for _, _, record in records
    )
    if repair_median >= 1.05:
        repair_result = "faster"
    elif repair_median >= 0.95:
        repair_result = "parity-band"
    else:
        repair_result = "slower"
    raw = HERE / f"results_v10_program_{table}.txt"
    markdown = HERE / f"results_v10_program_{table}.md"
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
        f"source_dirty={'yes' if source_dirty else 'no'}",
        f"accepted_decision={'pass' if accepted_pass else 'fail'}",
        f"repair_median={repair_median:.6f}",
        f"repair_result={repair_result}",
    ]
    lines = list(metadata)
    for entry, benchmark, record in records:
        lines.append(f"entry={entry['id']} binding_sha256={entry_binding(entry)}")
        lines.append(f"command={shlex.join(str(item) for item in benchmark)}")
        lines.append("record=" + " ".join(f"{key}={value}" for key, value in record.items()))
    raw.write_text("\n".join(lines) + "\n")

    md = [
        "<!-- Generated by benchmarks/v10_program_bench.py. Do not edit. -->",
        "",
        f"- Commit: `{commit}`",
        f"- Corpus: `{CORPUS.name}` version {corpus['meta']['version']}, SHA-256 `{corpus_hash}`",
        f"- Set: {table}",
        f"- Repetitions: one warm-up and {reps} counterbalanced timed runs per arm",
        f"- CPU affinity: `{affinity}`",
        f"- CPU: {cpu_model()}",
        f"- Platform: `{platform.platform()}`",
        f"- Cargo: `{text([*cargo, '--version'])}`",
        f"- Binary SHA-256: `{binary_hash}`",
        f"- Source tree dirty outside generated records: {'yes' if source_dirty else 'no'}",
        "- Diagram comparison: exact at every trajectory step in both arm pairs",
        "- Accepted-evaluation rule: at least 2.5 times faster in every entry, with compilation recovered within the trajectory",
        f"- Accepted-evaluation decision: {'pass' if accepted_pass else 'fail'}",
        "- Repair bands: faster at or above 1.05, parity from 0.95 through 1.05, slower below 0.95",
        f"- Repair median and result: {repair_median:.2f}x, {repair_result}",
        "",
        "| entry | atoms | vertices per atom | field | steps | compile (ms) | accepted evaluation (ms) | diagram recomputation (ms) | speedup | break-even steps | repair (ms) | rich recomputation (ms) | repair speedup |",
        "|:--|--:|--:|:--|--:|--:|--:|--:|--:|--:|--:|--:|--:|",
    ]
    for entry, _, record in records:
        md.append(
            f"| {entry['id']} | {entry['atoms']} | {entry['atom_vertices']} | Z/{entry['modulus']} | {entry['steps']} | "
            f"{int(record['compile_ns']) / 1e6:.3f} | "
            f"{int(record['evaluate_ns']) / 1e6:.3f} | "
            f"{int(record['diagram_ns']) / 1e6:.3f} | "
            f"{float(record['evaluate_speedup']):.2f}x | {record['break_even_steps']} | "
            f"{int(record['repair_ns']) / 1e6:.3f} | "
            f"{int(record['rich_ns']) / 1e6:.3f} | "
            f"{float(record['repair_speedup']):.2f}x |"
        )
    markdown.write_text("\n".join(md) + "\n")
    if not args.confirm:
        MANIFEST.write_text("\n".join(expected_manifest) + "\n")
    print(f"wrote {raw.relative_to(ROOT)} and {markdown.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
