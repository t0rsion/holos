"""Independent GUDHI slice ranks for the external bipersistence check."""

from __future__ import annotations

from collections.abc import Mapping
from itertools import combinations
from typing import Any

import gudhi
import numpy as np


def distance_matrix(case: Mapping[str, Any]) -> np.ndarray:
    """Build the dense matrix used by multipers from one frozen graph."""

    size = int(case["vertex_count"])
    matrix = np.full((size, size), np.inf, dtype=float)
    np.fill_diagonal(matrix, 0.0)
    for edge in case["edges"]:
        left, right, value = int(edge[0]), int(edge[1]), float(edge[2])
        matrix[left, right] = value
        matrix[right, left] = value
    return matrix


def slice_h1_rank(matrix: np.ndarray, scale: float, minimum_degree: int) -> int:
    """Return the F2 H1 rank of one degree-Rips slice."""

    active = _active_vertices(matrix, scale, minimum_degree)
    graph_edges = _active_edges(matrix, active, scale)
    simplex_tree = _simplex_tree(matrix, active, graph_edges)
    if simplex_tree.dimension() < 1:
        return 0
    simplex_tree.compute_persistence(homology_coeff_field=2, persistence_dim_max=True)
    betti_numbers = simplex_tree.betti_numbers()
    return int(betti_numbers[1]) if len(betti_numbers) > 1 else 0


def _active_vertices(
    matrix: np.ndarray, scale: float, minimum_degree: int
) -> list[int]:
    size = matrix.shape[0]
    degrees = np.zeros(size, dtype=int)
    for left in range(size):
        for right in range(left + 1, size):
            if matrix[left, right] <= scale:
                degrees[left] += 1
                degrees[right] += 1
    return [vertex for vertex in range(size) if degrees[vertex] >= minimum_degree]


def _active_edges(
    matrix: np.ndarray, active: list[int], scale: float
) -> set[tuple[int, int]]:
    return {
        (left, right)
        for left, right in combinations(active, 2)
        if matrix[left, right] <= scale
    }


def _simplex_tree(
    matrix: np.ndarray,
    active: list[int],
    graph_edges: set[tuple[int, int]],
) -> gudhi.SimplexTree:
    simplex_tree = gudhi.SimplexTree()
    for vertex in active:
        simplex_tree.insert([vertex], filtration=0.0)
    for left, right in graph_edges:
        simplex_tree.insert([left, right], filtration=float(matrix[left, right]))
    for triangle in combinations(active, 3):
        if all(
            tuple(sorted(edge)) in graph_edges for edge in combinations(triangle, 2)
        ):
            filtration = max(
                matrix[left, right] for left, right in combinations(triangle, 2)
            )
            simplex_tree.insert(list(triangle), filtration=float(filtration))
    return simplex_tree


def slice_ranks(case: Mapping[str, Any]) -> list[int]:
    """Return scale-major H1 ranks on the declared grid."""

    matrix = distance_matrix(case)
    return [
        slice_h1_rank(matrix, float(scale), int(minimum_degree))
        for scale in case["scales"]
        for minimum_degree in case["minimum_degrees"]
    ]
