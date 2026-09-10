from . import _core
from ._coerce import _triplets


def affine_events(n, edges, start, end, threshold=None, dimension=None, modulus=2):
    """Certify affine edge-order events and optional class relations.

    Each edge is ``(u, v, intercept, velocity)``. Input floats are exact
    dyadic coefficients. Event bounds enclose the exact rational root.
    """
    events, relations, persistent_ties = _core.affine_events(
        int(n),
        [(int(u), int(v), float(a), float(b)) for u, v, a, b in edges],
        float(start),
        float(end),
        None if threshold is None else float(threshold),
        None if dimension is None else int(dimension),
        int(modulus),
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
    (artifact, module, persistent_ties, nodes, arrows, intervals, generalized_ranks) = (
        _core.kinetic_zigzag(
            int(n),
            [(int(u), int(v), float(a), float(b)) for u, v, a, b in edges],
            float(start),
            float(end),
            int(dimension),
            float(scale),
            int(modulus),
        )
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
            {"direction": direction, "rank": rank} for direction, rank in arrows
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
            generalized_ranks[row * node_count : (row + 1) * node_count]
            for row in range(node_count)
        ],
    }


def intervene_cohomology(
    n,
    scenarios,
    candidates,
    dimension,
    scale,
    max_edits,
    modulus=2,
    oracle_limit=1_000_000,
    node_limit=1_000_000,
):
    """Certify one minimum-cost set of candidate edges across declared
    graph scenarios.

    Each scenario is ``(triplets, target)``. Each candidate is
    ``(u, v, positive_cost)``. The target is a position in that scenario's
    canonical basis from ``cohomology_space``.
    """
    values = _core.intervene_fixed_cohomology(
        int(n),
        [
            (_triplets(triplets), int(target))
            for triplets, target in scenarios
        ],
        [(int(u), int(v), int(cost)) for u, v, cost in candidates],
        int(dimension),
        float(scale),
        int(max_edits),
        int(modulus),
        int(oracle_limit),
        int(node_limit),
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
