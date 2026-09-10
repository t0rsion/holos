"""Read the frozen HOLOSTEM1 trajectory and build its fixed Rips complex."""

from __future__ import annotations

import math
from dataclasses import dataclass, replace
from itertools import pairwise
from pathlib import Path


@dataclass(frozen=True)
class Edge:
    """One canonical edge in the fixed graph."""

    u: int
    v: int


@dataclass(frozen=True)
class Trajectory:
    """A validated sequence of edge-weight snapshots."""

    dataset: str
    source_sha256: str
    vertices: int
    edges: tuple[Edge, ...]
    snapshot_times: tuple[int, ...]
    weights: tuple[tuple[float, ...], ...]


@dataclass(frozen=True)
class _Header:
    """Validated trajectory fields needed to decode snapshots."""

    dataset: str
    source_sha256: str
    vertices: int
    edge_count: int
    snapshot_count: int


@dataclass(frozen=True)
class Complex:
    """The fixed vertices, edges, and induced triangles used by both arms."""

    simplices: tuple[tuple[int, ...], ...]
    dimensions: tuple[int, ...]
    edge_positions: dict[tuple[int, int], int]


class _Tokens:
    def __init__(self, path: Path) -> None:
        self.values = path.read_text(encoding="ascii").split()
        self.cursor = 0

    def take(self, label: str) -> str:
        if self.cursor >= len(self.values):
            raise ValueError(f"missing {label}")
        value = self.values[self.cursor]
        self.cursor += 1
        return value

    def expect(self, label: str) -> None:
        actual = self.take(label)
        if actual != label:
            raise ValueError(f"expected {label}, found {actual}")

    def integer(self, label: str) -> int:
        value = self.take(label)
        try:
            parsed = int(value, 10)
        except ValueError as error:
            raise ValueError(f"invalid {label}: {value}") from error
        if parsed < 0:
            raise ValueError(f"invalid {label}: {value}")
        return parsed

    def real(self, label: str) -> float:
        value = self.take(label)
        try:
            parsed = float(value)
        except ValueError as error:
            raise ValueError(f"invalid {label}: {value}") from error
        if not math.isfinite(parsed) or parsed < 0:
            raise ValueError(f"invalid {label}: {value}")
        return parsed


def read_trajectory(path: Path) -> Trajectory:
    """Read and validate one deterministic HOLOSTEM1 trajectory."""

    tokens = _Tokens(path)
    header = _read_header(tokens)
    snapshot_times = _read_snapshot_times(tokens, header.snapshot_count)
    edges, weights = _read_edges(tokens, header)
    if tokens.cursor != len(tokens.values):
        raise ValueError("trajectory has trailing fields")
    return Trajectory(
        header.dataset,
        header.source_sha256,
        header.vertices,
        tuple(edges),
        snapshot_times,
        tuple(tuple(snapshot) for snapshot in weights),
    )


def _read_header(tokens: _Tokens) -> _Header:
    tokens.expect("HOLOSTEM1")
    tokens.expect("dataset")
    dataset = tokens.take("dataset name")
    tokens.expect("source_sha256")
    source_sha256 = tokens.take("source SHA-256")
    if not _is_sha256(source_sha256):
        raise ValueError("source SHA-256 is not 64 hexadecimal digits")

    vertices, edge_count, snapshot_count = _read_counts(tokens)
    _validate_counts(vertices, edge_count, snapshot_count)
    _read_preprocessing(tokens)
    return _Header(dataset, source_sha256, vertices, edge_count, snapshot_count)


def _is_sha256(value: str) -> bool:
    return len(value) == 64 and all(
        character in "0123456789abcdefABCDEF" for character in value
    )


def _read_counts(tokens: _Tokens) -> tuple[int, int, int]:
    tokens.expect("vertices")
    vertices = tokens.integer("vertex count")
    tokens.expect("edges")
    edge_count = tokens.integer("edge count")
    tokens.expect("snapshots")
    snapshot_count = tokens.integer("snapshot count")
    return vertices, edge_count, snapshot_count


def _validate_counts(vertices: int, edge_count: int, snapshot_count: int) -> None:
    if vertices == 0 or edge_count == 0 or snapshot_count < 2:
        raise ValueError("trajectory counts are outside the benchmark domain")
    if edge_count > 20_000_000 or snapshot_count > 1_000_000:
        raise ValueError("trajectory exceeds the benchmark size limit")
    if edge_count > 20_000_000 // snapshot_count:
        raise ValueError("trajectory exceeds the benchmark cell limit")


