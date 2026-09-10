"""Build, execute, and bind the registered program study."""

from __future__ import annotations

import argparse
import os
import platform
import subprocess
from pathlib import Path

from v10_checker import measure
from v10_checker import sha256 as checker_sha256

from .config import (
    benchmark_command,
    entry_binding,
    load_corpus,
    parse_record,
    sha256,
)
from .record import EntryResult, decisions, write_markdown, write_raw

ROOT = Path(__file__).resolve().parents[2]
HERE = ROOT / "benchmarks"
CORPUS = HERE / "v10_program_corpus.toml"
MANIFEST = HERE / "results_v10_program_screen_manifest.txt"
ARTIFACTS = HERE / "data" / "v10_program_checker"


def main() -> None:
    arguments = parse_arguments()
    if arguments.help:
        from v10_program_bench import __doc__

        print(__doc__)
        return
    execute(arguments)


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--confirm", action="store_true")
    parser.add_argument("-h", "--help", action="store_true")
    return parser.parse_args()


def execute(arguments: argparse.Namespace) -> None:
    """Build, execute, and record one registered program set."""

    repetitions = int(os.environ.get("REPS", "5"))
    if repetitions < 5:
        raise SystemExit("REPS must be at least 5")
    affinity = os.environ.get("PROGRAM_AFFINITY", "0-3,12-15")
    cargo = os.environ.get("CARGO", "cargo +1.92").split()
    run(
        [
            *cargo,
            "build",
            "--release",
            "-p",
            "program-bench",
            "-p",
            "holos-tda-check",
            "--locked",
        ],
        cwd=ROOT,
    )
    binary = ROOT / "target/release/program-bench"
    checker = ROOT / "target/release/holos-check"
    dirty = source_changes()
    if dirty and os.environ.get("ALLOW_DIRTY") != "1":
        raise SystemExit("worktree is dirty; commit first or set ALLOW_DIRTY=1")
    corpus = load_corpus(CORPUS)
    commit = text(["git", "rev-parse", "HEAD"], cwd=ROOT)
    binary_hash = sha256(binary)
    checker_hash = checker_sha256(checker)
    corpus_hash = sha256(CORPUS)
    expected_manifest = manifest_lines(
        commit, binary_hash, checker_hash, corpus_hash, corpus["screen"]
    )
    validate_manifest(arguments.confirm, expected_manifest)
    selected = "confirm" if arguments.confirm else "screen"
    results = execute_entries(
        binary, checker, corpus[selected], selected, repetitions, affinity
    )
    outcome = decisions(results)
    metadata = metadata_fields(
        cargo,
        commit,
        corpus,
        corpus_hash,
        binary_hash,
        checker_hash,
        selected,
        repetitions,
        affinity,
        dirty,
    )
    raw_metadata = [f"{key}={value}" for key, value in metadata.items()]
    raw_metadata.extend(f"{key}={value}" for key, value in outcome.items())
    stem = HERE / f"results_v10_program_{selected}"
    write_raw(stem.with_suffix(".txt"), raw_metadata, results)
    write_markdown(stem.with_suffix(".md"), metadata, outcome, results)
    if not arguments.confirm:
        MANIFEST.write_text("\n".join(expected_manifest) + "\n")
    print(f"wrote {stem.relative_to(ROOT)}.txt and {stem.relative_to(ROOT)}.md")


def validate_manifest(confirm: bool, expected: list[str]) -> None:
    if confirm and (
        not MANIFEST.is_file() or MANIFEST.read_text().splitlines() != expected
    ):
        raise SystemExit("confirmation needs the matching complete screen manifest")


def execute_entries(
    binary: Path,
    checker: Path,
    entries: list[dict],
    selected: str,
    repetitions: int,
    affinity: str,
) -> list[EntryResult]:
    results = []
    for entry in entries:
        prefix = ARTIFACTS / selected / entry["id"]
        command = benchmark_command(binary, entry, repetitions, prefix)
        output = text(["taskset", "-c", affinity, *command], cwd=ROOT)
        fields = parse_record(output)
        checked = measure(checker, prefix, repetitions, affinity)
        if checked.program.stat().st_size != int(fields["program_bytes"]):
            raise SystemExit("checker program size differs from the benchmark record")
        if checked.trace.stat().st_size != int(fields["trace_bytes"]):
            raise SystemExit("checker trace size differs from the benchmark record")
        results.append(
            EntryResult(entry, command, fields, entry_binding(entry), checked)
        )
    return results


def metadata_fields(
    cargo: list[str],
    commit: str,
    corpus: dict,
    corpus_hash: str,
    binary_hash: str,
    checker_hash: str,
    selected: str,
    repetitions: int,
    affinity: str,
    dirty: list[str],
) -> dict[str, str]:
    return {
        "commit": commit,
        "corpus": CORPUS.name,
        "corpus_version": str(corpus["meta"]["version"]),
        "corpus_sha256": corpus_hash,
        "set": selected,
        "repetitions": str(repetitions),
        "affinity": affinity,
        "cpu": cpu_model(),
        "platform": platform.platform(),
        "python": platform.python_version(),
        "cargo": text([*cargo, "--version"]),
        "binary_sha256": binary_hash,
        "checker_binary_sha256": checker_hash,
        "source_dirty": "yes" if dirty else "no",
    }


def manifest_lines(
    commit: str,
    binary_hash: str,
    checker_hash: str,
    corpus_hash: str,
    entries: list[dict],
) -> list[str]:
    return [
        f"commit={commit}",
        f"corpus_sha256={corpus_hash}",
        f"binary_sha256={binary_hash}",
        f"checker_binary_sha256={checker_hash}",
        *[f"entry={entry['id']}:{entry_binding(entry)}" for entry in entries],
    ]


def source_changes() -> list[str]:
    return [
        line
        for line in text(["git", "status", "--porcelain"], cwd=ROOT).splitlines()
        if "benchmarks/results_v10_program_" not in line
    ]


def cpu_model() -> str:
    try:
        for line in Path("/proc/cpuinfo").read_text().splitlines():
            if line.startswith("model name"):
                return line.split(":", 1)[1].strip()
    except OSError:
        pass
    return platform.processor() or "unknown"


def run(command: list[str], **kwargs) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(item) for item in command],
        text=True,
        capture_output=True,
        check=True,
        **kwargs,
    )


def text(command: list[str], **kwargs) -> str:
    return run(command, **kwargs).stdout.strip()
