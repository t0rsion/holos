"""Prepare, run, and bind the public temporal-graph control."""

from __future__ import annotations

import argparse
import os
import platform
import subprocess
from pathlib import Path

from v10_checker import measure
from v10_checker import sha256 as checker_sha256

from .config import load_corpus, parse_record, sha256
from .prepare import prepare
from .record import classify, write_markdown, write_raw

ROOT = Path(__file__).resolve().parents[2]
HERE = ROOT / "benchmarks"
CORPUS = HERE / "v10_public_program_corpus.toml"
DATA = HERE / "data" / "v10_public_program"


def main() -> None:
    arguments = parse_arguments()
    if arguments.help:
        from v10_public_program_bench import __doc__

        print(__doc__)
        return
    execute(arguments)


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("source", nargs="?")
    parser.add_argument("-h", "--help", action="store_true")
    return parser.parse_args()


def execute(arguments: argparse.Namespace) -> None:
    """Prepare the registered source and run the public program study."""

    if arguments.source is None:
        raise SystemExit("SOURCE is required")
    source = Path(arguments.source).resolve()
    corpus = load_corpus(CORPUS)
    entry = corpus["trajectory"][0]
    if sha256(source) != entry["source_sha256"]:
        raise SystemExit("SOURCE SHA-256 does not match the registered corpus")
    repetitions = int(os.environ.get("REPS", "5"))
    if repetitions < 5:
        raise SystemExit("REPS must be at least 5")
    affinity = os.environ.get("PROGRAM_AFFINITY", "0-3,12-15")
    cargo = os.environ.get("CARGO", "cargo +1.92").split()
    DATA.mkdir(parents=True, exist_ok=True)
    trajectory = DATA / f"{entry['id']}.holostem"
    prepared = prepare(source, trajectory, entry)
    run(
        [
            *cargo,
            "build",
            "--release",
            "-p",
            "program-bench",
            "-p",
            "holos-tda-check",
            "--bins",
            "--locked",
        ],
        cwd=ROOT,
    )
    binary = ROOT / "target/release/program-public-bench"
    checker = ROOT / "target/release/holos-check"
    dirty = source_changes()
    if dirty and os.environ.get("ALLOW_DIRTY") != "1":
        raise SystemExit("worktree is dirty; commit first or set ALLOW_DIRTY=1")
    command = [
        str(binary),
        str(trajectory),
        "--modulus",
        str(entry["modulus"]),
        "--reps",
        str(repetitions),
        "--artifact-prefix",
        str(DATA / f"{entry['id']}-checker"),
    ]
    fields = parse_record(text(["taskset", "-c", affinity, *command], cwd=ROOT))
    checked = measure(checker, DATA / f"{entry['id']}-checker", repetitions, affinity)
    if checked.program.stat().st_size != int(fields["program_bytes"]):
        raise SystemExit("checker program size differs from the benchmark record")
    if checked.trace.stat().st_size != int(fields["trace_bytes"]):
        raise SystemExit("checker trace size differs from the benchmark record")
    for key in ("vertices", "edges", "snapshots"):
        if int(fields[key]) != prepared[key]:
            raise SystemExit(f"benchmark {key} does not match the prepared trajectory")
    outcome = classify(fields)
    metadata = metadata_fields(
        cargo,
        corpus,
        entry,
        source,
        trajectory,
        binary,
        checker,
        repetitions,
        affinity,
        dirty,
    )
    stem = HERE / "results_v10_public_program"
    write_raw(stem.with_suffix(".txt"), metadata, outcome, fields, command, checked)
    write_markdown(stem.with_suffix(".md"), metadata, outcome, fields, checked)
    print(f"wrote {stem.relative_to(ROOT)}.txt and {stem.relative_to(ROOT)}.md")


def metadata_fields(
    cargo: list[str],
    corpus: dict,
    entry: dict,
    source: Path,
    trajectory: Path,
    binary: Path,
    checker: Path,
    repetitions: int,
    affinity: str,
    dirty: list[str],
) -> dict[str, str]:
    return {
        "commit": text(["git", "rev-parse", "HEAD"], cwd=ROOT),
        "corpus": CORPUS.name,
        "corpus_version": str(corpus["meta"]["version"]),
        "corpus_sha256": sha256(CORPUS),
        "source_url": entry["source_url"],
        "source_path": str(source),
        "source_sha256": sha256(source),
        "trajectory_sha256": sha256(trajectory),
        "repetitions": str(repetitions),
        "affinity": affinity,
        "cpu": cpu_model(),
        "platform": platform.platform(),
        "python": platform.python_version(),
        "cargo": text([*cargo, "--version"]),
        "binary_sha256": sha256(binary),
        "checker_binary_sha256": checker_sha256(checker),
        "source_dirty": "yes" if dirty else "no",
    }


def source_changes() -> list[str]:
    ignored_prefixes = (
        "benchmarks/data/v10_public_program/",
        "benchmarks/results_v10_public_program.",
    )
    return [
        line
        for line in text(["git", "status", "--porcelain"], cwd=ROOT).splitlines()
        if not line[3:].startswith(ignored_prefixes)
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
