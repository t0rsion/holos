"""Tests for source-bound persistent class and selected-coordinate records."""

import math
import os
from pathlib import Path
import subprocess

import pytest

import holos_tda


def test_persistent_class_record_contains_checked_witnesses():
    result = holos_tda.persistent_class_sparse(4, _cycle(), threshold=1.0)

    assert result["artifact"].startswith(b"HOLOSPC\0")
    assert result["class"]["basis_index"] == 0
    assert result["class"]["birth"] == 1.0
    assert result["class"]["death"] is None
    assert result["class"]["terms"]
    assert result["critical_pair"]["birth"]["vertices"]
    assert result["critical_pair"]["death"] is None
    assert result["cycle"]
    assert result["bounding_chain"] == []


def test_selected_coordinate_record_contains_class_and_harmonic_data():
    result = holos_tda.persistent_circular_sparse(4, _cycle(), threshold=1.0)

    assert result["artifact"].startswith(b"HOLOSPH\0")
    assert result["class"]["id"]
    coordinate = result["coordinate"]
    assert len(coordinate["phase"]) == 4
    assert coordinate["divisibility"] == 1
    assert len(coordinate["potential"]) == 4
    assert coordinate["integral"]


def test_persistent_bytes_are_accepted_by_holos_check(tmp_path):
    checker_name = os.environ.get("HOLOS_CHECK_BIN")
    if not checker_name:
        raise RuntimeError("persistent.py requires HOLOS_CHECK_BIN")
    checker = Path(checker_name)
    if not checker.is_file() or not os.access(checker, os.X_OK):
        raise AssertionError(f"HOLOS_CHECK_BIN is not executable: {checker}")

    artifacts = {
        "class.hpc": holos_tda.persistent_class_sparse(
            4, _cycle(), threshold=1.0
        )["artifact"],
        "class-above-threshold.hpc": holos_tda.persistent_class_sparse(
            4, _cycle(extra_edge=2.5), threshold=1.0
        )["artifact"],
        "class-condensed.hpc": holos_tda.persistent_class_condensed(
            _condensed_cycle(), threshold=1.0
        )["artifact"],
        "class-points.hpc": holos_tda.persistent_class_points(
            _point_cycle(), threshold=1.1
        )["artifact"],
        "class-square.hpc": holos_tda.persistent_class_square(
            _square_cycle(), threshold=1.0
        )["artifact"],
        "coordinate.hph": holos_tda.persistent_circular_sparse(
            4, _cycle(), threshold=1.0
        )["artifact"],
        "coordinate-condensed.hph": holos_tda.persistent_circular_condensed(
            _condensed_cycle(), threshold=1.0
        )["artifact"],
        "coordinate-points.hph": holos_tda.persistent_circular_points(
            _point_cycle(), threshold=1.1
        )["artifact"],
        "coordinate-square.hph": holos_tda.persistent_circular_square(
            _square_cycle(), threshold=1.0
        )["artifact"],
    }
    for name, payload in artifacts.items():
        path = tmp_path / name
        path.write_bytes(payload)
        result = subprocess.run(
            [os.fspath(checker), os.fspath(path)],
            check=False,
            capture_output=True,
            text=True,
            env=os.environ.copy(),
        )
        assert result.returncode == 0, (
            f"{checker} rejected {path}:\n{result.stdout}\n{result.stderr}"
        )


def test_persistent_class_binds_finite_edges_above_threshold():
    base = holos_tda.persistent_class_sparse(
        4, _cycle(extra_edge=1.5), threshold=1.0
    )
    changed = holos_tda.persistent_class_sparse(
        4, _cycle(extra_edge=2.5), threshold=1.0
    )

    assert base["artifact"] != changed["artifact"]
    assert base["class"] == changed["class"]
    assert base["critical_pair"] == changed["critical_pair"]
    assert base["cycle"] == changed["cycle"]
    assert base["bounding_chain"] == changed["bounding_chain"]


def test_persistent_condensed_binds_finite_edges_above_threshold():
    base = holos_tda.persistent_class_condensed(
        _condensed_cycle(extra_diagonal=1.5), threshold=1.0
    )
    changed = holos_tda.persistent_class_condensed(
        _condensed_cycle(extra_diagonal=2.5), threshold=1.0
    )

    assert base["artifact"] != changed["artifact"]
    assert base["class"] == changed["class"]
    assert base["critical_pair"] == changed["critical_pair"]
    assert base["cycle"] == changed["cycle"]


def test_persistent_points_binds_finite_edges_above_threshold():
    base = holos_tda.persistent_class_points(_point_cycle(-1.0 / 3.0), threshold=1.1)
    changed = holos_tda.persistent_class_points(_point_cycle(-1.0 / 9.0), threshold=1.1)

    assert base["artifact"] != changed["artifact"]
    assert base["class"]["birth"] == changed["class"]["birth"]
    assert base["class"]["death"] == changed["class"]["death"]
    assert base["cycle"] == changed["cycle"]


def test_persistent_square_binds_finite_edges_above_threshold():
    base_matrix = _square_cycle(extra_diagonal=1.5)
    changed_matrix = _square_cycle(extra_diagonal=2.5)
    base = holos_tda.persistent_class_square(base_matrix, threshold=1.0)
    changed = holos_tda.persistent_class_square(changed_matrix, threshold=1.0)

    assert base["artifact"] != changed["artifact"]
    assert base["class"] == changed["class"]
    assert base["critical_pair"] == changed["critical_pair"]
    assert base["cycle"] == changed["cycle"]


def test_persistent_square_requires_symmetric_zero_diagonal():
    asymmetric = _square_cycle()
    asymmetric[1][0] = 2.0
    with pytest.raises(ValueError, match="not symmetric"):
        holos_tda.persistent_class_square(asymmetric, threshold=1.0)

    nonzero_diagonal = _square_cycle()
    nonzero_diagonal[0][0] = 1.0
    with pytest.raises(ValueError, match="diagonal"):
        holos_tda.persistent_class_square(nonzero_diagonal, threshold=1.0)


def _cycle(extra_edge=None):
    edges = [
        (0, 1, 1.0),
        (1, 2, 1.0),
        (2, 3, 1.0),
        (0, 3, 1.0),
    ]
    if extra_edge is not None:
        edges.append((0, 2, extra_edge))
    return edges


def _square_cycle(extra_diagonal=2.0):
    return [
        [0.0, 1.0, extra_diagonal, 1.0],
        [1.0, 0.0, 1.0, extra_diagonal],
        [extra_diagonal, 1.0, 0.0, 1.0],
        [1.0, extra_diagonal, 1.0, 0.0],
    ]


def _condensed_cycle(extra_diagonal=2.0):
    return [1.0, extra_diagonal, 1.0, 1.0, extra_diagonal, 1.0]


def _point_cycle(first_coordinate=-1.0 / 3.0):
    second_coordinate = first_coordinate + 1.0
    third_coordinate = math.sqrt(-2.0 * first_coordinate * second_coordinate)
    return [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0 + first_coordinate, second_coordinate, third_coordinate],
        [0.0, 1.0, 0.0],
    ]
