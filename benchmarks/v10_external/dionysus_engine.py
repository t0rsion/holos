"""Benchmark arms for a maintained Dionysus vineyard."""

from __future__ import annotations

import math
import statistics
import time
from dataclasses import dataclass
from pathlib import Path

from dionysus_filtration import (
    Prepared,
    endpoint_filtration,
    endpoint_order,
    order_from_filtration,
)
from dionysus_pairing import (
    Bar,
    bars_from_vineyard,
    check_expected,
    fresh_bars,
    new_vineyard,
    transpose_to,
    write_bars,
)
from trajectory import Trajectory

PACKAGE_VERSION = "2.2.3"
SOURCE_COMMIT = "f7c1a37a25d4384d22bb1c904a1d5c934ea02c47"
LICENSE = "BSD-3-Clause-LBNL"


@dataclass
class Timings:
    """Raw timing samples for each baseline arm."""

    compile_ns: list[int]
    update_ns: list[int]
    full_update_ns: list[int]
    fresh_ns: list[int]


@dataclass
class _UpdateStats:
    """Counters and checked values from the first maintained update pass."""

    warm_values: list[list[Bar]]
    transition_swaps: list[int]
    swaps_total: int = 0
    pairing_switches_total: int = 0


def median(values: list[int]) -> int:
    """Return the upper middle sample used by the benchmark record."""

    return int(statistics.median_high(values))


def run(
    trajectory: Trajectory,
    prepared: Prepared,
    prime: int,
    method: str,
    repetitions: int,
    bars_path: Path | None,
) -> str:
    """Run exactness checks and the maintained versus fresh timing arms."""

    expected = [fresh_bars(prepared, values, prime) for values in prepared.values]
    timings = Timings([], [], [], [])
    initial = new_vineyard(prepared, prime, method)
    _validate_initial_ids(initial, prepared)
    check_expected(initial, prepared, prepared.values[0], expected[0], 0)

    _measure_compile(prepared, prime, method, repetitions, timings)

    target_orders = _target_orders(prepared)
    stats = _measure_maintained_updates(
        prepared, prime, method, repetitions, target_orders, expected, timings
    )

    _measure_full_updates(prepared, prime, method, repetitions, timings)
    _measure_fresh_updates(prepared, prime, repetitions, timings)

    if bars_path is not None:
        write_bars(bars_path, trajectory, stats.warm_values)
    return _record(trajectory, prepared, prime, method, repetitions, timings, stats)


def _validate_initial_ids(initial, prepared: Prepared) -> None:
    if [initial.cell_at(index) for index in range(len(initial))] != list(
        range(len(prepared.simplices))
    ):
        raise ValueError("Dionysus initial stable ids are not filtration positions")


def _measure_compile(
    prepared: Prepared,
    prime: int,
    method: str,
    repetitions: int,
    timings: Timings,
) -> None:
    for _ in range(repetitions):
        start = time.perf_counter_ns()
        new_vineyard(prepared, prime, method)
        timings.compile_ns.append(time.perf_counter_ns() - start)


def _target_orders(prepared: Prepared) -> list[list[int]]:
    return [
        endpoint_order(list(values), prepared.simplices, prepared.dimensions)
        for values in prepared.values
    ]


def _measure_maintained_updates(
    prepared: Prepared,
    prime: int,
    method: str,
    repetitions: int,
    target_orders: list[list[int]],
    expected: list[list[Bar]],
    timings: Timings,
) -> _UpdateStats:
    stats = _UpdateStats([expected[0]], [])
    timings.update_ns.append(
        _timed_maintained_update(prepared, prime, method, target_orders, stats)
    )
    stats.warm_values.extend(
        _warm_values(prepared, prime, method, target_orders, expected)
    )
    for _ in range(1, repetitions):
        timings.update_ns.append(
            _timed_maintained_update(prepared, prime, method, target_orders, None)
        )
    return stats


