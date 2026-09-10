"""Focused smoke tests for the finite bipersistence Python API."""

from holos_tda.bipersistence import (
    degree_rips_bipersistence,
    load_bipersistence_artifact,
)


def test_cycle_module_and_artifact():
    edges = [
        (0, 1, 1.0),
        (1, 2, 1.0),
        (2, 3, 1.0),
        (0, 3, 1.0),
    ]
    module = degree_rips_bipersistence(4, edges, modulus=47)

    _assert_cycle_module(module)
    _assert_cycle_atlas(module)
    _assert_cycle_artifact(module)


def _assert_cycle_module(module):

    assert module.scales == [0.0, 1.0]
    assert module.minimum_degrees == [3, 2, 1, 0]
    assert len(module.node_ranks) == 8
    assert module.map_rank((1, 1), (1, 3)) == 1
    assert module.rectangle_rank((1, 1), (1, 3)) == 1


def _assert_cycle_atlas(module):
    atlas = module.class_atlas((1, 1), [(0, 1)])
    assert atlas["base"] == (1, 1)
    assert atlas["base_class"] == [{"basis_index": 0, "coefficient": 1}]

    family = module.circular_family((1, 1), [(0, 1)])
    assert family["base"] == (1, 1)
    assert any(entry["coordinate"] is not None for entry in family["entries"])


def _assert_cycle_artifact(module):
    module.record_rectangle((1, 1), (1, 3))
    module.record_class_atlas((1, 1), [(0, 1)])
    module.record_circular_family((1, 1), [(0, 1)])
    artifact = module.artifact
    assert artifact.startswith(b"HOLOSBP\0")

    loaded = load_bipersistence_artifact(artifact)
    loaded.verify()
    assert loaded.summary["rectangles"] == 1
    assert loaded.summary["class_atlases"] == 1
    assert loaded.summary["circular_families"] == 1


def test_declared_grid_is_exact_and_checked():
    edges = [
        (0, 1, 1.0),
        (1, 2, 1.0),
        (2, 3, 1.0),
        (0, 3, 1.0),
        (0, 2, 2.0),
        (1, 3, 2.0),
    ]
    module = degree_rips_bipersistence(
        4,
        edges,
        scales=[1.0, 2.0],
        minimum_degrees=[2, 0],
    )

    assert module.scales == [1.0, 2.0]
    assert module.minimum_degrees == [2, 0]
    assert len(module.node_ranks) == 4
    module.record_rectangle((0, 0), (1, 1))
    region = [(0, 1), (1, 1), (1, 0)]
    assert module.region_rank(region) == 0
    module.record_region(region)
    loaded = load_bipersistence_artifact(module.artifact)
    loaded.verify()
    assert loaded.summary["nodes"] == 4
    assert loaded.summary["regions"] == 1
    assert loaded.regions[0]["grades"] == [(0, 1), (1, 0), (1, 1)]
