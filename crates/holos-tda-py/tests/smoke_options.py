"""Smoke tests for collapse settings, coefficients, and the console script."""

import subprocess

import holos_tda

from smoke_utils import condensed_square, cycle, square


def run():
    sq = square()
    pd = condensed_square()
    cyc = cycle()

    # collapse_edges=True yields the identical diagram through every entry point.
    _check_collapse_edges(sq, pd, cyc)
    _check_schedules(sq, cyc)
    _check_adaptive_error(sq)

    _check_coefficients()
    _check_cli()


def _check_collapse_edges(sq, pd, cyc):
    assert holos_tda.rips_points(sq, max_dim=1, collapse_edges=True) == holos_tda.rips_points(
        sq, max_dim=1
    )
    assert holos_tda.rips_condensed(
        pd, max_dim=1, collapse_edges=True
    ) == holos_tda.rips_condensed(pd, max_dim=1)
    assert holos_tda.rips_sparse(
        4, cyc, collapse_edges=True) == holos_tda.rips_sparse(4, cyc)


def _check_schedules(sq, cyc):
    # Every public schedule, both adaptive objectives, and a partial adaptive run
    # preserve the diagram.
    for schedule in ("ordered", "rounds"):
        assert holos_tda.rips_points(
            sq, max_dim=1, threads=2, collapse_edges=True,
            collapse_schedule=schedule,
        ) == holos_tda.rips_points(sq, max_dim=1)
    for objective in ("h1", "h2"):
        assert holos_tda.rips_points(
            sq, max_dim=1, collapse_edges=True, collapse_schedule="adaptive",
            collapse_objective=objective,
        ) == holos_tda.rips_points(sq, max_dim=1)
    assert holos_tda.rips_sparse(
        4, cyc, collapse_edges=True, collapse_schedule="adaptive",
        collapse_work_limit=0,
    ) == holos_tda.rips_sparse(4, cyc)


def _check_adaptive_error(sq):
    try:
        holos_tda.rips_points(sq, collapse_schedule="adaptive")
    except ValueError as error:
        assert "collapse_edges=True" in str(error)
    else:
        raise AssertionError("adaptive settings without collapse_edges must raise")


def _check_coefficients():
    # Coefficients: valid odd prime works, composite raises.
    holos_tda.rips_points([[0, 0], [1, 0]], modulus=3)
    try:
        holos_tda.rips_points([[0, 0], [1, 0]], modulus=4)
    except ValueError as error:
        assert "prime" in str(error)
    else:
        raise AssertionError("modulus=4 must raise ValueError")


def _check_cli():
    # Console script is the real CLI.
    out = subprocess.run(
        ["holos-tda", "--version"], capture_output=True, text=True, check=True
    )
    assert holos_tda.__version__ in out.stdout
