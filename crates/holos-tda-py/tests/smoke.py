"""Smoke tests for the built holos-tda wheel. Plain asserts, no test
framework. Run as `python py/tests/smoke.py` in an env with the wheel
installed."""

import math
import subprocess
import sys

import holos_tda

SQRT2 = math.sqrt(2.0)


def close(a, b, tol=1e-12):
    return a == b or abs(a - b) <= tol


# Unit square from points: 4 H0 bars (one essential), one H1 bar [1, sqrt 2).
bars = holos_tda.rips_points([[0, 0], [1, 0], [1, 1], [0, 1]], max_dim=1)
assert len([b for b in bars if b[0] == 0]) == 4
assert len([b for b in bars if b[0] == 0 and b[2] == math.inf]) == 1
(h1,) = [b for b in bars if b[0] == 1]
assert close(h1[1], 1.0) and close(h1[2], SQRT2)

# Condensed input follows SciPy pdist (upper-triangle) order. This n=4
# asymmetric matrix distinguishes pdist order from lower-triangle order:
# pdist [d01, d02, d03, d12, d13, d23] = [0.5, 0.5, 1, 10, 5, 6] gives H0
# finite deaths [0.5, 0.5, 1]. A lower-triangle misread gives [0.5, 0.5, 5].
bars = holos_tda.rips_condensed([0.5, 0.5, 1.0, 10.0, 5.0, 6.0], max_dim=0)
deaths = sorted(b[2] for b in bars if b[2] != math.inf)
assert deaths == [0.5, 0.5, 1.0], deaths

# pdist and points agree on the square.
pd = [1.0, SQRT2, 1.0, 1.0, SQRT2, 1.0]
assert holos_tda.rips_condensed(pd, max_dim=1) == holos_tda.rips_points(
    [[0, 0], [1, 0], [1, 1], [0, 1]], max_dim=1
)

# Sparse 4-cycle: the hole never fills (no diagonals listed).
bars = holos_tda.rips_sparse(4, [(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)])
(h1,) = [b for b in bars if b[0] == 1]
assert close(h1[1], 1.0) and h1[2] == math.inf

# threads=2 yields the identical diagram through every entry point.
sq = [[0, 0], [1, 0], [1, 1], [0, 1]]
assert holos_tda.rips_points(sq, max_dim=1, threads=2) == holos_tda.rips_points(
    sq, max_dim=1
)
assert holos_tda.rips_condensed(pd, max_dim=1, threads=2) == holos_tda.rips_condensed(
    pd, max_dim=1
)
cyc = [(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)]
assert holos_tda.rips_sparse(4, cyc, threads=2) == holos_tda.rips_sparse(4, cyc)
assert holos_tda.rips_sparse(4, cyc, factorization="force") == holos_tda.rips_sparse(4, cyc)

# Explain profile: equal diagram, one stable cocycle, and the same class after
# certified collapse and reverse lifting.
bars, classes = holos_tda.rips_points_classes(sq, max_dim=1, modulus=3)
assert bars == holos_tda.rips_points(sq, max_dim=1, modulus=3)
assert len(classes) == 1
assert classes[0]["birth"] == 1.0
assert close(classes[0]["death"], SQRT2)
assert classes[0]["modulus"] == 3
assert classes[0]["group_id"]
assert classes[0]["basis_index"] == 0
assert classes[0]["terms"]
collapsed_bars, collapsed_classes = holos_tda.rips_points_classes(
    sq, max_dim=1, modulus=3, threads=2, collapse_edges=True,
    collapse_schedule="rounds",
)
assert collapsed_bars == bars
assert collapsed_classes == classes

# A sparse atlas evaluates an order-preserving update without reduction,
# carries a portable proof, and recompiles exactly after an order event.
weighted = [
    (0, 1, 1.0), (0, 2, 2.0), (0, 3, 1.1),
    (1, 2, 1.2), (1, 3, 2.1), (2, 3, 1.3),
]
atlas = holos_tda.compile_sparse_atlas(4, weighted, modulus=3)
assert atlas.artifact.startswith(b"HOLOSATL")
assert atlas.result()["bars"] == holos_tda.rips_sparse(4, weighted, modulus=3)
loaded = holos_tda.load_sparse_atlas(4, weighted, atlas.artifact)
assert loaded.result() == atlas.result()
shifted = [(i, j, distance + 0.01) for i, j, distance in weighted]
update = atlas.update(4, shifted)
assert update["mode"] == "reused" and not update["events"]
changed = list(shifted)
changed[0] = (0, 1, 2.2)
update = atlas.update(4, changed)
assert update["mode"] == "recomputed" and update["events"]

