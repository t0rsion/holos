"""Smoke tests for sparse programs, traces, and optional Torch support."""

import holos_tda

from smoke_utils import weighted_graph


def run():
    weighted = weighted_graph()
    shifted = [(i, j, distance + 0.01) for i, j, distance in weighted]
    changed = list(shifted)
    changed[0] = (0, 1, 2.2)

    # A sparse program carries its atom proofs, reports exact work, updates
    # locally, and emits independently checked traces and interventions.
    program = holos_tda.compile_sparse_program(4, weighted, modulus=3)
    _check_program_basics(program, weighted)
    loaded = _check_program_evaluation(program, weighted, shifted)
    _check_program_updates(program, weighted, shifted, changed)
    _check_program_trace_and_proof(weighted, shifted)
    _check_intervention(loaded)
    _check_torch(weighted)
    _check_point_atlas()


def _check_program_basics(program, weighted):
    assert program.artifact.startswith(b"HOLOSPRG")
    assert program.proof.startswith(b"HOLOSPF\0")
    assert program.result()["bars"] == holos_tda.rips_sparse(4, weighted, modulus=3)
    assert program.summary()["cyclic_atoms"] == 1
    assert program.atoms()


def _check_program_evaluation(program, weighted, shifted):
    loaded = holos_tda.load_sparse_program(4, weighted, program.artifact)
    assert loaded.result() == program.result()
    evaluation = program.evaluate(4, shifted)
    assert evaluation["work"]["edges_checked"] == len(weighted)
    return loaded


def _check_program_updates(program, weighted, shifted, changed):
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


def _check_program_trace_and_proof(weighted, shifted):
    trace = holos_tda.compile_sparse_program_trace(4, weighted, [shifted], modulus=3)
    assert trace.startswith(b"HOLOSDLT")
    proof = holos_tda.compile_sparse_proof(4, weighted, [shifted], modulus=3)
    assert proof.startswith(b"HOLOSPF\0")
    assert holos_tda.verify_program_trace(trace) == {
        "steps": 1, "reused": 1, "repaired": 0, "recompiled": 0,
    }


def _check_intervention(loaded):
    space = loaded.result()["spaces"][0]
    intervention = loaded.intervene(
        0, (space["birth"] + space["death"]) / 2.0)
    assert intervention["status"] in ("optimal", "bounded_gap")
    assert intervention["artifact"].startswith(b"HOLOSINT")
    checked = holos_tda.verify_intervention(intervention["artifact"])
    assert checked["target"] == intervention["target"]


def _check_torch(weighted):
    # The optional torch module does not import PyTorch from holos_tda.
    # The call returns a strict derivative or raises ImportError.
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


def _check_point_atlas():
    # A point atlas exposes a conservative coordinate radius and analytic
    # coordinate sensitivities for each untied finite endpoint.
    point_atlas = holos_tda.compile_points_atlas(
        [[0, 0], [1, 0], [0, 2], [3, 4]], threshold=6.0
    )
    assert point_atlas.coordinate_radius > 0.0
    assert "bars" in point_atlas.result()
    assert isinstance(point_atlas.sensitivities(), list)
