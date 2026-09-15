from numbers import Integral

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


_INTEGRAL_COEFFICIENT_LIMIT = 1 << 31


def _integral_lift(rows):
    """Validate and normalize an optional integer cocycle lift."""
    if rows is None:
        return None
    try:
        rows = list(rows)
    except TypeError as error:
        raise TypeError(
            "integral_lift must be an iterable of (u, v, coefficient) rows"
        ) from error
    if not rows:
        raise ValueError("integral_lift must contain at least one row")

    result = [_integral_lift_row(row, index) for index, row in enumerate(rows)]
    _validate_integral_lift_order(result)
    return result


def _integral_lift_argument(rows, modulus, other):
    result = _integral_lift(rows)
    if result is not None and other is not None and int(modulus) == 2:
        raise ValueError(
            "modulus 2 continuation is unsupported with a supplied lift; "
            "continuation uses automatic lifting on the other graph, which "
            "requires an odd prime"
        )
    return result


def _integral_lift_row(row, index):
    u, v, coefficient = _integral_lift_values(row, index)
    _validate_integral_lift_edge(u, v, index)
    _validate_integral_lift_coefficient(coefficient, index)
    return u, v, coefficient


def _integral_lift_values(row, index):
    if isinstance(row, (str, bytes)):
        raise TypeError(f"integral_lift row {index} must contain three integer values")
    try:
        values = tuple(row)
    except TypeError as error:
        raise TypeError(
            f"integral_lift row {index} must contain three integer values"
        ) from error
    if len(values) != 3:
        raise ValueError(f"integral_lift row {index} must contain three values")
    if any(
        isinstance(value, bool) or not isinstance(value, Integral) for value in values
    ):
        raise TypeError(f"integral_lift row {index} must contain three integer values")
    return tuple(int(value) for value in values)


def _validate_integral_lift_edge(u, v, index):
    if u < 0 or v < 0 or u >= v:
        raise ValueError(f"integral_lift row {index} must satisfy 0 <= u < v")


def _validate_integral_lift_coefficient(coefficient, index):
    if coefficient == 0 or abs(coefficient) > _INTEGRAL_COEFFICIENT_LIMIT:
        raise ValueError(
            f"integral_lift row {index} coefficient must be nonzero and at most 2^31"
        )


def _validate_integral_lift_order(rows):
    if any(left[:2] >= right[:2] for left, right in zip(rows, rows[1:])):
        raise ValueError("integral_lift rows must be in ascending edge order")


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
    integral_lift=None,
):
    """Build a checked circular coordinate on a sparse Rips graph.

    ``cocycle`` accepts Ripser-shaped ``(u, v, coefficient)`` rows.
    Automatic lifting requires an odd prime. ``other_triplets`` asks for
    conservative continuation through the common active subcomplex.
    ``integral_lift`` supplies checked integer rows when centered lifting is
    unavailable, including for modulus 2. Continuation still uses automatic
    lifting on the other graph.
    """
    integral = _integral_lift_argument(integral_lift, modulus, other_triplets)
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
        integral,
    )
    return _circular_result(raw)


def circular_sparse_class(
    n,
    triplets,
    persistent_class,
    tolerance=1e-10,
    max_iterations=10_000,
    other_triplets=None,
    integral_lift=None,
):
    """Build a checked circular coordinate from a sparse Rips class record.

    The record must come from ``rips_sparse_classes``. Its provenance binds the
    active labeled graph, interval, field, representative scale, and class
    identity. ``integral_lift`` supplies checked integer rows when the record
    uses modulus 2 or centered lifting does not find a lift. Continuation
    still uses automatic lifting on the other graph.
    """
    persistent_class = _persistent_class_input(persistent_class)
    integral = _integral_lift_argument(
        integral_lift, persistent_class[2], other_triplets
    )
    raw = _core.circular_sparse_class(
        int(n),
        _triplets(triplets),
        persistent_class,
        tolerance,
        max_iterations,
        None
        if other_triplets is None
        else _triplets(other_triplets),
        integral,
    )
    return _circular_result(raw)


def _circular_condensed_result(
    data,
    cocycle,
    scale,
    modulus,
    tolerance,
    max_iterations,
    other,
    integral,
):
    raw = _core.circular_condensed(
        data,
        cocycle,
        float(scale),
        int(modulus),
        float(tolerance),
        int(max_iterations),
        other,
        integral,
    )
    return _circular_result(raw)


def circular_condensed(
    data,
    cocycle,
    scale,
    modulus=47,
    tolerance=1e-10,
    max_iterations=10_000,
    other=None,
    integral_lift=None,
):
    """Build a checked coordinate from SciPy ``pdist``-ordered distances.

    ``integral_lift`` supplies checked integer rows when centered lifting is
    unavailable, including for modulus 2. Continuation still uses automatic
    lifting on the other graph.
    """
    integral = _integral_lift_argument(integral_lift, modulus, other)
    return _circular_condensed_result(
        _float_values(data),
        _cocycle(cocycle),
        scale,
        modulus,
        tolerance,
        max_iterations,
        None if other is None else _float_values(other),
        integral,
    )


