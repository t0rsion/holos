"""Prepare explicit fixed-complex filtrations for the Dionysus baseline."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

import dionysus as d
from trajectory import Complex, Trajectory, snapshot_values


@dataclass(frozen=True)
class Prepared:
    """The initial sorted filtration and stable-cell metadata."""

    filtration: Any
    simplices: tuple[tuple[int, ...], ...]
    dimensions: tuple[int, ...]
    stable_by_simplex: dict[tuple[int, ...], int]
    values: tuple[tuple[float, ...], ...]


def endpoint_order(
    values: list[float],
    simplices: tuple[tuple[int, ...], ...],
    dimensions: tuple[int, ...],
) -> list[int]:
    """Return stable cells in the valid endpoint filtration order."""

    return sorted(
        range(len(simplices)),
        key=lambda index: (values[index], dimensions[index], simplices[index]),
    )


def initial_filtration(
    complex_: Complex,
    trajectory: Trajectory,
) -> tuple[Any, tuple[tuple[int, ...], ...], tuple[int, ...]]:
    """Construct and sort the initial explicit fixed Rips filtration."""

    values = snapshot_values(complex_, trajectory.weights[0])
    filtration = d.Filtration(
        [
            (list(simplex), value)
            for simplex, value in zip(complex_.simplices, values, strict=True)
        ]
    )
    filtration.sort()
    simplices = tuple(tuple(simplex) for simplex in filtration)
    dimensions = tuple(simplex.dimension() for simplex in filtration)
    return filtration, simplices, dimensions


def prepare(complex_: Complex, trajectory: Trajectory) -> Prepared:
    """Prepare stable-cell values for every trajectory snapshot."""

    filtration, simplices, dimensions = initial_filtration(complex_, trajectory)
    stable_by_simplex = {simplex: index for index, simplex in enumerate(simplices)}
    if len(stable_by_simplex) != len(simplices):
        raise ValueError("fixed complex contains duplicate simplices")
    values = []
    for snapshot in trajectory.weights:
        raw = snapshot_values(complex_, snapshot)
        raw_by_simplex = dict(zip(complex_.simplices, raw, strict=True))
        values.append([raw_by_simplex[simplex] for simplex in simplices])
    return Prepared(
        filtration,
        simplices,
        dimensions,
        stable_by_simplex,
        tuple(tuple(snapshot) for snapshot in values),
    )


def endpoint_filtration(prepared: Prepared, values: tuple[float, ...]) -> Any:
    """Construct and sort an endpoint filtration for the full-update arm."""

    filtration = d.Filtration(
        [
            (list(simplex), value)
            for simplex, value in zip(prepared.simplices, values, strict=True)
        ]
    )
    filtration.sort()
    return filtration


def order_from_filtration(
    filtration: Any,
    stable_by_simplex: dict[tuple[int, ...], int],
) -> list[int]:
    """Map a sorted Dionysus filtration back to stable cell identifiers."""

    try:
        return [stable_by_simplex[tuple(simplex)] for simplex in filtration]
    except KeyError as error:
        raise ValueError("endpoint filtration changed the fixed complex") from error
