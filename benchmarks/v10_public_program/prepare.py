"""Prepare the frozen weekly recency filtration."""

from __future__ import annotations

import gzip
from collections import Counter
from pathlib import Path

MAGIC = "HOLOSTEM1"


def prepare(source: Path, output: Path, entry: dict) -> dict[str, int]:
    events = read_events(source)
    validate_raw(events, entry)
    labels, normalized = normalize_events(events)
    edges = sorted({(u, v) for u, v, _ in normalized})
    counts = event_counts(normalized, edges, entry)
    snapshots, snapshot_times, maximum_score = score_snapshots(
        counts, len(edges), entry
    )
    weights = build_weights(edges, snapshots, entry, maximum_score)
    write_trajectory(
        output,
        entry,
        len(labels),
        edges,
        snapshot_times,
        weights,
        maximum_score,
    )
    return {
        "vertices": len(labels),
        "edges": len(edges),
        "snapshots": len(snapshots),
        "maximum_score": maximum_score,
    }


def normalize_events(
    events: list[tuple[int, int, int]],
) -> tuple[list[int], list[tuple[int, int, int]]]:
    labels = sorted({value for u, v, _ in events for value in (u, v)})
    relabel = {label: index for index, label in enumerate(labels)}
    normalized = [
        (min(relabel[u], relabel[v]), max(relabel[u], relabel[v]), timestamp)
        for u, v, timestamp in events
    ]
    return labels, normalized


def event_counts(
    normalized: list[tuple[int, int, int]],
    edges: list[tuple[int, int]],
    entry: dict,
) -> list[Counter[int]]:
    edge_position = {edge: index for index, edge in enumerate(edges)}
    last_bin = entry["last_timestamp"] // entry["bin_seconds"]
    counts = [Counter() for _ in range(last_bin + 1)]
    for u, v, timestamp in normalized:
        counts[timestamp // entry["bin_seconds"]][edge_position[(u, v)]] += 1
    return counts


def score_snapshots(
    counts: list[Counter[int]], edge_count: int, entry: dict
) -> tuple[list[list[int]], list[int], int]:
    scores = [0] * edge_count
    snapshots: list[list[int]] = []
    snapshot_times = []
    maximum_score = 0
    for bin_index, bin_counts in enumerate(counts):
        for edge_index in range(edge_count):
            scores[edge_index] = (
                entry["decay_numerator"] * scores[edge_index]
            ) // entry["decay_denominator"]
            scores[edge_index] += entry["score_scale"] * bin_counts[edge_index]
            maximum_score = max(maximum_score, scores[edge_index])
        if bin_index + 1 >= entry["warmup_bins"]:
            snapshots.append(list(scores))
            snapshot_times.append((bin_index + 1) * entry["bin_seconds"])
    return snapshots, snapshot_times, maximum_score


def build_weights(
    edges: list[tuple[int, int]],
    snapshots: list[list[int]],
    entry: dict,
    maximum_score: int,
) -> list[list[int]]:
    if maximum_score >= entry["weight_offset"]:
        raise SystemExit("weight_offset does not exceed the largest activity score")
    tie_base = len(edges) + 1
    weights = [
        [
            (entry["weight_offset"] - snapshot[edge_index]) * tie_base + edge_index
            for snapshot in snapshots
        ]
        for edge_index in range(len(edges))
    ]
    if max(max(row) for row in weights) >= 1 << 53:
        raise SystemExit("prepared weights are not exact f64 integers")
    return weights


def read_events(source: Path) -> list[tuple[int, int, int]]:
    try:
        with gzip.open(source, "rt", encoding="ascii") as stream:
            events = []
            for line_number, line in enumerate(stream, 1):
                if not line.strip():
                    continue
                fields = line.split()
                if len(fields) != 3:
                    raise SystemExit(
                        f"source row {line_number} does not contain three integers"
                    )
                events.append(tuple(map(int, fields)))
            return events
    except (OSError, ValueError) as error:
        raise SystemExit(f"cannot read temporal edge archive: {error}") from error


def validate_raw(events: list[tuple[int, int, int]], entry: dict) -> None:
    if not events:
        raise SystemExit("source contains no temporal edges")
    observed = observed_values(events)
    check_observed(observed, entry)
    check_event_values(events)


def observed_values(events: list[tuple[int, int, int]]) -> dict[str, int]:
    nodes = {value for u, v, _ in events for value in (u, v)}
    edges = {tuple(sorted((u, v))) for u, v, _ in events}
    timestamps = [timestamp for _, _, timestamp in events]
    return {
        "raw_events": len(events),
        "raw_vertices": len(nodes),
        "raw_undirected_edges": len(edges),
        "first_timestamp": min(timestamps),
        "last_timestamp": max(timestamps),
    }


def check_observed(observed: dict[str, int], entry: dict) -> None:
    for key, value in observed.items():
        if value != entry[key]:
            raise SystemExit(f"source {key} is {value}, expected {entry[key]}")


def check_event_values(events: list[tuple[int, int, int]]) -> None:
    if any(u == v for u, v, _ in events):
        raise SystemExit("source contains a self-loop")
    if any(timestamp < 0 for _, _, timestamp in events):
        raise SystemExit("source contains a negative timestamp")


def write_trajectory(
    output: Path,
    entry: dict,
    vertices: int,
    edges: list[tuple[int, int]],
    snapshot_times: list[int],
    weights: list[list[int]],
    maximum_score: int,
) -> None:
    lines = [
        MAGIC,
        f"dataset {entry['id']}",
        f"source_sha256 {entry['source_sha256']}",
        f"vertices {vertices}",
        f"edges {len(edges)}",
        f"snapshots {len(snapshot_times)}",
        f"bin_seconds {entry['bin_seconds']}",
        f"warmup_bins {entry['warmup_bins']}",
        f"decay_numerator {entry['decay_numerator']}",
        f"decay_denominator {entry['decay_denominator']}",
        f"score_scale {entry['score_scale']}",
        f"weight_offset {entry['weight_offset']}",
        f"maximum_score {maximum_score}",
        "snapshot_times " + " ".join(map(str, snapshot_times)),
        "edge_weights",
    ]
    lines.extend(
        f"{u} {v} " + " ".join(map(str, edge_weights))
        for (u, v), edge_weights in zip(edges, weights, strict=True)
    )
    output.write_text("\n".join(lines) + "\n")
