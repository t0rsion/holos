"""Parse producer reports and enforce the bipersistence evidence gates."""

from __future__ import annotations

import hashlib
import json
import re
import subprocess
from pathlib import Path
from typing import Any

from .data import GraphCase

PRODUCER_RE = re.compile(
    r"wrote degree-Rips H1 module with (?P<nodes>\d+) nodes, "
    r"(?P<covers>\d+) cover maps, (?P<rectangles>\d+) rectangle claims, "
    r"(?P<regions>\d+) connected-region claims, (?P<atlases>\d+) class atlases, "
    r"and (?P<circular>\d+) circular families"
)
CHECKER_RE = re.compile(
    r"verified degree-Rips H1 bipersistence over Z/(?P<modulus>\d+) "
    r"on (?P<vertices>\d+) vertices, (?P<edges>\d+) edges, "
    r"a (?P<scales>\d+) by (?P<densities>\d+) grid, "
    r"(?P<covers>\d+) cover maps, (?P<rectangles>\d+) rectangle claims, "
    r"(?P<regions>\d+) connected-region claims, "
    r"(?P<atlases>\d+) class atlases, and (?P<circular>\d+) circular families"
)
RESEARCH_RE = re.compile(
    r"kind=case family=bipersistence case=(?P<case>\S+) "
    r"producer_median_ns=(?P<producer>\d+) "
    r"checker_median_ns=(?P<checker>\d+) "
    r"artifact_bytes=(?P<bytes>\d+) (?P<work>.+)"
)


def parse_producer(stdout: str, name: str) -> dict[str, int]:
    """Parse the exact structural summary emitted by `holos`."""

    match = PRODUCER_RE.search(stdout)
    if match is None:
        raise RuntimeError(
            f"case {name}: could not parse holos producer summary: {stdout}"
        )
    return {key: int(value) for key, value in match.groupdict().items()}


def parse_checker(stdout: str, name: str) -> dict[str, int]:
    """Parse the independent checker summary."""

    match = CHECKER_RE.search(stdout)
    if match is None:
        raise RuntimeError(
            f"case {name}: could not parse holos-check summary: {stdout}"
        )
    return {key: int(value) for key, value in match.groupdict().items()}


def parse_report(path: Path, name: str) -> dict[str, Any]:
    """Read and identify one degree-Rips JSON report."""

    try:
        report = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise RuntimeError(f"case {name}: cannot read JSON report: {error}") from error
    if report.get("format") != "holos-degree-rips-report-v1":
        raise RuntimeError(f"case {name}: unexpected report format")
    return report


def validate_evidence(
    case: GraphCase,
    producer: dict[str, int],
    checker: dict[str, int],
    report: dict[str, Any],
) -> None:
    """Check all frozen axes, counts, claims, and region coordinates."""

    _validate_producer(case, producer)
    _validate_checker(case, checker)
    _validate_report(case, report)


def _validate_producer(case: GraphCase, producer: dict[str, int]) -> None:
    expected = {
        "nodes": case.expected_nodes,
        "covers": case.expected_cover_maps,
        "rectangles": len(case.rectangles),
        "regions": len(case.regions),
        "atlases": int(case.class_selection is not None),
        "circular": int(case.circular),
    }
    for key, value in expected.items():
        if producer[key] != value:
            raise RuntimeError(
                f"case {case.name}: producer reported {key}={producer[key]}, "
                f"expected {value}"
            )


def _validate_checker(case: GraphCase, checker: dict[str, int]) -> None:
    if checker["scales"] != len(case.scales) or checker["densities"] != len(
        case.minimum_degrees
    ):
        raise RuntimeError(f"case {case.name}: checker reported the wrong grid shape")
    if checker["covers"] != case.expected_cover_maps:
        raise RuntimeError(f"case {case.name}: checker reported the wrong cover count")
    if checker["rectangles"] != len(case.rectangles):
        raise RuntimeError(
            f"case {case.name}: checker reported the wrong rectangle count"
        )
    if checker["regions"] != len(case.regions):
        raise RuntimeError(f"case {case.name}: checker reported the wrong region count")
    if checker["atlases"] != int(case.class_selection is not None):
        raise RuntimeError(f"case {case.name}: checker reported the wrong atlas count")
    if checker["circular"] != int(case.circular):
        raise RuntimeError(f"case {case.name}: checker reported the wrong family count")


def _validate_report(case: GraphCase, report: dict[str, Any]) -> None:
    _validate_report_axes(case, report)
    _validate_report_counts(case, report)
    _validate_report_regions(case, report)


def _validate_report_axes(case: GraphCase, report: dict[str, Any]) -> None:
    if report.get("scales") != list(case.scales):
        raise RuntimeError(
            f"case {case.name}: report scales differ from the frozen grid"
        )
    if report.get("minimum_degrees") != list(case.minimum_degrees):
        raise RuntimeError(
            f"case {case.name}: report degrees differ from the frozen grid"
        )


def _validate_report_counts(case: GraphCase, report: dict[str, Any]) -> None:
    if len(report.get("nodes", [])) != case.expected_nodes:
        raise RuntimeError(f"case {case.name}: report omitted grid nodes")
    if len(report.get("rectangles", [])) != len(case.rectangles):
        raise RuntimeError(f"case {case.name}: report omitted rectangle ranks")
    if len(report.get("regions", [])) != len(case.regions):
        raise RuntimeError(f"case {case.name}: report omitted connected regions")
    if len(report.get("class_atlases", [])) != int(case.class_selection is not None):
        raise RuntimeError(f"case {case.name}: report omitted class atlases")
    if len(report.get("circular_families", [])) != int(case.circular):
        raise RuntimeError(f"case {case.name}: report omitted circular families")


def _validate_report_regions(case: GraphCase, report: dict[str, Any]) -> None:
    report_regions = {
        tuple(tuple(grade) for grade in region["grades"])
        for region in report.get("regions", [])
    }
    expected_regions = {tuple(sorted(grades)) for grades in case.regions}
    if report_regions != expected_regions:
        raise RuntimeError(f"case {case.name}: report region grades differ from input")


def reject_corruption(checker: Path, artifact: Path, directory: Path) -> bool:
    """Flip a payload byte and require independent verification to fail."""

    data = bytearray(artifact.read_bytes())
    if len(data) <= 32:
        raise RuntimeError("artifact is too short for a payload corruption test")
    offset = len(data) - 33
    data[offset] ^= 1
    corrupted = directory / "corrupted.holosbp"
    corrupted.write_bytes(data)
    completed = subprocess.run(
        [str(checker), str(corrupted)],
        text=True,
        capture_output=True,
        check=False,
    )
    if completed.returncode == 0:
        raise RuntimeError(
            "independent checker accepted a deliberately corrupted artifact"
        )
    return True


def sha256(path: Path) -> str:
    """Return the SHA-256 digest of one artifact or report."""

    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1 << 16), b""):
            digest.update(block)
    return digest.hexdigest()
