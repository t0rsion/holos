from . import _core
from ._coerce import _points, _triplets
from .records import _atlas_result, _event_record, _index_update, _program_result


class SparseAtlas:
    """A proof-carrying local model for a sparse weighted graph.

    A compiled region fixes the vertices, listed edges, threshold membership,
    and weak edge-weight order. ``evaluate`` runs no persistence reduction
    while that contract holds. ``update`` recompiles exactly after an event.
    """

    def __init__(self, inner):
        self._inner = inner

    @property
    def artifact(self):
        """Return canonical ``HOLOSATL`` bytes for the compiled region."""
        return self._inner.artifact

    def result(self):
        """Return the result for the most recent graph."""
        return _atlas_result(self._inner.result())

    def evaluate(self, n, triplets):
        """Evaluate weights in the current region without reduction."""
        return _atlas_result(self._inner.evaluate(int(n), _triplets(triplets)))

    def events(self, n, triplets):
        """Return changes that prevent reuse at the supplied graph."""
        return [
            _event_record(record)
            for record in self._inner.events(int(n), _triplets(triplets))
        ]

    def update(self, n, triplets):
        """Reuse the region or compile a new proof after an event."""
        mode, events, result = self._inner.update(int(n), _triplets(triplets))
        return {
            "mode": mode,
            "events": [_event_record(record) for record in events],
            "result": _atlas_result(result),
        }


class PointAtlas:
    """A Euclidean point atlas with a conservative displacement radius."""

    def __init__(self, inner):
        self._inner = inner

    @property
    def coordinate_radius(self):
        """Return the conservative per-point Euclidean displacement radius."""
        return self._inner.coordinate_radius

    def result(self):
        """Return the result for the most recent point cloud."""
        return _atlas_result(self._inner.result())

    def sensitivities(self):
        """Return analytic endpoint derivatives by point coordinate."""
        records = []
        for lineage, birth, death in self._inner.sensitivities():

            def expand(gradient):
                if gradient is None:
                    return None
                edge, terms = gradient
                return {"edge": edge, "terms": terms}

            records.append(
                {
                    "lineage": lineage,
                    "birth": expand(birth),
                    "death": expand(death),
                }
            )
        return records

    def evaluate(self, points):
        """Evaluate points inside the conservative displacement radius."""
        return _atlas_result(self._inner.evaluate(_points(points)))

    def update(self, points):
        """Reuse the point atlas or recompile after a radius event."""
        mode, result = self._inner.update(_points(points))
        return {"mode": mode, "result": _atlas_result(result)}


