#!/usr/bin/env python3
"""Run the registered circular-coordinate validation study."""

import argparse
import datetime
import hashlib
import importlib.metadata
import os
from pathlib import Path
import platform
import statistics
import subprocess
import time

import numpy as np
from dreimac import CircularCoords
import holos_tda
from ripser import ripser
from scipy.spatial.distance import pdist, squareform

MODULUS = 47
REPETITIONS = 7
RING_SEED = 808
CONTINUATION_CASES = (
    ("small", 0.02, 810, "unique"),
    ("loss", 0.40, 848, "no_extension"),
    ("ambiguity", 1.00, 908, "ambiguous"),
)


def arguments():
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", default="benchmarks/results_circular.md")
    parser.add_argument(
        "--artifacts", default="benchmarks/data/circular_artifacts"
    )
    parser.add_argument(
        "--checker", default=os.environ.get("HOLOS_CHECK", "target/release/holos-check")
    )
    return parser.parse_args()


def version(distribution):
    return importlib.metadata.version(distribution)


def alignment(actual, expected):
    actual_unit = np.exp(2j * np.pi * np.asarray(actual))
    expected_unit = np.exp(1j * np.asarray(expected))
    direct = abs(np.vdot(expected_unit, actual_unit)) / len(actual_unit)
    reverse = abs(np.vdot(np.conj(expected_unit), actual_unit)) / len(actual_unit)
    return float(max(direct, reverse))


def persistent_class(points):
    result = ripser(points, maxdim=1, coeff=MODULUS, do_cocycles=True)
    bars = result["dgms"][1]
    index = int(np.argmax(bars[:, 1] - bars[:, 0]))
    return result, index


def checked_artifact(checker, directory, name, artifact):
    path = directory / f"{name}.hcc"
    path.write_bytes(artifact)
    checked = subprocess.run(
        [checker, str(path)], capture_output=True, text=True, check=True
    )
    return checked.stdout.strip(), hashlib.sha256(artifact).hexdigest()


def timed_coordinate(points, cocycle, scale, other=None):
    durations = []
    output = None
    rows = points.tolist()
    other_rows = None if other is None else other.tolist()
    for _ in range(REPETITIONS):
        started = time.perf_counter()
        output = holos_tda.circular_points(
            rows,
            cocycle.tolist(),
            scale,
            modulus=MODULUS,
            other=other_rows,
        )
        durations.append(time.perf_counter() - started)
    return output, statistics.median(durations), max(durations)


def ring_case(label, count, noise, seed, checker, artifacts):
    theta = np.arange(count) * 2 * np.pi / count
    points = np.column_stack((np.cos(theta), np.sin(theta)))
    if noise:
        points += np.random.default_rng(seed).normal(0, noise, points.shape)
    result, index = persistent_class(points)
    birth, death = result["dgms"][1][index]
    scale = float((birth + death) / 2)
    cocycle = result["cocycles"][1][index]
    distances = squareform(pdist(points))
    inactive = sum(distances[int(u), int(v)] > scale for u, v, _ in cocycle)
    output, median, maximum = timed_coordinate(points, cocycle, scale)
    holos_alignment = alignment(output["coordinate"]["phase"], theta)
    dreimac_phase = CircularCoords(
        points, n_landmarks=count, prime=MODULUS, verbose=False
    ).get_coordinates(perc=0.5)
    dreimac_alignment = alignment(dreimac_phase / (2 * np.pi), theta)
    if inactive == 0 or holos_alignment < 0.98 or dreimac_alignment < 0.98:
        raise RuntimeError(f"{label} ring validation failed")
    checked, digest = checked_artifact(
        checker, artifacts, label, output["artifact"]
    )
    return {
        "label": label,
        "vertices": count,
        "birth": float(birth),
        "death": float(death),
        "scale": scale,
        "raw_terms": len(cocycle),
        "inactive_terms": inactive,
        "alignment": holos_alignment,
        "dreimac_alignment": dreimac_alignment,
        "residual": output["coordinate"]["relative_residual"],
        "bytes": len(output["artifact"]),
        "median": median,
        "maximum": maximum,
        "digest": digest,
        "checker": checked,
        "artifact": output["artifact"],
    }


def torus_case(checker, artifacts):
    side = 12
    u = np.repeat(np.arange(side) * 2 * np.pi / side, side)
    v = np.tile(np.arange(side) * 2 * np.pi / side, side)
    points = np.column_stack((np.cos(u), np.sin(u), np.cos(v), np.sin(v)))
    result = ripser(points, maxdim=1, coeff=MODULUS, do_cocycles=True)
    bars = result["dgms"][1]
    indices = np.argsort(-(bars[:, 1] - bars[:, 0]))[:2]
    rows = []
    phases = []
    for rank, index in enumerate(indices):
        birth, death = bars[index]
        scale = float(birth + 0.45 * (death - birth))
        cocycle = result["cocycles"][1][index]
        output, median, maximum = timed_coordinate(points, cocycle, scale)
        checked, digest = checked_artifact(
            checker, artifacts, f"torus-{rank}", output["artifact"]
        )
        phase = output["coordinate"]["phase"]
        phases.append(phase)
        rows.append(
            {
                "rank": rank,
                "birth": float(birth),
                "death": float(death),
                "scale": scale,
                "u": alignment(phase, u),
                "v": alignment(phase, v),
                "residual": output["coordinate"]["relative_residual"],
                "bytes": len(output["artifact"]),
                "median": median,
                "maximum": maximum,
                "digest": digest,
                "checker": checked,
            }
        )
    assignment = max(
        min(rows[0]["u"], rows[1]["v"]), min(rows[0]["v"], rows[1]["u"])
    )
    if assignment < 1 - 1e-12:
        raise RuntimeError("torus coordinate assignment failed")
    return rows, assignment


