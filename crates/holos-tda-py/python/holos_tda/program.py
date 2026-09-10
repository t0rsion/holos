from . import _core
from ._coerce import _triplets
from .records import _intervention, _program_result, _program_update, _program_work


class SparseProgram:
    """Checked H0 and H1 persistence over sparse graph atoms.

    ``evaluate`` accepts only changes covered by every touched reduction
    region. ``update`` repairs failed atoms locally and recompiles after a
    topology or threshold-membership change.
    """

    def __init__(self, inner):
        self._inner = inner

    @property
    def artifact(self):
        """Return canonical ``HOLOSPRG`` bytes for the current program."""
        return self._inner.artifact

    @property
    def proof(self):
        """Return canonical ``HOLOSPF`` bytes for the current state."""
        return self._inner.proof

    def result(self):
        """Return the exact current diagram and H1 class spaces."""
        return _program_result(self._inner.result())

    def summary(self):
        """Return structural sizes and the checked guard count."""
        names = (
            "atoms",
            "cyclic_atoms",
            "articulation_vertices",
            "zero_simplex_separators",
            "widest_separator",
            "separator_candidates_checked",
            "separator_search_complete",
            "largest_cyclic_atom_edges",
            "guards",
        )
        return dict(zip(names, self._inner.summary()))

    def atoms(self):
        """Return all bridge and cyclic atoms in stable order."""
        records = []
        for atom_id, vertices, edges, separators, cyclic in self._inner.atoms():
            records.append(
                {
                    "id": atom_id,
                    "vertices": vertices,
                    "edges": edges,
                    "separator_vertices": separators,
                    "cyclic": cyclic,
                }
            )
        return records

    def evaluate(self, n, triplets):
        """Evaluate weights covered by every touched checked region."""
        bars, work = self._inner.evaluate(int(n), _triplets(triplets))
        return {"bars": bars, "work": _program_work(work)}

    def update(self, n, triplets, correspondence=True):
        """Reuse, repair, or recompile the program for a new graph.

        Set ``correspondence=False`` to maintain the exact current state
        without computing relations to the preceding class spaces.
        """
        raw = self._inner.update(int(n), _triplets(triplets), bool(correspondence))
        return _program_update(raw)

    def update_many(self, n, updates, correspondence=True):
        """Apply an ordered update batch atomically.

        If one update fails, the current program does not change.
        Set ``correspondence=False`` for state-only updates.
        """
        raw_updates = [_triplets(update) for update in updates]
        return [
            _program_update(raw)
            for raw in self._inner.update_many(
                int(n), raw_updates, bool(correspondence)
            )
        ]

    def fork(self, n, alternatives):
        """Advance independent alternatives from the current state.

        The current program does not change. Each returned branch can
        receive later updates.
        """
        raw = [_triplets(alternative) for alternative in alternatives]
        return [
            {"update": _program_update(update), "program": SparseProgram(inner)}
            for update, inner in self._inner.fork(int(n), raw)
        ]

    def intervene(self, space, before, budget=1):
        """Certify a restricted intervention on one finite H1 space.

        ``space`` is the zero-based index in ``result()["spaces"]``.
        ``optimal`` is relative to the current reduction and its declared
        destroyer triangles. ``bounded_gap`` reports a feasible edit with a
        distinct checked lower bound.
        """
        return _intervention(
            self._inner.intervene(int(space), float(before), int(budget))
        )


def compile_sparse_program(n, triplets, threshold=None, modulus=2, threads=1):
    """Compile a checked H0 and H1 persistence program."""
    inner = _core.compile_sparse_program(
        int(n), _triplets(triplets), threshold, modulus, threads
    )
    return SparseProgram(inner)


def load_sparse_program(n, triplets, artifact):
    """Check ``HOLOSPRG`` bytes against a graph and load its program."""
    inner = _core.load_sparse_program(int(n), _triplets(triplets), bytes(artifact))
    return SparseProgram(inner)


def compile_sparse_program_trace(
    n, initial, updates, threshold=None, modulus=2, threads=1
):
    """Build canonical ``HOLOSDLT`` bytes for a graph trajectory."""
    raw_updates = [_triplets(update) for update in updates]
    return bytes(
        _core.compile_sparse_program_trace(
            int(n), _triplets(initial), raw_updates, threshold, modulus, threads
        )
    )


def compile_sparse_proof(n, initial, updates, threshold=None, modulus=2, threads=1):
    """Build one ``HOLOSPF`` proof DAG for a graph trajectory."""
    raw_updates = [_triplets(update) for update in updates]
    return bytes(
        _core.compile_sparse_proof(
            int(n), _triplets(initial), raw_updates, threshold, modulus, threads
        )
    )


def verify_program_trace(artifact):
    """Check ``HOLOSDLT`` bytes and return step counts."""
    steps, reused, repaired, recompiled = _core.verify_program_trace(bytes(artifact))
    return {
        "steps": steps,
        "reused": reused,
        "repaired": repaired,
        "recompiled": recompiled,
    }


def verify_intervention(artifact):
    """Check ``HOLOSINT`` bytes and return its claim."""
    return _intervention(_core.verify_intervention(bytes(artifact)))
