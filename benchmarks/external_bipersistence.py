"""Compare Holos degree-Rips H1 node ranks with external implementations."""

from __future__ import annotations

import argparse
import json
import os
import signal
import subprocess
import sys
import tempfile
from collections.abc import Mapping, Sequence
from pathlib import Path
from typing import Any

try:
    from .external_bipersistence_record import (
        combine_record,
        error_record,
        report_ranks,
        write_record,
    )
except ImportError:
    from external_bipersistence_record import (
        combine_record,
        error_record,
        report_ranks,
        write_record,
    )

try:
    from .external_bipersistence_provenance import collect as collect_provenance
except ImportError:
    from external_bipersistence_provenance import collect as collect_provenance


def main(arguments: Sequence[str] | None = None) -> int:
    """Run the external comparison and write a generated Markdown record."""

    options = _parse_arguments(arguments)
    _normalize_paths(options)
    input_data = json.loads(options.input.read_text(encoding="utf-8"))
    provenance = collect_provenance(options)
    if provenance["tree_state"] != "clean" and not provenance["allow_dirty"]:
        record = error_record(
            options,
            "registered run requires a clean tree; set ALLOW_DIRTY=1 for a local diagnostic",
            provenance,
        )
        write_record(options.output, record)
        return 1
    try:
        with tempfile.TemporaryDirectory(
            prefix="holos-external-bipersistence-"
        ) as name:
            temporary_root = Path(name)
            holos_cases = _run_holos(options.holos, input_data, temporary_root)
            external = _run_external(options)
    except (
        OSError,
        subprocess.SubprocessError,
        ValueError,
        json.JSONDecodeError,
    ) as error:
        record = error_record(
            options,
            f"{type(error).__name__}: {error}",
            provenance,
        )
        write_record(options.output, record)
        return 1

    record = combine_record(options, holos_cases, external, provenance)
    write_record(options.output, record)
    print(f"wrote {options.output.resolve()}")
    return 0 if record["status"] == "pass" else 1


def _run_holos(
    holos: Path,
    input_data: Mapping[str, Any],
    temporary_root: Path,
) -> list[dict[str, Any]]:
    """Build one report per frozen case and return its node ranks."""

    if not holos.is_file():
        raise ValueError(f"missing Holos binary: {holos}")
    results = []
    for case in input_data["cases"]:
        case_root = temporary_root / str(case["name"])
        case_root.mkdir()
        graph = case_root / "input.sparse"
        artifact = case_root / "output.holosbp"
        report = case_root / "output.json"
        _write_sparse_graph(case, graph)
        command = [
            str(holos),
            "bipersistence",
            str(graph),
            str(artifact),
            "--format",
            "sparse",
            "--threshold",
            _float_text(case["threshold"]),
            "--modulus",
            str(input_data["modulus"]),
            "--threads",
            "1",
            "--report",
            str(report),
        ]
        for scale in case["scales"]:
            command.extend(("--scale", _float_text(scale)))
        for minimum_degree in case["minimum_degrees"]:
            command.extend(("--minimum-degree", str(minimum_degree)))
        completed = subprocess.run(
            command,
            text=True,
            capture_output=True,
            check=False,
        )
        if completed.returncode != 0:
            details = (completed.stdout + "\n" + completed.stderr).strip()
            raise ValueError(f"Holos failed for {case['name']}: {details}")
        report_data = json.loads(report.read_text(encoding="utf-8"))
        ranks = report_ranks(report_data, case)
        results.append({"name": case["name"], "ranks": ranks})
    return results


def _run_external(options: argparse.Namespace) -> dict[str, Any]:
    """Run the external child so a native crash cannot kill this harness."""

    command = [
        str(options.multipers_python),
        str(options.child),
        "--input",
        str(options.input),
    ]
    try:
        completed = subprocess.run(
            command,
            text=True,
            capture_output=True,
            check=False,
            timeout=options.timeout,
        )
    except subprocess.TimeoutExpired:
        return {
            "status": "unavailable",
            "reason": f"external child exceeded {options.timeout:g}s",
            "command": command,
        }
    if completed.returncode < 0:
        number = -completed.returncode
        try:
            name = signal.Signals(number).name
        except ValueError:
            name = f"signal {number}"
        return {
            "status": "unavailable",
            "reason": f"external child terminated by {name}",
            "returncode": completed.returncode,
            "stderr": completed.stderr.strip(),
            "command": command,
        }
    if completed.returncode != 0:
        return {
            "status": "unavailable",
            "reason": "external child returned a nonzero status",
            "returncode": completed.returncode,
            "stderr": completed.stderr.strip(),
            "command": command,
        }
    try:
        value = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        return {
            "status": "unavailable",
            "reason": f"external child did not return JSON: {error}",
            "stdout": completed.stdout[-1000:],
            "stderr": completed.stderr.strip(),
            "command": command,
        }
    value["command"] = [
        "<external-python>",
        options.child.name,
        "--input",
        options.input.name,
    ]
    return value


def _write_sparse_graph(case: Mapping[str, Any], path: Path) -> None:
    lines = [
        f"# source_id={case['source_id']}",
        f"# vertex_count={case['vertex_count']} is implied by the largest index",
    ]
    lines.extend(
        f"{int(edge[0])} {int(edge[1])} {_float_text(edge[2])}"
        for edge in case["edges"]
    )
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def _float_text(value: Any) -> str:
    return format(float(value), ".17g")


def _normalize_paths(options: argparse.Namespace) -> None:
    """Resolve path options against the repository root."""

    options.root = options.root.resolve()
    for name in ("input", "holos", "child", "output", "requirements"):
        path = getattr(options, name)
        if not path.is_absolute():
            setattr(options, name, options.root / path)


def _parse_arguments(arguments: Sequence[str] | None) -> argparse.Namespace:
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=root)
    parser.add_argument(
        "--input",
        type=Path,
        default=root / "benchmarks/external_bipersistence_cases.json",
    )
    parser.add_argument("--holos", type=Path, default=root / "target/release/holos")
    parser.add_argument(
        "--multipers-python",
        type=Path,
        default=Path(os.environ.get("MULTIPERS_PYTHON", sys.executable)),
    )
    parser.add_argument(
        "--child",
        type=Path,
        default=Path(__file__).with_name("external_bipersistence_multipers.py"),
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=root / "benchmarks/results_external_bipersistence.md",
    )
    parser.add_argument(
        "--requirements",
        type=Path,
        default=root / "benchmarks/external_bipersistence_requirements.txt",
    )
    parser.add_argument(
        "--affinity",
        default=os.environ.get("AFFINITY", ""),
        help="requested CPU affinity recorded in the result",
    )
    parser.add_argument("--timeout", type=float, default=120.0)
    return parser.parse_args(arguments)


if __name__ == "__main__":
    raise SystemExit(main())
