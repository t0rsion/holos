#!/usr/bin/env python3
"""Deterministic sparse dissimilarity graphs.

Usage: gen_graph.py GENERATOR N SEED TAU PARAM OUT_SPARSE

Writes ripser-style triplets, one "i j d" line per edge, and prints one
metadata line of whitespace-separated key=value fields. The engineering
benchmark reads that line for the threshold and the graph shape.

These graphs are the sparse-only half of the engineering corpus. A point
cloud always has a dense distance matrix behind it; a graph here need not.
Absent edges are +inf. The input can be disconnected. The weights of the
synthetic generators satisfy no triangle inequality.

Generators, each with its own meaning for PARAM:
  knn        PARAM = k. Uniform points in the unit cube, in three
             dimensions. Every vertex keeps its k nearest neighbours, and
             the edge set is the union of the two directions, so degrees
             run above k. Weights are Euclidean distances
  powerlaw   PARAM = m. Preferential attachment: each new vertex draws m
             distinct partners with probability proportional to degree.
             Degrees are heavy-tailed
  block      PARAM = b. b planted blocks of geometrically decreasing size.
             Each vertex draws IN_DEGREE partners inside its block, and one
             vertex in CROSS_EVERY draws one partner outside it. Within-block
             weights are low, cross-block weights are high
  forest     PARAM = c. c random recursive trees, plus one isolated vertex
             in ISOLATE_EVERY. There is no cycle, so the complex is a graph
             and every bar lives in dimension 0
  nonmetric  PARAM = d. Each vertex draws d partners uniformly and each
             edge draws its weight independently, so the triangle
             inequality fails everywhere. One vertex in ISOLATE_EVERY takes
             no edge

Weights of every generator except knn come from a ladder of
WEIGHT_LEVELS exact binary fractions, so equal weights are common and the
threshold can land exactly on a realised value.

The threshold is T = TAU * (the largest weight the generator drew). Edges
above T are left out of the file, as densify_to_sparse.py does for a cloud.
TAU = 1.0 keeps every edge and puts T exactly on a realised weight, which
is the threshold-boundary tie case.

The sparse reader takes the point count from the largest index it sees, so
a trailing vertex with no edge would shrink the graph and drop an essential
H0 bar. Vertices are therefore relabelled: isolated vertices take the low
indices, vertices with an edge follow in their original order. Relabelling
moves no bar, because the barcode does not depend on the labelling.

Stdlib only. The same (GENERATOR, N, SEED, TAU, PARAM) yields byte-identical
output on one machine: every generator draws from random() and randrange()
alone, which Python keeps stable across releases.
"""
import math
import random
import sys

WEIGHT_LEVELS = 64
CLOUD_DIM = 3
IN_DEGREE = 6
CROSS_EVERY = 12
ISOLATE_EVERY = 8
BLOCK_DECAY = 0.7
LOW_BAND = 0.25


def ladder(rng, lo=0.0, hi=1.0):
    """One weight from the exact ladder, inside the band [lo, hi)."""
    span = hi - lo
    return lo + span * (1 + rng.randrange(WEIGHT_LEVELS)) / WEIGHT_LEVELS


def knn(rng, n, k):
    if k < 1 or k >= n:
        sys.exit(f"knn needs 1 <= PARAM < N, got {k}")
    pts = [[rng.random() for _ in range(CLOUD_DIM)] for _ in range(n)]
    edges = {}
    for i in range(n):
        pi = pts[i]
        near = sorted(
            ((math.dist(pi, pts[j]), j) for j in range(n) if j != i),
            key=lambda t: (t[0], t[1]),
        )[:k]
        for d, j in near:
            edges[(max(i, j), min(i, j))] = d
    return [(u, v, d) for (u, v), d in edges.items()]


def powerlaw(rng, n, m):
    if m < 1 or m >= n:
        sys.exit(f"powerlaw needs 1 <= PARAM < N, got {m}")
    # The repeated-endpoint list makes attachment proportional to degree
    # without a scan over the degrees.
    targets = list(range(m + 1))
    edges = {}
    for v in range(m + 1, n):
        chosen = set()
        while len(chosen) < m:
            chosen.add(targets[rng.randrange(len(targets))])
        for u in sorted(chosen):
            edges[(v, u)] = ladder(rng)
            targets.append(u)
        targets.extend([v] * m)
    return [(u, v, d) for (u, v), d in edges.items()]


