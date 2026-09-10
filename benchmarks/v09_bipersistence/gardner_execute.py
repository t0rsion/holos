"""Run and check the frozen Gardner degree-Rips queries."""

from __future__ import annotations

import hashlib
import json
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .execute import maximum, median_upper
from .gardner_data import (
    EXPECTED_NODE_RANKS,
    MINIMUM_DEGREES,
    REGION,
    SCALES,
    GardnerInput,
)

EXPECTED_EXTENSIONS = (
    ((0, 1), "unique", 0),
    ((0, 2), "ambiguous", 7),
    ((1, 1), "ambiguous", 2),
    ((1, 2), "ambiguous", 2),
    ((2, 1), "unique", 0),
    ((2, 2), "unique", 0),
)


@dataclass(frozen=True)
class GardnerResult:
    """Checked structural evidence and timings for the Gardner input."""

    report: dict[str, Any]
    producer_times_ns: tuple[int, ...]
    checker_times_ns: tuple[int, ...]
    artifact_sha256: str
    report_sha256: str
    artifact_bytes: int
    corruption_rejected: bool


def run(
    prepared: GardnerInput,
    holos: Path,
    checker: Path,
    repetitions: int,
    directory: Path,
) -> GardnerResult:
    """Run repeated producer and checker calls and enforce all frozen claims."""

    if repetitions < 1:
        raise ValueError("repetitions must be positive")
    directory.mkdir(parents=True, exist_ok=True)
    producer_times = []
    checker_times = []
    artifact_digest = None
    report_digest = None
    report = None
    final_artifact = None
    for repetition in range(repetitions):
        artifact = directory / f"gardner-{repetition}.holosbp"
        report_path = directory / f"gardner-{repetition}.json"
        command = _command(prepared, holos, artifact, report_path)
        elapsed, completed = _timed(command)
        _require_success(completed, "holos bipersistence")
        producer_times.append(elapsed)
        elapsed, completed = _timed([str(checker), str(artifact)])
        _require_success(completed, "holos-check")
        checker_times.append(elapsed)
        report = json.loads(report_path.read_text(encoding="utf-8"))
        _validate(report)
        current_artifact_digest = _sha256(artifact)
        current_report_digest = _sha256(report_path)
        if artifact_digest is not None and current_artifact_digest != artifact_digest:
            raise RuntimeError("repeated Gardner artifacts differ")
        if report_digest is not None and current_report_digest != report_digest:
            raise RuntimeError("repeated Gardner reports differ")
        artifact_digest = current_artifact_digest
        report_digest = current_report_digest
        final_artifact = artifact
    if report is None or final_artifact is None:
        raise RuntimeError("Gardner study produced no artifact")
    return GardnerResult(
        report=report,
        producer_times_ns=tuple(producer_times),
        checker_times_ns=tuple(checker_times),
        artifact_sha256=artifact_digest,
        report_sha256=report_digest,
        artifact_bytes=final_artifact.stat().st_size,
        corruption_rejected=_reject_corruption(checker, final_artifact, directory),
    )


def timing_summary(result: GardnerResult) -> dict[str, int]:
    """Return upper medians and maxima in nanoseconds."""

    return {
        "producer_median_ns": median_upper(result.producer_times_ns),
        "producer_maximum_ns": maximum(result.producer_times_ns),
        "checker_median_ns": median_upper(result.checker_times_ns),
        "checker_maximum_ns": maximum(result.checker_times_ns),
    }


def _command(
    prepared: GardnerInput, holos: Path, artifact: Path, report: Path
) -> list[str]:
    command = [
        str(holos),
        "bipersistence",
        str(prepared.graph),
        str(artifact),
        "--format",
        "sparse",
        "--threshold",
        format(SCALES[-1], ".17g"),
        "--modulus",
        "47",
        "--threads",
        "1",
        "--region",
        str(prepared.region),
        "--rectangle",
        "0",
        "1",
        "2",
        "2",
        "--circular",
        "--report",
        str(report),
    ]
    for scale in SCALES:
        command.extend(("--scale", format(scale, ".17g")))
    for degree in MINIMUM_DEGREES:
        command.extend(("--minimum-degree", str(degree)))
    for cocycle in prepared.cocycles:
        command.extend(("--class-cocycle", "0", "1", str(cocycle)))
    return command


def _validate(report: dict[str, Any]) -> None:
    _validate_grid_claims(report)
    _validate_atlases(report)
    _validate_families(report)


def _validate_grid_claims(report: dict[str, Any]) -> None:
    if report.get("format") != "holos-degree-rips-report-v1":
        raise RuntimeError("Gardner report has the wrong format")
    ranks = tuple(node["rank"] for node in report.get("nodes", []))
    if ranks != EXPECTED_NODE_RANKS:
        raise RuntimeError(f"Gardner node ranks changed: {ranks!r}")
    regions = report.get("regions", [])
    if regions != [{"grades": [list(grade) for grade in REGION], "rank": 2}]:
        raise RuntimeError(f"Gardner connected-region evidence changed: {regions!r}")
    rectangles = report.get("rectangles", [])
    if len(rectangles) != 1 or rectangles[0].get("rank") != 2:
        raise RuntimeError(f"Gardner rectangle evidence changed: {rectangles!r}")


def _validate_atlases(report: dict[str, Any]) -> None:
    atlases = report.get("class_atlases", [])
    if len(atlases) != 2:
        raise RuntimeError("Gardner report omitted a selected-class atlas")
    for atlas in atlases:
        observed = tuple(
            (tuple(item["grade"]), item["kind"], item["ambiguity_rank"])
            for item in atlas["extensions"]
        )
        if observed != EXPECTED_EXTENSIONS or atlas["region_count"] != 4:
            raise RuntimeError(f"Gardner class atlas changed: {observed!r}")


def _validate_families(report: dict[str, Any]) -> None:
    families = report.get("circular_families", [])
    if len(families) != 2:
        raise RuntimeError("Gardner report omitted a circular family")
    for family in families:
        observed = tuple(
            (tuple(item["grade"]), item["kind"], item["phase"] is not None)
            for item in family["entries"]
        )
        expected = tuple(
            (grade, kind, kind == "unique") for grade, kind, _ in EXPECTED_EXTENSIONS
        )
        if observed != expected:
            raise RuntimeError(f"Gardner circular-family gate changed: {observed!r}")
        for item in family["entries"]:
            if item["phase"] is not None and len(item["phase"]) != 400:
                raise RuntimeError("Gardner circular coordinate has the wrong length")


def _timed(command: list[str]) -> tuple[int, subprocess.CompletedProcess[str]]:
    started = time.monotonic_ns()
    completed = subprocess.run(command, text=True, capture_output=True, check=False)
    return time.monotonic_ns() - started, completed


def _require_success(completed: subprocess.CompletedProcess[str], label: str) -> None:
    if completed.returncode != 0:
        raise RuntimeError(
            f"{label} failed with exit code {completed.returncode}: "
            f"{completed.stdout}\n{completed.stderr}".strip()
        )


def _reject_corruption(checker: Path, artifact: Path, directory: Path) -> bool:
    data = bytearray(artifact.read_bytes())
    data[-33] ^= 1
    corrupted = directory / "gardner-corrupted.holosbp"
    corrupted.write_bytes(data)
    completed = subprocess.run(
        [str(checker), str(corrupted)], text=True, capture_output=True, check=False
    )
    if completed.returncode == 0:
        raise RuntimeError("holos-check accepted a corrupted Gardner artifact")
    return True


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()