def _timed_maintained_update(
    prepared: Prepared,
    prime: int,
    method: str,
    target_orders: list[list[int]],
    stats: _UpdateStats | None,
) -> int:
    vineyard = new_vineyard(prepared, prime, method)
    order = list(range(len(prepared.simplices)))
    start = time.perf_counter_ns()
    for snapshot in range(1, len(prepared.values)):
        swaps, pairing_switches = transpose_to(vineyard, order, target_orders[snapshot])
        if stats is not None:
            stats.transition_swaps.append(swaps)
            stats.swaps_total += swaps
            stats.pairing_switches_total += pairing_switches
    return time.perf_counter_ns() - start


def _warm_values(
    prepared: Prepared,
    prime: int,
    method: str,
    target_orders: list[list[int]],
    expected: list[list[Bar]],
) -> list[list[Bar]]:
    vineyard = new_vineyard(prepared, prime, method)
    order = list(range(len(prepared.simplices)))
    values = []
    for snapshot in range(1, len(prepared.values)):
        transpose_to(vineyard, order, target_orders[snapshot])
        check_expected(
            vineyard,
            prepared,
            prepared.values[snapshot],
            expected[snapshot],
            snapshot,
        )
        values.append(bars_from_vineyard(vineyard, prepared, prepared.values[snapshot]))
    return values


def _measure_full_updates(
    prepared: Prepared,
    prime: int,
    method: str,
    repetitions: int,
    timings: Timings,
) -> None:
    for _ in range(repetitions):
        vineyard = new_vineyard(prepared, prime, method)
        order = list(range(len(prepared.simplices)))
        start = time.perf_counter_ns()
        for snapshot in range(1, len(prepared.values)):
            endpoint = endpoint_filtration(prepared, prepared.values[snapshot])
            target = order_from_filtration(endpoint, prepared.stable_by_simplex)
            transpose_to(vineyard, order, target)
        timings.full_update_ns.append(time.perf_counter_ns() - start)


def _measure_fresh_updates(
    prepared: Prepared, prime: int, repetitions: int, timings: Timings
) -> None:
    for _ in range(repetitions):
        start = time.perf_counter_ns()
        for values in prepared.values[1:]:
            fresh_bars(prepared, values, prime)
        timings.fresh_ns.append(time.perf_counter_ns() - start)


def _record(
    trajectory: Trajectory,
    prepared: Prepared,
    prime: int,
    method: str,
    repetitions: int,
    timings: Timings,
    stats: _UpdateStats,
) -> str:
    cells = len(prepared.simplices)
    triangles = sum(dimension == 2 for dimension in prepared.dimensions)
    encoded_levels = len(
        {weight for snapshot in trajectory.weights for weight in snapshot}
    )
    update = median(timings.update_ns)
    full_update = median(timings.full_update_ns)
    fresh = median(timings.fresh_ns)
    speedup = fresh / update if update else math.inf
    full_speedup = fresh / full_update if full_update else math.inf
    max_swaps = max(stats.transition_swaps, default=0)
    return (
        f"format=holos-dionysus-v1 dataset={trajectory.dataset} "
        f"source_sha256={trajectory.source_sha256} vertices={trajectory.vertices} "
        f"edges={len(trajectory.edges)} snapshots={len(trajectory.weights)} modulus={prime} "
        f"method={method} package_version={PACKAGE_VERSION} source_commit={SOURCE_COMMIT} "
        f"license={LICENSE} repetitions={repetitions} cells={cells} triangles={triangles} "
        f"weight_encoding=global_rank encoded_levels={encoded_levels} "
        f"compile_ns={median(timings.compile_ns)} update_ns={update} "
        f"full_update_ns={full_update} fresh_ns={fresh} update_speedup={speedup:.9g} "
        f"full_speedup={full_speedup:.9g} swaps_total={stats.swaps_total} "
        f"pairing_switches_total={stats.pairing_switches_total} max_transition_swaps={max_swaps} "
        "exact=yes "
        f"compile_samples_ns={','.join(map(str, timings.compile_ns))} "
        f"update_samples_ns={','.join(map(str, timings.update_ns))} "
        f"full_update_samples_ns={','.join(map(str, timings.full_update_ns))} "
        f"fresh_samples_ns={','.join(map(str, timings.fresh_ns))}"
    )
