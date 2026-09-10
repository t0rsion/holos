"""Frozen synthetic inputs and structural contracts for the bipersistence study."""

from __future__ import annotations

from collections.abc import Sequence
from dataclasses import dataclass
from pathlib import Path

Edge = tuple[int, int, float]
Grade = tuple[int, int]
Rectangle = tuple[int, int, int, int]
ClassSelection = tuple[int, int, int]


@dataclass(frozen=True)
class GraphCase:
    """One explicit graph and its declared finite bipersistence queries."""

    name: str
    source_id: str
    vertex_count: int
    edges: tuple[Edge, ...]
    threshold: float
    scales: tuple[float, ...]
    minimum_degrees: tuple[int, ...]
    rectangles: tuple[Rectangle, ...]
    regions: tuple[tuple[Grade, ...], ...]
    class_selection: ClassSelection | None
    circular: bool

    @property
    def expected_nodes(self) -> int:
        """Return the number of nodes in the declared product grid."""

        return len(self.scales) * len(self.minimum_degrees)

    @property
    def expected_cover_maps(self) -> int:
        """Return the number of horizontal and vertical cover maps."""

        scale_covers = max(len(self.scales) - 1, 0) * len(self.minimum_degrees)
        degree_covers = len(self.scales) * max(len(self.minimum_degrees) - 1, 0)
        return scale_covers + degree_covers


TWO_CYCLES = GraphCase(
    name="two-cycles",
    source_id="explicit-two-cycles-v1",
    vertex_count=7,
    edges=(
        (0, 1, 1.0),
        (1, 2, 1.0),
        (2, 3, 1.0),
        (0, 3, 1.0),
        (0, 4, 2.0),
        (4, 5, 2.0),
        (5, 6, 2.0),
        (0, 6, 2.0),
    ),
    threshold=3.0,
    scales=(1.0, 2.0, 3.0),
    minimum_degrees=(6, 5, 4, 3, 2, 1, 0),
    rectangles=((0, 6, 1, 6), (1, 6, 2, 6)),
    regions=(
        (
            (0, 6),
            (1, 5),
            (1, 6),
            (2, 5),
        ),
    ),
    class_selection=(0, 6, 0),
    circular=True,
)


SQUARE_WITH_DIAGONALS = GraphCase(
    name="square-with-diagonals",
    source_id="explicit-square-diagonals-v1",
    vertex_count=4,
    edges=(
        (0, 1, 1.0),
        (1, 2, 1.0),
        (2, 3, 1.0),
        (0, 3, 1.0),
        (0, 2, 2.0),
        (1, 3, 2.0),
    ),
    threshold=2.0,
    scales=(1.0, 2.0),
    minimum_degrees=(3, 2, 1, 0),
    rectangles=((0, 2, 1, 2),),
    regions=(
        (
            (0, 2),
            (1, 1),
            (1, 2),
        ),
    ),
    class_selection=(0, 2, 0),
    circular=False,
)


CASES: tuple[GraphCase, ...] = (TWO_CYCLES, SQUARE_WITH_DIAGONALS)


def write_sparse_graph(case: GraphCase, path: Path) -> None:
    """Write one case in the sparse triplet format consumed by `holos`."""

    lines = [
        f"# source_id={case.source_id}",
        f"# vertex_count={case.vertex_count} is implied by the largest index",
    ]
    lines.extend(f"{u} {v} {weight:.17g}" for u, v, weight in case.edges)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def write_region(grades: Sequence[Grade], path: Path) -> None:
    """Write one connected region as scale and density index rows."""

    lines = ["# scale_index density_index"]
    lines.extend(f"{scale} {density}" for scale, density in grades)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")
