"""Render records for the external degree-Rips comparison."""

from __future__ import annotations

import datetime as dt
import json
from argparse import Namespace
from collections.abc import Mapping, Sequence
from pathlib import Path
from typing import Any


def combine_record(
    options: Namespace,
    holos_cases: Sequence[Mapping[str, Any]],
    external: Mapping[str, Any],
    provenance: Mapping[str, Any],
) -> dict[str, Any]:
    """Join Holos and external results while preserving unavailable states."""

    external_cases = {case["name"]: case for case in external.get("cases", [])}
    comparisons = []
    for holos_case in holos_cases:
        external_case = external_cases.get(holos_case["name"])
        if external_case is None:
            comparisons.append(
                {
                    "name": holos_case["name"],
                    "holos_ranks": holos_case["ranks"],
                    "status": "unavailable",
                    "reason": external.get("reason", "external case was omitted"),
                }
            )
            continue
        holos_ranks = holos_case["ranks"]
        multipers_ranks = external_case["multipers_hilbert_ranks"]
        gudhi_ranks = external_case["gudhi_slice_ranks"]
        comparisons.append(
            {
                "name": holos_case["name"],
                "holos_ranks": holos_ranks,
                "multipers_hilbert_ranks": multipers_ranks,
                "gudhi_slice_ranks": gudhi_ranks,
                "multipers_matches_holos": multipers_ranks == holos_ranks,
                "gudhi_matches_holos": gudhi_ranks == holos_ranks,
                "status": "pass"
                if multipers_ranks == holos_ranks and gudhi_ranks == holos_ranks
                else "mismatch",
            }
        )
    if external["status"] in {"unavailable", "error"}:
        status = "unavailable"
    elif external["status"] == "mismatch" or any(
        case["status"] == "mismatch" for case in comparisons
    ):
        status = "mismatch"
    else:
        status = "pass"
    return {
        "status": status,
        "generated_at": dt.datetime.now(dt.timezone.utc)
        .replace(microsecond=0)
        .isoformat(),
        "input": _relative(provenance, options.input),
        "holos": _relative(provenance, options.holos),
        "external_python": provenance["external_python"],
        "external": external,
        "cases": comparisons,
        "provenance": provenance,
        "semantic_limits": [
            "The comparison is H1 node rank on the declared finite grid over F2.",
            (
                "The harness does not equate a multipers signed measure with a Holos "
                "generalized rectangle rank."
            ),
            (
                "A rectangle with a minimum and maximum has the corner-map rank, but "
                "no rectangle query is compared here."
            ),
            (
                "Infinite multipers boundary atoms are excluded when a declared scale "
                "is below that boundary."
            ),
        ],
    }


def error_record(
    options: Namespace,
    reason: str,
    provenance: Mapping[str, Any],
) -> dict[str, Any]:
    """Create a record for a producer-side setup failure."""

    return {
        "status": "error",
        "generated_at": dt.datetime.now(dt.timezone.utc)
        .replace(microsecond=0)
        .isoformat(),
        "input": _relative(provenance, options.input),
        "holos": _relative(provenance, options.holos),
        "external_python": provenance["external_python"],
        "external": {"status": "unavailable", "reason": reason},
        "cases": [],
        "semantic_limits": [],
        "provenance": provenance,
    }


def report_ranks(report: Mapping[str, Any], case: Mapping[str, Any]) -> list[int]:
    """Read and order node ranks from a Holos JSON report."""

    expected = {
        (scale, density): None
        for scale in range(len(case["scales"]))
        for density in range(len(case["minimum_degrees"]))
    }
    for node in report.get("nodes", []):
        key = (int(node["scale_index"]), int(node["density_index"]))
        if key not in expected:
            raise ValueError(f"Holos report contains an unexpected node {key}")
        expected[key] = int(node["rank"])
    if any(value is None for value in expected.values()):
        raise ValueError(f"Holos report omitted a node for {case['name']}")
    return [
        expected[(scale, density)]
        for scale in range(len(case["scales"]))
        for density in range(len(case["minimum_degrees"]))
    ]


def write_record(path: Path, record: Mapping[str, Any]) -> None:
    """Write a generated Markdown record."""

    path.parent.mkdir(parents=True, exist_ok=True)
    lines = [
        "# External degree-Rips validation",
        "",
        f"- status: `{record['status']}`",
        f"- generated: `{record['generated_at']}`",
        f"- input: `{record['input']}`",
        f"- Holos: `{record['holos']}`",
        f"- external Python: `{record['external_python']}`",
        "",
        "## External environment",
        "",
        f"- status: `{record['external'].get('status', 'unknown')}`",
    ]
    for key in (
        "multipers_version",
        "gudhi_version",
        "reason",
        "exception",
        "returncode",
    ):
        if key in record["external"]:
            lines.append(f"- {key.replace('_', ' ')}: `{record['external'][key]}`")
    lines.extend(
        (
            "",
            "## Cases",
            "",
            "| case | Holos | multipers | GUDHI | result |",
            "|:--|:--|:--|:--|:--|",
        )
    )
    for case in record["cases"]:
        lines.append(
            f"| `{case['name']}` | `{case['holos_ranks']}` | "
            f"`{case.get('multipers_hilbert_ranks', 'unavailable')}` | "
            f"`{case.get('gudhi_slice_ranks', 'unavailable')}` | `{case['status']}` |"
        )
    lines.extend(("", "## Semantic limits", ""))
    lines.extend(f"- {limit}" for limit in record["semantic_limits"])
    provenance = record["provenance"]
    lines.extend(
        (
            "",
            "## Provenance",
            "",
            f"- commit: `{provenance['commit']}`",
            f"- tree state: `{provenance['tree_state']}`",
            f"- tree entries: `{provenance['tree_entries']}`",
            f"- input SHA-256: `{provenance['input_sha256']}`",
            f"- Holos SHA-256: `{provenance['holos_sha256']}`",
            f"- external interpreter: `{provenance['external_python']}`",
            f"- external environment: `{provenance['external_environment']}`",
            f"- external interpreter version: `{provenance['external_python_version']}`",
            f"- external interpreter SHA-256: `{provenance['external_python_sha256']}`",
            f"- child script SHA-256: `{provenance['child_sha256']}`",
            f"- requirements: `{provenance['requirements']}`",
            f"- requirements SHA-256: `{provenance['requirements_sha256']}`",
            f"- requested affinity: `{provenance['requested_affinity']}`",
            f"- effective affinity: `{provenance['effective_affinity']}`",
            "",
            "Recreate the external environment with:",
            "",
            "```text",
            *provenance["recreate_commands"],
            "```",
        )
    )
    lines.extend(
        (
            "",
            "## Reproduce",
            "",
            (
                "Create Python 3.12.12 with the pinned packages in "
                "`external_bipersistence_requirements.txt`."
            ),
            (
                "Then run `benchmarks/external_bipersistence.sh` with "
                "`MULTIPERS_PYTHON` set to that interpreter."
            ),
            "",
            "The input cases are tracked in `external_bipersistence_cases.json`.",
            "The generated result is a local record and is not a release claim.",
            "",
            "## Raw external result",
            "",
            "```json",
            json.dumps(record["external"], indent=2, sort_keys=True),
            "```",
            "",
        )
    )
    path.write_text("\n".join(lines), encoding="utf-8")


def _relative(provenance: Mapping[str, Any], candidate: Path) -> str:
    try:
        return candidate.resolve().relative_to(Path(provenance["root"])).as_posix()
    except ValueError:
        return candidate.name
