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


def distance_matrix(points):
    size = len(points)
    matrix = [[0.0] * size for _ in range(size)]
    for i, point in enumerate(points):
        for j in range(i):
            distance = math.dist(point, points[j])
            matrix[i][j] = distance
            matrix[j][i] = distance
    return matrix


def graph_degrees(matrix, threshold):
    degree = [0] * len(matrix)
    edges = 0
    for i, row in enumerate(matrix):
        for j in range(i):
            if row[j] <= threshold:
                degree[i] += 1
                degree[j] += 1
                edges += 1
    return degree, edges


def vertex_order(degree):
    isolated = [vertex for vertex, value in enumerate(degree) if value == 0]
    connected = [vertex for vertex, value in enumerate(degree) if value > 0]
    return isolated + connected, len(isolated)


def write_sparse(path, matrix, order, threshold):
    with open(path, "w") as output:
        for a in range(1, len(order)):
            row = matrix[order[a]]
            for b in range(a):
                distance = row[order[b]]
                if distance <= threshold:
                    output.write(f"{a} {b} {distance!r}\n")


def write_lower(path, matrix):
    with open(path, "w") as output:
        for i in range(1, len(matrix)):
            output.write(" ".join(repr(matrix[i][j]) for j in range(i)) + "\n")


def metadata(size, edges, degree, isolated, relabelled, radius, tau, threshold):
    pairs = size * (size - 1) // 2
    return (
        f"n={size} pairs={pairs} edges={edges} density={edges / pairs:.6f} "
        f"mean_degree={2 * edges / size:.2f} max_degree={max(degree)} "
        f"isolated={isolated} relabelled={relabelled} "
        f"enclosing_radius={radius!r} tau={tau!r} threshold={threshold!r}"
    )


def main() -> None:
    if len(sys.argv) not in (4, 5):
        sys.exit("usage: densify_to_sparse.py CLOUD TAU OUT_SPARSE [OUT_LOWER]")
    cloud_path, tau, sparse_path = sys.argv[1], float(sys.argv[2]), sys.argv[3]
    lower_path = sys.argv[4] if len(sys.argv) == 5 else None

    pts = read_cloud(cloud_path)
    n = len(pts)
    if n < 2:
        sys.exit(f"{cloud_path}: need at least two points, got {n}")

    dmat = distance_matrix(pts)
    radius = min(max(row) for row in dmat)
    threshold = tau * radius
    degree, edges = graph_degrees(dmat, threshold)
    if edges == 0:
        sys.exit(f"{cloud_path}: threshold {threshold!r} keeps no edge")

    order, isolated = vertex_order(degree)
    relabelled = "yes" if any(old != new for new, old in enumerate(order)) else "no"
    write_sparse(sparse_path, dmat, order, threshold)
    if lower_path is not None:
        write_lower(lower_path, dmat)
    print(metadata(n, edges, degree, isolated, relabelled, radius, tau, threshold))


if __name__ == "__main__":
    main()
