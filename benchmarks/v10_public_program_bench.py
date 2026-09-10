#!/usr/bin/env python3
"""Run the public temporal-graph control.

Usage: v10_public_program_bench.py SOURCE

SOURCE is the unmodified gzip archive named by the registered corpus. The
runner verifies its SHA-256 digest, prepares the frozen weekly recency
trajectory, and compares persistence-program updates with fresh exact H0 and
H1 diagrams. It also measures producer replay and the independent program and
trace checkers in fresh processes.

Environment:
  CARGO             Cargo command with an optional toolchain. The default is
                    ``cargo +1.92``.
  REPS              Timed repetitions per arm. The default and minimum are 5.
  PROGRAM_AFFINITY  CPUs used by the study. The default is ``0-3,12-15``.
  ALLOW_DIRTY       Set to 1 to create a record from a dirty tree. Such a
                    record is void for release claims.
"""

from v10_public_program.execute import main


if __name__ == "__main__":
    main()
