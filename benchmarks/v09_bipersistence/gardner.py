"""Run the registered Gardner degree-Rips class-extension study."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
from collections.abc import Sequence
from datetime import datetime, timezone
from pathlib import Path

from .gardner_data import GardnerInput, prepare
from .gardner_execute import run
from .gardner_record import write_record
from .record import collect_environment


def main(arguments: Sequence[str] | None = None) -> int:
    """Prepare the source data, run all gates, and write the record."""

    options = _arguments(arguments)
    root = options.root.resolve()
    try:
        _require_tree_policy(root)
        binaries = {
            "holos": options.holos.resolve(),
            "holos-check": options.checker.resolve(),
        }
        _require_files(binaries)
        archive = options.archive.resolve()
        source = options.source.resolve()
        _require_files({"archive": archive})
        if not source.is_dir():
            raise RuntimeError(f"analysis source is not a directory: {source}")
        with tempfile.TemporaryDirectory(prefix="holos-v09-gardner-") as name:
            temporary = Path(name)
            prepared = prepare(archive, source, temporary / "input")
            result = run(
                prepared,
                binaries["holos"],
                binaries["holos-check"],
                options.repetitions,
                temporary / "runs",
            )
            _archive_evidence(
                prepared,
                temporary / "runs",
                options.repetitions,
                options.archive_output.resolve(),
            )
        environment = collect_environment(
            root, binaries, options.affinity, options.cargo_command
        )
        environment["recorded_at"] = (
            datetime.now(timezone.utc).replace(microsecond=0).isoformat()
        )
        write_record(options.output.resolve(), environment, prepared.metadata, result)
    except (OSError, RuntimeError, subprocess.SubprocessError, ValueError) as error:
        print(f"v09-gardner: {error}", file=sys.stderr)
        return 1
    print(f"wrote {options.output.resolve()}")
    return 0


def _arguments(arguments: Sequence[str] | None) -> argparse.Namespace:
    root = Path(__file__).resolve().parents[2]
    default_data = root / "local/data/v09_gardner"
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--root", type=Path, default=root)
    parser.add_argument("--holos", type=Path, default=root / "target/release/holos")
    parser.add_argument(
        "--checker", type=Path, default=root / "target/release/holos-check"
    )
    parser.add_argument(
        "--reps", type=int, default=int(os.environ.get("REPS", "3")), dest="repetitions"
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path(
            os.environ.get(
                "OUTPUT", root / "benchmarks/results_v09_bipersistence_gardner.md"
            )
        ),
    )
    parser.add_argument(
        "--archive-output",
        type=Path,
        default=root / "local/records/v09_gardner",
        help="local directory for the final artifact, full report, and prepared inputs",
    )
    parser.add_argument(
        "--affinity", default=os.environ.get("V09_AFFINITY", "0-3,12-15")
    )
    parser.add_argument(
        "--cargo-command", default=os.environ.get("CARGO", "cargo +1.92")
    )
    parser.epilog = (
        f"Local defaults often use {default_data}, but archive and source are required."
    )
    return parser.parse_args(arguments)


def _require_tree_policy(root: Path) -> None:
    status = subprocess.run(
        ["git", "-C", str(root), "status", "--porcelain", "--untracked-files=all"],
        text=True,
        capture_output=True,
        check=True,
    ).stdout
    if status and os.environ.get("ALLOW_DIRTY") != "1":
        raise RuntimeError(
            "worktree is dirty; commit the study inputs first or set ALLOW_DIRTY=1"
        )


def _require_files(paths: dict[str, Path]) -> None:
    missing = [f"{name}={path}" for name, path in paths.items() if not path.is_file()]
    if missing:
        raise RuntimeError("missing required file: " + ", ".join(missing))


def _archive_evidence(
    prepared: GardnerInput, runs: Path, repetitions: int, output: Path
) -> None:
    """Copy the consumed input and final checked output into a local archive."""

    output.mkdir(parents=True, exist_ok=True)
    sources = {
        "input.sparse": prepared.graph,
        "region.txt": prepared.region,
        "class-0.cocycle": prepared.cocycles[0],
        "class-1.cocycle": prepared.cocycles[1],
        "input.json": prepared.graph.parent / "input.json",
        "artifact.holosbp": runs / f"gardner-{repetitions - 1}.holosbp",
        "report.json": runs / f"gardner-{repetitions - 1}.json",
    }
    manifest = {}
    for name, source in sources.items():
        destination = output / name
        shutil.copy2(source, destination)
        manifest[name] = {
            "bytes": destination.stat().st_size,
            "sha256": _sha256(destination),
        }
    (output / "manifest.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )


def _sha256(source: Path) -> str:
    digest = hashlib.sha256()
    with source.open("rb") as stream:
        for block in iter(lambda: stream.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


if __name__ == "__main__":
    raise SystemExit(main())
