"""Smoke tests for cohomology, kinetic events, and intervention plans."""

import holos_tda

from smoke_utils import SQRT2, coverage_graph as make_coverage_graph
from smoke_utils import octahedron_boundary


def run():
    # The octahedron boundary is an H2 sphere. Adding one antipodal edge fills it.
    octahedron = octahedron_boundary()
    _check_fixed_cohomology(octahedron)
    affine = [
        (u, v, value, 0.0) for u, v, value in octahedron
    ] + [(0, 1, 3.0, -2.0)]
    _check_affine_events(affine)
    _check_kinetic_zigzag(affine)
    _check_intervention(octahedron)

    first_cycle, second_cycle = _cycles()
    _check_link_plan(first_cycle, second_cycle)
    _check_cohomology_synthesis(first_cycle)
    _check_kinetic_synthesis()

    coverage = make_coverage_graph()
    _check_coverage(coverage)
    _check_geometric_coverage()
    _check_affine_coverage(coverage)


def _check_fixed_cohomology(octahedron):
    cohomology = holos_tda.cohomology_space(
        6, octahedron, dimension=2, scale=2.0, modulus=5)
    assert cohomology["rank"] == 1
    assert len(cohomology["basis"][0]["terms"]) > 0
    identity = holos_tda.cohomology_relation(
        6, octahedron, octahedron, dimension=2, scale=2.0, modulus=5)
    assert identity["isomorphism"] and identity["relation_rank"] == 1


def _check_affine_events(affine):
    events = holos_tda.affine_events(
        6, affine, 0.0, 1.0, threshold=2.0, dimension=2, modulus=5)
    assert any(event["before_rank"] == 1 and event["after_rank"] == 0
               for event in events["cohomology"])


def _check_kinetic_zigzag(affine):
    zigzag = holos_tda.kinetic_zigzag(
        6, affine, 0.0, 1.0, dimension=2, scale=2.0, modulus=5)
    _check_zigzag_ranks(zigzag)
    _check_zigzag_arrows(zigzag)
    _check_zigzag_intervals(zigzag)


def _check_zigzag_ranks(zigzag):
    assert zigzag["artifact"].startswith(b"HOLOSZZ\0")
    assert zigzag["nodes"][0]["rank"] == 1
    assert all(node["rank"] == 0 for node in zigzag["nodes"][1:])
    assert all(arrow["rank"] == 0 for arrow in zigzag["arrows"])


def _check_zigzag_arrows(zigzag):
    assert [arrow["direction"] for arrow in zigzag["arrows"]] == [
        "backward" if position % 2 == 0 else "forward"
        for position in range(len(zigzag["arrows"]))
    ]


def _check_zigzag_intervals(zigzag):
    assert any(interval["start"] == interval["end"] == 0
               for interval in zigzag["intervals"])
    assert len(zigzag["generalized_ranks"]) == len(zigzag["nodes"])


def _check_intervention(octahedron):
    intervention = holos_tda.intervene_cohomology(
        6, [(octahedron, 0)], [(0, 1, 1)], dimension=2, scale=2.0,
        max_edits=1, modulus=5)
    assert intervention["status"] == "optimal"
    assert intervention["edits"] == [(0, 1, 1)]
    assert intervention["lower_bound_cost"] == intervention["upper_bound_cost"] == 1
    assert intervention["before_ranks"] == [1]
    assert intervention["after_ranks"] == [0]
    assert intervention["artifact"].startswith(b"HOLOSCI\0")


def _cycles():
    # One weighted link set satisfies named H1 requirements in different graphs.
    first_cycle = [
        (0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0),
    ]
    second_cycle = [
        (0, 1, 1.0), (1, 4, 1.0), (4, 5, 1.0), (0, 5, 1.0),
    ]
    return first_cycle, second_cycle


def _check_link_plan(first_cycle, second_cycle):
    link_plan = holos_tda.intervene_cohomology(
        6, [(first_cycle, 0), (second_cycle, 0)],
        [(0, 2, 4), (0, 4, 7)], dimension=1, scale=1.0,
        max_edits=2, modulus=3,
    )
    assert link_plan["status"] == "optimal"
    assert link_plan["edits"] == [(0, 2, 4), (0, 4, 7)]
    assert link_plan["lower_bound_cost"] == link_plan["upper_bound_cost"] == 11
    assert link_plan["before_ranks"] == [1, 1]
    assert link_plan["after_ranks"] == [0, 0]