def _read_preprocessing(tokens: _Tokens) -> None:

    tokens.expect("bin_seconds")
    if tokens.integer("bin_seconds") == 0:
        raise ValueError("bin_seconds must be positive")
    tokens.expect("warmup_bins")
    if tokens.integer("warmup_bins") == 0:
        raise ValueError("warmup_bins must be positive")
    tokens.expect("decay_numerator")
    decay_numerator = tokens.integer("decay_numerator")
    tokens.expect("decay_denominator")
    decay_denominator = tokens.integer("decay_denominator")
    if decay_denominator == 0 or decay_numerator >= decay_denominator:
        raise ValueError("invalid decay constants")
    tokens.expect("score_scale")
    if tokens.integer("score_scale") == 0:
        raise ValueError("score_scale must be positive")
    tokens.expect("weight_offset")
    weight_offset = tokens.integer("weight_offset")
    if weight_offset == 0:
        raise ValueError("weight_offset must be positive")
    tokens.expect("maximum_score")
    maximum_score = tokens.integer("maximum_score")
    if maximum_score >= weight_offset:
        raise ValueError("maximum_score must be below weight_offset")


def _read_snapshot_times(tokens: _Tokens, snapshot_count: int) -> tuple[int, ...]:
    tokens.expect("snapshot_times")
    snapshot_times = tuple(
        tokens.integer("snapshot time") for _ in range(snapshot_count)
    )
    if any(left >= right for left, right in pairwise(snapshot_times)):
        raise ValueError("snapshot times are not strictly increasing")
    return snapshot_times


def _read_edges(
    tokens: _Tokens, header: _Header
) -> tuple[list[Edge], list[list[float]]]:
    tokens.expect("edge_weights")
    edges: list[Edge] = []
    snapshots = [[] for _ in range(header.snapshot_count)]
    previous: Edge | None = None
    for _ in range(header.edge_count):
        edge = _read_edge(tokens, header.vertices, previous)
        edges.append(edge)
        previous = edge
        _read_edge_weights(tokens, snapshots, edge)
    return edges, snapshots


def _read_edge(tokens: _Tokens, vertices: int, previous: Edge | None) -> Edge:
    edge = Edge(tokens.integer("edge endpoint"), tokens.integer("edge endpoint"))
    if (
        edge.u >= edge.v
        or edge.v >= vertices
        or (previous is not None and (edge.u, edge.v) <= (previous.u, previous.v))
    ):
        raise ValueError("trajectory edges are not canonical")
    return edge


def _read_edge_weights(
    tokens: _Tokens, snapshots: list[list[float]], edge: Edge
) -> None:
    for snapshot in snapshots:
        snapshot.append(tokens.real("edge weight"))


def build_complex(trajectory: Trajectory) -> Complex:
    """Build every induced triangle of the fixed listed graph."""

    simplices, edge_positions, adjacency = _vertices_and_edges(trajectory)
    simplices.extend(_triangles(trajectory.vertices, adjacency))
    dimensions = tuple(len(simplex) - 1 for simplex in simplices)
    return Complex(tuple(simplices), dimensions, edge_positions)


def _vertices_and_edges(
    trajectory: Trajectory,
) -> tuple[list[tuple[int, ...]], dict[tuple[int, int], int], list[list[bool]]]:
    simplices = [(vertex,) for vertex in range(trajectory.vertices)]
    edge_positions: dict[tuple[int, int], int] = {}
    adjacency = [[False] * trajectory.vertices for _ in range(trajectory.vertices)]
    for position, edge in enumerate(trajectory.edges):
        edge_positions[(edge.u, edge.v)] = position
        simplices.append((edge.u, edge.v))
        adjacency[edge.u][edge.v] = True
        adjacency[edge.v][edge.u] = True
    return simplices, edge_positions, adjacency


def _triangles(
    vertices: int, adjacency: list[list[bool]]
) -> list[tuple[int, int, int]]:
    triangles: list[tuple[int, int, int]] = []
    for u in range(vertices):
        for v in range(u + 1, vertices):
            if not adjacency[u][v]:
                continue
            for w in range(v + 1, vertices):
                if adjacency[u][w] and adjacency[v][w]:
                    triangles.append((u, v, w))
    return triangles


def global_rank_trajectory(trajectory: Trajectory) -> Trajectory:
    """Encode all edge weights by exact global ranks for Dionysus' f32 data type."""

    distinct = sorted(
        {weight for snapshot in trajectory.weights for weight in snapshot}
    )
    if len(distinct) >= 2**24:
        raise ValueError("Dionysus rank encoding exceeds the exact f32 integer range")
    ranks = {weight: float(index + 1) for index, weight in enumerate(distinct)}
    weights = tuple(
        tuple(ranks[weight] for weight in snapshot) for snapshot in trajectory.weights
    )
    return replace(trajectory, weights=weights)


def snapshot_values(complex_: Complex, snapshot: tuple[float, ...]) -> list[float]:
    """Return one filtration value for each fixed simplex."""

    values: list[float] = []
    for simplex in complex_.simplices:
        if len(simplex) == 1:
            values.append(0.0)
            continue
        diameter = 0.0
        for left, u in enumerate(simplex):
            for v in simplex[left + 1 :]:
                try:
                    edge_position = complex_.edge_positions[(u, v)]
                except KeyError as error:
                    raise ValueError(
                        f"complex contains missing edge {(u, v)}"
                    ) from error
                diameter = max(diameter, snapshot[edge_position])
        values.append(diameter)
    return values
