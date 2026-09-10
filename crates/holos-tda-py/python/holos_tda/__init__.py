"""Vietoris-Rips persistent homology with a ripser-class engine.

Diagram functions return ``(dim, birth, death)`` tuples in canonical order:
by dimension, then birth, then death. Essential classes have
``death == math.inf``.
"""

# Private imports below preserve the historical root namespace.
# ruff: noqa: F401
import sys

from holos_tda import _core
from holos_tda._core import GIT_HASH, __version__

from ._coerce import (
    _condensed_rows,
    _persistent_class_input,
    _square_rows,
    _triplets,
)
from .atlas import (
    PointAtlas,
    SparseAtlas,
    SparseIndex,
    compile_points_atlas,
    compile_sparse_atlas,
    compile_sparse_index,
    load_sparse_atlas,
)
from .circular import (
    _circular_coordinate_record,
    _circular_result,
    circular_condensed,
    circular_condensed_class,
    circular_coordinates,
    circular_coordinates_class,
    circular_points,
    circular_points_class,
    circular_sparse,
    circular_sparse_class,
)
from .cli import main
from .collapse import (
    cohomology_relation,
    cohomology_space,
    compile_collapse_portfolio,
    compile_explicit_persistence,
    compile_relative_interface,
    merge_relative_interfaces,
)
from .coverage import (
    _coverage_candidates,
    _coverage_result,
    relative_coverage,
    synthesize_affine_coverage,
    synthesize_coverage,
    synthesize_geometric_coverage,
)
from .program import (
    SparseProgram,
    compile_sparse_program,
    compile_sparse_program_trace,
    compile_sparse_proof,
    load_sparse_program,
    verify_intervention,
    verify_program_trace,
)
from .records import (
    _atlas_result,
    _class_record,
    _class_result,
    _continuation,
    _correspondence,
    _event_record,
    _index_event,
    _index_update,
    _index_work,
    _intervention,
    _program_event,
    _program_result,
    _program_update,
    _program_work,
)
from .rips import (
    rips_condensed,
    rips_condensed_classes,
    rips_points,
    rips_points_classes,
    rips_sparse,
    rips_sparse_classes,
)
from .synthesis import (
    _synthesis,
    synthesize_affine_cohomology,
    synthesize_cohomology,
)
from .topology import affine_events, intervene_cohomology, kinetic_zigzag

__all__ = [
    "GIT_HASH",
    "PointAtlas",
    "SparseAtlas",
    "SparseIndex",
    "SparseProgram",
    "__version__",
    "affine_events",
    "circular_condensed",
    "circular_condensed_class",
    "circular_coordinates",
    "circular_coordinates_class",
    "circular_points",
    "circular_points_class",
    "circular_sparse",
    "circular_sparse_class",
    "cohomology_relation",
    "cohomology_space",
    "compile_collapse_portfolio",
    "compile_explicit_persistence",
    "compile_points_atlas",
    "compile_relative_interface",
    "compile_sparse_atlas",
    "compile_sparse_index",
    "compile_sparse_program",
    "compile_sparse_program_trace",
    "compile_sparse_proof",
    "intervene_cohomology",
    "kinetic_zigzag",
    "load_sparse_atlas",
    "load_sparse_program",
    "main",
    "merge_relative_interfaces",
    "relative_coverage",
    "rips_condensed",
    "rips_condensed_classes",
    "rips_points",
    "rips_points_classes",
    "rips_sparse",
    "rips_sparse_classes",
    "synthesize_affine_cohomology",
    "synthesize_affine_coverage",
    "synthesize_cohomology",
    "synthesize_coverage",
    "synthesize_geometric_coverage",
    "verify_intervention",
    "verify_program_trace",
]
