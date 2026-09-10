from . import _core
from ._coerce import (
    _cocycle,
    _condensed_rows,
    _float_values,
    _persistent_class_input,
    _points,
    _square_rows,
    _triplets,
)


def _circular_coordinate_record(raw):
    (
        phase,
        multiplier,
        divisibility,
        energy,
        relative_residual,
        iterations,
        class_coordinates,
    ) = raw
    return {
        "phase": phase,
        "field_multiplier": multiplier,
        "divisibility": divisibility,
        "energy": energy,
        "relative_residual": relative_residual,
        "iterations": iterations,
        "class": [
            {"basis_index": index, "coefficient": coefficient}
            for index, coefficient in class_coordinates
        ],
    }


def _circular_result(raw):
    artifact, coordinate, status, ambiguity_rank, continued = raw
    return {
        "artifact": bytes(artifact),
        "coordinate": _circular_coordinate_record(coordinate),
        "continuation": status,
        "ambiguity_rank": ambiguity_rank,
        "continued_coordinate": (
            None if continued is None else _circular_coordinate_record(continued)
        ),
    }


def circular_sparse(
    n,
    triplets,
    cocycle,
    scale,
    modulus=47,
    tolerance=1e-10,
    max_iterations=10_000,
    other_triplets=None,
):
    """Build a checked circular coordinate on a sparse Rips graph.

    ``cocycle`` accepts Ripser-shaped ``(u, v, coefficient)`` rows. Ripser
    must use the same odd prime. ``other_triplets`` asks for conservative
    continuation through the common active subcomplex.
    """
    raw = _core.circular_sparse(
        int(n),
        _triplets(triplets),
        _cocycle(cocycle),
        float(scale),
        int(modulus),
        float(tolerance),
        int(max_iterations),
        None
        if other_triplets is None
        else _triplets(other_triplets),
    )
    return _circular_result(raw)


def circular_sparse_class(
    n,
    triplets,
    persistent_class,
    tolerance=1e-10,
    max_iterations=10_000,
    other_triplets=None,
):
    """Build a checked circular coordinate from a sparse Rips class record.

    The record must come from ``rips_sparse_classes``. Its provenance binds the
    active labeled graph, interval, field, representative scale, and class
    identity. Use an odd-prime record, such as ``modulus=47``. These bindings
    do not accept a separate integral lift for modulus 2.
    """
    raw = _core.circular_sparse_class(
        int(n),
        _triplets(triplets),
        _persistent_class_input(persistent_class),
        tolerance,
        max_iterations,
        None
        if other_triplets is None
        else _triplets(other_triplets),
    )
    return _circular_result(raw)


def circular_condensed(
    data, cocycle, scale, modulus=47, tolerance=1e-10, max_iterations=10_000, other=None
):
    """Build a checked coordinate from SciPy ``pdist``-ordered distances."""
    raw = _core.circular_condensed(
        _float_values(data),
        _cocycle(cocycle),
        float(scale),
        int(modulus),
        float(tolerance),
        int(max_iterations),
        None if other is None else _float_values(other),
    )
    return _circular_result(raw)


def circular_condensed_class(
    data, persistent_class, tolerance=1e-10, max_iterations=10_000, other=None
):
    """Build a checked circular coordinate from a condensed Rips class record.

    The record must come from ``rips_condensed_classes``. Its provenance binds
    the active labeled graph, interval, field, representative scale, and class
    identity. Use an odd-prime record, such as ``modulus=47``. These bindings
    do not accept a separate integral lift for modulus 2.
    """
    raw = _core.circular_condensed_class(
        _float_values(data),
        _persistent_class_input(persistent_class),
        tolerance,
        max_iterations,
        None if other is None else _float_values(other),
    )
    return _circular_result(raw)


def circular_coordinates(
    distances,
    cocycle,
    scale,
    modulus=47,
    tolerance=1e-10,
    max_iterations=10_000,
    other=None,
):
    """Build a checked coordinate from a square distance matrix.

    This accepts the same H1 cocycle rows returned by
    ``ripser(..., coeff=modulus, do_cocycles=True)``. The matrix and cocycle
    must use the same vertex labels.
    """
    rows = _square_rows(distances, "distances must be a square matrix")
    other_rows = None
    if other is not None:
        other_rows = _square_rows(
            other,
            "other must have the same square shape as distances",
            len(rows),
        )
    return circular_condensed(
        _condensed_rows(rows),
        cocycle,
        scale,
        modulus,
        tolerance,
        max_iterations,
        None if other_rows is None else _condensed_rows(other_rows),
    )


def circular_coordinates_class(
    distances, persistent_class, tolerance=1e-10, max_iterations=10_000, other=None
):
    """Build a checked circular coordinate from a square-distance class record.

    The record must come from ``rips_points_classes`` or
    ``rips_condensed_classes``. Its provenance binds the active labeled graph,
    interval, field, representative scale, and class identity. Use an
    odd-prime record, such as ``modulus=47``. This binding does not accept a
    separate integral lift for modulus 2.
    """
    rows = _square_rows(distances, "distances must be a square matrix")
    other_rows = None
    if other is not None:
        other_rows = _square_rows(
            other,
            "other must have the same square shape as distances",
            len(rows),
        )
    return circular_condensed_class(
        _condensed_rows(rows),
        persistent_class,
        tolerance,
        max_iterations,
        None if other_rows is None else _condensed_rows(other_rows),
    )


def circular_points(
    points,
    cocycle,
    scale,
    modulus=47,
    tolerance=1e-10,
    max_iterations=10_000,
    threads=1,
    other=None,
):
    """Build a checked coordinate on an exact threshold graph of points."""
    raw = _core.circular_points(
        _points(points),
        _cocycle(cocycle),
        float(scale),
        int(modulus),
        float(tolerance),
        int(max_iterations),
        int(threads),
        None
        if other is None
        else _points(other),
    )
    return _circular_result(raw)


def circular_points_class(
    points,
    persistent_class,
    tolerance=1e-10,
    max_iterations=10_000,
    threads=1,
    other=None,
):
    """Build a checked circular coordinate from a point-cloud class record.

    The record must come from ``rips_points_classes``. Its provenance binds the
    active labeled graph, interval, field, representative scale, and class
    identity. Use an odd-prime record, such as ``modulus=47``. These bindings
    do not accept a separate integral lift for modulus 2.
    """
    raw = _core.circular_points_class(
        _points(points),
        _persistent_class_input(persistent_class),
        tolerance,
        max_iterations,
        threads,
        None if other is None else _points(other),
    )
    return _circular_result(raw)
