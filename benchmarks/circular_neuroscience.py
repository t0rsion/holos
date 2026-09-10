#!/usr/bin/env python3
"""Validate circular coordinates on the Gardner grid-cell recording."""

import argparse
import datetime
import hashlib
import importlib.metadata
import os
import platform
import statistics
import subprocess
import sys
import tempfile
import time
import zipfile
from pathlib import Path

import holos_tda
import numpy as np
from ripser import ripser
from scipy import sparse
from scipy.sparse.linalg import lsmr
from scipy.spatial.distance import pdist, squareform
from sklearn import preprocessing

ARCHIVE_MD5 = "379bfdca61cd54d5f58cab9d3ba477de"
SOURCE_COMMIT = "ce920c1d849edc6b6eb40dc091f38f27b1226009"
CONJUNCTIVE_SHA256 = "9e1dbe4c60fad90e8b1ee1eb8394da4d4911bc9d6ef17fe2045b0430a1d475f8"
MEMBER = "Toroidal_topology_grid_cell_data/rat_r_day1_grid_modules_1_2_3.npz"
MODULUS = 47
SAMPLE_COUNT = 400
ACTIVE_TIMES = 15_000
SAMPLE_STEP = 5
PCA_DIMENSION = 6
DENOISING_NEIGHBORS = 1_000
FUZZY_NEIGHBORS = 400
DECODE_FRACTION = 0.99
REPETITIONS = 5
CELL_DROP_SEED = 1808
CELL_DROP_FRACTIONS = (0.02, 0.05)


def arguments():
    parser = argparse.ArgumentParser()
    parser.add_argument("--archive", required=True)
    parser.add_argument("--source", required=True)
    parser.add_argument(
        "--output", default="benchmarks/results_circular_neuroscience.md"
    )
    parser.add_argument(
        "--artifacts", default="benchmarks/data/circular_neuroscience_artifacts"
    )
    parser.add_argument(
        "--checker", default=os.environ.get("HOLOS_CHECK", "target/release/holos-check")
    )
    return parser.parse_args()


def file_digest(path, algorithm):
    digest = hashlib.new(algorithm)
    with open(path, "rb") as source:
        while block := source.read(1024 * 1024):
            digest.update(block)
    return digest.hexdigest()


def load_source(source):
    commit = subprocess.run(
        ["git", "-C", source, "rev-parse", "HEAD"],
        capture_output=True,
        text=True,
        check=True,
    ).stdout.strip()
    if commit != SOURCE_COMMIT:
        raise RuntimeError(f"analysis source is {commit}, expected {SOURCE_COMMIT}")
    conjunctive = Path(source) / "is_conjunctive_all.npz"
    if file_digest(conjunctive, "sha256") != CONJUNCTIVE_SHA256:
        raise RuntimeError("conjunctive-cell flags have the wrong SHA-256")
    sys.path.insert(0, source)
    np.str = str
    previous = os.getcwd()
    os.chdir(source)
    try:
        import utils
    finally:
        os.chdir(previous)
    return utils


def prepare_recording(archive, source, utils):
    if file_digest(archive, "md5") != ARCHIVE_MD5:
        raise RuntimeError("Gardner archive has the wrong MD5")
    with tempfile.TemporaryDirectory(prefix="holos-gardner-") as temporary:
        with zipfile.ZipFile(archive) as zipped:
            zipped.extract(MEMBER, temporary)
        folder = str(Path(temporary) / "Toroidal_topology_grid_cell_data") + "/"
        previous = os.getcwd()
        os.chdir(source)
        try:
            spikes, x, y, azimuth, sample_time = utils.get_spikes(
                "R",
                "2",
                "day1",
                "OF",
                bType="pure",
                bSmooth=True,
                bSpeed=True,
                folder=folder,
            )
        finally:
            os.chdir(previous)
    candidate_times = np.arange(0, spikes.shape[0], SAMPLE_STEP)
    activity = np.sum(spikes[candidate_times, :], axis=1)
    selected = np.sort(np.argsort(activity)[-ACTIVE_TIMES:])
    active = candidate_times[selected]
    scaled = preprocessing.scale(spikes[active, :])
    reduced, _, _ = utils.pca(scaled, dim=PCA_DIMENSION)
    reduced = np.asarray(reduced.real, dtype=float)
    sample, _, _ = utils.sample_denoising(
        reduced,
        k=DENOISING_NEIGHBORS,
        num_sample=SAMPLE_COUNT,
        omega=1,
        metric="cosine",
    )
    source_indices = active[sample]
    distance = fuzzy_distance(reduced[sample, :], utils)
    return {
        "spikes": spikes,
        "active_times": active,
        "landmark_positions": sample,
        "source_indices": source_indices,
        "distance": distance,
        "x": x[source_indices],
        "y": y[source_indices],
        "azimuth": azimuth[source_indices],
        "sample_time": sample_time[source_indices],
    }


