"""Vietoris-Rips persistent homology with a ripser-class engine.

Thin Python bindings over the ``holos-tda`` Rust crate. Each function returns
the persistence diagram as a list of ``(dim, birth, death)`` tuples in
canonical order: by dimension, then birth, then death. Essential classes
have ``death == math.inf``.
"""

import sys

from holos_tda import _core
from holos_tda._core import GIT_HASH, __version__

__all__ = [
    "GIT_HASH",
    "__version__",
    "PointAtlas",
    "SparseAtlas",
    "SparseIndex",
    "SparseProgram",
    "affine_events",
    "cohomology_relation",
    "cohomology_space",
    "compile_collapse_portfolio",
    "compile_explicit_persistence",
    "compile_points_atlas",
    "compile_relative_interface",
    "compile_sparse_atlas",
    "compile_sparse_index",
    "compile_sparse_program",
    "compile_sparse_proof",
    "compile_sparse_program_trace",
    "load_sparse_atlas",
    "load_sparse_program",
    "main",
    "intervene_cohomology",
    "kinetic_zigzag",
    "merge_relative_interfaces",
    "rips_condensed",
    "rips_condensed_classes",
    "rips_points",
    "rips_points_classes",
    "rips_sparse",
    "rips_sparse_classes",
    "relative_coverage",
    "synthesize_affine_coverage",
    "synthesize_affine_cohomology",
    "synthesize_coverage",
    "synthesize_geometric_coverage",
    "synthesize_cohomology",
    "verify_intervention",
    "verify_program_trace",
]


