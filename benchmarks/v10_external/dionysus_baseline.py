"""Run the Dionysus 2.2.3 maintained vineyard baseline."""

from __future__ import annotations

import argparse
import sys
import time
from pathlib import Path

try:
    import dionysus as d
    from dionysus_engine import run
    from dionysus_filtration import prepare
    from trajectory import build_complex, global_rank_trajectory, read_trajectory
except ImportError as error:  # pragma: no cover - exercised by the runner
    raise SystemExit(
        "dionysus 2.2.3 is required; use the pinned benchmark environment"
    ) from error


PACKAGE_VERSION = "2.2.3"


def main(argv: list[str] | None = None) -> int:
    """Parse command-line options and emit one machine-readable record."""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trajectory", type=Path)
    parser.add_argument("--bars", type=Path)
    parser.add_argument("--repetitions", type=int, default=5)
    parser.add_argument("--modulus", type=int, default=2)
    parser.add_argument(
        "--method", choices=("matrix_v", "matrix_u"), default="matrix_v"
    )
    arguments = parser.parse_args(argv)
    if arguments.repetitions < 5:
        parser.error("--repetitions must be at least 5")
    if arguments.modulus not in (2, 3, 5):
        parser.error("--modulus must be 2, 3, or 5")
    if getattr(d, "__version__", None) != PACKAGE_VERSION:
        parser.error(f"Dionysus {PACKAGE_VERSION} is required")
    try:
        preparation_start = time.perf_counter_ns()
        trajectory = global_rank_trajectory(read_trajectory(arguments.trajectory))
        complex_ = build_complex(trajectory)
        prepared = prepare(complex_, trajectory)
        preparation_ns = time.perf_counter_ns() - preparation_start
        record = run(
            trajectory,
            prepared,
            arguments.modulus,
            arguments.method,
            arguments.repetitions,
            arguments.bars,
        )
        print(f"{record} preparation_ns={preparation_ns}")
        return 0
    except (OSError, ValueError, RuntimeError) as error:
        print(f"dionysus-baseline: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