class SparseIndex:
    """Versioned persistence for a sparse graph.

    The listed edges form a fixed envelope. Updates inside that envelope
    path-copy changed relative cores and share the rest. An envelope change
    compiles a new root and returns a new cold checkpoint.
    """

    def __init__(self, inner):
        self._inner = inner

    @property
    def snapshot(self):
        """Return canonical ``HOLOSIP`` bytes for the current version."""
        return bytes(self._inner.snapshot)

    @property
    def version(self):
        """Return the content identifier of the current root."""
        return self._inner.version

    @property
    def max_dim(self):
        """Return the highest maintained homology dimension."""
        return self._inner.max_dim

    def result(self):
        """Return the exact current persistence diagram."""
        return self._inner.result()

    def explain(self):
        """Compute canonical H1 class spaces for the current graph."""
        return _program_result(self._inner.explain())

    def summary(self):
        """Return structural and algebraic sizes of the interface tree."""
        names = (
            "nodes",
            "leaves",
            "separators",
            "component_splits",
            "widest_separator",
            "largest_interface_vertices",
            "largest_interface_edges",
            "composed_interfaces",
            "materialized_interfaces",
            "relative_interfaces",
            "relative_input_cells",
            "relative_core_cells",
            "largest_relative_core_cells",
            "relative_cancellations",
            "root_composed",
            "separator_candidates_checked",
            "separator_search_complete",
        )
        structural, relative, control = self._inner.summary()
        summary = dict(zip(names, tuple(structural) + tuple(relative) + tuple(control)))
        summary["max_dim"] = self._inner.max_dim
        return summary

    def interfaces(self):
        """Return all index interfaces in deterministic preorder."""
        records = []
        for (
            digest,
            depth,
            vertices,
            separator,
            protected_vertices,
            edges,
            children,
            mode,
            reduction_columns,
            columns_by_dimension,
            relative_size,
        ) in self._inner.interfaces():
            relative_input_cells, relative_core_cells, relative_cancellations = (
                relative_size
            )
            records.append(
                {
                    "digest": digest,
                    "depth": depth,
                    "vertices": vertices,
                    "separator": separator,
                    "protected_vertices": protected_vertices,
                    "edges": edges,
                    "children": children,
                    "mode": mode,
                    "reduction_columns": reduction_columns,
                    "columns_by_dimension": columns_by_dimension,
                    "relative_input_cells": relative_input_cells,
                    "relative_core_cells": relative_core_cells,
                    "relative_cancellations": relative_cancellations,
                }
            )
        return records

    def update(self, n, triplets, correspondence=True):
        """Install an exact next version and return its proof and work."""
        return _index_update(
            self._inner.update(int(n), _triplets(triplets), bool(correspondence))
        )

    def update_many(self, n, updates, correspondence=True):
        """Apply an ordered update batch atomically."""
        raw_updates = [_triplets(update) for update in updates]
        return [
            _index_update(raw)
            for raw in self._inner.update_many(
                int(n), raw_updates, bool(correspondence)
            )
        ]

    def patch(self, edits, correspondence=True):
        """Apply one atomic active-topology patch and return its proof."""
        records = []
        for edit in edits:
            kind = str(edit[0])
            if kind == "deactivate":
                if len(edit) != 3:
                    raise ValueError("deactivate requires kind, u, and v")
                records.append((kind, int(edit[1]), int(edit[2]), None))
            else:
                if len(edit) != 4:
                    raise ValueError(f"{kind} requires kind, u, v, and value")
                records.append((kind, int(edit[1]), int(edit[2]), float(edit[3])))
        return _index_update(self._inner.patch(records, bool(correspondence)))

    def fork(self, n, alternatives):
        """Advance alternatives without changing the current version."""
        raw = [_triplets(alternative) for alternative in alternatives]
        return [
            {"update": _index_update(update), "index": SparseIndex(inner)}
            for update, inner in self._inner.fork(int(n), raw)
        ]

    def diff(self, other):
        """Compare structural sharing and diagrams with another version."""
        same_envelope, shared_nodes, removed, added = self._inner.diff(other._inner)
        return {
            "same_envelope": same_envelope,
            "shared_nodes": shared_nodes,
            "diagram_delta": {"removed": removed, "added": added},
        }


def compile_sparse_atlas(
    n,
    triplets,
    threshold=None,
    modulus=2,
    threads=1,
    factorization="off",
    collapse_edges=False,
    collapse_schedule="serial",
    collapse_objective="h2",
    collapse_work_limit=None,
):
    """Compile a proof-carrying H0 and H1 atlas for a sparse graph."""
    inner = _core.compile_sparse_atlas(
        int(n),
        _triplets(triplets),
        threshold,
        modulus,
        threads,
        factorization,
        collapse_edges,
        collapse_schedule,
        collapse_objective,
        collapse_work_limit,
    )
    return SparseAtlas(inner)


def compile_sparse_index(
    n,
    triplets,
    max_dim=1,
    threshold=None,
    modulus=2,
    threads=1,
    separator_width=4,
    separator_search_limit=100_000,
    leaf_vertices=4,
    interface_policy="relative",
):
    """Compile a versioned sparse persistence index."""
    inner = _core.compile_sparse_index(
        int(n),
        _triplets(triplets),
        int(max_dim),
        threshold,
        modulus,
        threads,
        int(separator_width),
        int(separator_search_limit),
        int(leaf_vertices),
        str(interface_policy),
    )
    return SparseIndex(inner)


def load_sparse_atlas(n, triplets, artifact):
    """Check ``HOLOSATL`` bytes against a graph and load its local model."""
    inner = _core.load_sparse_atlas(int(n), _triplets(triplets), bytes(artifact))
    return SparseAtlas(inner)


def compile_points_atlas(
    points,
    threshold,
    modulus=2,
    threads=1,
    factorization="off",
    collapse_edges=False,
    collapse_schedule="serial",
    collapse_objective="h2",
    collapse_work_limit=None,
):
    """Compile a finite-threshold point atlas and coordinate sensitivities."""
    inner = _core.compile_points_atlas(
        _points(points),
        float(threshold),
        modulus,
        threads,
        factorization,
        collapse_edges,
        collapse_schedule,
        collapse_objective,
        collapse_work_limit,
    )
    return PointAtlas(inner)
