from . import _core
from ._coerce import _triplets


def compile_collapse_portfolio(
    n,
    triplets,
    max_dim=1,
    threshold=None,
    threads=4,
    score="columns",
    adaptive_objective="h1",
    adaptive_work_limit=None,
):
    """Select the exact minimum over three checked collapse schedules.

    The declared portfolio contains serial, rounds, and adaptive schedules.
    The result proves the minimum score within this finite set. It does not
    claim a globally minimum collapse sequence.
    """
    artifact, selected, entries = _core.compile_collapse_portfolio(
        int(n),
        _triplets(triplets),
        int(max_dim),
        threshold,
        int(threads),
        str(score),
        str(adaptive_objective),
        adaptive_work_limit,
    )
    return {
        "artifact": bytes(artifact),
        "selected": selected,
        "entries": [
            {
                "schedule": schedule,
                "simplex_counts": simplex_counts,
                "surviving_edges": surviving_edges,
            }
            for schedule, simplex_counts, surviving_edges in entries
        ],
    }


def compile_explicit_persistence(simplices, max_dim=1, modulus=2):
    """Certify persistence for an explicit scalar filtered complex.

    Each simplex is ``(vertices, grade)``. List every nonempty face exactly
    once. The artifact records checked ``D V = R`` factorizations through
    boundary dimension ``max_dim + 1``.
    """
    artifact, bars, simplex_counts, column_counts = _core.compile_explicit_persistence(
        [
            ([int(vertex) for vertex in vertices], float(grade))
            for vertices, grade in simplices
        ],
        int(max_dim),
        int(modulus),
    )
    return {
        "artifact": bytes(artifact),
        "bars": bars,
        "simplex_counts": simplex_counts,
        "column_counts": column_counts,
    }


def compile_relative_interface(
    n, triplets, protected=(), max_dim=1, threshold=None, modulus=2
):
    """Compile a proof-carrying chain core relative to protected vertices.

    The protected vertices induce the separator subcomplex.
    """
    artifact, bars, work = _core.compile_relative_interface(
        int(n),
        _triplets(triplets),
        [int(vertex) for vertex in protected],
        int(max_dim),
        threshold,
        int(modulus),
    )
    return {
        "artifact": bytes(artifact),
        "bars": bars,
        "input_cells": work[0],
        "cancellations": work[1],
        "core_cells": work[2],
    }


def merge_relative_interfaces(artifacts, store, separator=(), protected=()):
    """Durably compose ordered relative-interface artifacts.

    The content-addressed store retains shards and intermediate folds. A retry
    with the same inputs resumes or returns the committed result.
    """
    manifest, result, job, work = _core.merge_relative_interfaces(
        [bytes(artifact) for artifact in artifacts],
        str(store),
        [int(vertex) for vertex in separator],
        [int(vertex) for vertex in protected],
    )
    return {
        "manifest": bytes(manifest),
        "result": bytes(result),
        "job": job,
        "shards": work[0],
        "folds_reused": work[1],
        "folds_computed": work[2],
        "peak_artifact_bytes": work[3],
    }


def cohomology_space(n, triplets, dimension, scale, modulus=2):
    """Compute a canonical fixed-scale cohomology basis.

    Terms use ``([v0, ..., vq], coefficient)`` on oriented simplices.
    The space is defined in any bounded dimension, including H0.
    """
    identifier, simplex_counts, basis = _core.fixed_cohomology(
        int(n),
        _triplets(triplets),
        int(dimension),
        float(scale),
        int(modulus),
    )
    return {
        "id": identifier,
        "dimension": int(dimension),
        "scale": float(scale),
        "modulus": int(modulus),
        "rank": len(basis),
        "simplex_counts": simplex_counts,
        "basis": [
            {"id": class_id, "basis_index": index, "terms": terms}
            for index, (class_id, terms) in enumerate(basis)
        ],
    }


def cohomology_relation(n, old_triplets, new_triplets, dimension, scale, modulus=2):
    """Relate two cohomology spaces on their common active subcomplex."""
    values = _core.relate_fixed_cohomology(
        int(n),
        _triplets(old_triplets),
        _triplets(new_triplets),
        int(dimension),
        float(scale),
        int(modulus),
    )
    return {
        "old_rank": values[0],
        "new_rank": values[1],
        "old_image_rank": values[2],
        "new_image_rank": values[3],
        "old_kernel_rank": values[4],
        "new_kernel_rank": values[5],
        "relation_rank": values[6],
        "isomorphism": values[7],
        "basis": [{"old": old, "new": new} for old, new in values[8]],
    }
