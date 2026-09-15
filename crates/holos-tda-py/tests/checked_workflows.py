"""Smoke-test Python artifacts with the independent ``holos-check`` binary."""

import os
from pathlib import Path
import subprocess
import tempfile

import holos_tda
from holos_tda.bipersistence import degree_rips_bipersistence


def run():
    checker = _checker_path()
    cycle = [
        (0, 1, 1.0),
        (1, 2, 1.0),
        (2, 3, 1.0),
        (0, 3, 1.0),
    ]
    weighted = [
        (0, 1, 1.0),
        (0, 2, 2.0),
        (0, 3, 1.1),
        (1, 2, 1.2),
        (1, 3, 2.1),
        (2, 3, 1.3),
    ]
    shifted = [(u, v, value + 0.01) for u, v, value in weighted]

    with tempfile.TemporaryDirectory(prefix="holos-checked-") as directory:
        root = Path(directory)
        _, classes = holos_tda.rips_sparse_classes(4, cycle, modulus=47)
        circular = holos_tda.circular_sparse_class(4, cycle, classes[0])
        _write(root / "coordinate.hcc", circular["artifact"])
        _check(checker, root / "coordinate.hcc")

        _, mod_two_classes = holos_tda.rips_sparse_classes(4, cycle, modulus=2)
        supplied = holos_tda.circular_sparse_class(
            4,
            cycle,
            mod_two_classes[0],
            integral_lift=mod_two_classes[0]["terms"],
        )
        _write(root / "coordinate-supplied.hcc", supplied["artifact"])
        _check(checker, root / "coordinate-supplied.hcc")

        module = degree_rips_bipersistence(
            4,
            cycle,
            scales=[1.0],
            minimum_degrees=[2, 0],
            modulus=47,
        )
        module.record_rectangle((0, 0), (0, 1))
        module.record_class_atlas((0, 0), [(0, 1)])
        module.record_circular_family((0, 0), [(0, 1)])
        _write(root / "module.hbp", module.artifact)
        _check(checker, root / "module.hbp")

        program = holos_tda.compile_sparse_program(5, weighted, modulus=3)
        _write(root / "program.hsp", program.artifact)
        graph = root / "source.graph"
        graph.write_text(_graph_text(weighted, 5), encoding="utf-8")
        assert graph.read_text(encoding="utf-8").splitlines()[0] == "5"
        _check(checker, root / "program.hsp", graph)

        wrong_graph = root / "wrong.graph"
        wrong_graph.write_text(_graph_text(shifted, 5), encoding="utf-8")
        result = _run(checker, root / "program.hsp", wrong_graph)
        assert result.returncode != 0, "the checker accepted a mismatched source graph"

        omitted_count = root / "omitted-count.graph"
        omitted_count.write_text(
            _graph_text(weighted, 5, include_vertex_count=False), encoding="utf-8"
        )
        result = _run(checker, root / "program.hsp", omitted_count)
        assert result.returncode != 0, "the checker accepted an inferred vertex count"

        trace = holos_tda.compile_sparse_program_trace(
            5, weighted, [shifted], modulus=3
        )
        _write(root / "trace.hst", trace)
        _check(checker, root / "trace.hst")


def _checker_path():
    value = os.environ.get("HOLOS_CHECK_BIN")
    if not value:
        raise RuntimeError("checked_workflows.py requires HOLOS_CHECK_BIN")
    checker = Path(value)
    if not checker.is_file() or not os.access(checker, os.X_OK):
        raise RuntimeError(f"HOLOS_CHECK_BIN is not an executable file: {checker}")
    return checker


def _graph_text(triplets, vertex_count, include_vertex_count=True):
    rows = [str(vertex_count)] if include_vertex_count else []
    rows.extend(f"{u} {v} {value}" for u, v, value in triplets)
    return "\n".join(rows) + "\n"


def _write(path, data):
    path.write_bytes(bytes(data))


def _check(checker, artifact, *extra):
    result = _run(checker, artifact, *extra)
    if result.returncode:
        raise AssertionError(
            f"{checker} rejected {artifact.name}:\n{result.stdout}\n{result.stderr}"
        )


def _run(checker, artifact, *extra):
    return subprocess.run(
        [os.fspath(checker), os.fspath(artifact), *(os.fspath(path) for path in extra)],
        check=False,
        capture_output=True,
        text=True,
        env=os.environ.copy(),
    )


if __name__ == "__main__":
    run()
