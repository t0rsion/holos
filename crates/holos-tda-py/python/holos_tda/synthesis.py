from . import _core
from ._coerce import _triplets


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


def synthesize_cohomology(
    n,
    states,
    candidates,
    dimension,
    scale,
    max_rank,
    max_edits,
    modulus=2,
    oracle_limit=2_000_000,
    node_limit=2_000_000,
):
    """Synthesize one minimum-cost plan across finite graph states.

    Each state is a sparse triplet list. The target is its complete canonical
    cohomology space. States whose rank is already at most ``max_rank`` are
    omitted. Each candidate is ``(u, v, positive_cost)`` and applies to every
    retained state.
    """
    values = _core.synthesize_fixed_cohomology(
        int(n),
        [_triplets(state) for state in states],
        [(int(u), int(v), int(cost)) for u, v, cost in candidates],
        int(dimension),
        float(scale),
        int(max_rank),
        int(max_edits),
        int(modulus),
        int(oracle_limit),
        int(node_limit),
    )
    return _synthesis(values)


def synthesize_affine_cohomology(
    n,
    edges,
    start,
    end,
    candidates,
    dimension,
    scale,
    max_rank,
    max_edits,
    modulus=2,
    oracle_limit=2_000_000,
    node_limit=2_000_000,
):
    """Synthesize a plan over a complete affine critical-state schedule.

    Each trajectory edge is ``(u, v, intercept, velocity)``. The result
    certifies a Rips cohomology rank condition at both endpoints, every exact
    threshold event, and every open threshold cell. It does not certify
    physical sensor coverage.
    """
    values = _core.synthesize_affine_cohomology(
        int(n),
        [(int(u), int(v), float(a), float(b)) for u, v, a, b in edges],
        float(start),
        float(end),
        [(int(u), int(v), int(cost)) for u, v, cost in candidates],
        int(dimension),
        float(scale),
        int(max_rank),
        int(max_edits),
        int(modulus),
        int(oracle_limit),
        int(node_limit),
    )
    return _synthesis(values)
