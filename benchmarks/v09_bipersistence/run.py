"""Orchestrate the registered synthetic bipersistence study."""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
import tempfile
from collections import Counter
from collections.abc import Sequence
from pathlib import Path

from .execute import StudyResult, run_study
from .record import collect_environment, write_record

DEFAULT_AFFINITY = "0-3,12-15"


def main(arguments: Sequence[str] | None = None) -> int:
    """Run the study after checking the worktree and required binaries."""

    options = _parse_arguments(arguments)
    root = options.root.resolve()
    try:
        _require_tree_policy(root)
        binaries = {
            "holos": options.holos.resolve(),
            "holos-check": options.checker.resolve(),
            "research-bench": options.research_bench.resolve(),
        }
        _require_binaries(binaries)
        with tempfile.TemporaryDirectory(prefix="holos-v09-bipersistence-") as name:
            study = run_study(
                binaries["holos"],
                binaries["holos-check"],
                binaries["research-bench"],
                options.repetitions,
                Path(name),
            )
        _require_atlas_coverage(study)
        environment = collect_environment(
            root,
            binaries,
            options.affinity,
            options.cargo_command,
        )
        write_record(options.output.resolve(), environment, study)
    except (OSError, RuntimeError, subprocess.SubprocessError, ValueError) as error:
        print(f"v09-bipersistence: {error}", file=sys.stderr)
        return 1
    print(f"wrote {options.output.resolve()}")
    return 0


def _parse_arguments(arguments: Sequence[str] | None) -> argparse.Namespace:
    root = Path(__file__).resolve().parents[2]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=root)
    parser.add_argument(
        "--holos", type=Path, default=root / "target/release/holos"
    )
    parser.add_argument(
        "--checker", type=Path, default=root / "target/release/holos-check"
    )
    parser.add_argument(
        "--research-bench",
        type=Path,
        default=root / "target/release/research-bench",
    )
    parser.add_argument(
        "--reps",
        type=int,
        default=int(os.environ.get("REPS", "5")),
        dest="repetitions",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path(os.environ.get("OUTPUT", root / "benchmarks/results_v09_bipersistence.md")),
    )
    parser.add_argument(
        "--affinity",
        default=os.environ.get("V09_AFFINITY", DEFAULT_AFFINITY),
    )
    parser.add_argument(
        "--cargo-command",
        default=os.environ.get("CARGO", "cargo +1.92"),
    )
    return parser.parse_args(arguments)


def _require_tree_policy(root: Path) -> None:
    completed = subprocess.run(
        ["git", "-C", str(root), "status", "--porcelain", "--untracked-files=all"],
        text=True,
        capture_output=True,
        check=True,
    )
    if completed.stdout and os.environ.get("ALLOW_DIRTY") != "1":
        raise RuntimeError(
            "worktree is dirty; commit the study inputs first or set ALLOW_DIRTY=1"
        )


def _require_binaries(binaries: dict[str, Path]) -> None:
    missing = [f"{name}={path}" for name, path in binaries.items() if not path.is_file()]
    if missing:
        raise RuntimeError("missing release binary: " + ", ".join(missing))


def _require_atlas_coverage(study: StudyResult) -> None:
    """Require unique, ambiguous, and absent extension fibers in the run."""

    kinds = Counter()
    for result in study.cases:
        kinds.update(result.atlas_kinds)
    required = {"unique", "ambiguous", "no_extension"}
    missing = sorted(required - set(kinds))
    if missing:
        raise RuntimeError("class-atlas study omitted extension kinds: " + ", ".join(missing))


if __name__ == "__main__":
    raise SystemExit(main())