def fuzzy_distance(points, utils):
    distances = squareform(pdist(points, "cosine"))
    neighbors = min(FUZZY_NEIGHBORS, len(points))
    indices = np.argsort(distances, axis=1)[:, :neighbors]
    neighbor_distances = distances[np.arange(len(points))[:, None], indices].copy()
    sigmas, rhos = utils.smooth_knn_dist(
        neighbor_distances, neighbors, local_connectivity=0
    )
    rows, cols, values = utils.compute_membership_strengths(
        indices, neighbor_distances, sigmas, rhos
    )
    membership = sparse.coo_matrix(
        (values, (rows, cols)), shape=(len(points), len(points))
    )
    membership.eliminate_zeros()
    transpose = membership.transpose()
    membership = membership + transpose - membership.multiply(transpose)
    membership.eliminate_zeros()
    with np.errstate(divide="ignore"):
        result = -np.log(membership.toarray())
    np.fill_diagonal(result, 0)
    return result


def active_counts(distance, scale):
    adjacency = (distance <= scale) & (distance > 0)
    edges = int(np.sum(np.triu(adjacency, 1)))
    integer_adjacency = adjacency.astype(np.int64)
    triangles = int(
        np.trace(integer_adjacency @ integer_adjacency @ integer_adjacency) // 6
    )
    return edges, triangles


def canonical_field_terms(cocycle, distance, scale):
    terms = {}
    for a, b, coefficient in np.asarray(cocycle, dtype=np.int64):
        a = int(a)
        b = int(b)
        if distance[a, b] > scale:
            continue
        edge = (min(a, b), max(a, b))
        value = int(coefficient) if a < b else -int(coefficient)
        terms[edge] = (terms.get(edge, 0) + value) % MODULUS
        if terms[edge] == 0:
            del terms[edge]
    first = terms[min(terms)]
    inverse = pow(first, -1, MODULUS)
    return {edge: value * inverse % MODULUS for edge, value in terms.items()}


def reference_phase(distance, cocycle, scale, multiplier):
    source = canonical_field_terms(cocycle, distance, scale)
    u, v = np.where(np.triu((distance <= scale) & (distance > 0), 1))
    edge_positions = {(int(a), int(b)): index for index, (a, b) in enumerate(zip(u, v))}
    integral = np.zeros(len(u))
    for edge, coefficient in source.items():
        residue = coefficient * multiplier % MODULUS
        integral[edge_positions[edge]] = (
            residue - MODULUS if residue > MODULUS // 2 else residue
        )
    rows = np.repeat(np.arange(len(u)), 2)
    columns = np.column_stack((u, v)).ravel()
    values = np.tile([-1.0, 1.0], len(u))
    coboundary = sparse.coo_matrix(
        (values, (rows, columns)), shape=(len(u), len(distance))
    ).tocsr()
    potential = lsmr(
        coboundary,
        -integral,
        atol=1e-14,
        btol=1e-14,
        maxiter=10_000,
    )[0]
    return potential % 1


def circle_alignment(a, b):
    a = np.exp(2j * np.pi * np.asarray(a))
    b = np.exp(2j * np.pi * np.asarray(b))
    return float(max(abs(np.vdot(a, b)), abs(np.vdot(np.conj(a), b))) / len(a))


def checked_artifact(checker, directory, name, artifact):
    path = directory / f"{name}.hcc"
    path.write_bytes(artifact)
    subprocess.run([checker, str(path)], capture_output=True, text=True, check=True)
    return hashlib.sha256(artifact).hexdigest()


def timed_coordinate(distance, cocycle, scale):
    durations = []
    output = None
    rows = distance.tolist()
    cocycle_rows = np.asarray(cocycle, dtype=np.int64).tolist()
    for _ in range(REPETITIONS):
        started = time.perf_counter()
        output = holos_tda.circular_coordinates(
            rows, cocycle_rows, scale, modulus=MODULUS
        )
        durations.append(time.perf_counter() - started)
    return output, statistics.median(durations), max(durations)