# A sparse index path-copies exact interfaces, emits warm proof deltas, and
# keeps class computation off the update path unless requested.
index = holos_tda.compile_sparse_index(4, weighted, modulus=3)
assert index.snapshot.startswith(b"HOLOSIP\0")
assert len(index.version) == 64
assert index.result() == holos_tda.rips_sparse(4, weighted, modulus=3)
assert index.summary()["nodes"] >= 1
assert index.interfaces()[0]["digest"] == index.version
index_update = index.update(4, shifted, correspondence=False)
assert index_update["bars"] == holos_tda.rips_sparse(4, shifted, modulus=3)
assert index_update["delta_proof"].startswith(b"HOLOSDP\0")
assert index_update["snapshot_proof"] is None
assert index_update["correspondence"] == []
branches = index.fork(4, [changed])
assert len(branches) == 1
assert branches[0]["index"].result() == branches[0]["update"]["bars"]
assert branches[0]["index"].diff(index)["same_envelope"]

# The relative index carries H2 through an exact filtered core. An octahedron
# boundary has one essential H2 class.
octahedron = [
    (u, v, 1.0 + (u + v) / 100.0)
    for u in range(6) for v in range(u + 1, 6)
    if (u, v) not in {(0, 1), (2, 3), (4, 5)}
]
h2_index = holos_tda.compile_sparse_index(
    6, octahedron, max_dim=2, modulus=5)
assert h2_index.max_dim == 2
assert len([bar for bar in h2_index.result() if bar[0] == 2]) == 1
root_interface = h2_index.interfaces()[0]
assert root_interface["mode"] == "relative"
assert root_interface["relative_input_cells"] >= root_interface["relative_core_cells"]

# The materialized control retains the full graded reduction.
materialized_h2 = holos_tda.compile_sparse_index(
    6, octahedron, max_dim=2, modulus=5, interface_policy="materialize")
assert len(materialized_h2.interfaces()[0]["columns_by_dimension"]) == 3

# A relative interface fixes a noncontractible four-cycle separator and
# returns bytes accepted by the separate checker.
relative = holos_tda.compile_relative_interface(
    6,
    [
        (0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0),
        (0, 4, 2.0), (1, 4, 2.0), (2, 5, 2.5), (3, 5, 2.5),
    ],
    protected=[0, 1, 2, 3],
    max_dim=2,
    modulus=5,
)
assert relative["artifact"].startswith(b"HOLOSRI\0")
assert relative["input_cells"] >= relative["core_cells"]
assert len([bar for bar in relative["bars"] if bar[0] == 1]) == 1

# Fixed-scale cohomology, affine events, and weighted interventions
# use the same dimension-generic contract. The octahedron boundary is an H2
# sphere. Adding one antipodal edge fills it.
cohomology = holos_tda.cohomology_space(
    6, octahedron, dimension=2, scale=2.0, modulus=5)
assert cohomology["rank"] == 1
assert len(cohomology["basis"][0]["terms"]) > 0
identity = holos_tda.cohomology_relation(
    6, octahedron, octahedron, dimension=2, scale=2.0, modulus=5)
assert identity["isomorphism"] and identity["relation_rank"] == 1

affine = [
    (u, v, value, 0.0) for u, v, value in octahedron
] + [(0, 1, 3.0, -2.0)]
events = holos_tda.affine_events(
    6, affine, 0.0, 1.0, threshold=2.0, dimension=2, modulus=5)
assert any(event["before_rank"] == 1 and event["after_rank"] == 0
           for event in events["cohomology"])

zigzag = holos_tda.kinetic_zigzag(
    6, affine, 0.0, 1.0, dimension=2, scale=2.0, modulus=5)
assert zigzag["artifact"].startswith(b"HOLOSZZ\0")
assert zigzag["nodes"][0]["rank"] == 1
assert all(node["rank"] == 0 for node in zigzag["nodes"][1:])
assert all(arrow["rank"] == 0 for arrow in zigzag["arrows"])
assert [arrow["direction"] for arrow in zigzag["arrows"]] == [
    "backward" if position % 2 == 0 else "forward"
    for position in range(len(zigzag["arrows"]))
]
assert any(interval["start"] == interval["end"] == 0
           for interval in zigzag["intervals"])
