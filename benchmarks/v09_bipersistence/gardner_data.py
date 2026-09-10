"""Prepare the frozen Gardner input for the degree-Rips study."""

from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass
from pathlib import Path

import numpy as np
from ripser import ripser

from benchmarks.circular_neuroscience import (
    ARCHIVE_MD5,
    MEMBER,
    MODULUS,
    SOURCE_COMMIT,
    canonical_field_terms,
    load_source,
    prepare_recording,
)

SCALES = (1.0, 2.0, 4.88695400387049)
MINIMUM_DEGREES = (20, 10, 0)
REGION = ((0, 1), (1, 0), (1, 1), (1, 2), (2, 1))
EXPECTED_NODE_RANKS = (0, 8, 13, 4, 4, 4, 2, 2, 2)
EXPECTED_LONGEST_BARS = (
    (0.21474279463291168, 5.4620819091796875),
    (0.3258897364139557, 4.933025360107422),
)
EXPECTED_EDGE_COUNT = 12_092
CLASS_COUNT = 2


@dataclass(frozen=True)
class GardnerInput:
    """Paths and provenance for one prepared 400-landmark input."""

    graph: Path
    region: Path
    cocycles: tuple[Path, ...]
    metadata: dict[str, object]


def prepare(archive: Path, source: Path, directory: Path) -> GardnerInput:
    """Rebuild the 400-landmark graph and two longest H1 cocycles."""

    directory.mkdir(parents=True, exist_ok=True)
    utils = load_source(str(source))
    prepared = prepare_recording(str(archive), str(source), utils)
    distance = prepared["distance"]
    persistence = ripser(
        distance,
        maxdim=1,
        coeff=MODULUS,
        do_cocycles=True,
        distance_matrix=True,
    )
    bars = persistence["dgms"][1]
    order = np.argsort(-(bars[:, 1] - bars[:, 0]))[:CLASS_COUNT]
    longest = tuple((float(bars[i, 0]), float(bars[i, 1])) for i in order)
    if not np.allclose(longest, EXPECTED_LONGEST_BARS, rtol=0.0, atol=1e-12):
        raise RuntimeError(f"longest H1 intervals changed: {longest!r}")

    graph = directory / "gardner.sparse"
    edge_count = _write_graph(distance, graph)
    if edge_count != EXPECTED_EDGE_COUNT:
        raise RuntimeError(
            f"Gardner graph has {edge_count} edges, expected {EXPECTED_EDGE_COUNT}"
        )
    region = directory / "region.txt"
    region.write_text(
        "# scale_index density_index\n"
        + "".join(f"{scale} {density}\n" for scale, density in REGION),
        encoding="utf-8",
    )
    cocycles = []
    for rank, index in enumerate(order):
        path = directory / f"class-{rank}.cocycle"
        terms = canonical_field_terms(
            persistence["cocycles"][1][index], distance, SCALES[0]
        )
        path.write_text(
            "".join(f"{u} {v} {value}\n" for (u, v), value in sorted(terms.items())),
            encoding="utf-8",
        )
        cocycles.append(path)

    metadata = {
        "data_doi": "10.6084/m9.figshare.16764508.v6",
        "archive_member": MEMBER,
        "archive_md5": ARCHIVE_MD5,
        "source_commit": SOURCE_COMMIT,
        "landmarks": int(distance.shape[0]),
        "cells": int(prepared["spikes"].shape[1]),
        "edges": edge_count,
        "longest_h1_intervals": longest,
        "graph_sha256": _sha256(graph),
        "cocycle_sha256": [_sha256(path) for path in cocycles],
    }
    (directory / "input.json").write_text(
        json.dumps(metadata, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return GardnerInput(graph, region, tuple(cocycles), metadata)


def _write_graph(distance: np.ndarray, path: Path) -> int:
    lines = []
    threshold = SCALES[-1]
    for u in range(len(distance)):
        for v in range(u + 1, len(distance)):
            value = float(distance[u, v])
            if np.isfinite(value) and value <= threshold:
                lines.append(f"{u} {v} {value:.17g}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return len(lines)


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()
