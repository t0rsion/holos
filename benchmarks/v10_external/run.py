"""Run the pinned Dionysus vineyard comparison and render its record."""

from __future__ import annotations

import argparse
import hashlib
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path

from dionysus_record import parse_record, write_markdown, write_raw

ROOT = Path(__file__).resolve().parents[2]
HERE = ROOT / "benchmarks" / "v10_external"
DEFAULT_TRAJECTORY = (
    ROOT
    / "benchmarks"
    / "data"
    / "v10_public_program"
    / "snap-email-eu-dept3-z2.holostem"
)
DEFAULT_HOLOS = ROOT / "target" / "release" / "holos"
DEFAULT_PYTHON = ROOT / "local" / "external" / "dionysus-venv" / "bin" / "python"
DEFAULT_OUTPUT = ROOT / "benchmarks" / "results_v10_external_dionysus"
SOURCE_COMMIT = "f7c1a37a25d4384d22bb1c904a1d5c934ea02c47"
PACKAGE_VERSION = "2.2.3"
LICENSE = "BSD-3-Clause-LBNL"


def sha256(path: Path) -> str:
    """Hash a regular file in bounded memory."""

    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def run_command(command: list[str]) -> subprocess.CompletedProcess[str]:
    """Run one command and retain its output for a failure report."""

    completed = subprocess.run(
        command, cwd=ROOT, text=True, capture_output=True, check=False
    )
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip()
        raise RuntimeError(
            f"command failed ({completed.returncode}): {' '.join(command)}\n{detail}"
        )
    return completed


def revision() -> str:
    """Return the current Holos revision."""

    return run_command(["git", "rev-parse", "HEAD"]).stdout.strip()


def dirty() -> bool:
    """Report uncommitted files in the worktree."""

    return bool(run_command(["git", "status", "--porcelain"]).stdout.strip())


def wheel_path() -> Path:
    """Find the locally downloaded pinned Dionysus wheel."""

    matches = sorted(
        (ROOT / "local" / "external" / "dionysus_dist").glob("dionysus-2.2.3-*.whl")
    )
    if len(matches) != 1:
        raise RuntimeError("expected exactly one local Dionysus 2.2.3 wheel")
    return matches[0]


