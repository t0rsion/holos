#!/usr/bin/env python3
"""Deterministic point clouds.

Usage: gen_cloud.py N DIM SEED [FAMILY] > cloud.csv

Families:
  cube      uniform in the unit cube (the default, and the 3-argument form)
  sphere    uniform on the sphere of radius 0.5 centred at (0.5, ..., 0.5)
  clusters  8 Gaussian clusters, sigma 0.05, centres uniform in [0.15, 0.85]
  torus     uniform on a torus, major radius 0.35, minor radius 0.15, in the
            first three coordinates; further coordinates hold 0.5, so ambient
            padding leaves every distance unchanged
  lattice   the first N points of a regular grid, in odometer order. Every
            distance is a grid step times the square root of an integer, so
            equal distances are common and exact. SEED is unused
  duplicate 4 copies of each of ceil(N/4) cube points, truncated to N, so
            every group holds a zero distance
  equal     N copies of the centre point. Every distance is zero. SEED is
            unused

Stdlib only. The same (N, DIM, SEED, FAMILY) yields byte-identical output on
one machine: every family draws from random() alone, which Python keeps stable
across releases, and shapes those draws with libm.
"""
import math
import random
import sys

CLUSTER_COUNT = 8
DUPLICATE_COPIES = 4
CLUSTER_SIGMA = 0.05
TORUS_MAJOR = 0.35
TORUS_MINOR = 0.15


def normal(rng):
    """One standard normal from two random() draws, by Box-Muller."""
    u = rng.random()
    while u == 0.0:
        u = rng.random()
    return math.sqrt(-2.0 * math.log(u)) * math.cos(2.0 * math.pi * rng.random())


def cube(rng, n, dim):
    return [[rng.random() for _ in range(dim)] for _ in range(n)]


def sphere(rng, n, dim):
    pts = []
    while len(pts) < n:
        v = [normal(rng) for _ in range(dim)]
        norm = math.sqrt(sum(x * x for x in v))
        if norm == 0.0:
            continue
        pts.append([0.5 + 0.5 * x / norm for x in v])
    return pts


def clusters(rng, n, dim):
    centres = [
        [0.15 + 0.7 * rng.random() for _ in range(dim)] for _ in range(CLUSTER_COUNT)
    ]
    pts = []
    for i in range(n):
        centre = centres[i % CLUSTER_COUNT]
        pts.append([c + CLUSTER_SIGMA * normal(rng) for c in centre])
    return pts


def torus(rng, n, dim):
    if dim < 3:
        sys.exit("torus needs DIM >= 3")
    pts = []
    while len(pts) < n:
        theta = 2.0 * math.pi * rng.random()
        phi = 2.0 * math.pi * rng.random()
        radius = TORUS_MAJOR + TORUS_MINOR * math.cos(phi)
        # Rejection makes the sample uniform over the surface. Sampling phi
        # uniformly would pile points on the inner rim.
        if rng.random() > radius / (TORUS_MAJOR + TORUS_MINOR):
            continue
        pts.append(
            [
                0.5 + radius * math.cos(theta),
                0.5 + radius * math.sin(theta),
                0.5 + TORUS_MINOR * math.sin(phi),
            ]
            + [0.5] * (dim - 3)
        )
    return pts


def lattice(rng, n, dim):
    side = 1
    while side**dim < n:
        side += 1
    if side < 2:
        sys.exit("lattice needs N >= 2")
    step = 1.0 / (side - 1)
    pts = []
    for i in range(n):
        rest, coords = i, []
        for _ in range(dim):
            coords.append(step * (rest % side))
            rest //= side
        pts.append(coords)
    return pts


def duplicate(rng, n, dim):
    distinct = -(-n // DUPLICATE_COPIES)
    pts = []
    for _ in range(distinct):
        point = [rng.random() for _ in range(dim)]
        pts.extend([list(point) for _ in range(DUPLICATE_COPIES)])
    return pts[:n]


def equal(rng, n, dim):
    return [[0.5] * dim for _ in range(n)]


FAMILIES = {
    "cube": cube,
    "sphere": sphere,
    "clusters": clusters,
    "torus": torus,
    "lattice": lattice,
    "duplicate": duplicate,
    "equal": equal,
}


def main() -> None:
    if len(sys.argv) not in (4, 5):
        sys.exit("usage: gen_cloud.py N DIM SEED [FAMILY]")
    n, dim, seed = (int(a) for a in sys.argv[1:4])
    family = sys.argv[4] if len(sys.argv) == 5 else "cube"
    if family not in FAMILIES:
        sys.exit(f"unknown family {family!r}; pick one of {', '.join(sorted(FAMILIES))}")
    rng = random.Random(seed)
    for point in FAMILIES[family](rng, n, dim):
        print(",".join(repr(x) for x in point))


if __name__ == "__main__":
    main()
