#!/usr/bin/env python3
"""Run the registered persistence-program study.

Usage: v10_program_bench.py [--confirm]

The screen must finish before confirmation. Each entry times result-sensitive
evaluation, updates with and without class correspondence, exact
recomputation, producer-crate replay, and independent checker processes. The
driver retains every raw timing sample and the checker peak memory.

Environment:
  CARGO             Cargo command with an optional toolchain. The default is
                    ``cargo +1.92``.
  REPS              Timed repetitions per arm. The default and minimum are 5.
  PROGRAM_AFFINITY  CPUs used by the study. The default is ``0-3,12-15``.
  ALLOW_DIRTY       Set to 1 to create a record from a dirty tree. Such a
                    record is void for release claims.
"""

from v10_program.execute import main


if __name__ == "__main__":
    main()
