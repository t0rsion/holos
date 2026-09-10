"""Smoke tests for basic Vietoris-Rips and persistence entry points."""

import math

import holos_tda

from smoke_utils import SQRT2, close, condensed_square, cycle, square


def run():
    _check_point_persistence()
    pd = _check_condensed_input()
    _check_sparse_input()
    _check_collapse_portfolio()
    _check_explicit_complex()
    _check_thread_consistency(pd)


def _check_point_persistence():
    # Unit square from points: 4 H0 bars (one essential), one H1 bar [1, sqrt 2).
    bars = holos_tda.rips_points(square(), max_dim=1)
    _check_h0_bars(bars)
    _check_h1_bar(bars)


def _check_h0_bars(bars):
    assert len([b for b in bars if b[0] == 0]) == 4
    assert len([b for b in bars if b[0] == 0 and b[2] == math.inf]) == 1


def _check_h1_bar(bars):
    (h1,) = [b for b in bars if b[0] == 1]
    assert close(h1[1], 1.0) and close(h1[2], SQRT2)


def _check_condensed_input():
    # Condensed input follows SciPy pdist (upper-triangle) order. This n=4
    # asymmetric matrix distinguishes pdist order from lower-triangle order:
    # pdist [d01, d02, d03, d12, d13, d23] = [0.5, 0.5, 1, 10, 5, 6] gives H0
    # finite deaths [0.5, 0.5, 1]. A lower-triangle misread gives [0.5, 0.5, 5].
    bars = holos_tda.rips_condensed(
        [0.5, 0.5, 1.0, 10.0, 5.0, 6.0], max_dim=0)
    deaths = sorted(b[2] for b in bars if b[2] != math.inf)
    assert deaths == [0.5, 0.5, 1.0], deaths

    # pdist and points agree on the square.
    pd = condensed_square()
    assert holos_tda.rips_condensed(pd, max_dim=1) == holos_tda.rips_points(
        square(), max_dim=1
    )
    return pd


def _check_sparse_input():
    # Sparse 4-cycle: the hole never fills (no diagonals listed).
    bars = holos_tda.rips_sparse(4, cycle())
    (h1,) = [b for b in bars if b[0] == 1]
    assert close(h1[1], 1.0) and h1[2] == math.inf


def _check_collapse_portfolio():
    # The exact collapse portfolio checks all three declared schedules and emits
    # a portable finite-choice proof.
    tetrahedron = [
        (u, v, 1.0) for u in range(4) for v in range(u + 1, 4)
    ]
    portfolio = holos_tda.compile_collapse_portfolio(
        4, tetrahedron, max_dim=1, threads=2)
    assert portfolio["artifact"].startswith(b"HOLOSPOR")
    assert len(portfolio["entries"]) == 3
    assert portfolio["entries"][portfolio["selected"]]["simplex_counts"]


def _check_explicit_complex():
    # The explicit-complex boundary certifies a non-flag four-cycle directly.
    explicit_cycle = [([vertex], 0.0) for vertex in range(4)] + [
        ([0, 1], 1.0), ([1, 2], 1.0), ([2, 3], 1.0), ([0, 3], 1.0),
    ]
    explicit = holos_tda.compile_explicit_persistence(
        explicit_cycle, max_dim=1, modulus=3)
    assert explicit["artifact"].startswith(b"HOLOSEXP")
    assert len([bar for bar in explicit["bars"]
                if bar[0] == 1 and bar[2] == math.inf]) == 1


def _check_thread_consistency(pd):
    # threads=2 yields the identical diagram through every entry point.
    sq = square()
    assert holos_tda.rips_points(sq, max_dim=1, threads=2) == holos_tda.rips_points(
        sq, max_dim=1
    )
    assert holos_tda.rips_condensed(
        pd, max_dim=1, threads=2) == holos_tda.rips_condensed(pd, max_dim=1)
    cyc = cycle()
    assert holos_tda.rips_sparse(4, cyc, threads=2) == holos_tda.rips_sparse(4, cyc)
    assert holos_tda.rips_sparse(
        4, cyc, factorization="force") == holos_tda.rips_sparse(4, cyc)
