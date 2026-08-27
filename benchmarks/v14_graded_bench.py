#!/usr/bin/env python3
"""Run the registered version 0.14 graded-interface study.

Usage: v14_graded_bench.py [--confirm]

The screen must finish before confirmation. Each entry contains weighted
octahedron boundaries joined at one vertex. Every atom contributes one
essential H2 class. The compose arm omits the parent reduction. The baseline
retains a reduction over every parent scope. Both arms process identical
versions and check every diagram with the separate checker.

Environment:
  CARGO          Cargo command, including an optional toolchain. The default
                 is ``cargo``.
  REPS           Timed repetitions per arm. The default and minimum are 5.
  INDEX_AFFINITY CPUs for the study. The default is ``0-3,12-15``.
  ALLOW_DIRTY    Set to 1 to record a dirty source tree. Such a record is void
                 for release claims.
"""

from __future__ import annotations

import argparse
import hashlib
import os
from pathlib import Path
import platform
import shlex
import subprocess
import tomllib


ROOT = Path(__file__).resolve().parent.parent
HERE = ROOT / "benchmarks"
CORPUS = HERE / "v14_graded_corpus.toml"
MANIFEST = HERE / "results_v14_graded_screen_manifest.txt"


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


def command(binary, entry, reps, materialize):
    result = [
        binary,
        "--atoms", entry["atoms"],
        "--atom-vertices", entry["atom_vertices"],
        "--seed", entry["seed"],
        "--steps", entry["steps"],
        "--branches", entry["branches"],
        "--reps", reps,
        "--modulus", entry["modulus"],
        "--max-dim", 2,
        "--cross-polytope",
    ]
    if materialize:
        result.append("--materialize")
    return result


def fields(line):
    values = {}
    for item in line.split():
        key, value = item.split("=", 1)
        values[key] = value
    if values.get("format") != "holos-index-bench-v2":
        raise SystemExit(f"unexpected benchmark record: {line}")
    return values


def entry_binding(entry):
    return hashlib.sha256(repr(sorted(entry.items())).encode()).hexdigest()