assert len(zigzag["generalized_ranks"]) == len(zigzag["nodes"])

intervention = holos_tda.intervene_cohomology(
    6, [(octahedron, 0)], [(0, 1, 1)], dimension=2, scale=2.0,
    max_edits=1, modulus=5)
assert intervention["status"] == "optimal"
assert intervention["edits"] == [(0, 1, 1)]
assert intervention["lower_bound_cost"] == intervention["upper_bound_cost"] == 1
assert intervention["before_ranks"] == [1]
assert intervention["after_ranks"] == [0]
assert intervention["artifact"].startswith(b"HOLOSCI\0")

# One weighted link set satisfies named H1 requirements in different graphs.
first_cycle = [
    (0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0),
]
second_cycle = [
    (0, 1, 1.0), (1, 4, 1.0), (4, 5, 1.0), (0, 5, 1.0),
]
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

# Relative coverage returns an explicit filling chain. Failure-tolerant
# synthesis selects both redundant center sensors under one allowed failure.
coverage_graph = [
    (0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0),
    (0, 4, 1.0), (1, 4, 1.0), (2, 4, 1.0), (3, 4, 1.0),
    (0, 5, 1.0), (1, 5, 1.0), (2, 5, 1.0), (3, 5, 1.0),
]
coverage = holos_tda.relative_coverage(
    6, coverage_graph, [0, 1, 2, 3, 4], [0, 1, 2, 3], 1.0, 1.0,
    modulus=3,
)
assert coverage["criterion_holds"]
assert len(coverage["witness"]) == 4
assert coverage["physical_claim_is_conditional"]

coverage_plan = holos_tda.synthesize_coverage(
    6, [coverage_graph], [0, 1, 2, 3], [(4, 2), (5, 3)],
    broadcast_radius=1.0, sensing_radius=1.0, max_activations=2,
    failable=[4, 5], failure_budget=1, modulus=3,
)
assert coverage_plan["status"] == "optimal"
assert coverage_plan["actions"] == [(4, 2, [0]), (5, 3, [0])]
assert coverage_plan["lower_bound_cost"] == 5
assert coverage_plan["upper_bound_cost"] == 5
assert coverage_plan["artifact"].startswith(b"HOLOSCOV")
assert coverage_plan["selected_failure_checks"] == 2

affine_coverage_plan = holos_tda.synthesize_affine_coverage(
    5,
    [(u, v, value, 0.0) for u, v, value in coverage_graph if v < 5],
    0.0, 1.0, [0, 1, 2, 3], [(4, 1)],
    broadcast_radius=1.0, sensing_radius=1.0, max_activations=1,
)
assert affine_coverage_plan["status"] == "optimal"
assert affine_coverage_plan["states"] == 3
assert affine_coverage_plan["upper_bound_cost"] == 1

# A sparse program carries its atom proofs, reports exact work, updates
# locally, and emits independently checked traces and interventions.
program = holos_tda.compile_sparse_program(4, weighted, modulus=3)
assert program.artifact.startswith(b"HOLOSPRG")
assert program.proof.startswith(b"HOLOSPF\0")
assert program.result()["bars"] == holos_tda.rips_sparse(4, weighted, modulus=3)
assert program.summary()["cyclic_atoms"] == 1
assert program.atoms()
loaded = holos_tda.load_sparse_program(4, weighted, program.artifact)
assert loaded.result() == program.result()
evaluation = program.evaluate(4, shifted)
assert evaluation["work"]["edges_checked"] == len(weighted)
update = program.update(4, shifted)
assert update["mode"] == "reused"
assert update["work"]["atoms_reused"] == 1
batched = holos_tda.compile_sparse_program(4, weighted, modulus=3)
batch_updates = batched.update_many(4, [shifted, changed])
assert len(batch_updates) == 2
state_only = holos_tda.compile_sparse_program(4, weighted, modulus=3)
state_update = state_only.update(4, shifted, correspondence=False)
assert state_update["correspondence"] == []
branched = holos_tda.compile_sparse_program(
    4, weighted, modulus=3, threads=2).fork(4, [shifted, changed])
assert len(branched) == 2
assert all(branch["program"].result() == branch["update"]["result"]
           for branch in branched)
