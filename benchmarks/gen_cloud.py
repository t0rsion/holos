#!/usr/bin/env python3
"""Deterministic uniform point cloud in the unit cube.

Usage: gen_cloud.py N DIM SEED > cloud.csv

Stdlib only. The same (N, DIM, SEED) always yields byte-identical output:
Python's Mersenne Twister is stable across releases for random().
"""
import random
import sys


def main() -> None:
    if len(sys.argv) != 4:
        sys.exit("usage: gen_cloud.py N DIM SEED")
    n, dim, seed = (int(a) for a in sys.argv[1:])
    rng = random.Random(seed)
    for _ in range(n):
        print(",".join(repr(rng.random()) for _ in range(dim)))


if __name__ == "__main__":
    main()
