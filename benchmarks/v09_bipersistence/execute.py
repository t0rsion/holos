"""Run the bipersistence gates and parse their evidence."""

from __future__ import annotations

import subprocess
import time
from collections import Counter
from collections.abc import Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from .data import GraphCase, write_region, write_sparse_graph
from .evidence import (
    RESEARCH_RE,
    parse_checker,
    parse_producer,
    parse_report,
    reject_corruption,
    sha256,
    validate_evidence,
)


@dataclass
class CaseResult:
    """Observed evidence and timings for one frozen graph case."""

    case: GraphCase
    producer_times_ns: list[int]
    checker_times_ns: list[int]
    producer: dict[str, int]
    checker: dict[str, int]
    report: dict[str, Any]
    artifact_sha256: str
    report_sha256: str
    corruption_rejected: bool

    @property
    def atlas_kinds(self) -> dict[str, int]:
        """Return extension classifications counted in the JSON report."""

        kinds = Counter()
        for atlas in self.report.get("class_atlases", []):
            for extension in atlas.get("extensions", []):
                kinds[extension["kind"]] += 1
        return dict(sorted(kinds.items()))


@dataclass
class StudyResult:
    """All deterministic gates and measurements from one study run."""

    cases: list[CaseResult]
    circular_modulus_gate: bool
    research_bench_times_ns: list[int]
    research_bench_work: str


