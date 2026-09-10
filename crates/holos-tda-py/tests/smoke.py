"""Smoke tests for the built holos-tda wheel.

Plain asserts, no test framework. Run with the wheel installed.
"""

import holos_tda

from smoke_cohomology import run as run_cohomology
from smoke_coordinates import run as run_coordinates
from smoke_index import run as run_index
from smoke_options import run as run_options
from smoke_program import run as run_program
from smoke_rips import run as run_rips


def main():
    run_rips()
    run_coordinates()
    run_index()
    run_cohomology()
    run_program()
    run_options()
    print("smoke OK", holos_tda.__version__, holos_tda.GIT_HASH)


if __name__ == "__main__":
    main()
