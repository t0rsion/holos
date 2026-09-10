from . import _core
from ._coerce import _triplets


def relative_coverage(
    n, triplets, active, fence, broadcast_radius, sensing_radius, modulus=2
):
    """Check one relative fence-filling coverage criterion.

    The result implies physical coverage only under the controlled-boundary
    domain, placement, fence, and communication assumptions.
    """
    holds, witness, active_edges, active_triangles = _core.check_relative_coverage(
        int(n),
        _triplets(triplets),
        sorted(int(vertex) for vertex in active),
        [int(vertex) for vertex in fence],
        float(broadcast_radius),
        float(sensing_radius),
        int(modulus),
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


def synthesize_coverage(
    n,
    states,
    fence,
    candidates,
    broadcast_radius,
    sensing_radius,
    max_activations,
    base=(),
    failable=(),
    failure_budget=0,
    modulus=2,
    oracle_limit=2_000_000,
    node_limit=2_000_000,
):
    """Certify a minimum-cost plan across finite communication states.

    A candidate is ``(vertex, positive_cost)`` for all states or
    ``(vertex, positive_cost, state_indices)`` for explicit support. Fence
    sensors are active automatically. Physical coverage is conditional on
    the controlled-boundary domain, placement, fence, and communication
    assumptions.
    """
    values = _core.synthesize_finite_coverage(
        int(n),
        [_triplets(state) for state in states],
        [int(vertex) for vertex in fence],
        [int(vertex) for vertex in base],
        [int(vertex) for vertex in failable],
        _coverage_candidates(candidates),
        float(broadcast_radius),
        float(sensing_radius),
        int(failure_budget),
        int(max_activations),
        int(modulus),
        int(oracle_limit),
        int(node_limit),
    )
    return _coverage_result(values)


def synthesize_geometric_coverage(
    n,
    states,
    coordinates,
    fence,
    candidates,
    broadcast_radius,
    sensing_radius,
    max_activations,
    base=(),
    failable=(),
    failure_budget=0,
    modulus=2,
    oracle_limit=2_000_000,
    node_limit=2_000_000,
):
    """Certify finite coverage and bind every state to planar coordinates.

    ``coordinates`` contains one ``(x, y)`` sequence per state. The checker
    proves that the fence is a simple polygon, every sensor lies inside it,
    and each state is the complete Euclidean broadcast-radius graph.
    """
    values = _core.synthesize_geometric_coverage(
        int(n),
        [_triplets(state) for state in states],
        [[(float(x), float(y)) for x, y in state] for state in coordinates],
        [int(vertex) for vertex in fence],
        [int(vertex) for vertex in base],
        [int(vertex) for vertex in failable],
        _coverage_candidates(candidates),
        float(broadcast_radius),
        float(sensing_radius),
        int(failure_budget),
        int(max_activations),
        int(modulus),
        int(oracle_limit),
        int(node_limit),
    )
    return _coverage_result(values, geometry_checked=True)


def synthesize_affine_coverage(
    n,
    edges,
    start,
    end,
    fence,
    candidates,
    broadcast_radius,
    sensing_radius,
    max_activations,
    base=(),
    failable=(),
    failure_budget=0,
    modulus=2,
    oracle_limit=2_000_000,
    node_limit=2_000_000,
):
    """Certify coverage over a complete affine communication schedule.

    Each edge is ``(u, v, intercept, velocity)``. Encoded weights are exact
    dyadic affine values. The result does not prove Euclidean realization.
    Physical coverage is conditional on the controlled-boundary domain,
    placement, fence, and communication assumptions.
    """
    values = _core.synthesize_affine_coverage(
        int(n),
        [(int(u), int(v), float(a), float(b)) for u, v, a, b in edges],
        float(start),
        float(end),
        [int(vertex) for vertex in fence],
        [int(vertex) for vertex in base],
        [int(vertex) for vertex in failable],
        _coverage_candidates(candidates),
        float(broadcast_radius),
        float(sensing_radius),
        int(failure_budget),
        int(max_activations),
        int(modulus),
        int(oracle_limit),
        int(node_limit),
    )
    return _coverage_result(values)
