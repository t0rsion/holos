"""Barcode extraction and adjacent transposition operations."""

from __future__ import annotations

import math
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import dionysus as d
from dionysus_filtration import Prepared, endpoint_filtration
from trajectory import Trajectory


@dataclass(frozen=True)
class Bar:
    """One finite or infinite persistence interval."""

    dimension: int
    birth: float
    death: float


def bar_key(bar: Bar) -> tuple[int, float, bool, float]:
    """Sort finite deaths before infinite deaths."""

    return bar.dimension, bar.birth, math.isinf(bar.death), bar.death


def bars_from_vineyard(
    vineyard: Any,
    prepared: Prepared,
    values: tuple[float, ...],
) -> list[Bar]:
    """Extract H0 and H1 bars from a maintained Dionysus pairing."""

    bars: list[Bar] = []
    unpaired = vineyard.unpaired
    for cell in range(len(vineyard)):
        partner = vineyard.pair(cell)
        if partner == unpaired:
            dimension = prepared.dimensions[cell]
            if dimension <= 1:
                bars.append(Bar(dimension, values[cell], math.inf))
            continue
        if vineyard.position(cell) >= vineyard.position(partner):
            continue
        birth_dimension = prepared.dimensions[cell]
        death_dimension = prepared.dimensions[partner]
        if death_dimension != birth_dimension + 1:
            raise ValueError("Dionysus returned a pair with non-adjacent dimensions")
        if birth_dimension <= 1 and values[partner] > values[cell]:
            bars.append(Bar(birth_dimension, values[cell], values[partner]))
    return sorted(bars, key=bar_key)


def bars_from_diagrams(matrix: Any, filtration: Any) -> list[Bar]:
    """Extract the same H0 and H1 convention from a fresh reduction."""

    bars: list[Bar] = []
    for dimension, diagram in enumerate(d.init_diagrams(matrix, filtration)):
        if dimension > 1:
            break
        for point in diagram:
            bars.append(Bar(dimension, float(point.birth), float(point.death)))
    return sorted(bars, key=bar_key)


def equal_bars(left: list[Bar], right: list[Bar]) -> bool:
    """Compare barcodes exactly after both engines use f64 values."""

    return left == right


def transpose_to(vineyard: Any, order: list[int], target: list[int]) -> tuple[int, int]:
    """Apply adjacent transpositions until the target stable order is reached."""

    if len(order) != len(target) or set(order) != set(target):
        raise ValueError("target order does not contain the current stable cells")
    positions = {cell: index for index, cell in enumerate(order)}
    swaps = 0
    pairing_switches = 0
    for target_position, cell in enumerate(target):
        position = positions[cell]
        if position < target_position:
            raise ValueError("target order moved a previously fixed cell")
        while position > target_position:
            left = order[position - 1]
            right = order[position]
            swapped_left, swapped_right, switched = vineyard.transpose(position - 1)
            if (swapped_left, swapped_right) != (left, right):
                raise ValueError("Dionysus transpose returned unexpected stable cells")
            order[position - 1], order[position] = right, left
            positions[left] = position
            positions[right] = position - 1
            position -= 1
            swaps += 1
            pairing_switches += int(switched)
    if order != target:
        raise ValueError("Dionysus transpositions did not reach the endpoint order")
    return swaps, pairing_switches


def fresh_bars(prepared: Prepared, values: tuple[float, ...], prime: int) -> list[Bar]:
    """Compute a fresh Dionysus barcode for one endpoint."""

    filtration = endpoint_filtration(prepared, values)
    matrix = d.homology_persistence(filtration, prime, method="clearing")
    return bars_from_diagrams(matrix, filtration)


def new_vineyard(prepared: Prepared, prime: int, method: str) -> Any:
    """Construct the maintained state from the first endpoint."""

    return d.Vineyard(prepared.filtration, field=d.Zp(prime), method=method)


def check_expected(
    vineyard: Any,
    prepared: Prepared,
    values: tuple[float, ...],
    expected: list[Bar],
    snapshot: int,
) -> None:
    """Reject a maintained state that disagrees with a fresh barcode."""

    actual = bars_from_vineyard(vineyard, prepared, values)
    if not equal_bars(actual, expected):
        raise ValueError(
            f"Dionysus warm state disagrees with fresh reduction at snapshot {snapshot}"
        )


def write_bars(path: Path, trajectory: Trajectory, values: list[list[Bar]]) -> None:
    """Write the maintained barcode trajectory for cross-engine comparison."""

    lines = [
        f"format=holos-dionysus-bars-v1 dataset={trajectory.dataset} snapshots={len(values)}"
    ]
    for snapshot, bars in enumerate(values):
        for bar in bars:
            death = "inf" if math.isinf(bar.death) else format(bar.death, ".17g")
            lines.append(f"{snapshot} {bar.dimension} {bar.birth:.17g} {death}")
    path.write_text("\n".join(lines) + "\n", encoding="ascii")
