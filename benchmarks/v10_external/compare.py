"""Compare an external warm-update barcode trajectory with Holos."""

from __future__ import annotations

import argparse
import csv
import math
import subprocess
import tempfile
from pathlib import Path

from trajectory import Edge, global_rank_trajectory, read_trajectory


def read_bars(path: Path, snapshot_count: int) -> list[list[tuple[int, float, float]]]:
    lines = path.read_text(encoding="ascii").splitlines()
    _validate_bars_header(lines)
    bars = [[] for _ in range(snapshot_count)]
    for line in lines[1:]:
        snapshot, bar = _parse_bar(line, snapshot_count)
        bars[snapshot].append(bar)
    _sort_bars(bars)
    return bars


def _validate_bars_header(lines: list[str]) -> None:
    if (
        not lines
        or not lines[0].startswith("format=holos-")
        or "-bars-v1 " not in lines[0]
    ):
        raise ValueError("unexpected external bars header")


def _parse_bar(line: str, snapshot_count: int) -> tuple[int, tuple[int, float, float]]:
    fields = line.split()
    if len(fields) != 4:
        raise ValueError(f"malformed BATS bar: {line}")
    snapshot, dimension = int(fields[0]), int(fields[1])
    if snapshot < 0 or snapshot >= snapshot_count:
        raise ValueError(f"external snapshot is outside the trajectory: {snapshot}")
    death = math.inf if fields[3] == "inf" else float(fields[3])
    return snapshot, (dimension, float(fields[2]), death)


def _sort_bars(bars: list[list[tuple[int, float, float]]]) -> None:
    for value in bars:
        value.sort(key=lambda bar: (bar[0], bar[1], math.isinf(bar[2]), bar[2]))


def sparse_text(
    vertices: int, edges: tuple[Edge, ...], weights: tuple[float, ...]
) -> str:
    rows = [f"# vertices={vertices}"]
    rows.extend(
        f"{edge.u} {edge.v} {weight:.17g}"
        for edge, weight in zip(edges, weights, strict=True)
    )
    return "\n".join(rows) + "\n"


def holos_bars(
    binary: Path, sparse: Path, modulus: int
) -> list[tuple[int, float, float]]:
    completed = subprocess.run(
        [
            str(binary),
            str(sparse),
            "--format",
            "sparse",
            "--dim",
            "1",
            "--modulus",
            str(modulus),
            "--output",
            "csv",
        ],
        text=True,
        capture_output=True,
        check=True,
    )
    rows = csv.DictReader(completed.stdout.splitlines())
    bars = []
    for row in rows:
        death = math.inf if row["death"] == "inf" else float(row["death"])
        bars.append((int(row["dim"]), float(row["birth"]), death))
    bars.sort(key=lambda bar: (bar[0], bar[1], math.isinf(bar[2]), bar[2]))
    return bars


def equal(
    left: list[tuple[int, float, float]], right: list[tuple[int, float, float]]
) -> bool:
    return left == right


def compare(
    trajectory_path: Path,
    bars: Path,
    holos: Path,
    modulus: int,
    rank_values: bool,
) -> tuple[int, int]:
    trajectory = read_trajectory(trajectory_path)
    if rank_values:
        trajectory = global_rank_trajectory(trajectory)
    observed = read_bars(bars, len(trajectory.weights))
    matches = 0
    with tempfile.TemporaryDirectory(prefix="holos-v10-external-") as name:
        root = Path(name)
        for index, weights in enumerate(trajectory.weights):
            sparse = root / f"snapshot-{index}.sparse"
            sparse.write_text(
                sparse_text(trajectory.vertices, trajectory.edges, weights),
                encoding="ascii",
            )
            actual = holos_bars(holos, sparse, modulus)
            if not equal(observed[index], actual):
                raise ValueError(
                    f"external and Holos diagrams differ at snapshot {index}"
                )
            matches += 1
    return matches, len(trajectory.weights)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trajectory", type=Path)
    parser.add_argument("bars", type=Path)
    parser.add_argument("--holos", type=Path, required=True)
    parser.add_argument("--modulus", type=int, default=2)
    parser.add_argument(
        "--rank-values",
        action="store_true",
        help="apply the Dionysus-compatible global rank encoding before comparison",
    )
    arguments = parser.parse_args()
    matches, total = compare(
        arguments.trajectory,
        arguments.bars,
        arguments.holos,
        arguments.modulus,
        arguments.rank_values,
    )
    print(
        f"format=holos-external-compare-v1 matches={matches} snapshots={total} exact=yes"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