trace = holos_tda.compile_sparse_program_trace(4, weighted, [shifted], modulus=3)
assert trace.startswith(b"HOLOSDLT")
proof = holos_tda.compile_sparse_proof(4, weighted, [shifted], modulus=3)
assert proof.startswith(b"HOLOSPF\0")
assert holos_tda.verify_program_trace(trace) == {
    "steps": 1, "reused": 1, "repaired": 0, "recompiled": 0,
}
space = loaded.result()["spaces"][0]
intervention = loaded.intervene(
    0, (space["birth"] + space["death"]) / 2.0)
assert intervention["status"] in ("optimal", "bounded_gap")
assert intervention["artifact"].startswith(b"HOLOSINT")
checked = holos_tda.verify_intervention(intervention["artifact"])
assert checked["target"] == intervention["target"]

# The optional torch module imports without importing PyTorch in the base
# package. Its call either returns an exact strict derivative or explains the
# missing optional dependency.
from holos_tda.torch import finite_h1_intervals
try:
    import torch
except ImportError:
    try:
        finite_h1_intervals(4, [(u, v) for u, v, _ in weighted], [
            distance for _, _, distance in weighted])
    except ImportError as error:
        assert "requires PyTorch" in str(error)
    else:
        raise AssertionError("the optional torch call must report missing PyTorch")
else:
    weights = torch.tensor(
        [distance for _, _, distance in weighted],
        dtype=torch.float64,
        requires_grad=True,
    )
    endpoints = finite_h1_intervals(
        4, [(u, v) for u, v, _ in weighted], weights, modulus=3)
    endpoints.sum().backward()
    assert endpoints.shape == (1, 2)
    assert weights.grad is not None

# A point atlas exposes a conservative coordinate radius and analytic
# coordinate sensitivities for each untied finite endpoint.
point_atlas = holos_tda.compile_points_atlas(
    [[0, 0], [1, 0], [0, 2], [3, 4]], threshold=6.0
)
assert point_atlas.coordinate_radius > 0.0
assert "bars" in point_atlas.result()
assert isinstance(point_atlas.sensitivities(), list)

# collapse_edges=True yields the identical diagram through every entry point.
assert holos_tda.rips_points(sq, max_dim=1, collapse_edges=True) == holos_tda.rips_points(
    sq, max_dim=1
)
assert holos_tda.rips_condensed(
    pd, max_dim=1, collapse_edges=True
) == holos_tda.rips_condensed(pd, max_dim=1)
assert holos_tda.rips_sparse(4, cyc, collapse_edges=True) == holos_tda.rips_sparse(4, cyc)

# Every public schedule, both adaptive objectives, and a partial adaptive run
# preserve the diagram.
for schedule in ("ordered", "rounds"):
    assert holos_tda.rips_points(
        sq, max_dim=1, threads=2, collapse_edges=True,
        collapse_schedule=schedule,
    ) == holos_tda.rips_points(sq, max_dim=1)
for objective in ("h1", "h2"):
    assert holos_tda.rips_points(
        sq, max_dim=1, collapse_edges=True, collapse_schedule="adaptive",
        collapse_objective=objective,
    ) == holos_tda.rips_points(sq, max_dim=1)
assert holos_tda.rips_sparse(
    4, cyc, collapse_edges=True, collapse_schedule="adaptive",
    collapse_work_limit=0,
) == holos_tda.rips_sparse(4, cyc)
try:
    holos_tda.rips_points(sq, collapse_schedule="adaptive")
except ValueError as e:
    assert "collapse_edges=True" in str(e)
else:
    raise AssertionError("adaptive settings without collapse_edges must raise")

# collapse_edges=True yields the identical diagram through every entry point.
assert holos_tda.rips_points(sq, max_dim=1, collapse_edges=True) == holos_tda.rips_points(
    sq, max_dim=1
)
assert holos_tda.rips_condensed(
    pd, max_dim=1, collapse_edges=True
) == holos_tda.rips_condensed(pd, max_dim=1)
assert holos_tda.rips_sparse(4, cyc, collapse_edges=True) == holos_tda.rips_sparse(4, cyc)

# Coefficients: valid odd prime works, composite raises.
holos_tda.rips_points([[0, 0], [1, 0]], modulus=3)
try:
    holos_tda.rips_points([[0, 0], [1, 0]], modulus=4)
except ValueError as e:
    assert "prime" in str(e)
else:
    raise AssertionError("modulus=4 must raise ValueError")

# Console script is the real CLI.
out = subprocess.run(
    ["holos-tda", "--version"], capture_output=True, text=True, check=True
)
assert holos_tda.__version__ in out.stdout

print("smoke OK", holos_tda.__version__, holos_tda.GIT_HASH)
sys.exit(0)
