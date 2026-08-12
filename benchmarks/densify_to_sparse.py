#!/usr/bin/env python3
"""Threshold a point cloud into ripser-style sparse triplets.

Usage: densify_to_sparse.py CLOUD TAU OUT_SPARSE [OUT_LOWER]

Computes every pairwise distance once, takes the enclosing radius
R = min over i of max over j d(i, j), sets the threshold T = TAU * R, and
writes one "i j d" line per pair with d <= T. TAU = 1.0 reproduces the dense
default threshold, and the multiplication by 1.0 is exact.

OUT_LOWER, when given, receives the condensed lower triangle of the same
distances. The dense and the sparse input then carry bit-identical values:
both are repr() of the same math.dist result.

The sparse reader takes the point count from the largest index it sees, so a
trailing vertex with no edge would shrink the graph and drop an essential H0
bar. Vertices are therefore relabelled: isolated vertices take the low
indices, vertices with an edge follow in their original order. Relabelling
moves no bar, because the barcode does not depend on the labelling. OUT_LOWER
keeps the original labelling.

Prints one metadata line of whitespace-separated key=value fields.
"""
import math
import sys


def read_cloud(path):
    pts = []
    with open(path) as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            pts.append([float(t) for t in line.replace(",", " ").split()])
    return pts


def main() -> None:
    if len(sys.argv) not in (4, 5):
        sys.exit("usage: densify_to_sparse.py CLOUD TAU OUT_SPARSE [OUT_LOWER]")
    cloud_path, tau, sparse_path = sys.argv[1], float(sys.argv[2]), sys.argv[3]
    lower_path = sys.argv[4] if len(sys.argv) == 5 else None

    pts = read_cloud(cloud_path)
    n = len(pts)
    if n < 2:
        sys.exit(f"{cloud_path}: need at least two points, got {n}")

    dmat = [[0.0] * n for _ in range(n)]
    for i in range(n):
        row, pi = dmat[i], pts[i]
        for j in range(i):
            d = math.dist(pi, pts[j])
            row[j] = d
            dmat[j][i] = d

    radius = min(max(row) for row in dmat)
    threshold = tau * radius

    degree = [0] * n
    edges = 0
    for i in range(n):
        row = dmat[i]
        for j in range(i):
            if row[j] <= threshold:
                degree[i] += 1
                degree[j] += 1
                edges += 1
    if edges == 0:
        sys.exit(f"{cloud_path}: threshold {threshold!r} keeps no edge")

    order = [i for i in range(n) if degree[i] == 0]
    isolated = len(order)
    order += [i for i in range(n) if degree[i] > 0]
    relabelled = "yes" if any(old != new for new, old in enumerate(order)) else "no"

    with open(sparse_path, "w") as f:
        for a in range(1, n):
            i = dmat[order[a]]
            for b in range(a):
                d = i[order[b]]
                if d <= threshold:
                    f.write(f"{a} {b} {d!r}\n")

    if lower_path is not None:
        with open(lower_path, "w") as f:
            for i in range(1, n):
                f.write(" ".join(repr(dmat[i][j]) for j in range(i)) + "\n")

    pairs = n * (n - 1) // 2
    print(
        f"n={n} pairs={pairs} edges={edges} density={edges / pairs:.6f} "
        f"mean_degree={2 * edges / n:.2f} max_degree={max(degree)} "
        f"isolated={isolated} relabelled={relabelled} "
        f"enclosing_radius={radius!r} tau={tau!r} threshold={threshold!r}"
    )


if __name__ == "__main__":
    main()
