"""Smoke tests for circular coordinates and persistent class records."""

import holos_tda

from smoke_utils import SQRT2, close, condensed_square, cycle, distance_square, square


def run():
    cyc = cycle()
    pd = condensed_square()
    sq = square()
    distance_sq = distance_square()

    # Ripser-shaped cocycles feed checked harmonic circular coordinates directly.
    _check_circular_rows(cyc, distance_sq)
    _check_point_classes(sq)

    # A scalar class record carries its active graph binding into the direct
    # circular-coordinate path.
    _, sparse_classes = holos_tda.rips_sparse_classes(4, cyc, modulus=47)
    sparse_class = sparse_classes[0]
    _check_sparse_class(cyc, sparse_class)
    _check_condensed_class(pd)
    _check_point_class(sq, distance_sq)


def _check_circular_rows(cyc, distance_sq):
    circular = holos_tda.circular_sparse(
        4, cyc, [(0, 1, 1)], scale=1.0, modulus=47)
    assert circular["artifact"].startswith(b"HOLOSCC\0")
    assert len(circular["coordinate"]["phase"]) == 4
    assert circular["coordinate"]["divisibility"] == 1
    assert circular["coordinate"]["relative_residual"] <= 1e-10
    from_matrix = holos_tda.circular_coordinates(
        distance_sq, [(0, 1, 1)], scale=1.0, modulus=47)
    assert from_matrix["coordinate"]["phase"] == circular["coordinate"]["phase"]


def _check_point_classes(sq):
    # Equal diagram, one stable cocycle, and the same class after certified
    # collapse and reverse lifting.
    bars, classes = holos_tda.rips_points_classes(sq, max_dim=1, modulus=3)
    assert bars == holos_tda.rips_points(sq, max_dim=1, modulus=3)
    _check_point_class_record(classes)
    _check_collapsed_point_classes(sq, bars, classes)


def _check_point_class_record(classes):
    assert len(classes) == 1
    assert classes[0]["birth"] == 1.0
    assert close(classes[0]["death"], SQRT2)
    assert classes[0]["modulus"] == 3
    assert classes[0]["group_id"]
    assert classes[0]["basis_index"] == 0
    assert classes[0]["terms"]


def _check_collapsed_point_classes(sq, bars, classes):
    collapsed_bars, collapsed_classes = holos_tda.rips_points_classes(
        sq, max_dim=1, modulus=3, threads=2, collapse_edges=True,
        collapse_schedule="rounds",
    )
    assert collapsed_bars == bars
    assert collapsed_classes == classes


def _check_sparse_class(cyc, sparse_class):
    assert sparse_class["provenance"]["schema"] == "holos-persistent-class-source-v1"
    assert sparse_class["provenance"]["class_digest"] == sparse_class["id"]
    from_sparse_class = holos_tda.circular_sparse_class(4, cyc, sparse_class)
    from_sparse_rows = holos_tda.circular_sparse(
        4, cyc, sparse_class["terms"], sparse_class["scale"],
        modulus=sparse_class["modulus"])
    assert from_sparse_class["coordinate"]["phase"] == from_sparse_rows["coordinate"]["phase"]
    _reject_changed_graph(cyc, sparse_class)
    _reject_unknown_schema(cyc, sparse_class)
    _reject_changed_class_digest(cyc, sparse_class)


def _reject_changed_graph(cyc, sparse_class):
    changed_cyc = [(u, v, 0.9 if (u, v) == (0, 1) else distance)
                   for u, v, distance in cyc]
    try:
        holos_tda.circular_sparse_class(4, changed_cyc, sparse_class)
    except ValueError as error:
        assert "different active graph" in str(error)
    else:
        raise AssertionError("a class from another graph must be rejected")


def _reject_unknown_schema(cyc, sparse_class):
    unknown_schema = dict(sparse_class)
    unknown_schema["provenance"] = dict(sparse_class["provenance"])
    unknown_schema["provenance"]["schema"] = "unknown"
    try:
        holos_tda.circular_sparse_class(4, cyc, unknown_schema)
    except ValueError as error:
        assert "schema is not supported" in str(error)
    else:
        raise AssertionError("an unknown provenance schema must be rejected")


def _reject_changed_class_digest(cyc, sparse_class):
    changed_class_digest = dict(sparse_class)
    changed_class_digest["provenance"] = dict(sparse_class["provenance"])
    changed_class_digest["provenance"]["class_digest"] = "00" * 32
    try:
        holos_tda.circular_sparse_class(4, cyc, changed_class_digest)
    except ValueError as error:
        assert "identity differs" in str(error)
    else:
        raise AssertionError("a class identity mismatch must be rejected")


def _check_condensed_class(pd):
    _, condensed_classes = holos_tda.rips_condensed_classes(pd, modulus=47)
    condensed_class = condensed_classes[0]
    from_condensed_class = holos_tda.circular_condensed_class(pd, condensed_class)
    from_condensed_rows = holos_tda.circular_condensed(
        pd, condensed_class["terms"], condensed_class["scale"],
        modulus=condensed_class["modulus"])
    assert from_condensed_class["coordinate"]["phase"] == from_condensed_rows["coordinate"]["phase"]


def _check_point_class(sq, distance_sq):
    _, point_classes = holos_tda.rips_points_classes(sq, modulus=47)
    point_class = point_classes[0]
    from_point_class = holos_tda.circular_points_class(sq, point_class)
    from_point_rows = holos_tda.circular_points(
        sq, point_class["terms"], point_class["scale"],
        modulus=point_class["modulus"])
    assert from_point_class["coordinate"]["phase"] == from_point_rows["coordinate"]["phase"]
    from_square_class = holos_tda.circular_coordinates_class(distance_sq, point_class)
    assert from_square_class["coordinate"]["phase"] == from_point_class["coordinate"]["phase"]