def compile_collapse_portfolio(n, triplets, max_dim=1, threshold=None,
                               threads=4, score="columns",
                               adaptive_objective="h1",
                               adaptive_work_limit=None):
    """Select the exact minimum over three checked collapse schedules.

    The declared portfolio contains serial, rounds, and adaptive schedules.
    The result proves the minimum score within this finite set. It does not
    claim a globally minimum collapse sequence.
    """
    artifact, selected, entries = _core.compile_collapse_portfolio(
        int(n),
        [(int(u), int(v), float(value)) for u, v, value in triplets],
        int(max_dim), threshold, int(threads), str(score),
        str(adaptive_objective), adaptive_work_limit,
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
    artifact, bars, simplex_counts, column_counts = (
        _core.compile_explicit_persistence(
            [([int(vertex) for vertex in vertices], float(grade))
             for vertices, grade in simplices],
            int(max_dim), int(modulus),
        )
    )
    return {
        "artifact": bytes(artifact),
        "bars": bars,
        "simplex_counts": simplex_counts,
        "column_counts": column_counts,
    }


def compile_relative_interface(n, triplets, protected=(), max_dim=1,
                               threshold=None, modulus=2):
    """Compile a proof-carrying chain core relative to protected vertices.

    The protected vertices induce the separator subcomplex. The returned
    artifact is accepted by ``holos-check``.

    Returns:
        A dictionary with ``artifact``, ``bars``, ``input_cells``,
        ``cancellations``, and ``core_cells``.
    """
    artifact, bars, work = _core.compile_relative_interface(
        int(n),
        [(int(i), int(j), float(d)) for i, j, d in triplets],
        [int(vertex) for vertex in protected],
        int(max_dim), threshold, int(modulus),
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
    The basis works in any bounded dimension, including H0.
    """
    identifier, simplex_counts, basis = _core.fixed_cohomology(
        int(n),
        [(int(u), int(v), float(value)) for u, v, value in triplets],
        int(dimension), float(scale), int(modulus),
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


def cohomology_relation(n, old_triplets, new_triplets, dimension, scale,
                        modulus=2):
    """Relate two cohomology spaces on their common active subcomplex."""
    values = _core.relate_fixed_cohomology(
        int(n),
        [(int(u), int(v), float(value)) for u, v, value in old_triplets],
        [(int(u), int(v), float(value)) for u, v, value in new_triplets],
        int(dimension), float(scale), int(modulus),
    )
    return {
        "old_rank": values[0],
        "new_rank": values[1],
        "old_image_rank": values[2],
        "new_image_rank": values[3],
        "relation_rank": values[4],
        "isomorphism": values[5],
        "basis": [{"old": old, "new": new} for old, new in values[6]],
    }


def affine_events(n, edges, start, end, threshold=None, dimension=None,
                  modulus=2):
    """Certify affine edge-order events and optional class relations.

    Each edge is ``(u, v, intercept, velocity)``. Input floats are exact
    dyadic coefficients. Event bounds enclose the exact rational root.
    """
    events, relations, persistent_ties = _core.affine_events(
        int(n),
        [(int(u), int(v), float(a), float(b)) for u, v, a, b in edges],
        float(start), float(end),
        None if threshold is None else float(threshold),
        None if dimension is None else int(dimension), int(modulus),
    )
    return {
        "events": [
            {"time": time, "lower": lower, "upper": upper, "kinds": kinds}
            for time, lower, upper, kinds in events
        ],
        "cohomology": [
            {
                "time": time,
                "before_rank": before,
                "after_rank": after,
                "relation_rank": relation,
            }
            for time, before, after, relation in relations
        ],
        "persistent_ties": persistent_ties,
    }


def kinetic_zigzag(n, edges, start, end, dimension, scale, modulus=2):
    """Decompose exact fixed-scale class dynamics into zigzag intervals.

    Open time cells alternate with exact event complexes. Interval records
    describe isotypic class spaces. A multiplicity greater than one does not
    assign identities to the repeated copies.
    """
    (artifact, module, persistent_ties, nodes, arrows, intervals,
     generalized_ranks) = _core.kinetic_zigzag(
        int(n),
        [(int(u), int(v), float(a), float(b)) for u, v, a, b in edges],
        float(start), float(end), int(dimension), float(scale), int(modulus),
    )
    node_count = len(nodes)
    return {
        "artifact": bytes(artifact),
        "module": module,
        "persistent_ties": persistent_ties,
        "nodes": [
            {
                "kind": kind,
                "time": time,
                "rank": rank,
                "active_edges": active_edges,
                "space": space,
            }
            for kind, time, rank, active_edges, space in nodes
        ],
        "arrows": [
            {"direction": direction, "rank": rank}
            for direction, rank in arrows
        ],
        "intervals": [
            {
                "id": interval_id,
                "start": first,
                "end": last,
                "multiplicity": multiplicity,
            }
            for interval_id, first, last, multiplicity in intervals
        ],
        "generalized_ranks": [
            generalized_ranks[row * node_count:(row + 1) * node_count]
            for row in range(node_count)
        ],
    }


def intervene_cohomology(n, scenarios, candidates, dimension, scale,
                         max_edits, modulus=2, oracle_limit=1_000_000,
                         node_limit=1_000_000):
    """Certify one minimum-cost edit across declared graph scenarios.

    Each scenario is ``(triplets, target)``. Each candidate is
    ``(u, v, positive_cost)``. The target is a position in that scenario's
    canonical basis from ``cohomology_space``.
    """
    values = _core.intervene_fixed_cohomology(
        int(n),
        [([(int(u), int(v), float(value)) for u, v, value in triplets],
          int(target)) for triplets, target in scenarios],
        [(int(u), int(v), int(cost)) for u, v, cost in candidates],
        int(dimension), float(scale), int(max_edits), int(modulus),
        int(oracle_limit), int(node_limit),
    )
    return {
        "artifact": bytes(values[0]),
        "status": values[1],
        "edits": values[2],
        "lower_bound_cost": values[3],
        "upper_bound_cost": values[4],
        "oracle_calls": values[5],
        "search_nodes": values[6],
        "cache_hits": values[7],
        "root_blockers": values[8],
        "before_ranks": values[9],
        "after_ranks": values[10],
    }


def relative_coverage(n, triplets, active, fence, broadcast_radius,
                      sensing_radius, modulus=2):
    """Check one relative fence-filling coverage criterion.

    The result implies physical coverage only under the controlled-boundary
    domain, sensor-placement, fence, and communication assumptions.
    """
    holds, witness, active_edges, active_triangles = (
        _core.check_relative_coverage(
            int(n),
            [(int(u), int(v), float(value)) for u, v, value in triplets],
            sorted(int(vertex) for vertex in active),
            [int(vertex) for vertex in fence],
            float(broadcast_radius), float(sensing_radius), int(modulus),
        )
    )
    return {
        "criterion_holds": holds,
        "witness": witness,
        "active_edges": active_edges,
        "active_triangles": active_triangles,
        "physical_claim_is_conditional": True,
    }


def _coverage_candidates(candidates):
    output = []
    for candidate in candidates:
        if len(candidate) == 2:
            vertex, cost = candidate
            states = None
        elif len(candidate) == 3:
            vertex, cost, states = candidate
            states = None if states is None else [int(state) for state in states]
        else:
            raise ValueError(
                "each coverage candidate must be (vertex, cost) or "
                "(vertex, cost, states)"
            )
        output.append((int(vertex), int(cost), states))
    return output


def _coverage_result(values, geometry_checked=False):
    return {
        "artifact": bytes(values[0]),
        "status": values[1],
        "actions": values[2],
        "lower_bound_cost": values[3],
        "upper_bound_cost": values[4],
        "producer_oracle_calls": values[5],
        "producer_search_nodes": values[6],
        "proof_nodes": values[7],
        "proof_topology_checks": values[8],
        "selected_failure_checks": values[9],
        "minimum_witness_triangles": values[10],
        "states": values[11],
        "geometry_checked": geometry_checked,
        "physical_claim_is_conditional": not geometry_checked,
    }


def synthesize_coverage(n, states, fence, candidates, broadcast_radius,
                        sensing_radius, max_activations, base=(),
                        failable=(), failure_budget=0, modulus=2,
                        oracle_limit=2_000_000, node_limit=2_000_000):
    """Certify a minimum-cost plan across finite communication states.

    A candidate is ``(vertex, positive_cost)`` for all states or
    ``(vertex, positive_cost, state_indices)`` for explicit support. Fence
    sensors are active automatically. The physical conclusion is conditional
    on the controlled-boundary geometric assumptions.
    """
    values = _core.synthesize_finite_coverage(
        int(n),
        [[(int(u), int(v), float(value)) for u, v, value in state]
         for state in states],
        [int(vertex) for vertex in fence],
        [int(vertex) for vertex in base],
        [int(vertex) for vertex in failable],
        _coverage_candidates(candidates),
        float(broadcast_radius), float(sensing_radius),
        int(failure_budget), int(max_activations), int(modulus),
        int(oracle_limit), int(node_limit),
    )
    return _coverage_result(values)


def synthesize_geometric_coverage(
        n, states, coordinates, fence, candidates, broadcast_radius,
        sensing_radius, max_activations, base=(), failable=(),
        failure_budget=0, modulus=2, oracle_limit=2_000_000,
        node_limit=2_000_000):
    """Certify finite coverage and bind every state to planar coordinates.

    ``coordinates`` contains one ``(x, y)`` sequence per state. The checker
    proves that the fence is a simple polygon, every sensor lies inside it,
    and each state is the complete Euclidean broadcast-radius graph.
    """
    values = _core.synthesize_geometric_coverage(
        int(n),
        [[(int(u), int(v), float(value)) for u, v, value in state]
         for state in states],
        [[(float(x), float(y)) for x, y in state]
         for state in coordinates],
        [int(vertex) for vertex in fence],
        [int(vertex) for vertex in base],
        [int(vertex) for vertex in failable],
        _coverage_candidates(candidates),
        float(broadcast_radius), float(sensing_radius),
        int(failure_budget), int(max_activations), int(modulus),
        int(oracle_limit), int(node_limit),
    )
    return _coverage_result(values, geometry_checked=True)


def synthesize_affine_coverage(n, edges, start, end, fence, candidates,
                               broadcast_radius, sensing_radius,
                               max_activations, base=(), failable=(),
                               failure_budget=0, modulus=2,
                               oracle_limit=2_000_000,
                               node_limit=2_000_000):
    """Certify coverage over a complete affine communication schedule.

    Each edge is ``(u, v, intercept, velocity)``. Encoded weights are exact
    dyadic affine values. The function does not prove Euclidean realization.
    Physical coverage is conditional on the controlled-boundary assumptions.
    """
    values = _core.synthesize_affine_coverage(
        int(n),
        [(int(u), int(v), float(a), float(b)) for u, v, a, b in edges],
        float(start), float(end),
        [int(vertex) for vertex in fence],
        [int(vertex) for vertex in base],
        [int(vertex) for vertex in failable],
        _coverage_candidates(candidates),
        float(broadcast_radius), float(sensing_radius),
        int(failure_budget), int(max_activations), int(modulus),
        int(oracle_limit), int(node_limit),
    )
    return _coverage_result(values)


def _synthesis(values):
    return {
        "artifact": bytes(values[0]),
        "status": values[1],
        "actions": values[2],
        "lower_bound_cost": values[3],
        "upper_bound_cost": values[4],
        "producer_oracle_calls": values[5],
        "producer_search_nodes": values[6],
        "proof_nodes": values[7],
        "proof_topology_checks": values[8],
        "before_ranks": values[9],
        "after_ranks": values[10],
    }


def synthesize_cohomology(n, states, candidates, dimension, scale,
                          max_rank, max_edits, modulus=2,
                          oracle_limit=2_000_000, node_limit=2_000_000):
    """Synthesize one minimum-cost plan across finite graph states.

    Each state is a sparse triplet list. The target is its complete canonical
    cohomology space. States whose rank is already at most ``max_rank`` are
    omitted. Each candidate is ``(u, v, positive_cost)`` and applies to every
    retained state.
    """
    values = _core.synthesize_fixed_cohomology(
        int(n),
        [[(int(u), int(v), float(value)) for u, v, value in state]
         for state in states],
        [(int(u), int(v), int(cost)) for u, v, cost in candidates],
        int(dimension), float(scale), int(max_rank), int(max_edits),
        int(modulus), int(oracle_limit), int(node_limit),
    )
    return _synthesis(values)


def synthesize_affine_cohomology(n, edges, start, end, candidates,
                                 dimension, scale, max_rank, max_edits,
                                 modulus=2, oracle_limit=2_000_000,
                                 node_limit=2_000_000):
    """Synthesize a plan over a complete affine critical-state schedule.

    Each trajectory edge is ``(u, v, intercept, velocity)``. The result
    certifies a Rips cohomology rank condition at both endpoints, every exact
    threshold event, and every open threshold cell. It does not by itself
    certify physical sensor coverage.
    """
    values = _core.synthesize_affine_cohomology(
        int(n),
        [(int(u), int(v), float(a), float(b)) for u, v, a, b in edges],
        float(start), float(end),
        [(int(u), int(v), int(cost)) for u, v, cost in candidates],
        int(dimension), float(scale), int(max_rank), int(max_edits),
        int(modulus), int(oracle_limit), int(node_limit),
    )
    return _synthesis(values)


def rips_points(points, max_dim=1, threshold=None, modulus=2, threads=1,
                factorization="off",
                collapse_edges=False, collapse_schedule="serial",
                collapse_objective="h2", collapse_work_limit=None):
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

    Returns:
        List of ``(dim, birth, death)`` tuples.
    """
    return _core.rips_points([list(map(float, p)) for p in points],
                             max_dim, threshold, modulus, threads,
                             factorization,
                             collapse_edges, collapse_schedule,
                             collapse_objective, collapse_work_limit)


def rips_condensed(data, max_dim=1, threshold=None, modulus=2, threads=1,
                   factorization="off",
                   collapse_edges=False, collapse_schedule="serial",
                   collapse_objective="h2", collapse_work_limit=None):
    """Compute Rips persistence of a condensed distance matrix.

    The layout is upper-triangular and row-major, the same as
    ``scipy.spatial.distance.pdist``.

    Args:
        data: flat sequence of the ``n(n-1)/2`` pairwise distances.
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

    Returns:
        List of ``(dim, birth, death)`` tuples.
    """
    return _core.rips_condensed(list(map(float, data)),
                                max_dim, threshold, modulus, threads,
                                factorization,
                                collapse_edges, collapse_schedule,
                                collapse_objective, collapse_work_limit)


def rips_sparse(n, triplets, max_dim=1, threshold=None, modulus=2, threads=1,
                factorization="off",
                collapse_edges=False, collapse_schedule="serial",
                collapse_objective="h2", collapse_work_limit=None):
    """Compute Rips persistence of a sparse distance matrix.

    Pairs not listed are absent at every scale. With ``threshold=None``, all
    listed edges enter the filtration.

    Args:
        n: number of points.
        triplets: iterable of ``(i, j, distance)`` entries.
        max_dim: highest homology dimension to compute.
        threshold: truncate the filtration at this scale.
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

    Returns:
        List of ``(dim, birth, death)`` tuples.
    """
    return _core.rips_sparse(n, [(int(i), int(j), float(d)) for i, j, d in triplets],
                             max_dim, threshold, modulus, threads,
                             factorization,
                             collapse_edges, collapse_schedule,
                             collapse_objective, collapse_work_limit)


def _class_result(raw):
    """Convert the extension's compact records to named dictionaries."""
    bars, records = raw
    classes = []
    for group_id, class_id, basis_index, birth, death, modulus, scale, terms in records:
        classes.append({
            "group_id": group_id,
            "id": class_id,
            "basis_index": basis_index,
            "birth": birth,
            "death": death,
            "essential": death is None,
            "modulus": modulus,
            "scale": scale,
            "terms": terms,
        })
    return bars, classes


def _class_record(record):
    """Expand one compact extension class record."""
    group_id, class_id, basis_index, birth, death, modulus, scale, terms = record
    return {
        "group_id": group_id,
        "id": class_id,
        "basis_index": basis_index,
        "birth": birth,
        "death": death,
        "essential": death is None,
        "modulus": modulus,
        "scale": scale,
        "terms": terms,
    }


def _atlas_result(raw):
    """Expand one compact atlas evaluation."""
    bars, raw_spaces, raw_sensitivities = raw
    spaces = []
    for lineage, group_id, birth, death, basis, critical in raw_spaces:
        spaces.append({
            "lineage": lineage,
            "group_id": group_id,
            "birth": birth,
            "death": death,
            "essential": death is None,
            "multiplicity": len(basis),
            "basis": [_class_record(record) for record in basis],
            "critical_pairs": [
                {
                    "birth": {"vertices": pair[0], "value": pair[1]},
                    "death": None if pair[2] is None else {
                        "vertices": pair[2][0], "value": pair[2][1]
                    },
                }
                for pair in critical
            ],
        })
    sensitivities = []
    for lineage, birth, death in raw_sensitivities:
        sensitivities.append({
            "lineage": lineage,
            "birth": {"kind": birth[0], "edges": birth[1]},
            "death": {"kind": death[0], "edges": death[1]},
        })
    return {
        "bars": bars,
        "spaces": spaces,
        "sensitivities": sensitivities,
    }


def _event_record(record):
    """Expand one compact validity-region event."""
    kind, first, second, old_first, new_first, old_second, new_second = record
    return {
        "kind": kind,
        "first": first,
        "second": second,
        "old_first": old_first,
        "new_first": new_first,
        "old_second": old_second,
        "new_second": new_second,
    }


def _program_result(raw):
    """Expand a compositional program result."""
    bars, raw_spaces = raw
    spaces = []
    for group_id, birth, death, basis, critical in raw_spaces:
        spaces.append({
            "group_id": group_id,
            "birth": birth,
            "death": death,
            "essential": death is None,
            "multiplicity": len(basis),
            "basis": [_class_record(record) for record in basis],
            "critical_pairs": [
                {
                    "birth": {"vertices": pair[0], "value": pair[1]},
                    "death": None if pair[2] is None else {
                        "vertices": pair[2][0], "value": pair[2][1]
                    },
                }
                for pair in critical
            ],
        })
    return {"bars": bars, "spaces": spaces}


def _program_work(raw):
    names = (
        "edges_checked",
        "h0_edges_scanned",
        "guards_checked",
        "atoms_touched",
        "atoms_reused",
        "atoms_repaired",
        "atoms_rebuilt",
        "reduction_columns_reused",
        "reduction_columns_reduced",
        "reduction_column_additions",
    )
    return dict(zip(names, raw))


def _program_event(raw):
    kind, atom, edge, guard = raw
    return {"kind": kind, "atom": atom, "edge": edge, "guard": guard}


def _continuation(raw):
    kind, old_spaces, new_spaces, transport = raw
    return {
        "kind": kind,
        "old_spaces": old_spaces,
        "new_spaces": new_spaces,
        "transport": [
            {"old": old, "new": new, "coefficient": coefficient}
            for old, new, coefficient in transport
        ],
    }


def _correspondence(raw):
    """Expand one exact common-subcomplex class relation."""
    (old_space, new_space, scale, old_rank, new_rank, old_image_rank,
     new_image_rank, relation_rank, basis) = raw
    return {
        "old_space": old_space,
        "new_space": new_space,
        "scale": scale,
        "old_rank": old_rank,
        "new_rank": new_rank,
        "old_image_rank": old_image_rank,
        "new_image_rank": new_image_rank,
        "relation_rank": relation_rank,
        "is_isomorphism": (
            relation_rank == old_rank == new_rank
            and old_image_rank == old_rank
            and new_image_rank == new_rank
        ),
        "basis": [
            {
                "old": [
                    {"basis": item[0], "coefficient": item[1]}
                    for item in vector[0]
                ],
                "new": [
                    {"basis": item[0], "coefficient": item[1]}
                    for item in vector[1]
                ],
            }
            for vector in basis
        ],
    }


def _program_update(raw):
    """Expand one checked program update."""
    mode, events, continuation, correspondence, work, result = raw
    return {
        "mode": mode,
        "events": [_program_event(record) for record in events],
        "continuation": [_continuation(record) for record in continuation],
        "correspondence": [
            _correspondence(record) for record in correspondence
        ],
        "work": _program_work(work),
        "result": _program_result(result),
    }


def _index_work(raw):
    names = (
        "edges_checked",
        "nodes_touched",
        "nodes_shared",
        "nodes_repaired",
        "nodes_rebuilt",
        "nodes_composed",
        "relative_nodes_rebuilt",
        "relative_nodes_composed",
        "relative_input_cells",
        "relative_core_cells",
        "relative_cancellations",
        "reduction_columns_reused",
        "reduction_columns_reduced",
        "reduction_column_additions",
    )
    raw = list(raw)
    relative = list(raw.pop(6))
    return dict(zip(names, raw[:6] + relative + raw[6:]))


def _index_event(raw):
    kind, node, edge = raw
    return {"kind": kind, "node": node, "edge": edge}


def _index_update(raw):
    (mode, bars, removed, added, events, correspondence, work, version,
     delta_proof, snapshot_proof) = raw
    return {
        "mode": mode,
        "bars": bars,
        "diagram_delta": {"removed": removed, "added": added},
        "events": [_index_event(record) for record in events],
        "correspondence": [
            _correspondence(record) for record in correspondence
        ],
        "work": _index_work(work),
        "version": version,
        "delta_proof": (
            None if delta_proof is None else bytes(delta_proof)
        ),
        "snapshot_proof": (
            None if snapshot_proof is None else bytes(snapshot_proof)
        ),
    }


def _intervention(raw):
    status, target, lower, upper, edits, result, artifact = raw
    return {
        "status": status,
        "target": target,
        "lower_bound": lower,
        "upper_bound": upper,
        "edits": [
            {"edge": edge, "before": before, "after": after}
            for edge, before, after in edits
        ],
        "result": None if result is None else _program_result(result),
        "artifact": None if artifact is None else bytes(artifact),
    }


def _triplets(triplets):
    return [(int(i), int(j), float(distance)) for i, j, distance in triplets]


class SparseAtlas:
    """A proof-carrying local model for a sparse weighted graph.

    A compiled region fixes the vertices, listed edges, threshold membership,
    and weak edge-weight order. ``evaluate`` runs no persistence reduction
    while that contract holds. ``update`` recompiles exactly after an event.
    """

    def __init__(self, inner):
        self._inner = inner

    @property
    def artifact(self):
        """Return canonical ``HOLOSATL`` bytes for the compiled region."""
        return self._inner.artifact

    def result(self):
        """Return the result for the most recent graph."""
        return _atlas_result(self._inner.result())

    def evaluate(self, n, triplets):
        """Evaluate weights in the current region without reduction."""
        return _atlas_result(self._inner.evaluate(int(n), _triplets(triplets)))

    def events(self, n, triplets):
        """Return changes that prevent reuse at the supplied graph."""
        return [
            _event_record(record)
            for record in self._inner.events(int(n), _triplets(triplets))
        ]

    def update(self, n, triplets):
        """Reuse the region or compile a new proof after an event."""
        mode, events, result = self._inner.update(int(n), _triplets(triplets))
        return {
            "mode": mode,
            "events": [_event_record(record) for record in events],
            "result": _atlas_result(result),
        }


class PointAtlas:
    """A Euclidean point atlas with a conservative displacement radius."""

    def __init__(self, inner):
        self._inner = inner

    @property
    def coordinate_radius(self):
        """Return the certified per-point Euclidean displacement radius."""
        return self._inner.coordinate_radius

    def result(self):
        """Return the result for the most recent point cloud."""
        return _atlas_result(self._inner.result())

    def sensitivities(self):
        """Return analytic endpoint derivatives by point coordinate."""
        records = []
        for lineage, birth, death in self._inner.sensitivities():
            def expand(gradient):
                if gradient is None:
                    return None
                edge, terms = gradient
                return {"edge": edge, "terms": terms}
            records.append({
                "lineage": lineage,
                "birth": expand(birth),
                "death": expand(death),
            })
        return records

    def evaluate(self, points):
        """Evaluate points inside the certified displacement radius."""
        rows = [list(map(float, point)) for point in points]
        return _atlas_result(self._inner.evaluate(rows))

    def update(self, points):
        """Reuse the point atlas or recompile after a radius event."""
        rows = [list(map(float, point)) for point in points]
        mode, result = self._inner.update(rows)
        return {"mode": mode, "result": _atlas_result(result)}


class SparseIndex:
    """Versioned exact persistence for a sparse graph.

    The listed edges form a fixed envelope. Updates inside that envelope
    path-copy changed relative cores and share the rest. An envelope change
    compiles a new root and returns a new cold checkpoint.
    """

    def __init__(self, inner):
        self._inner = inner

    @property
    def snapshot(self):
        """Return canonical ``HOLOSIP`` bytes for the current version."""
        return bytes(self._inner.snapshot)

    @property
    def version(self):
        """Return the content identifier of the current root."""
        return self._inner.version

    @property
    def max_dim(self):
        """Return the highest maintained homology dimension."""
        return self._inner.max_dim

    def result(self):
        """Return the exact current persistence diagram."""
        return self._inner.result()

    def explain(self):
        """Compute canonical H1 class spaces for the current graph."""
        return _program_result(self._inner.explain())

    def summary(self):
        """Return structural and algebraic sizes of the interface tree."""
        names = (
            "nodes",
            "leaves",
            "separators",
            "component_splits",
            "widest_separator",
            "largest_interface_vertices",
            "largest_interface_edges",
            "composed_interfaces",
            "materialized_interfaces",
            "relative_interfaces",
            "relative_input_cells",
            "relative_core_cells",
            "largest_relative_core_cells",
            "relative_cancellations",
            "root_composed",
            "separator_candidates_checked",
            "separator_search_complete",
        )
        structural, relative, control = self._inner.summary()
        summary = dict(zip(names, tuple(structural) + tuple(relative) + tuple(control)))
        summary["max_dim"] = self._inner.max_dim
        return summary

    def interfaces(self):
        """Return all index interfaces in deterministic preorder."""
        records = []
        for (digest, depth, vertices, separator, protected_vertices, edges, children,
             mode, reduction_columns, columns_by_dimension,
             relative_size) in self._inner.interfaces():
            relative_input_cells, relative_core_cells, relative_cancellations = relative_size
            records.append({
                "digest": digest,
                "depth": depth,
                "vertices": vertices,
                "separator": separator,
                "protected_vertices": protected_vertices,
                "edges": edges,
                "children": children,
                "mode": mode,
                "reduction_columns": reduction_columns,
                "columns_by_dimension": columns_by_dimension,
                "relative_input_cells": relative_input_cells,
                "relative_core_cells": relative_core_cells,
                "relative_cancellations": relative_cancellations,
            })
        return records

    def update(self, n, triplets, correspondence=True):
        """Install an exact next version and return its proof and work."""
        return _index_update(self._inner.update(
            int(n), _triplets(triplets), bool(correspondence)))

    def update_many(self, n, updates, correspondence=True):
        """Apply an ordered update batch atomically."""
        raw_updates = [_triplets(update) for update in updates]
        return [
            _index_update(raw)
            for raw in self._inner.update_many(
                int(n), raw_updates, bool(correspondence))
        ]

    def patch(self, edits, correspondence=True):
        """Apply one atomic active-topology patch and return its proof."""
        records = []
        for edit in edits:
            kind = str(edit[0])
            if kind == "deactivate":
                if len(edit) != 3:
                    raise ValueError("deactivate requires kind, u, and v")
                records.append((kind, int(edit[1]), int(edit[2]), None))
            else:
                if len(edit) != 4:
                    raise ValueError(
                        f"{kind} requires kind, u, v, and value")
                records.append(
                    (kind, int(edit[1]), int(edit[2]), float(edit[3])))
        return _index_update(
            self._inner.patch(records, bool(correspondence)))

    def fork(self, n, alternatives):
        """Advance alternatives without changing the current version."""
        raw = [_triplets(alternative) for alternative in alternatives]
        return [
            {"update": _index_update(update), "index": SparseIndex(inner)}
            for update, inner in self._inner.fork(int(n), raw)
        ]

    def diff(self, other):
        """Compare structural sharing and diagrams with another version."""
        same_envelope, shared_nodes, removed, added = self._inner.diff(
            other._inner)
        return {
            "same_envelope": same_envelope,
            "shared_nodes": shared_nodes,
            "diagram_delta": {"removed": removed, "added": added},
        }


class SparseProgram:
    """Checked H0 and H1 persistence over sparse graph atoms.

    ``evaluate`` accepts only changes covered by every touched reduction
    region. ``update`` repairs failed atoms locally and recompiles after a
    topology or threshold-membership change.
    """

    def __init__(self, inner):
        self._inner = inner

    @property
    def artifact(self):
        """Return canonical ``HOLOSPRG`` bytes for the current program."""
        return self._inner.artifact

    @property
    def proof(self):
        """Return canonical ``HOLOSPF`` bytes for the current state."""
        return self._inner.proof

    def result(self):
        """Return the exact current diagram and H1 class spaces."""
        return _program_result(self._inner.result())

    def summary(self):
        """Return structural sizes and the checked guard count."""
        names = (
            "atoms",
            "cyclic_atoms",
            "articulation_vertices",
            "zero_simplex_separators",
            "widest_separator",
            "separator_candidates_checked",
            "separator_search_complete",
            "largest_cyclic_atom_edges",
            "guards",
        )
        return dict(zip(names, self._inner.summary()))

    def atoms(self):
        """Return all bridge and cyclic atoms in stable order."""
        records = []
        for atom_id, vertices, edges, separators, cyclic in self._inner.atoms():
            records.append({
                "id": atom_id,
                "vertices": vertices,
                "edges": edges,
                "separator_vertices": separators,
                "cyclic": cyclic,
            })
        return records

    def evaluate(self, n, triplets):
        """Evaluate weights covered by every touched checked region."""
        bars, work = self._inner.evaluate(int(n), _triplets(triplets))
        return {"bars": bars, "work": _program_work(work)}

    def update(self, n, triplets, correspondence=True):
        """Reuse, repair, or recompile the program for a new graph.

        Set ``correspondence=False`` to maintain the exact current state
        without computing relations to the preceding class spaces.
        """
        raw = self._inner.update(
            int(n), _triplets(triplets), bool(correspondence))
        return _program_update(raw)

    def update_many(self, n, updates, correspondence=True):
        """Apply an ordered update batch atomically.

        If one update fails, the current program does not change.
        Set ``correspondence=False`` for state-only updates.
        """
        raw_updates = [_triplets(update) for update in updates]
        return [
            _program_update(raw)
            for raw in self._inner.update_many(
                int(n), raw_updates, bool(correspondence))
        ]

    def fork(self, n, alternatives):
        """Advance independent alternatives from the current state.

        The current program does not change. Each returned branch can receive
        later updates independently.
        """
        raw = [_triplets(alternative) for alternative in alternatives]
        return [
            {"update": _program_update(update), "program": SparseProgram(inner)}
            for update, inner in self._inner.fork(int(n), raw)
        ]

    def intervene(self, space, before, budget=1):
        """Certify a restricted intervention on one finite H1 space.

        ``space`` is the zero-based index in ``result()["spaces"]``.
        ``optimal`` is relative to the current reduction and its declared
        destroyer triangles. ``bounded_gap`` reports a feasible edit with a
        distinct checked lower bound.
        """
        return _intervention(
            self._inner.intervene(int(space), float(before), int(budget)))


def compile_sparse_atlas(n, triplets, threshold=None, modulus=2, threads=1,
                         factorization="off", collapse_edges=False,
                         collapse_schedule="serial", collapse_objective="h2",
                         collapse_work_limit=None):
    """Compile a proof-carrying H0 and H1 atlas for a sparse graph."""
    inner = _core.compile_sparse_atlas(
        int(n), _triplets(triplets), threshold, modulus, threads,
        factorization, collapse_edges, collapse_schedule, collapse_objective,
        collapse_work_limit)
    return SparseAtlas(inner)


def compile_sparse_index(n, triplets, max_dim=1, threshold=None, modulus=2,
                         threads=1,
                         separator_width=4,
                         separator_search_limit=100_000,
                         leaf_vertices=4, interface_policy="relative"):
    """Compile a versioned exact sparse persistence index."""
    inner = _core.compile_sparse_index(
        int(n), _triplets(triplets), int(max_dim), threshold, modulus, threads,
        int(separator_width), int(separator_search_limit), int(leaf_vertices),
        str(interface_policy))
    return SparseIndex(inner)


def load_sparse_atlas(n, triplets, artifact):
    """Check ``HOLOSATL`` bytes against a graph and load its local model."""
    inner = _core.load_sparse_atlas(
        int(n), _triplets(triplets), bytes(artifact))
    return SparseAtlas(inner)


def compile_points_atlas(points, threshold, modulus=2, threads=1,
                         factorization="off", collapse_edges=False,
                         collapse_schedule="serial", collapse_objective="h2",
                         collapse_work_limit=None):
    """Compile a finite-threshold point atlas and coordinate sensitivities."""
    rows = [list(map(float, point)) for point in points]
    inner = _core.compile_points_atlas(
        rows, float(threshold), modulus, threads, factorization,
        collapse_edges, collapse_schedule, collapse_objective,
        collapse_work_limit)
    return PointAtlas(inner)


def compile_sparse_program(n, triplets, threshold=None, modulus=2, threads=1):
    """Compile a checked compositional H0 and H1 persistence program."""
    inner = _core.compile_sparse_program(
        int(n), _triplets(triplets), threshold, modulus, threads)
    return SparseProgram(inner)


def load_sparse_program(n, triplets, artifact):
    """Check ``HOLOSPRG`` bytes against a graph and load its program."""
    inner = _core.load_sparse_program(
        int(n), _triplets(triplets), bytes(artifact))
    return SparseProgram(inner)


def compile_sparse_program_trace(n, initial, updates, threshold=None,
                                 modulus=2, threads=1):
    """Build canonical ``HOLOSDLT`` bytes for a graph trajectory."""
    raw_updates = [_triplets(update) for update in updates]
    return bytes(_core.compile_sparse_program_trace(
        int(n), _triplets(initial), raw_updates, threshold, modulus, threads))


def compile_sparse_proof(n, initial, updates, threshold=None,
                         modulus=2, threads=1):
    """Build one ``HOLOSPF`` proof DAG for a graph trajectory."""
    raw_updates = [_triplets(update) for update in updates]
    return bytes(_core.compile_sparse_proof(
        int(n), _triplets(initial), raw_updates, threshold, modulus, threads))


def verify_program_trace(artifact):
    """Independently check ``HOLOSDLT`` bytes and return step counts."""
    steps, reused, repaired, recompiled = _core.verify_program_trace(
        bytes(artifact))
    return {
        "steps": steps,
        "reused": reused,
        "repaired": repaired,
        "recompiled": recompiled,
    }


def verify_intervention(artifact):
    """Independently check ``HOLOSINT`` bytes and return its claim."""
    return _intervention(_core.verify_intervention(bytes(artifact)))


def rips_points_classes(points, max_dim=1, threshold=None, modulus=2,
                        threads=1, factorization="off", collapse_edges=False,
                        collapse_schedule="serial", collapse_objective="h2",
                        collapse_work_limit=None):
    """Compute a diagram and one canonical H1 basis cocycle per positive bar.

    The result is ``(bars, classes)``. Each class is a dictionary with its
    class-space identifier, basis identifier, interval, field, representative
    scale, and ``(u, v, c)`` terms. Equal intervals share one ``group_id``.
    A finite class is represented immediately below its death.
    """
    raw = _core.rips_points_classes(
        [list(map(float, point)) for point in points], max_dim, threshold,
        modulus, threads, factorization, collapse_edges, collapse_schedule,
        collapse_objective, collapse_work_limit)
    return _class_result(raw)


def rips_condensed_classes(data, max_dim=1, threshold=None, modulus=2,
                           threads=1, factorization="off", collapse_edges=False,
                           collapse_schedule="serial", collapse_objective="h2",
                           collapse_work_limit=None):
    """Compute a diagram and stable H1 cocycles from condensed distances."""
    raw = _core.rips_condensed_classes(
        list(map(float, data)), max_dim, threshold, modulus, threads,
        factorization, collapse_edges, collapse_schedule, collapse_objective,
        collapse_work_limit)
    return _class_result(raw)


def rips_sparse_classes(n, triplets, max_dim=1, threshold=None, modulus=2,
                        threads=1, factorization="off", collapse_edges=False,
                        collapse_schedule="serial", collapse_objective="h2",
                        collapse_work_limit=None):
    """Compute a diagram and stable H1 cocycles from sparse distances."""
    raw = _core.rips_sparse_classes(
        n, [(int(i), int(j), float(distance)) for i, j, distance in triplets],
        max_dim, threshold, modulus, threads, factorization, collapse_edges,
        collapse_schedule, collapse_objective, collapse_work_limit)
    return _class_result(raw)


def main(argv=None):
    """Entry point for the ``holos-tda`` console script."""
    args = list(sys.argv[1:]) if argv is None else list(argv)
    raise SystemExit(_core.run_cli(["holos"] + args))
