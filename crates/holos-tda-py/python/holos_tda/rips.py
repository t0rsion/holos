from . import _core
from ._coerce import _float_values, _points, _triplets
from .records import _class_result


def rips_points(
    points,
    max_dim=1,
    threshold=None,
    modulus=2,
    threads=1,
    factorization="off",
    collapse_edges=False,
    collapse_schedule="serial",
    collapse_objective="h2",
    collapse_work_limit=None,
):
    """Compute Rips persistence of a Euclidean point cloud.

    Args:
        points: sequence of points, each a sequence of float coordinates of
            the same dimension.
        max_dim: highest homology dimension to compute.
        threshold: truncate the filtration at this scale. ``None`` uses the
            enclosing radius.
        modulus: coefficient field Z/p; must be a prime below 32768.
        threads: reduction worker threads. 1 runs the serial engine. The
            diagram is identical at any thread count.
        factorization: ``"auto"``, ``"off"``, or ``"force"`` for the
            vertex-biconnected sparse-graph split. The default is ``"off"``.
        collapse_edges: collapse dominated edges before the engine runs.
            The diagram is identical either way.
        collapse_schedule: ``"serial"``, ``"ordered"``, ``"rounds"``, or
            ``"adaptive"``. The default is ``"serial"``.
        collapse_objective: ``"h1"`` or ``"h2"`` for the adaptive schedule.
        collapse_work_limit: maximum adaptive removability tests. ``None``
            runs to a fixed point.
    """
    return _core.rips_points(
        _points(points),
        max_dim,
        threshold,
        modulus,
        threads,
        factorization,
        collapse_edges,
        collapse_schedule,
        collapse_objective,
        collapse_work_limit,
    )


def rips_condensed(
    data,
    max_dim=1,
    threshold=None,
    modulus=2,
    threads=1,
    factorization="off",
    collapse_edges=False,
    collapse_schedule="serial",
    collapse_objective="h2",
    collapse_work_limit=None,
):
    """Compute Rips persistence of a condensed distance matrix.

    The layout is upper-triangular and row-major, the same as
    ``scipy.spatial.distance.pdist``. ``data`` is the ``n(n-1)/2`` pairwise
    distances. Other keyword arguments match ``rips_points``.
    """
    return _core.rips_condensed(
        _float_values(data),
        max_dim,
        threshold,
        modulus,
        threads,
        factorization,
        collapse_edges,
        collapse_schedule,
        collapse_objective,
        collapse_work_limit,
    )


def rips_sparse(
    n,
    triplets,
    max_dim=1,
    threshold=None,
    modulus=2,
    threads=1,
    factorization="off",
    collapse_edges=False,
    collapse_schedule="serial",
    collapse_objective="h2",
    collapse_work_limit=None,
):
    """Compute Rips persistence of a sparse distance matrix.

    ``n`` is the vertex count. ``triplets`` are ``(i, j, distance)`` entries.
    Pairs not listed are absent at every scale. With ``threshold=None``, all
    listed edges enter the filtration. Other keyword arguments match
    ``rips_points``.
    """
    return _core.rips_sparse(
        n,
        _triplets(triplets),
        max_dim,
        threshold,
        modulus,
        threads,
        factorization,
        collapse_edges,
        collapse_schedule,
        collapse_objective,
        collapse_work_limit,
    )


def rips_points_classes(
    points,
    max_dim=1,
    threshold=None,
    modulus=2,
    threads=1,
    factorization="off",
    collapse_edges=False,
    collapse_schedule="serial",
    collapse_objective="h2",
    collapse_work_limit=None,
):
    """Compute a diagram and one canonical H1 basis cocycle per positive bar.

    The result is ``(bars, classes)``. Each class record includes ``group_id``,
    ``id``, ``basis_index``, ``birth``, ``death``, ``essential``, ``modulus``,
    ``scale``, ``terms``, and source ``provenance``. Equal intervals share one
    ``group_id``. A finite class is represented immediately below its death.
    Circular class bindings require an odd-prime ``modulus``, such as 47.
    """
    raw = _core.rips_points_classes(
        _points(points),
        max_dim,
        threshold,
        modulus,
        threads,
        factorization,
        collapse_edges,
        collapse_schedule,
        collapse_objective,
        collapse_work_limit,
    )
    return _class_result(raw)


def rips_condensed_classes(
    data,
    max_dim=1,
    threshold=None,
    modulus=2,
    threads=1,
    factorization="off",
    collapse_edges=False,
    collapse_schedule="serial",
    collapse_objective="h2",
    collapse_work_limit=None,
):
    """Compute source-bound H1 cocycles from condensed distances.

    Circular class bindings require an odd-prime ``modulus``, such as 47.
    """
    raw = _core.rips_condensed_classes(
        _float_values(data),
        max_dim,
        threshold,
        modulus,
        threads,
        factorization,
        collapse_edges,
        collapse_schedule,
        collapse_objective,
        collapse_work_limit,
    )
    return _class_result(raw)


def rips_sparse_classes(
    n,
    triplets,
    max_dim=1,
    threshold=None,
    modulus=2,
    threads=1,
    factorization="off",
    collapse_edges=False,
    collapse_schedule="serial",
    collapse_objective="h2",
    collapse_work_limit=None,
):
    """Compute source-bound H1 cocycles from sparse distances.

    Circular class bindings require an odd-prime ``modulus``, such as 47.
    """
    raw = _core.rips_sparse_classes(
        n,
        _triplets(triplets),
        max_dim,
        threshold,
        modulus,
        threads,
        factorization,
        collapse_edges,
        collapse_schedule,
        collapse_objective,
        collapse_work_limit,
    )
    return _class_result(raw)
