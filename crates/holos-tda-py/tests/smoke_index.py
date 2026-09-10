"""Smoke tests for sparse atlases, indexes, and relative interfaces."""

import holos_tda

from smoke_utils import octahedron_boundary, weighted_graph


def run():
    weighted = weighted_graph()
    shifted = [(i, j, distance + 0.01) for i, j, distance in weighted]
    changed = list(shifted)
    changed[0] = (0, 1, 2.2)

    # A sparse atlas evaluates an order-preserving update without reduction,
    # carries a portable proof, and recompiles exactly after an order event.
    atlas = holos_tda.compile_sparse_atlas(4, weighted, modulus=3)
    _check_atlas(atlas, weighted, shifted, changed)

    # A sparse index path-copies exact interfaces, emits warm proof deltas, and
    # keeps class computation off the update path unless requested.
    index = holos_tda.compile_sparse_index(4, weighted, modulus=3)
    _check_index(index, weighted, shifted, changed)
    _check_relative_index()


def _check_atlas(atlas, weighted, shifted, changed):
    assert atlas.artifact.startswith(b"HOLOSATL")
    assert atlas.result()["bars"] == holos_tda.rips_sparse(4, weighted, modulus=3)
    loaded = holos_tda.load_sparse_atlas(4, weighted, atlas.artifact)
    assert loaded.result() == atlas.result()
    update = atlas.update(4, shifted)
    assert update["mode"] == "reused" and not update["events"]
    update = atlas.update(4, changed)
    assert update["mode"] == "recomputed" and update["events"]


def _check_index(index, weighted, shifted, changed):
    _check_index_metadata(index, weighted)
    _check_index_update(index, shifted)
    _check_index_branches(index, changed)


def _check_index_metadata(index, weighted):
    assert index.snapshot.startswith(b"HOLOSIP\0")
    assert len(index.version) == 64
    assert index.result() == holos_tda.rips_sparse(4, weighted, modulus=3)
    assert index.summary()["nodes"] >= 1
    assert index.interfaces()[0]["digest"] == index.version


def _check_index_update(index, shifted):
    index_update = index.update(4, shifted, correspondence=False)
    assert index_update["bars"] == holos_tda.rips_sparse(4, shifted, modulus=3)
    assert index_update["delta_proof"].startswith(b"HOLOSDP\0")
    assert index_update["snapshot_proof"] is None
    assert index_update["correspondence"] == []


def _check_index_branches(index, changed):
    branches = index.fork(4, [changed])
    assert len(branches) == 1
    assert branches[0]["index"].result() == branches[0]["update"]["bars"]
    assert branches[0]["index"].diff(index)["same_envelope"]


def _check_relative_index():
    # The relative index carries H2 through an exact filtered core. An octahedron
    # boundary has one essential H2 class.
    octahedron = octahedron_boundary()
    h2_index = holos_tda.compile_sparse_index(
        6, octahedron, max_dim=2, modulus=5)
    _check_relative_h2(h2_index)

    # The materialized control retains the full graded reduction.
    materialized_h2 = holos_tda.compile_sparse_index(
        6, octahedron, max_dim=2, modulus=5, interface_policy="materialize")
    assert len(materialized_h2.interfaces()[0]["columns_by_dimension"]) == 3

    _check_relative_interface()


def _check_relative_h2(h2_index):
    assert h2_index.max_dim == 2
    assert len([bar for bar in h2_index.result() if bar[0] == 2]) == 1
    root_interface = h2_index.interfaces()[0]
    assert root_interface["mode"] == "relative"
    assert root_interface["relative_input_cells"] >= root_interface["relative_core_cells"]


def _check_relative_interface():
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
