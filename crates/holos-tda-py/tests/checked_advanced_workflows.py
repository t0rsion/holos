"""Run installed Python advanced artifacts through a separate ``holos-check`` process."""

import math
import os
from pathlib import Path
import subprocess
import tempfile

import holos_tda


def run():
    checker = _checker_path()
    with tempfile.TemporaryDirectory(prefix="holos-checked-advanced-") as directory:
        root = Path(directory)
        _check_zigzag(checker, root)
        _check_synthesis(checker, root)
        _check_coverage(checker, root)


def _check_zigzag(checker, root):
    affine = [(u, v, weight, 0.0) for u, v, weight in _octahedron()]
    affine.append((0, 1, 3.0, -2.0))
    zigzag = holos_tda.kinetic_zigzag(
        6, affine, 0.0, 1.0, dimension=2, scale=2.0, modulus=5
    )
    artifact = zigzag["artifact"]
    assert artifact.startswith(b"HOLOSZZ\0")
    _check(checker, _write(root / "zigzag.hzz", artifact), "kinetic zigzag")


def _check_synthesis(checker, root):
    first_cycle = _cycle()
    two_cycles = first_cycle + [
        (4, 5, 1.0), (5, 6, 1.0), (6, 7, 1.0), (4, 7, 1.0),
    ]
    finite = holos_tda.synthesize_cohomology(
        8, [two_cycles, two_cycles], [(0, 2, 4), (4, 6, 7)],
        dimension=1, scale=1.0, max_rank=0, max_edits=2, modulus=3,
    )
    assert finite["status"] == "optimal"
    assert finite["artifact"].startswith(b"HOLOSSYN")
    _check(checker, _write(root / "synthesis-finite.hsyn", finite["artifact"]),
           "listed-state synthesis")

    affine = holos_tda.synthesize_affine_cohomology(
        4, _affine_cycle(), 0.0, 1.5, [(1, 3, 2)],
        dimension=1, scale=1.0, max_rank=0, max_edits=1, modulus=3,
    )
    assert affine["status"] == "optimal"
    assert affine["artifact"].startswith(b"HOLOSSYN")
    _check(checker, _write(root / "synthesis-affine.hsyn", affine["artifact"]),
           "complete affine synthesis")


def _check_coverage(checker, root):
    coverage = _coverage_graph()
    finite = holos_tda.synthesize_coverage(
        6, [coverage], [0, 1, 2, 3], [(4, 2), (5, 3)],
        broadcast_radius=1.0, sensing_radius=1.0, max_activations=2,
        failable=[4, 5], failure_budget=1, modulus=3,
    )
    assert finite["status"] == "optimal"
    assert finite["artifact"].startswith(b"HOLOSCOV")
    _check(checker, _write(root / "coverage-finite.hcov", finite["artifact"]),
           "listed-state relative coverage")

    affine_edges = [(u, v, weight, 0.0) for u, v, weight in coverage if v < 5]
    affine = holos_tda.synthesize_affine_coverage(
        5, affine_edges, 0.0, 1.0, [0, 1, 2, 3], [(4, 1)],
        broadcast_radius=1.0, sensing_radius=1.0, max_activations=1,
    )
    assert affine["status"] == "optimal"
    assert affine["artifact"].startswith(b"HOLOSCOV")
    _check(checker, _write(root / "coverage-affine.hcov", affine["artifact"]),
           "complete affine relative coverage")

    geometry = holos_tda.synthesize_geometric_coverage(
        5, [_geometry_graph()],
        [[(0, 0), (2, 0), (2, 2), (0, 2), (1, 1)]],
        [0, 1, 2, 3], [(4, 1)],
        broadcast_radius=2.0, sensing_radius=2.0, max_activations=1,
    )
    assert geometry["geometry_checked"]
    assert geometry["artifact"].startswith(b"HOLOSGEO")
    _check(checker, _write(root / "coverage-geometry.hgeo", geometry["artifact"]),
           "planar coverage")


def _checker_path():
    value = os.environ.get("HOLOS_CHECK_BIN")
    if not value:
        raise RuntimeError("checked_advanced_workflows.py requires HOLOS_CHECK_BIN")
    checker = Path(value)
    if not checker.is_file() or not os.access(checker, os.X_OK):
        raise RuntimeError(
            f"HOLOS_CHECK_BIN is not an executable file: {checker}"
        )
    return checker


def _run(checker, artifact):
    return subprocess.run(
        [os.fspath(checker), os.fspath(artifact)],
        check=False,
        capture_output=True,
        text=True,
        env=os.environ.copy(),
    )


def _check(checker, artifact, marker):
    result = _run(checker, artifact)
    if result.returncode:
        raise AssertionError(
            f"{checker} rejected {artifact.name}:\n{result.stdout}\n{result.stderr}"
        )
    assert marker in result.stdout, (
        f"{checker} reported the wrong source for {artifact.name}: {result.stdout}"
    )


def _write(path, data):
    path.write_bytes(bytes(data))
    return path


def _cycle():
    return [
        (0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0),
    ]


def _affine_cycle():
    return [
        (0, 1, 1.0, 0.0), (1, 2, 1.0, 0.0),
        (2, 3, 1.0, 0.0), (0, 3, 1.0, 0.0),
        (0, 2, 2.0, -1.0),
    ]


def _octahedron():
    return [
        (u, v, 1.0 + (u + v) / 100.0)
        for u in range(6)
        for v in range(u + 1, 6)
        if (u, v) not in {(0, 1), (2, 3), (4, 5)}
    ]


def _coverage_graph():
    return [
        (0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0),
        (0, 4, 1.0), (1, 4, 1.0), (2, 4, 1.0), (3, 4, 1.0),
        (0, 5, 1.0), (1, 5, 1.0), (2, 5, 1.0), (3, 5, 1.0),
    ]


def _geometry_graph():
    side = 2.0
    diagonal = math.sqrt(2.0)
    return [
        (0, 1, side), (1, 2, side), (2, 3, side), (0, 3, side),
        (0, 4, diagonal), (1, 4, diagonal),
        (2, 4, diagonal), (3, 4, diagonal),
    ]


if __name__ == "__main__":
    run()