def continuation_cases(checker, artifacts):
    count = 64
    theta = np.arange(count) * 2 * np.pi / count
    old = np.column_stack((np.cos(theta), np.sin(theta)))
    result, index = persistent_class(old)
    birth, death = result["dgms"][1][index]
    scale = float(birth + 0.45 * (death - birth))
    cocycle = result["cocycles"][1][index]
    rows = []
    for label, sigma, seed, expected in CONTINUATION_CASES:
        new = old + np.random.default_rng(seed).normal(0, sigma, old.shape)
        output, median, maximum = timed_coordinate(old, cocycle, scale, new)
        if output["continuation"] != expected:
            raise RuntimeError(
                f"continuation {label} was {output['continuation']}, expected {expected}"
            )
        checked, digest = checked_artifact(
            checker, artifacts, f"continuation-{label}", output["artifact"]
        )
        rows.append(
            {
                "label": label,
                "sigma": sigma,
                "seed": seed,
                "status": output["continuation"],
                "ambiguity": output["ambiguity_rank"],
                "median": median,
                "maximum": maximum,
                "bytes": len(output["artifact"]),
                "digest": digest,
                "checker": checked,
            }
        )
    return rows


def corruption_case(checker, directory, artifact):
    changed = bytearray(artifact)
    changed[len(changed) // 2] ^= 1
    path = directory / "corrupt.hcc"
    path.write_bytes(changed)
    checked = subprocess.run([checker, str(path)], capture_output=True, text=True)
    if checked.returncode == 0:
        raise RuntimeError("the checker accepted a changed artifact")
    return checked.stderr.strip()


def write_record(path, ring_rows, torus_rows, torus_assignment, continuations, corruption):
    affinity = ",".join(str(cpu) for cpu in sorted(os.sched_getaffinity(0)))
    commit = subprocess.run(
        ["git", "rev-parse", "HEAD"], capture_output=True, text=True, check=True
    ).stdout.strip()
    lines = [
        "# Circular-coordinate validation record",
        "",
        f"- UTC date: {datetime.datetime.now(datetime.timezone.utc).isoformat()}",
        f"- Commit: `{commit}`",
        f"- Python: `{platform.python_version()}`",
        f"- holos-tda: `{version('holos-tda')}`",
        f"- NumPy: `{version('numpy')}`",
        f"- SciPy: `{version('scipy')}`",
        f"- ripser.py: `{version('ripser')}`",
        f"- DREiMac: `{version('dreimac')}`",
        f"- CPU affinity: `{affinity}`",
        f"- Timed repetitions: {REPETITIONS}",
        f"- Prime: {MODULUS}",
        "",
        "## Ring recovery",
        "",
        "| case | n | birth | death | scale | raw terms | inactive at scale | holos alignment | DREiMac alignment | relative residual | artifact bytes | median s | maximum s |",
        "|:--|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|",
    ]
    for row in ring_rows:
        lines.append(
            f"| {row['label']} | {row['vertices']} | {row['birth']:.9g} | {row['death']:.9g} | {row['scale']:.9g} | {row['raw_terms']} | {row['inactive_terms']} | {row['alignment']:.15g} | {row['dreimac_alignment']:.15g} | {row['residual']:.9g} | {row['bytes']} | {row['median']:.9g} | {row['maximum']:.9g} |"
        )
    lines.extend(
        [
            "",
            "## Flat torus recovery",
            "",
            "| class | birth | death | scale | u alignment | v alignment | relative residual | artifact bytes | median s | maximum s |",
            "|--:|--:|--:|--:|--:|--:|--:|--:|--:|--:|",
        ]
    )
    for row in torus_rows:
        lines.append(
            f"| {row['rank']} | {row['birth']:.9g} | {row['death']:.9g} | {row['scale']:.9g} | {row['u']:.15g} | {row['v']:.15g} | {row['residual']:.9g} | {row['bytes']} | {row['median']:.9g} | {row['maximum']:.9g} |"
        )
    lines.extend(
        [
            "",
            f"The best one-to-one latent-angle assignment has minimum alignment {torus_assignment:.15g}.",
            "",
            "## Continuation classification",
            "",
            "| case | noise sigma | seed | status | ambiguity rank | artifact bytes | median s | maximum s |",
            "|:--|--:|--:|:--|--:|--:|--:|--:|",
        ]
    )
    for row in continuations:
        lines.append(
            f"| {row['label']} | {row['sigma']:.2f} | {row['seed']} | {row['status']} | {row['ambiguity']} | {row['bytes']} | {row['median']:.9g} | {row['maximum']:.9g} |"
        )
    lines.extend(
        [
            "",
            "Every artifact in the tables passed `holos-check`. The checker rejected a one-byte content change:",
            "",
            f"`{corruption}`",
            "",
            "Artifact SHA-256 values",
            "",
        ]
    )
    for row in ring_rows + torus_rows + continuations:
        lines.append(f"- `{row['digest']}`")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main():
    args = arguments()
    output = Path(args.output)
    artifacts = Path(args.artifacts)
    artifacts.mkdir(parents=True, exist_ok=True)
    exact = ring_case("ring-exact", 64, 0.0, RING_SEED, args.checker, artifacts)
    noisy = ring_case("ring-noisy", 128, 0.05, RING_SEED, args.checker, artifacts)
    torus, assignment = torus_case(args.checker, artifacts)
    continuations = continuation_cases(args.checker, artifacts)
    corruption = corruption_case(args.checker, artifacts, exact["artifact"])
    output.parent.mkdir(parents=True, exist_ok=True)
    write_record(output, [exact, noisy], torus, assignment, continuations, corruption)
    print(output)


if __name__ == "__main__":
    main()