def class_rows(distance, persistence, scale, checker, artifacts):
    bars = persistence["dgms"][1]
    order = np.argsort(-(bars[:, 1] - bars[:, 0]))[:2]
    rows = []
    for rank, index in enumerate(order):
        cocycle = persistence["cocycles"][1][index]
        output, median, maximum = timed_coordinate(distance, cocycle, scale)
        coordinate = output["coordinate"]
        reference = reference_phase(
            distance, cocycle, scale, coordinate["field_multiplier"]
        )
        agreement = circle_alignment(coordinate["phase"], reference)
        if agreement < 1 - 1e-12:
            raise RuntimeError(f"real class {rank} differs from the SciPy reference")
        digest = checked_artifact(
            checker, artifacts, f"gardner-r2-class-{rank}", output["artifact"]
        )
        rows.append(
            {
                "rank": rank,
                "index": int(index),
                "birth": float(bars[index, 0]),
                "death": float(bars[index, 1]),
                "agreement": agreement,
                "residual": coordinate["relative_residual"],
                "divisibility": coordinate["divisibility"],
                "bytes": len(output["artifact"]),
                "median": median,
                "maximum": maximum,
                "digest": digest,
            }
        )
    return rows, order


def cell_drop_rows(prepared, utils, persistence, order, scale, checker, artifacts):
    rng = np.random.default_rng(CELL_DROP_SEED)
    cell_order = rng.permutation(prepared["spikes"].shape[1])
    rows = []
    for fraction in CELL_DROP_FRACTIONS:
        drop = round(fraction * len(cell_order))
        keep = np.sort(cell_order[drop:])
        selected = prepared["spikes"][prepared["active_times"]][:, keep]
        reduced, _, _ = utils.pca(preprocessing.scale(selected), dim=PCA_DIMENSION)
        landmarks = np.asarray(reduced.real, dtype=float)[
            prepared["landmark_positions"]
        ]
        changed = fuzzy_distance(landmarks, utils)
        changed_edges, _ = active_counts(changed, scale)
        for rank, index in enumerate(order):
            started = time.perf_counter()
            output = holos_tda.circular_coordinates(
                prepared["distance"].tolist(),
                np.asarray(persistence["cocycles"][1][index], dtype=np.int64).tolist(),
                scale,
                modulus=MODULUS,
                other=changed.tolist(),
            )
            elapsed = time.perf_counter() - started
            digest = checked_artifact(
                checker,
                artifacts,
                f"gardner-r2-drop-{drop}-class-{rank}",
                output["artifact"],
            )
            rows.append(
                {
                    "fraction": fraction,
                    "dropped": drop,
                    "remaining": len(keep),
                    "class": rank,
                    "edges": changed_edges,
                    "status": output["continuation"],
                    "ambiguity": output["ambiguity_rank"],
                    "seconds": elapsed,
                    "bytes": len(output["artifact"]),
                    "digest": digest,
                }
            )
    return rows


