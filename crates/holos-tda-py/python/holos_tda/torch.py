"""Strict PyTorch derivatives for finite sparse H1 intervals.

This module is optional. Importing :mod:`holos_tda` does not import PyTorch.
"""

from holos_tda import compile_sparse_program


def _torch():
    try:
        import torch
    except ImportError as error:
        raise ImportError(
            "holos_tda.torch requires PyTorch; install torch separately"
        ) from error
    return torch


def _edge_key(edge):
    u, v = map(int, edge)
    if u == v:
        raise ValueError("edge endpoints must differ")
    return (u, v) if u < v else (v, u)


class StrictSparseProgram:
    """Stateful strict derivatives along one fixed sparse graph topology.

    The input edge weights must be distinct. No weight may equal the fixed
    threshold. A call can still repair a reduction region. The returned
    derivative then uses the newly checked critical pairs.
    """

    def __init__(self, n, edges, weights, threshold=None, modulus=2,
                 threads=1):
        torch = _torch()
        keys = [_edge_key(edge) for edge in edges]
        if len(set(keys)) != len(keys):
            raise ValueError("edges must be unique")
        if any(u >= int(n) or v >= int(n) for u, v in keys):
            raise ValueError("an edge endpoint is outside the vertex set")
        tensor = torch.as_tensor(weights)
        self._check_weights(tensor, threshold)
        if tensor.numel() != len(keys):
            raise ValueError(
                f"received {tensor.numel()} weights for {len(keys)} edges"
            )
        self.n = int(n)
        self.edges = keys
        self.threshold = threshold
        self.modulus = int(modulus)
        self.threads = int(threads)
        self._weights = [float(value) for value in tensor.detach().cpu()]
        self._program = compile_sparse_program(
            self.n,
            self._triplets(self._weights),
            threshold,
            self.modulus,
            self.threads,
        )

    @staticmethod
    def _check_weights(weights, threshold):
        torch = _torch()
        if weights.ndim != 1:
            raise ValueError("weights must be a one-dimensional tensor")
        if not weights.is_floating_point():
            raise TypeError("weights must have a floating-point dtype")
        if not bool(torch.isfinite(weights).all()):
            raise ValueError("weights must be finite")
        if bool((weights < 0).any()):
            raise ValueError("weights must be non-negative")
        values = weights.detach().cpu().tolist()
        if len(set(values)) != len(values):
            raise ValueError("strict derivatives require distinct edge weights")
        if threshold is not None and any(value == threshold for value in values):
            raise ValueError(
                "strict derivatives require every weight to differ from threshold"
            )

    def _triplets(self, values):
        return [
            (u, v, value)
            for (u, v), value in zip(self.edges, values)
        ]

    def _evaluate(self, weights):
        self._check_weights(weights, self.threshold)
        if weights.numel() != len(self.edges):
            raise ValueError(
                f"received {weights.numel()} weights for {len(self.edges)} edges"
            )
        values = [float(value) for value in weights.detach().cpu()]
        if values != self._weights:
            updated = self._program.update(self.n, self._triplets(values))
            result = updated["result"]
            self._weights = values
        else:
            result = self._program.result()
        positions = {edge: index for index, edge in enumerate(self.edges)}
        rows = []
        indices = []
        for space in result["spaces"]:
            for pair in space["critical_pairs"]:
                death = pair["death"]
                if death is None:
                    continue
                birth_edge = _edge_key(pair["birth"]["vertices"])
                birth_index = positions[birth_edge]
                triangle = death["vertices"]
                triangle_edges = [
                    _edge_key((triangle[0], triangle[1])),
                    _edge_key((triangle[0], triangle[2])),
                    _edge_key((triangle[1], triangle[2])),
                ]
                death_index = max(
                    (positions[edge] for edge in triangle_edges),
                    key=values.__getitem__,
                )
                rows.append((values[birth_index], values[death_index]))
                indices.append((birth_index, death_index))
        return rows, indices

    def finite_h1_intervals(self, weights):
        """Return finite H1 endpoints with an exact local backward map."""
        torch = _torch()
        owner = self

        class FiniteH1(torch.autograd.Function):
            @staticmethod
            def forward(ctx, current):
                rows, indices = owner._evaluate(current)
                ctx.indices = indices
                ctx.input_size = current.numel()
                if not rows:
                    return current.new_empty((0, 2))
                return current.new_tensor(rows)

            @staticmethod
            def backward(ctx, output_gradient):
                gradient = output_gradient.new_zeros(ctx.input_size)
                for row, (birth, death) in enumerate(ctx.indices):
                    gradient[birth] += output_gradient[row, 0]
                    gradient[death] += output_gradient[row, 1]
                return gradient

        return FiniteH1.apply(weights)


def finite_h1_intervals(n, edges, weights, threshold=None, modulus=2,
                        threads=1):
    """Compute finite H1 endpoints and their strict edge-weight derivative."""
    program = StrictSparseProgram(
        n, edges, weights, threshold=threshold, modulus=modulus,
        threads=threads)
    return program.finite_h1_intervals(weights)


__all__ = ["StrictSparseProgram", "finite_h1_intervals"]
