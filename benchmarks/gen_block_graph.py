#!/usr/bin/env python3
"""Generate a chain of weighted complete graph blocks.

Usage: gen_block_graph.py BLOCKS BLOCK_SIZE SEED > graph.sparse

Consecutive blocks share one articulation vertex and no edge. Each block has
BLOCK_SIZE vertices and contains every pair. Edge values are Euclidean
distances between seeded local 3D coordinates. The graph is nonmetric across
blocks because one articulation can have different local coordinates in its
two blocks. Sparse Rips input permits that.
"""

import math
import random
import sys


def main():
    if len(sys.argv) != 4:
        sys.exit("usage: gen_block_graph.py BLOCKS BLOCK_SIZE SEED")
    blocks, size, seed = map(int, sys.argv[1:])
    if blocks < 1:
        sys.exit("BLOCKS must be positive")
    if size < 3:
        sys.exit("BLOCK_SIZE must be at least 3")
    rng = random.Random(seed)
    anchor = 0
    next_vertex = 1
    for _ in range(blocks):
        vertices = [anchor] + list(range(next_vertex, next_vertex + size - 1))
        points = [[rng.random() for _ in range(3)] for _ in vertices]
        for i in range(1, size):
            for j in range(i):
                print(vertices[j], vertices[i], repr(math.dist(points[i], points[j])))
        anchor = vertices[-1]
        next_vertex += size - 1


if __name__ == "__main__":
    main()
