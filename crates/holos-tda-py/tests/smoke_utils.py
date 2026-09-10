"""Shared fixtures for the Python wheel smoke tests."""

import math


SQRT2 = math.sqrt(2.0)


def close(a, b, tol=1e-12):
    return a == b or abs(a - b) <= tol


def square():
    return [[0, 0], [1, 0], [1, 1], [0, 1]]


def condensed_square():
    return [1.0, SQRT2, 1.0, 1.0, SQRT2, 1.0]


def cycle():
    return [(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)]


def distance_square():
    return [
        [0.0, 1.0, SQRT2, 1.0],
        [1.0, 0.0, 1.0, SQRT2],
        [SQRT2, 1.0, 0.0, 1.0],
        [1.0, SQRT2, 1.0, 0.0],
    ]


def weighted_graph():
    return [
        (0, 1, 1.0), (0, 2, 2.0), (0, 3, 1.1),
        (1, 2, 1.2), (1, 3, 2.1), (2, 3, 1.3),
    ]


def octahedron_boundary():
    return [
        (u, v, 1.0 + (u + v) / 100.0)
        for u in range(6) for v in range(u + 1, 6)
        if (u, v) not in {(0, 1), (2, 3), (4, 5)}
    ]


def coverage_graph():
    return [
        (0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0),
        (0, 4, 1.0), (1, 4, 1.0), (2, 4, 1.0), (3, 4, 1.0),
        (0, 5, 1.0), (1, 5, 1.0), (2, 5, 1.0), (3, 5, 1.0),
    ]