def parse_args(argv: list[str] | None) -> argparse.Namespace:
    """Parse benchmark options."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--trajectory", type=Path, default=DEFAULT_TRAJECTORY)
    parser.add_argument("--holos", type=Path, default=DEFAULT_HOLOS)
    parser.add_argument("--python", type=Path, default=DEFAULT_PYTHON)
    parser.add_argument("--output-prefix", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--repetitions", type=int, default=5)
    parser.add_argument("--modulus", type=int, default=2)
    parser.add_argument(
        "--method", choices=("matrix_v", "matrix_u"), default="matrix_v"
    )
    parser.add_argument("--affinity", default="0-3,12-15")
    parser.add_argument(
        "--allow-dirty",
        action="store_true",
        help="run with uncommitted files and mark the record as dirty",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    """Run the baseline, exact comparison, and record generation."""

    arguments = parse_args(argv)
    resolve_paths(arguments)
    validate_inputs(arguments)
    worktree_dirty = dirty()
    validate_dirty(worktree_dirty, arguments.allow_dirty)
    wheel = wheel_path()
    revision_value = revision()
    validate_revision(revision_value)
    output_prefix = arguments.output_prefix
    output_prefix.parent.mkdir(parents=True, exist_ok=True)
    python_command = baseline_command(arguments)
    completed_output, comparison = run_comparison(arguments, python_command)
    record = parse_record(completed_output)
    validate_exactness(record, comparison)
    metadata = provenance(arguments, wheel, revision_value, worktree_dirty)
    record_command = [*python_command, "--bars", "<temporary>/bars.txt"]
    write_raw(
        output_prefix.with_suffix(".txt"),
        metadata,
        " ".join(completed_output.splitlines()),
        comparison,
        record_command,
    )
    write_markdown(
        output_prefix.with_suffix(".md"),
        metadata,
        record,
        comparison,
    )
    print(completed_output, end="")
    print(comparison)
    print(f"raw={output_prefix.with_suffix('.txt')}")
    print(f"markdown={output_prefix.with_suffix('.md')}")
    return 0


def resolve_paths(arguments: argparse.Namespace) -> None:
    arguments.trajectory = arguments.trajectory.resolve()
    arguments.holos = arguments.holos.resolve()
    arguments.python = arguments.python.absolute()
    arguments.output_prefix = arguments.output_prefix.resolve()


def validate_inputs(arguments: argparse.Namespace) -> None:
    if arguments.repetitions < 5:
        raise SystemExit("--repetitions must be at least 5")
    if arguments.modulus not in (2, 3, 5):
        raise SystemExit("--modulus must be 2, 3, or 5")
    if not arguments.trajectory.is_file():
        raise SystemExit(
            "registered trajectory is missing; run v10_public_program_bench.py "
            "with the registered SNAP source first"
        )
    if not arguments.holos.is_file():
        raise SystemExit("Holos binary must exist")
    if not arguments.python.is_file():
        raise SystemExit(f"Dionysus Python interpreter is missing: {arguments.python}")


def validate_dirty(worktree_dirty: bool, allow_dirty: bool) -> None:
    if worktree_dirty and not allow_dirty:
        raise SystemExit("worktree is dirty; commit first or pass --allow-dirty")


def validate_revision(revision_value: str) -> None:
    if revision_value == "":
        raise SystemExit("cannot determine the Holos revision")


def baseline_command(arguments: argparse.Namespace) -> list[str]:
    return [
        "taskset",
        "-c",
        arguments.affinity,
        str(arguments.python),
        str(HERE / "dionysus_baseline.py"),
        str(arguments.trajectory),
        "--repetitions",
        str(arguments.repetitions),
        "--modulus",
        str(arguments.modulus),
        "--method",
        arguments.method,
    ]


def run_comparison(
    arguments: argparse.Namespace, python_command: list[str]
) -> tuple[str, str]:
    with tempfile.TemporaryDirectory(prefix="holos-v10-dionysus-") as temporary:
        bars = Path(temporary) / "bars.txt"
        completed = run_command([*python_command, "--bars", str(bars)])
        comparison_command = [
            "taskset",
            "-c",
            arguments.affinity,
            sys.executable,
            str(HERE / "compare.py"),
            str(arguments.trajectory),
            str(bars),
            "--holos",
            str(arguments.holos),
            "--modulus",
            str(arguments.modulus),
            "--rank-values",
        ]
        comparison = run_command(comparison_command).stdout.strip()
    return completed.stdout, comparison


def validate_exactness(record: dict[str, str], comparison: str) -> None:
    if record.get("exact") != "yes" or "exact=yes" not in comparison:
        raise RuntimeError("exactness comparison did not pass")


def provenance(
    arguments: argparse.Namespace,
    wheel: Path,
    revision_value: str,
    worktree_dirty: bool,
) -> dict[str, str]:
    return {
        "format": "holos-v10-external-provenance-v1",
        "date_utc": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "commit": revision_value + ("-DIRTY" if worktree_dirty else ""),
        "trajectory": str(arguments.trajectory.relative_to(ROOT)),
        "trajectory_sha256": sha256(arguments.trajectory),
        "holos_binary": str(arguments.holos.relative_to(ROOT)),
        "holos_sha256": sha256(arguments.holos),
        "package_version": PACKAGE_VERSION,
        "source_commit": SOURCE_COMMIT,
        "wheel": str(wheel.relative_to(ROOT)),
        "wheel_sha256": sha256(wheel),
        "license": LICENSE,
        "affinity": arguments.affinity,
        "source_dirty": "yes" if worktree_dirty else "no",
    }


if __name__ == "__main__":
    raise SystemExit(main())