def block(rng, n, blocks):
    if blocks < 1 or blocks > n:
        sys.exit(f"block needs 1 <= PARAM <= N, got {blocks}")
    sizes, weight = [], 1.0
    for _ in range(blocks):
        sizes.append(weight)
        weight *= BLOCK_DECAY
    total = sum(sizes)
    sizes = [max(2, int(n * s / total)) for s in sizes]
    members, start = [], 0
    for size in sizes:
        members.append(list(range(start, min(start + size, n))))
        start += size
    members[-1].extend(range(start, n))
    home = {v: b for b, group in enumerate(members) for v in group}

    edges = {}
    for group in members:
        if len(group) < 2:
            continue
        for v in group:
            for _ in range(IN_DEGREE):
                u = group[rng.randrange(len(group))]
                if u != v:
                    edges[(max(u, v), min(u, v))] = ladder(rng, 0.0, LOW_BAND)
    for v in range(0, n, CROSS_EVERY):
        u = rng.randrange(n)
        if home.get(u) != home.get(v):
            edges[(max(u, v), min(u, v))] = ladder(rng, LOW_BAND, 1.0)
    return [(u, v, d) for (u, v), d in edges.items()]


def forest(rng, n, components):
    if components < 1 or components > n:
        sys.exit(f"forest needs 1 <= PARAM <= N, got {components}")
    joined = [v for v in range(n) if v % ISOLATE_EVERY != 0]
    if len(joined) <= components:
        sys.exit(f"forest needs more than {components} non-isolated vertices")
    roots, rest = joined[:components], joined[components:]
    grown = list(roots)
    edges = {}
    for v in rest:
        u = grown[rng.randrange(len(grown))]
        edges[(max(u, v), min(u, v))] = ladder(rng)
        grown.append(v)
    return [(u, v, d) for (u, v), d in edges.items()]


def nonmetric(rng, n, degree):
    if degree < 1 or degree >= n:
        sys.exit(f"nonmetric needs 1 <= PARAM < N, got {degree}")
    joined = [v for v in range(n) if v % ISOLATE_EVERY != 0]
    if len(joined) < 2:
        sys.exit("nonmetric needs at least two non-isolated vertices")
    edges = {}
    for v in joined:
        for _ in range(degree):
            u = joined[rng.randrange(len(joined))]
            if u != v:
                edges[(max(u, v), min(u, v))] = ladder(rng)
    return [(u, v, d) for (u, v), d in edges.items()]


GENERATORS = {
    "knn": knn,
    "powerlaw": powerlaw,
    "block": block,
    "forest": forest,
    "nonmetric": nonmetric,
}


def components_of(n, edges):
    """Connected components, isolated vertices included."""
    parent = list(range(n))

    def find(x):
        while parent[x] != x:
            parent[x] = parent[parent[x]]
            x = parent[x]
        return x

    for u, v, _ in edges:
        ru, rv = find(u), find(v)
        if ru != rv:
            parent[ru] = rv
    return sum(1 for v in range(n) if find(v) == v)


def main() -> None:
    if len(sys.argv) != 7:
        sys.exit("usage: gen_graph.py GENERATOR N SEED TAU PARAM OUT_SPARSE")
    name = sys.argv[1]
    if name not in GENERATORS:
        sys.exit(f"unknown generator {name!r}; pick one of {', '.join(sorted(GENERATORS))}")
    n, seed = int(sys.argv[2]), int(sys.argv[3])
    tau = float(sys.argv[4])
    param = int(sys.argv[5])
    out_path = sys.argv[6]
    if n < 2:
        sys.exit(f"need at least two vertices, got {n}")

    rng = random.Random(seed)
    drawn = GENERATORS[name](rng, n, param)
    if not drawn:
        sys.exit(f"{name}: the generator drew no edge")

    max_weight = max(d for _, _, d in drawn)
    threshold = tau * max_weight
    edges = [(u, v, d) for u, v, d in drawn if d <= threshold]
    if not edges:
        sys.exit(f"{name}: threshold {threshold!r} keeps no edge")

    degree = [0] * n
    for u, v, _ in edges:
        degree[u] += 1
        degree[v] += 1
    order = [v for v in range(n) if degree[v] == 0]
    isolated = len(order)
    order += [v for v in range(n) if degree[v] > 0]
    label = [0] * n
    for new, old in enumerate(order):
        label[old] = new
    relabelled = "yes" if any(old != new for new, old in enumerate(order)) else "no"

    # Sorted output keeps the file byte-identical whatever order the
    # generator produced its edges in.
    rows = sorted(
        (max(label[u], label[v]), min(label[u], label[v]), d) for u, v, d in edges
    )
    with open(out_path, "w") as f:
        for u, v, d in rows:
            f.write(f"{u} {v} {d!r}\n")

    pairs = n * (n - 1) // 2
    print(
        f"generator={name} n={n} param={param} pairs={pairs} edges={len(edges)} "
        f"density={len(edges) / pairs:.6f} mean_degree={2 * len(edges) / n:.2f} "
        f"max_degree={max(degree)} isolated={isolated} "
        f"components={components_of(n, edges)} relabelled={relabelled} "
        f"max_weight={max_weight!r} tau={tau!r} threshold={threshold!r}"
    )


if __name__ == "__main__":
    main()
