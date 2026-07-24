"""Vietoris-Rips persistent homology with a ripser-class engine.

Thin Python bindings over the `holos-tda` Rust crate. Each function returns
the persistence diagram as a list of ``(dim, birth, death)`` tuples in
canonical order (by dimension, then birth, then death); essential classes
have ``death == math.inf``.
"""

import sys

from holos_tda import _core
from holos_tda._core import GIT_HASH, __version__

__all__ = [
    "GIT_HASH",
    "__version__",
    "main",
    "rips_condensed",
    "rips_points",
    "rips_sparse",
]


def rips_points(points, max_dim=1, threshold=None, modulus=2):
    """Compute Rips persistence of a Euclidean point cloud.

    Args:
        points: sequence of points, each a sequence of float coordinates
            (all of the same dimension).
        max_dim: highest homology dimension to compute.
        threshold: truncate the filtration at this scale; ``None`` uses the
            enclosing radius.
        modulus: coefficient field Z/p; must be a prime below 32768.

    Returns:
        List of ``(dim, birth, death)`` tuples.
    """
    return _core.rips_points([list(map(float, p)) for p in points],
                             max_dim, threshold, modulus)


def rips_condensed(data, max_dim=1, threshold=None, modulus=2):
    """Compute Rips persistence of a condensed (upper-triangular, row-major)
    distance matrix, as produced by ``scipy.spatial.distance.pdist``.

    Args:
        data: flat sequence of the n*(n-1)/2 pairwise distances.
        max_dim: highest homology dimension to compute.
        threshold: truncate the filtration at this scale; ``None`` uses the
            enclosing radius.
        modulus: coefficient field Z/p; must be a prime below 32768.

    Returns:
        List of ``(dim, birth, death)`` tuples.
    """
    return _core.rips_condensed(list(map(float, data)),
                                max_dim, threshold, modulus)


def rips_sparse(n, triplets, max_dim=1, threshold=None, modulus=2):
    """Compute Rips persistence of a sparse distance matrix.

    Pairs not listed are absent at every scale; with ``threshold=None`` all
    listed edges enter the filtration.

    Args:
        n: number of points.
        triplets: iterable of ``(i, j, distance)`` entries.
        max_dim: highest homology dimension to compute.
        threshold: truncate the filtration at this scale.
        modulus: coefficient field Z/p; must be a prime below 32768.

    Returns:
        List of ``(dim, birth, death)`` tuples.
    """
    return _core.rips_sparse(n, [(int(i), int(j), float(d)) for i, j, d in triplets],
                             max_dim, threshold, modulus)


def main(argv=None):
    """Entry point for the ``holos-tda`` console script."""
    args = list(sys.argv[1:]) if argv is None else list(argv)
    raise SystemExit(_core.run_cli(["holos"] + args))