def _check_cohomology_synthesis(first_cycle):
    # A basis-independent full-space target is synthesized across finite states.
    two_cycles = first_cycle + [
        (4, 5, 1.0), (5, 6, 1.0), (6, 7, 1.0), (4, 7, 1.0),
    ]
    synthesis = holos_tda.synthesize_cohomology(
        8, [two_cycles, two_cycles], [(0, 2, 4), (4, 6, 7)],
        dimension=1, scale=1.0, max_rank=0, max_edits=2, modulus=3,
    )
    assert synthesis["status"] == "optimal"
    assert synthesis["actions"] == [(0, 2, 4), (4, 6, 7)]
    assert synthesis["lower_bound_cost"] == synthesis["upper_bound_cost"] == 11
    assert synthesis["artifact"].startswith(b"HOLOSSYN")
    assert synthesis["proof_topology_checks"] < synthesis["producer_oracle_calls"]


def _check_kinetic_synthesis():
    kinetic_synthesis = holos_tda.synthesize_affine_cohomology(
        4,
        [(0, 1, 1.0, 0.0), (1, 2, 1.0, 0.0),
         (2, 3, 1.0, 0.0), (0, 3, 1.0, 0.0),
         (0, 2, 2.0, -1.0)],
        0.0, 1.5, [(1, 3, 2)], dimension=1, scale=1.0,
        max_rank=0, max_edits=1, modulus=3,
    )
    assert kinetic_synthesis["status"] == "optimal"
    assert kinetic_synthesis["upper_bound_cost"] == 2


def _check_coverage(coverage):
    # Relative coverage returns an explicit filling chain. Failure-tolerant
    # synthesis selects both redundant center sensors under one allowed failure.
    coverage_result = holos_tda.relative_coverage(
        6, coverage, [0, 1, 2, 3, 4], [0, 1, 2, 3], 1.0, 1.0,
        modulus=3,
    )
    assert coverage_result["criterion_holds"]
    assert len(coverage_result["witness"]) == 4
    assert coverage_result["physical_claim_is_conditional"]

    coverage_plan = holos_tda.synthesize_coverage(
        6, [coverage], [0, 1, 2, 3], [(4, 2), (5, 3)],
        broadcast_radius=1.0, sensing_radius=1.0, max_activations=2,
        failable=[4, 5], failure_budget=1, modulus=3,
    )
    assert coverage_plan["status"] == "optimal"
    assert coverage_plan["actions"] == [(4, 2, [0]), (5, 3, [0])]
    assert coverage_plan["lower_bound_cost"] == 5
    assert coverage_plan["upper_bound_cost"] == 5
    assert coverage_plan["artifact"].startswith(b"HOLOSCOV")
    assert coverage_plan["selected_failure_checks"] == 2


def _check_geometric_coverage():
    geometric_graph = [
        (0, 1, 2.0), (1, 2, 2.0), (2, 3, 2.0), (0, 3, 2.0),
        (0, 4, SQRT2), (1, 4, SQRT2),
        (2, 4, SQRT2), (3, 4, SQRT2),
    ]
    geometric_plan = holos_tda.synthesize_geometric_coverage(
        5, [geometric_graph],
        [[(0, 0), (2, 0), (2, 2), (0, 2), (1, 1)]],
        [0, 1, 2, 3], [(4, 1)], broadcast_radius=2.0,
        sensing_radius=2.0, max_activations=1,
    )
    assert geometric_plan["artifact"].startswith(b"HOLOSGEO")
    assert geometric_plan["geometry_checked"]
    assert not geometric_plan["physical_claim_is_conditional"]


def _check_affine_coverage(coverage):
    affine_coverage_plan = holos_tda.synthesize_affine_coverage(
        5,
        [(u, v, value, 0.0) for u, v, value in coverage if v < 5],
        0.0, 1.0, [0, 1, 2, 3], [(4, 1)],
        broadcast_radius=1.0, sensing_radius=1.0, max_activations=1,
    )
    assert affine_coverage_plan["status"] == "optimal"
    assert affine_coverage_plan["states"] == 3
    assert affine_coverage_plan["upper_bound_cost"] == 1
