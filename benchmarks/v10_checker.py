"""Independent program-checker measurements shared by temporal-graph studies."""

from __future__ import annotations

import hashlib
import os
import shlex
import statistics
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path

import measure as process_measure


@dataclass(frozen=True)
class CheckerMeasurement:
    """Raw independent process measurements for one program and trace."""

    graph: Path
    program: Path
    trace: Path
    program_command: tuple[str, ...]
    trace_command: tuple[str, ...]
    program_samples_ns: tuple[int, ...]
    trace_samples_ns: tuple[int, ...]
    program_rss_kib: tuple[int, ...]
    trace_rss_kib: tuple[int, ...]

    def fields(self) -> dict[str, str]:
        """Return stable record fields."""
        return {
            "independent_program_check_ns": str(median(self.program_samples_ns)),
            "independent_trace_check_ns": str(median(self.trace_samples_ns)),
            "independent_program_samples_ns": samples(self.program_samples_ns),
            "independent_trace_samples_ns": samples(self.trace_samples_ns),
            "independent_program_max_rss_kib": str(max(self.program_rss_kib)),
            "independent_trace_max_rss_kib": str(max(self.trace_rss_kib)),
            "checker_graph_sha256": sha256(self.graph),
            "checker_program_sha256": sha256(self.program),
            "checker_trace_sha256": sha256(self.trace),
        }


def measure(
    checker: Path,
    prefix: Path,
    repetitions: int,
    affinity: str,
) -> CheckerMeasurement:
    """Run each independent checker in a fresh process."""
    if not sys.platform.startswith("linux"):
        raise SystemExit("Linux process accounting is required")
    graph = suffixed(prefix, ".graph")
    program = suffixed(prefix, ".program")
    trace = suffixed(prefix, ".trace")
    for path in (graph, program, trace):
        if not path.is_file():
            raise SystemExit(f"benchmark did not write checker input {path}")
    program_command = (str(checker), str(program), str(graph))
    trace_command = (str(checker), str(trace))
    run_checked(("taskset", "-c", affinity, *program_command), "persistence program")
    run_checked(("taskset", "-c", affinity, *trace_command), "persistence trace")
    program_times: list[int] = []
    trace_times: list[int] = []
    program_rss: list[int] = []
    trace_rss: list[int] = []
    for repetition in range(repetitions):
        arms = (
            (
                (
                    program_command,
                    "persistence program",
                    program_times,
                    program_rss,
                ),
                (trace_command, "persistence trace", trace_times, trace_rss),
            )
            if repetition % 2 == 0
            else (
                (trace_command, "persistence trace", trace_times, trace_rss),
                (
                    program_command,
                    "persistence program",
                    program_times,
                    program_rss,
                ),
            )
        )
        for arm, label, times, memory in arms:
            elapsed, rss = run_measured(arm, label, affinity)
            times.append(elapsed)
            memory.append(rss)
    return CheckerMeasurement(
        graph,
        program,
        trace,
        program_command,
        trace_command,
        tuple(program_times),
        tuple(trace_times),
        tuple(program_rss),
        tuple(trace_rss),
    )


def command_text(command: tuple[str, ...]) -> str:
    """Render a checker command for a raw record."""
    return shlex.join(command)


def sha256(path: Path) -> str:
    """Return one file digest."""
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def suffixed(prefix: Path, suffix: str) -> Path:
    """Append an artifact suffix without replacing a caller suffix."""
    return Path(str(prefix) + suffix)


def median(values: tuple[int, ...]) -> int:
    """Return the recorded upper median."""
    return int(statistics.median_high(values))


def samples(values: tuple[int, ...]) -> str:
    """Render raw integer samples."""
    return ",".join(map(str, values))


def run_checked(command: tuple[str, ...], label: str) -> str:
    """Run one checker process and validate its success message."""
    completed = subprocess.run(
        list(command),
        text=True,
        capture_output=True,
        check=True,
    )
    output = completed.stdout.strip()
    if not output.startswith("verified H0 and H1 persistence"):
        raise SystemExit(f"independent {label} returned an unexpected record: {output}")
    return output


def run_measured(
    command: tuple[str, ...], label: str, affinity: str
) -> tuple[int, int]:
    """Run one Linux child and return wall time and peak RSS in KiB."""
    target = os.fsencode(command[0])
    launch = ("taskset", "-c", affinity, *command)
    peak_rss = 0
    started = time.perf_counter_ns()
    with (
        tempfile.TemporaryFile(mode="w+t", encoding="utf-8") as output_file,
        tempfile.TemporaryFile(mode="w+t", encoding="utf-8") as error_file,
    ):
        process = subprocess.Popen(
            list(launch),
            stdout=output_file,
            stderr=error_file,
        )
        while process.poll() is None:
            current_rss = process_measure.vmhwm_kb(process.pid, target)
            if current_rss is not None:
                peak_rss = max(peak_rss, current_rss)
            elapsed = time.perf_counter_ns() - started
            time.sleep(0.0005 if elapsed < 20_000_000 else 0.01)
        elapsed = time.perf_counter_ns() - started
        output_file.seek(0)
        error_file.seek(0)
        output = output_file.read().strip()
        error = error_file.read().strip()
    if process.returncode != 0:
        detail = f": {error}" if error else ""
        raise SystemExit(
            f"independent {label} failed with status {process.returncode}{detail}"
        )
    if not output.startswith("verified H0 and H1 persistence"):
        raise SystemExit(f"independent {label} returned an unexpected record: {output}")
    return elapsed, peak_rss