def _circular_condensed_class_result(
    data,
    persistent_class,
    tolerance,
    max_iterations,
    other,
    integral,
):
    raw = _core.circular_condensed_class(
        data,
        persistent_class,
        tolerance,
        max_iterations,
        other,
        integral,
    )
    return _circular_result(raw)


def circular_condensed_class(
    data,
    persistent_class,
    tolerance=1e-10,
    max_iterations=10_000,
    other=None,
    integral_lift=None,
):
    """Build a checked circular coordinate from a condensed Rips class record.

    The record must come from ``rips_condensed_classes``. Its provenance binds
    the active labeled graph, interval, field, representative scale, and class
    identity. ``integral_lift`` supplies checked integer rows when the record
    uses modulus 2 or centered lifting does not find a lift. Continuation
    still uses automatic lifting on the other graph.
    """
    persistent_class = _persistent_class_input(persistent_class)
    integral = _integral_lift_argument(
        integral_lift, persistent_class[2], other
    )
    return _circular_condensed_class_result(
        _float_values(data),
        persistent_class,
        tolerance,
        max_iterations,
        None if other is None else _float_values(other),
        integral,
    )


def circular_coordinates(
    distances,
    cocycle,
    scale,
    modulus=47,
    tolerance=1e-10,
    max_iterations=10_000,
    other=None,
    integral_lift=None,
):
    """Build a checked coordinate from a square distance matrix.

    This accepts the same H1 cocycle rows returned by
    ``ripser(..., coeff=modulus, do_cocycles=True)``. The matrix and cocycle
    must use the same vertex labels. ``integral_lift`` supplies checked
    integer rows when centered lifting is unavailable, including for modulus
    2. Continuation still uses automatic lifting on the other graph.
    """
    integral = _integral_lift_argument(integral_lift, modulus, other)
    rows = _square_rows(distances, "distances must be a square matrix")
    other_rows = None
    if other is not None:
        other_rows = _square_rows(
            other,
            "other must have the same square shape as distances",
            len(rows),
        )
    return _circular_condensed_result(
        _float_values(_condensed_rows(rows)),
        _cocycle(cocycle),
        scale,
        modulus,
        tolerance,
        max_iterations,
        None
        if other_rows is None
        else _float_values(_condensed_rows(other_rows)),
        integral,
    )


def circular_coordinates_class(
    distances,
    persistent_class,
    tolerance=1e-10,
    max_iterations=10_000,
    other=None,
    integral_lift=None,
):
    """Build a checked circular coordinate from a square-distance class record.

    The record must come from ``rips_points_classes`` or
    ``rips_condensed_classes``. Its provenance binds the active labeled graph,
    interval, field, representative scale, and class identity.
    ``integral_lift`` supplies checked integer rows when the record uses
    modulus 2 or centered lifting does not find a lift. Continuation still
    uses automatic lifting on the other graph.
    """
    persistent_class = _persistent_class_input(persistent_class)
    integral = _integral_lift_argument(integral_lift, persistent_class[2], other)
    rows = _square_rows(distances, "distances must be a square matrix")
    other_rows = None
    if other is not None:
        other_rows = _square_rows(
            other,
            "other must have the same square shape as distances",
            len(rows),
        )
    return _circular_condensed_class_result(
        _float_values(_condensed_rows(rows)),
        persistent_class,
        tolerance,
        max_iterations,
        None
        if other_rows is None
        else _float_values(_condensed_rows(other_rows)),
        integral,
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
    integral_lift=None,
):
    """Build a checked coordinate on an exact threshold graph of points.

    ``integral_lift`` supplies checked integer rows when centered lifting is
    unavailable, including for modulus 2. Continuation still uses automatic
    lifting on the other graph.
    """
    integral = _integral_lift_argument(integral_lift, modulus, other)
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
        integral,
    )
    return _circular_result(raw)


def circular_points_class(
    points,
    persistent_class,
    tolerance=1e-10,
    max_iterations=10_000,
    threads=1,
    other=None,
    integral_lift=None,
):
    """Build a checked circular coordinate from a point-cloud class record.

    The record must come from ``rips_points_classes``. Its provenance binds the
    active labeled graph, interval, field, representative scale, and class
    identity. ``integral_lift`` supplies checked integer rows when the record
    uses modulus 2 or centered lifting does not find a lift. Continuation
    still uses automatic lifting on the other graph.
    """
    persistent_class = _persistent_class_input(persistent_class)
    integral = _integral_lift_argument(
        integral_lift, persistent_class[2], other
    )
    raw = _core.circular_points_class(
        _points(points),
        persistent_class,
        tolerance,
        max_iterations,
        threads,
        None if other is None else _points(other),
        integral,
    )
    return _circular_result(raw)
