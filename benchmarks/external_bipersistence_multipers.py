"""Run multipers and GUDHI in an isolated child process."""

from __future__ import annotations

import argparse
import json
import math
import sys
from collections.abc import Mapping, Sequence
from pathlib import Path
from typing import Any


def main(arguments: Sequence[str] | None = None) -> int:
    """Compute external ranks and print one JSON result to standard output."""

    options = _parse_arguments(arguments)
    try:
        import gudhi
        import multipers
        import numpy as np
        from multipers import signed_measure
        from multipers.filtrations import DegreeRips
    except (ImportError, OSError, RuntimeError, ValueError) as error:
        _write(
            {
                "status": "unavailable",
                "reason": "external Python import failed",
                "exception": f"{type(error).__name__}: {error}",
            }
        )
        return 0

    from external_bipersistence_gudhi import distance_matrix, slice_ranks

    try:
        input_data = json.loads(options.input.read_text(encoding="utf-8"))
        results = []
        for case in input_data["cases"]:
            matrix = distance_matrix(case)
            filtration = DegreeRips(
                distance_matrix=matrix,
                ks=np.arange(int(case["vertex_count"])),
                threshold_radius=float(case["threshold"]),
                squeeze=True,
                normalize=False,
            )
            if filtration.dimension < 2:
                filtration.expansion(2)
            grid = filtration.filtration_grid
            measure_points, measure_weights = signed_measure(
                filtration,
                degree=1,
                invariant="hilbert",
                grid=grid,
                clean=False,
                n_jobs=1,
            )[0]
            multipers_ranks = _ranks_from_hilbert_measure(
                measure_points,
                measure_weights,
                case,
            )
            gudhi_ranks = slice_ranks(case)
            results.append(
                {
                    "name": case["name"],
                    "native_grid": [
                        [
                            value if math.isfinite(float(value)) else "inf"
                            for value in axis
                        ]
                        for axis in grid
                    ],
                    "hilbert_measure": _finite_measure(measure_points, measure_weights),
                    "multipers_hilbert_ranks": multipers_ranks,
                    "gudhi_slice_ranks": gudhi_ranks,
                    "multipers_matches_gudhi": multipers_ranks == gudhi_ranks,
                }
            )
        _write(
            {
                "status": "pass"
                if all(item["multipers_matches_gudhi"] for item in results)
                else "mismatch",
                "multipers_version": getattr(multipers, "__version__", "unknown"),
                "gudhi_version": getattr(gudhi, "__version__", "unknown"),
                "cases": results,
            }
        )
        return 0
    except (
        AttributeError,
        IndexError,
        OSError,
        RuntimeError,
        TypeError,
        ValueError,
    ) as error:
        _write(
            {
                "status": "error",
                "reason": "external computation failed",
                "exception": f"{type(error).__name__}: {error}",
            }
        )
        return 0


def _ranks_from_hilbert_measure(
    points: Any,
    weights: Any,
    case: Mapping[str, Any],
) -> list[int]:
    """Recover node ranks by summing atoms below each grid coordinate.

    Multipers stores the minimum-degree axis as ``-k``. Infinite scale atoms
    lie beyond a finite Holos threshold and are not included.
    """

    finite = [
        (float(point[0]), float(point[1]), round(float(weight)))
        for point, weight in zip(points, weights)
        if math.isfinite(float(point[0])) and math.isfinite(float(point[1]))
    ]
    return [
        sum(
            weight
            for point_scale, point_degree, weight in finite
            if point_scale <= float(scale) and point_degree <= -float(minimum_degree)
        )
        for scale in case["scales"]
        for minimum_degree in case["minimum_degrees"]
    ]


def _finite_measure(points: Any, weights: Any) -> list[dict[str, float | int]]:
    """Serialize finite measure atoms and omit the infinite boundary atoms."""

    return [
        {
            "scale": float(point[0]),
            "degree": float(point[1]),
            "weight": round(float(weight)),
        }
        for point, weight in zip(points, weights)
        if math.isfinite(float(point[0])) and math.isfinite(float(point[1]))
    ]


def _parse_arguments(arguments: Sequence[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, required=True)
    return parser.parse_args(arguments)


def _write(value: Mapping[str, Any]) -> None:
    sys.stdout.write(json.dumps(value, sort_keys=True) + "\n")


if __name__ == "__main__":
    raise SystemExit(main())