def median_upper(values: Sequence[int]) -> int:
    """Return the upper middle value from a nonempty run sample."""

    if not values:
        raise ValueError("cannot summarize an empty run sample")
    return sorted(values)[len(values) // 2]


def maximum(values: Sequence[int]) -> int:
    """Return the largest value from a nonempty run sample."""

    if not values:
        raise ValueError("cannot summarize an empty run sample")
    return max(values)


def run_study(
    holos: Path,
    checker: Path,
    research_bench: Path,
    repetitions: int,
    temporary_root: Path,
) -> StudyResult:
    """Run every producer, checker, integrity, and API integration gate."""

    if repetitions < 1:
        raise ValueError("repetitions must be positive")
    temporary_root.mkdir(parents=True, exist_ok=True)
    case_results = []
    for case in _cases():
        case_results.append(
            _run_case(
                case,
                holos,
                checker,
                repetitions,
                temporary_root / case.name,
            )
        )
    circular_gate = _run_circular_modulus_gate(
        _cases()[0], holos, temporary_root / "circular-modulus-gate"
    )
    research_times, research_work = _run_research_bench(
        research_bench, repetitions, temporary_root / "research-bench"
    )
    return StudyResult(
        cases=case_results,
        circular_modulus_gate=circular_gate,
        research_bench_times_ns=research_times,
        research_bench_work=research_work,
    )


def _cases() -> tuple[GraphCase, ...]:
    from .data import CASES

    return CASES


def _run_case(
    case: GraphCase,
    holos: Path,
    checker: Path,
    repetitions: int,
    directory: Path,
) -> CaseResult:
    directory.mkdir(parents=True, exist_ok=True)
    graph_path = directory / "input.sparse"
    write_sparse_graph(case, graph_path)
    region_paths = []
    for index, grades in enumerate(case.regions):
        region_path = directory / f"region-{index}.txt"
        write_region(grades, region_path)
        region_paths.append(region_path)

    producer_times = []
    checker_times = []
    final_artifact = None
    final_report = None
    producer_evidence = None
    checker_evidence = None
    report_data = None
    artifact_digest = None
    report_digest = None
    for repetition in range(repetitions):
        artifact_path = directory / f"artifact-{repetition}.holosbp"
        report_path = directory / f"report-{repetition}.json"
        producer_command = _producer_command(
            case, holos, graph_path, artifact_path, report_path, region_paths
        )
        elapsed, completed = _timed(producer_command)
        _require_success(completed, "holos bipersistence")
        producer_times.append(elapsed)
        producer_evidence = parse_producer(completed.stdout, case.name)

        elapsed, completed = _timed([str(checker), str(artifact_path)])
        _require_success(completed, "holos-check")
        checker_times.append(elapsed)
        checker_evidence = parse_checker(completed.stdout, case.name)

        report_data = parse_report(report_path, case.name)
        validate_evidence(case, producer_evidence, checker_evidence, report_data)
        current_artifact_digest = sha256(artifact_path)
        current_report_digest = sha256(report_path)
        if artifact_digest is not None and current_artifact_digest != artifact_digest:
            raise RuntimeError(f"case {case.name}: repeated artifacts are not deterministic")
        if report_digest is not None and current_report_digest != report_digest:
            raise RuntimeError(f"case {case.name}: repeated reports are not deterministic")
        artifact_digest = current_artifact_digest
        report_digest = current_report_digest
        final_artifact = artifact_path
        final_report = report_path

    if final_artifact is None or final_report is None:
        raise RuntimeError(f"case {case.name}: no artifact was produced")
    corruption_rejected = reject_corruption(checker, final_artifact, directory)
    return CaseResult(
        case=case,
        producer_times_ns=producer_times,
        checker_times_ns=checker_times,
        producer=producer_evidence,
        checker=checker_evidence,
        report=report_data,
        artifact_sha256=artifact_digest,
        report_sha256=report_digest,
        corruption_rejected=corruption_rejected,
    )


def _producer_command(
    case: GraphCase,
    holos: Path,
    graph_path: Path,
    artifact_path: Path,
    report_path: Path,
    region_paths: Sequence[Path],
) -> list[str]:
    command = [
        str(holos),
        "bipersistence",
        str(graph_path),
        str(artifact_path),
        "--format",
        "sparse",
        "--threshold",
        _float_text(case.threshold),
        "--modulus",
        "47",
        "--threads",
        "1",
        "--report",
        str(report_path),
    ]
    for scale in case.scales:
        command.extend(("--scale", _float_text(scale)))
    for degree in case.minimum_degrees:
        command.extend(("--minimum-degree", str(degree)))
    for rectangle in case.rectangles:
        command.extend(("--rectangle", *(str(value) for value in rectangle)))
    for region_path in region_paths:
        command.extend(("--region", str(region_path)))
    if case.class_selection is not None:
        command.extend(("--class", *(str(value) for value in case.class_selection)))
    if case.circular:
        command.append("--circular")
    return command


def _run_circular_modulus_gate(case: GraphCase, holos: Path, directory: Path) -> bool:
    """Require the automatic circular-family field gate to reject modulus two."""

    directory.mkdir(parents=True, exist_ok=True)
    graph_path = directory / "input.sparse"
    write_sparse_graph(case, graph_path)
    artifact_path = directory / "rejected.holosbp"
    report_path = directory / "rejected.json"
    command = _producer_command(
        case, holos, graph_path, artifact_path, report_path, []
    )
    modulus_position = command.index("47")
    command[modulus_position] = "2"
    completed = subprocess.run(
        command,
        cwd=directory,
        text=True,
        capture_output=True,
        check=False,
    )
    if completed.returncode == 0:
        raise RuntimeError("modulus-two circular-family gate unexpectedly succeeded")
    combined = f"{completed.stdout}\n{completed.stderr}"
    expected = "automatic circular families require an odd prime modulus"
    if expected not in combined:
        raise RuntimeError(
            "modulus-two circular-family gate failed for an unexpected reason: "
            + combined.strip()
        )
    return True


def _run_research_bench(
    binary: Path, repetitions: int, directory: Path
) -> tuple[list[int], str]:
    """Run the Rust API study and require its nonrectangular region claim."""

    directory.mkdir(parents=True, exist_ok=True)
    times = []
    work = None
    for _ in range(repetitions):
        elapsed, completed = _timed([str(binary), "--reps", "1"])
        _require_success(completed, "research-bench")
        times.append(elapsed)
        match = RESEARCH_RE.search(completed.stdout)
        if match is None:
            raise RuntimeError("research-bench did not report its bipersistence case")
        work = match.group("work")
        required = ("regions=1", "class_atlases=1", "circular_families=1", "checker=independent")
        missing = [term for term in required if term not in work]
        if missing:
            raise RuntimeError(
                "research-bench bipersistence case omitted required evidence: "
                + ", ".join(missing)
            )
    if work is None:
        raise RuntimeError("research-bench produced no work description")
    return times, work


def _timed(command: Sequence[str]) -> tuple[int, subprocess.CompletedProcess[str]]:
    started = time.monotonic_ns()
    completed = subprocess.run(
        list(command),
        text=True,
        capture_output=True,
        check=False,
    )
    return time.monotonic_ns() - started, completed


def _require_success(completed: subprocess.CompletedProcess[str], label: str) -> None:
    if completed.returncode == 0:
        return
    raise RuntimeError(
        f"{label} failed with exit code {completed.returncode}: "
        f"{completed.stdout}\n{completed.stderr}".strip()
    )


def _float_text(value: float) -> str:
    return format(value, ".17g")