def ratio(numerator, denominator):
    return float(numerator) / float(denominator)


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
    affinity = os.environ.get("INDEX_AFFINITY", "0-3,12-15")
    cargo = os.environ.get("CARGO", "cargo").split()
    subprocess.run(
        [*cargo, "build", "--release", "-p", "index-bench", "--locked"],
        cwd=ROOT,
        check=True,
    )
    binary = ROOT / "target/release/index-bench"
    dirty_lines = text(["git", "status", "--porcelain"], cwd=ROOT).splitlines()
    source_dirty = [
        line for line in dirty_lines
        if "benchmarks/results_v14_graded_" not in line
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
        if (not MANIFEST.is_file()
                or MANIFEST.read_text().splitlines() != expected_manifest):
            raise SystemExit("confirmation needs the matching complete screen manifest")

    table = "confirm" if args.confirm else "screen"
    records = []
    for position, entry in enumerate(corpus[table]):
        compose_command = command(binary, entry, reps, False)
        baseline_command = command(binary, entry, reps, True)
        ordered = (
            [("compose", compose_command), ("materialize", baseline_command)]
            if position % 2 == 0 else
            [("materialize", baseline_command), ("compose", compose_command)]
        )
        pair = {}
        for label, benchmark in ordered:
            invocation = ["taskset", "-c", affinity, *benchmark]
            result = run(invocation, cwd=ROOT)
            pair[label] = (benchmark, fields(result.stdout.strip()))
        records.append((entry, pair))

    coverage_pass = all(
        compose["shape"] == "cross-polytope"
        and int(compose["max_dim"]) == 2
        and int(compose["h2_bars"]) == entry["atoms"]
        and int(baseline["h2_bars"]) == entry["atoms"]
        for entry, pair in records
        for compose, baseline in [(pair["compose"][1], pair["materialize"][1])]
    )
    structural_pass = all(
        compose["root_composed"] == "true"
        and int(compose["composed_interfaces"]) > 0
        and int(compose["largest_interface_vertices"])
        <= int(compose["atom_vertices"])
        and baseline["root_composed"] == "false"
        and int(baseline["largest_interface_vertices"])
        == int(baseline["vertices"])
        for _, pair in records
        for compose, baseline in [(pair["compose"][1], pair["materialize"][1])]
    )
    update_pass = all(
        ratio(baseline["warm_ns"], compose["warm_ns"]) >= 2.0
        for _, pair in records
        for compose, baseline in [(pair["compose"][1], pair["materialize"][1])]
    )
    proof_pass = all(
        ratio(baseline["warm_proof_bytes"], compose["warm_proof_bytes"]) >= 1.5
        for _, pair in records
        for compose, baseline in [(pair["compose"][1], pair["materialize"][1])]
    )

    raw = HERE / f"results_v14_graded_{table}.txt"
    markdown = HERE / f"results_v14_graded_{table}.md"
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
        f"coverage_decision={'pass' if coverage_pass else 'fail'}",
        f"structural_decision={'pass' if structural_pass else 'fail'}",
        f"update_decision={'pass' if update_pass else 'fail'}",
        f"proof_decision={'pass' if proof_pass else 'fail'}",
    ]
    lines = list(metadata)
    for entry, pair in records:
        lines.append(f"entry={entry['id']} binding_sha256={entry_binding(entry)}")
        for label in ["compose", "materialize"]:
            benchmark, record = pair[label]
            lines.append(f"{label}_command={shlex.join(str(item) for item in benchmark)}")
            lines.append(f"{label}_record=" + " ".join(
                f"{key}={value}" for key, value in record.items()))
    raw.write_text("\n".join(lines) + "\n")

    md = [
        "<!-- Generated by benchmarks/v14_graded_bench.py. Do not edit. -->",
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
        "- Result comparison: exact H0, H1, and H2 diagrams at every version",
        "- Coverage rule: one essential H2 class per atom in both policies",
        f"- Coverage decision: {'pass' if coverage_pass else 'fail'}",
        "- Structural rule: composed root, no parent scope above one atom, and a full-scope baseline",
        f"- Structural decision: {'pass' if structural_pass else 'fail'}",
        "- Update rule: composed median time is at least 2.0 times faster in every entry",
        f"- Update decision: {'pass' if update_pass else 'fail'}",
        "- Proof rule: the composed warm stream is at least 1.5 times smaller in every entry",
        f"- Proof decision: {'pass' if proof_pass else 'fail'}",
        "",
        "| entry | atoms | vertices | field | largest interface | update compose/materialize (ms) | speedup | proof compose/materialize (MiB) | terms compose/materialize | compression | check compose/materialize (ms) |",
        "|:--|--:|--:|:--|--:|--:|--:|--:|--:|--:|--:|",
    ]
    for entry, pair in records:
        compose = pair["compose"][1]
        baseline = pair["materialize"][1]
        update_speedup = ratio(baseline["warm_ns"], compose["warm_ns"])
        proof_compression = ratio(
            baseline["warm_proof_bytes"], compose["warm_proof_bytes"])
        md.append(
            f"| {entry['id']} | {entry['atoms']} | {compose['vertices']} | Z/{entry['modulus']} | "
            f"{compose['largest_interface_vertices']}/{baseline['largest_interface_vertices']} | "
            f"{int(compose['warm_ns']) / 1e6:.3f}/{int(baseline['warm_ns']) / 1e6:.3f} | {update_speedup:.2f}x | "
            f"{int(compose['warm_proof_bytes']) / (1 << 20):.3f}/{int(baseline['warm_proof_bytes']) / (1 << 20):.3f} | "
            f"{compose['warm_proof_terms']}/{baseline['warm_proof_terms']} | {proof_compression:.2f}x | "
            f"{int(compose['warm_verify_ns']) / 1e6:.3f}/{int(baseline['warm_verify_ns']) / 1e6:.3f} |"
        )
    markdown.write_text("\n".join(md) + "\n")
    if not args.confirm:
        MANIFEST.write_text("\n".join(expected_manifest) + "\n")
    print(f"wrote {raw.relative_to(ROOT)} and {markdown.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
