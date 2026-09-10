use super::*;
use holos_tda::DistanceMatrix;

/// The complete graph on `n` vertices, every edge at distance 1.
pub(crate) fn complete_matrix(n: usize) -> DistanceMatrix {
    let mut edges = Vec::new();
    for u in 0..n {
        for v in (u + 1)..n {
            edges.push((u, v, 1.0));
        }
    }
    dense_from_edges(n, &edges)
}

/// `blocks` disjoint copies of K4, every edge at distance 1.
pub(crate) fn disjoint_k4_matrix(blocks: usize) -> DistanceMatrix {
    let n = 4 * blocks;
    let mut edges = Vec::new();
    for b in 0..blocks {
        let base = 4 * b;
        for u in 0..4 {
            for v in (u + 1)..4 {
                edges.push((base + u, base + v, 1.0));
            }
        }
    }
    dense_from_edges(n, &edges)
}

/// K4 minus the pair (0, 3): two triangles sharing the edge (1, 2).
pub(crate) fn diamond_matrix() -> DistanceMatrix {
    dense_from_edges(
        4,
        &[
            (0, 1, 1.0),
            (0, 2, 1.0),
            (1, 2, 1.0),
            (1, 3, 1.0),
            (2, 3, 1.0),
        ],
    )
}

/// Edge (0, 1) has two candidates, 2 and 3, that are not adjacent, so no
/// apex dominates it in round 1. Vertices 4 and 5 block the two edges that
/// would otherwise dissolve candidate 2. Round 1 drops (0, 3), which drops
/// candidate 3 and makes (0, 1) removable in round 2.
pub(crate) fn later_round_matrix() -> DistanceMatrix {
    dense_from_edges(
        6,
        &[
            (0, 1, 1.0),
            (0, 2, 1.0),
            (1, 2, 1.0),
            (0, 3, 1.0),
            (1, 3, 1.0),
            (0, 4, 1.0),
            (2, 4, 1.0),
            (1, 5, 1.0),
            (2, 5, 1.0),
        ],
    )
}

/// Vertices 0 and 1 share every other vertex as a common neighbor, and
/// vertex 2 is adjacent to all of them, so (0, 1) is removable and its
/// closed common neighborhood is the whole graph. With 76 vertices that set
/// is far past the marking limit, so production must drop fine marking and
/// retest everything in the next round. The leaves carry no edges among
/// themselves, so the graph still reaches its fixed point in three rounds.
pub(crate) const FALLBACK_N: usize = 76;

pub(crate) fn fallback_matrix() -> DistanceMatrix {
    let mut edges = vec![(0, 1, 1.0)];
    for x in 2..FALLBACK_N {
        edges.push((0, x, 1.0));
        edges.push((1, x, 1.0));
    }
    for x in 3..FALLBACK_N {
        edges.push((2, x, 1.0));
    }
    dense_from_edges(FALLBACK_N, &edges)
}

// K64,64 with every present edge at distance 1. The bipartite graph is
// triangle-free, so no edge has a candidate and the collapse must return the
// input untouched. The barcode is known: one component and
// b1 = 4096 - 128 + 1 = 3969 essential H1 classes.
pub(crate) const BIP_N: usize = 128;
pub(crate) const K4_N: usize = 4;

pub(crate) fn bipartite_dense() -> DistanceMatrix {
    let mut data = Vec::with_capacity(BIP_N * (BIP_N - 1) / 2);
    for i in 1..BIP_N {
        for j in 0..i {
            let across = (i < BIP_N / 2) != (j < BIP_N / 2);
            data.push(if across { 1.0 } else { f64::INFINITY });
        }
    }
    DistanceMatrix::from_condensed(data).unwrap()
}

// The same bipartite block plus a disjoint K4. The K4 is the only place
// where a removal can happen, so it separates the two halves of the
// schedule.
pub(crate) fn bipartite_k4_dense() -> DistanceMatrix {
    let n = BIP_N + K4_N;
    let mut data = Vec::with_capacity(n * (n - 1) / 2);
    for i in 1..n {
        for j in 0..i {
            let across = i < BIP_N && j < BIP_N && (i < BIP_N / 2) != (j < BIP_N / 2);
            let inside_k4 = i >= BIP_N && j >= BIP_N;
            data.push(if across || inside_k4 {
                1.0
            } else {
                f64::INFINITY
            });
        }
    }
    DistanceMatrix::from_condensed(data).unwrap()
}

/// L1 distances on a 4x4 grid: only the integers 1 through 6, so almost
/// every candidate shares a birth level with its neighbors.
pub(crate) fn tie_heavy_grid() -> DistanceMatrix {
    let side = 4i64;
    let coords: Vec<(i64, i64)> = (0..side)
        .flat_map(|x| (0..side).map(move |y| (x, y)))
        .collect();
    let mut condensed = Vec::new();
    for i in 1..coords.len() {
        for j in 0..i {
            let d = (coords[i].0 - coords[j].0).abs() + (coords[i].1 - coords[j].1).abs();
            condensed.push(d as f64);
        }
    }
    DistanceMatrix::from_condensed(condensed).unwrap()
}

/// A seeded random graph on two values plus absent pairs.
pub(crate) fn random_matrix(seed: u64, n: usize, density: f64) -> DistanceMatrix {
    let mut rng = Rng::new(seed);
    let mut edges = Vec::new();
    for u in 0..n {
        for v in (u + 1)..n {
            if rng.uniform() < density {
                edges.push((u, v, if rng.uniform() < 0.5 { 1.0 } else { 2.0 }));
            }
        }
    }
    dense_from_edges(n, &edges)
}

// Generic point cloud: distinct values, no ties, no absent edges.
pub(crate) fn battery_points() -> DistanceMatrix {
    let mut rng = Rng::new(0x51ee_d001);
    let points: Vec<Vec<f64>> = (0..7).map(|_| vec![rng.uniform(), rng.uniform()]).collect();
    DistanceMatrix::from_points(&points).unwrap()
}

// Two-value palette: every comparison in the predicate meets a tie.
pub(crate) fn battery_ties() -> DistanceMatrix {
    let mut rng = Rng::new(0x51ee_d002);
    let palette = [1.0, 2.0];
    let condensed: Vec<f64> = (0..6 * 5 / 2)
        .map(|_| palette[rng.below(palette.len())])
        .collect();
    DistanceMatrix::from_condensed(condensed).unwrap()
}