def write_record(path, elapsed, prepared, bars, scale, edges, triangles, classes, drops):
    commit = subprocess.run(
        ["git", "rev-parse", "HEAD"], capture_output=True, text=True, check=True
    ).stdout.strip()
    affinity = ",".join(str(cpu) for cpu in sorted(os.sched_getaffinity(0)))
    lines = [
        "# Gardner R2 circular-coordinate validation record",
        "",
        f"- UTC date: {datetime.datetime.now(datetime.timezone.utc).isoformat()}",
        f"- Commit: `{commit}`",
        "- Data DOI: `10.6084/m9.figshare.16764508.v6`",
        f"- Archive MD5: `{ARCHIVE_MD5}`",
        f"- Analysis source commit: `{SOURCE_COMMIT}`",
        f"- Conjunctive flags SHA-256: `{CONJUNCTIVE_SHA256}`",
        f"- Python: `{platform.python_version()}`",
        f"- holos-tda: `{importlib.metadata.version('holos-tda')}`",
        f"- NumPy: `{importlib.metadata.version('numpy')}`",
        f"- SciPy: `{importlib.metadata.version('scipy')}`",
        f"- ripser.py: `{importlib.metadata.version('ripser')}`",
        f"- scikit-learn: `{importlib.metadata.version('scikit-learn')}`",
        f"- CPU affinity: `{affinity}`",
        f"- Total study time: {elapsed:.9g} s",
        "",
        "The input is rat R, module 2, day 1, open field. The pipeline uses the authors' tagged preprocessing code. It keeps pure cells, 10 ms bins, 50 ms smoothing, the 2.5 cm/s speed filter, six principal components, the cosine metric, and prime 47.",
        "",
        (
            f"The study examines every {SAMPLE_STEP}th bin and retains the "
            f"{ACTIVE_TIMES} bins with the largest population activity. The "
            f"authors' density sampler uses {DENOISING_NEIGHBORS} neighbors "
            f"and omega 1. The study uses {SAMPLE_COUNT} landmarks instead of "
            "the paper's 1,200. This tractability change sets the "
            f"fuzzy-neighbor count to {FUZZY_NEIGHBORS}. The study does not "
            "reproduce the paper's population analysis."
        ),
        "",
        f"The filtered activity matrix has {prepared['spikes'].shape[0]} time bins and {prepared['spikes'].shape[1]} pure cells. The fixed graph has {edges} edges and {triangles} triangles at scale {scale:.15g}.",
        "",
        "## Longest H1 intervals",
        "",
        "| order | birth | death | persistence |",
        "|--:|--:|--:|--:|",
    ]
    for rank, (birth, death) in enumerate(bars[:10]):
        lines.append(
            f"| {rank} | {birth:.9g} | {death:.9g} | {death - birth:.9g} |"
        )
    lines.extend(
        [
            "",
            "## Coordinate checks",
            "",
            "| class | birth | death | SciPy unit-circle alignment | relative residual | divisibility | artifact bytes | median s | maximum s |",
            "|--:|--:|--:|--:|--:|--:|--:|--:|--:|",
        ]
    )
    for row in classes:
        lines.append(
            f"| {row['rank']} | {row['birth']:.9g} | {row['death']:.9g} | {row['agreement']:.15g} | {row['residual']:.9g} | {row['divisibility']} | {row['bytes']} | {row['median']:.9g} | {row['maximum']:.9g} |"
        )
    lines.extend(
        [
            "",
            "## Cell-drop continuation",
            "",
            f"The cell order uses seed {CELL_DROP_SEED}. Calibration used another seed before these fractions were frozen.",
            "Each cell-drop case re-estimates the standardized PCA on the same active time bins and reuses the fixed landmark times.",
            "",
            "| dropped fraction | dropped cells | remaining cells | class | new edges | status | ambiguity rank | seconds | artifact bytes |",
            "|--:|--:|--:|--:|--:|:--|--:|--:|--:|",
        ]
    )
    for row in drops:
        lines.append(
            f"| {row['fraction']:.2f} | {row['dropped']} | {row['remaining']} | {row['class']} | {row['edges']} | {row['status']} | {row['ambiguity']} | {row['seconds']:.9g} | {row['bytes']} |"
        )
    lines.extend(["", "Every listed artifact passed `holos-check`.", ""])
    for row in classes + drops:
        lines.append(f"- `{row['digest']}`")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main():
    args = arguments()
    started = time.perf_counter()
    source = str(Path(args.source).resolve())
    utils = load_source(source)
    prepared = prepare_recording(str(Path(args.archive).resolve()), source, utils)
    persistence = ripser(
        prepared["distance"],
        maxdim=1,
        coeff=MODULUS,
        do_cocycles=True,
        distance_matrix=True,
    )
    bars = persistence["dgms"][1]
    order = np.argsort(-(bars[:, 1] - bars[:, 0]))
    ordered_bars = bars[order]
    scale = float(
        ordered_bars[1, 0]
        + DECODE_FRACTION * (ordered_bars[1, 1] - ordered_bars[1, 0])
    )
    edges, triangles = active_counts(prepared["distance"], scale)
    artifacts = Path(args.artifacts)
    artifacts.mkdir(parents=True, exist_ok=True)
    classes, selected_order = class_rows(
        prepared["distance"], persistence, scale, args.checker, artifacts
    )
    drops = cell_drop_rows(
        prepared,
        utils,
        persistence,
        selected_order,
        scale,
        args.checker,
        artifacts,
    )
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    write_record(
        output,
        time.perf_counter() - started,
        prepared,
        ordered_bars,
        scale,
        edges,
        triangles,
        classes,
        drops,
    )
    print(output)


if __name__ == "__main__":
    main()
