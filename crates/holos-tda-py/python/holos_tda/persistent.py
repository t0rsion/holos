"""Source-bound persistent H1 class and selected-coordinate artifacts."""

from . import _core
from ._coerce import (
    _float_values,
    _points,
    _square_rows,
    _triplets,
)
from .circular import _integral_lift
from .records import _class_record


def _critical_pair(raw):
    birth_vertices, birth_value, death = raw
    return {
        "birth": {"vertices": birth_vertices, "value": birth_value},
        "death": (
            None
            if death is None
            else {"vertices": death[0], "value": death[1]}
        ),
    }


def _witness_cycle(raw):
    return [
        {"u": u, "v": v, "coefficient": coefficient}
        for u, v, coefficient in raw
    ]


def _witness_chain(raw):
    return [
        {"vertices": vertices, "coefficient": coefficient}
        for vertices, coefficient in raw
    ]


def _persistent_result(raw):
    artifact, persistent_class, critical_pair, cycle, bounding_chain = raw
    return {
        "artifact": bytes(artifact),
        "class": _class_record(persistent_class),
        "critical_pair": _critical_pair(critical_pair),
        "cycle": _witness_cycle(cycle),
        "bounding_chain": _witness_chain(bounding_chain),
    }


def _selected_coordinate_record(raw):
    (
        phase,
        field_multiplier,
        divisibility,
        energy,
        max_residual,
        relative_residual,
        iterations,
        integral,
        potential,
        tolerance,
    ) = raw
    return {
        "phase": phase,
        "field_multiplier": field_multiplier,
        "divisibility": divisibility,
        "energy": energy,
        "max_residual": max_residual,
        "relative_residual": relative_residual,
        "iterations": iterations,
        "integral": integral,
        "potential": potential,
        "tolerance": tolerance,
    }


def _persistent_coordinate_result(raw):
    result = _persistent_result(raw[:5])
    result["coordinate"] = _selected_coordinate_record(raw[5])
    return result


def persistent_class_sparse(
    n,
    triplets,
    max_dim=1,
    threshold=None,
    modulus=2,
    threads=1,
    space_index=0,
    basis_index=0,
):
    """Build a source-bound persistent H1 class artifact on sparse input.

    ``space_index`` selects the interval-ordered persistent class space and
    ``basis_index`` selects its canonical basis vector. The result includes
    the artifact bytes and the selected class witnesses.
    """
    raw = _core.persistent_class_sparse(
        int(n),
        _triplets(triplets),
        int(max_dim),
        threshold,
        int(modulus),
        int(threads),
        int(space_index),
        int(basis_index),
    )
    return _persistent_result(raw)


def persistent_class_condensed(
    data,
    max_dim=1,
    threshold=None,
    modulus=2,
    space_index=0,
    basis_index=0,
):
    """Build a source-bound persistent class artifact from ``pdist`` data."""
    raw = _core.persistent_class_condensed(
        _float_values(data),
        int(max_dim),
        threshold,
        int(modulus),
        int(space_index),
        int(basis_index),
    )
    return _persistent_result(raw)


def persistent_class_points(
    points,
    max_dim=1,
    threshold=None,
    modulus=2,
    threads=1,
    space_index=0,
    basis_index=0,
):
    """Build an artifact from all finite pairwise Euclidean distances."""
    raw = _core.persistent_class_points(
        _points(points),
        int(max_dim),
        threshold,
        int(modulus),
        int(threads),
        int(space_index),
        int(basis_index),
    )
    return _persistent_result(raw)


def persistent_class_square(
    distances,
    max_dim=1,
    threshold=None,
    modulus=2,
    space_index=0,
    basis_index=0,
):
    """Build an artifact from a symmetric square distance matrix."""
    rows = _square_rows(distances, "distances must be a square matrix")
    raw = _core.persistent_class_square(
        _points(rows),
        int(max_dim),
        threshold,
        int(modulus),
        int(space_index),
        int(basis_index),
    )
    return _persistent_result(raw)


def persistent_circular_sparse(
    n,
    triplets,
    max_dim=1,
    threshold=None,
    modulus=47,
    tolerance=1e-10,
    max_iterations=10_000,
    space_index=0,
    basis_index=0,
    integral_lift=None,
):
    """Build a selected persistent circular coordinate on sparse input.

    The class is selected by ``space_index`` and ``basis_index`` in the native
    producer. ``integral_lift`` is validated as integer edge rows and supports
    modulus two. The result carries the nested class witnesses and coordinate.
    """
    raw = _core.persistent_circular_sparse(
        int(n),
        _triplets(triplets),
        int(max_dim),
        threshold,
        int(modulus),
        float(tolerance),
        int(max_iterations),
        int(space_index),
        int(basis_index),
        _integral_lift(integral_lift),
    )
    return _persistent_coordinate_result(raw)


def persistent_circular_condensed(
    data,
    max_dim=1,
    threshold=None,
    modulus=47,
    tolerance=1e-10,
    max_iterations=10_000,
    space_index=0,
    basis_index=0,
    integral_lift=None,
):
    """Build a selected persistent circular coordinate from ``pdist`` data."""
    raw = _core.persistent_circular_condensed(
        _float_values(data),
        int(max_dim),
        threshold,
        int(modulus),
        float(tolerance),
        int(max_iterations),
        int(space_index),
        int(basis_index),
        _integral_lift(integral_lift),
    )
    return _persistent_coordinate_result(raw)


def persistent_circular_points(
    points,
    max_dim=1,
    threshold=None,
    modulus=47,
    tolerance=1e-10,
    max_iterations=10_000,
    threads=1,
    space_index=0,
    basis_index=0,
    integral_lift=None,
):
    """Build a coordinate from all finite pairwise Euclidean distances."""
    raw = _core.persistent_circular_points(
        _points(points),
        int(max_dim),
        threshold,
        int(modulus),
        float(tolerance),
        int(max_iterations),
        int(threads),
        int(space_index),
        int(basis_index),
        _integral_lift(integral_lift),
    )
    return _persistent_coordinate_result(raw)


def persistent_circular_square(
    distances,
    max_dim=1,
    threshold=None,
    modulus=47,
    tolerance=1e-10,
    max_iterations=10_000,
    space_index=0,
    basis_index=0,
    integral_lift=None,
):
    """Build a coordinate from a symmetric square distance matrix."""
    rows = _square_rows(distances, "distances must be a square matrix")
    raw = _core.persistent_circular_square(
        _points(rows),
        int(max_dim),
        threshold,
        int(modulus),
        float(tolerance),
        int(max_iterations),
        int(space_index),
        int(basis_index),
        _integral_lift(integral_lift),
    )
    return _persistent_coordinate_result(raw)
