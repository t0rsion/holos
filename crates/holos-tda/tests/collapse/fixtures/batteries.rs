use super::*;

pub(crate) fn battery_points() -> DistanceMatrix {
    let mut rng = Rng::new(0x51ee_d001);
    let points: Vec<Vec<f64>> = (0..7).map(|_| vec![rng.uniform(), rng.uniform()]).collect();
    DistanceMatrix::from_points(&points).unwrap()
}

pub(crate) fn battery_ties() -> DistanceMatrix {
    let mut rng = Rng::new(0x51ee_d002);
    let palette = [1.0, 2.0];
    let condensed: Vec<f64> = (0..6 * 5 / 2)
        .map(|_| palette[rng.below(palette.len())])
        .collect();
    DistanceMatrix::from_condensed(condensed).unwrap()
}

pub(crate) fn battery_zeros() -> DistanceMatrix {
    let sites = [[0.0, 0.0], [1.0, 0.0], [0.5, 0.9]];
    let mut points = Vec::new();
    for site in sites {
        points.push(site.to_vec());
        points.push(site.to_vec());
    }
    DistanceMatrix::from_points(&points).unwrap()
}

pub(crate) fn battery_infinite() -> DistanceMatrix {
    let mut rng = Rng::new(0x51ee_d003);
    let palette = [1.0, 2.0, 3.0];
    let condensed: Vec<f64> = (0..7 * 6 / 2)
        .map(|_| {
            if rng.uniform() < 0.3 {
                f64::INFINITY
            } else {
                palette[rng.below(palette.len())]
            }
        })
        .collect();
    DistanceMatrix::from_condensed(condensed).unwrap()
}

pub(crate) fn battery_disconnected() -> DistanceMatrix {
    let edges = [
        (0, 1, 1.0),
        (0, 2, 1.0),
        (1, 2, 1.0),
        (3, 4, 2.0),
        (3, 5, 2.0),
        (4, 5, 2.0),
        (6, 7, 1.0),
        (6, 8, 2.0),
        (7, 8, 2.0),
    ];
    dense_from_edges(9, &edges)
}

pub(crate) fn scale_dense(dist: &DistanceMatrix, factor: f64) -> DistanceMatrix {
    let n = dist.len();
    let mut condensed = Vec::with_capacity(n * (n - 1) / 2);
    for i in 1..n {
        for j in 0..i {
            condensed.push(factor * dist.get(i, j));
        }
    }
    DistanceMatrix::from_condensed(condensed).unwrap()
}

pub(crate) fn permute_dense(dist: &DistanceMatrix, perm: &[usize]) -> DistanceMatrix {
    let n = dist.len();
    let mut condensed = Vec::with_capacity(n * (n - 1) / 2);
    for i in 1..n {
        for j in 0..i {
            condensed.push(dist.get(perm[i], perm[j]));
        }
    }
    DistanceMatrix::from_condensed(condensed).unwrap()
}

/// A single fixed apex cannot certify edge (0, 1): vertex 2 is the only
/// candidate at level 1, but it is not adjacent to vertex 3, which joins the
/// candidate set at level 2. Vertex 4 covers the upper level. Vertices 5
/// through 9 are private blockers: each one keeps a value-2 edge alive
/// through the first sweep, so the candidate set of (0, 1) is still complete
/// when the schedule reaches it.
pub(crate) fn level_dependent_apex_matrix() -> DistanceMatrix {
    let edges = [
        (0, 1, 1.0),
        (0, 2, 1.0),
        (1, 2, 1.0),
        (2, 4, 1.0),
        (0, 3, 2.0),
        (1, 3, 2.0),
        (0, 4, 2.0),
        (1, 4, 2.0),
        (3, 4, 2.0),
        (0, 5, 2.0),
        (3, 5, 2.0),
        (1, 6, 2.0),
        (3, 6, 2.0),
        (0, 7, 2.0),
        (4, 7, 2.0),
        (1, 8, 2.0),
        (4, 8, 2.0),
        (3, 9, 2.0),
        (4, 9, 2.0),
    ];
    dense_from_edges(10, &edges)
}

/// Edge (0, 1) has two candidates, 2 and 3, that are not adjacent, so no apex
/// dominates it in pass 1. Vertices 4 and 5 block the two edges that would
/// otherwise dissolve the candidate 2. Edge (0, 3) leaves in pass 1, which
/// drops candidate 3 and makes (0, 1) removable in pass 2.
pub(crate) fn later_pass_matrix() -> DistanceMatrix {
    let edges = [
        (0, 1, 1.0),
        (0, 2, 1.0),
        (1, 2, 1.0),
        (0, 3, 1.0),
        (1, 3, 1.0),
        (0, 4, 1.0),
        (2, 4, 1.0),
        (1, 5, 1.0),
        (2, 5, 1.0),
    ];
    dense_from_edges(6, &edges)
}

pub(crate) fn bipartite_dist(i: usize, j: usize) -> f64 {
    if (i < BIP_N / 2) != (j < BIP_N / 2) {
        1.0
    } else {
        f64::INFINITY
    }
}

pub(crate) fn bipartite_dense() -> DistanceMatrix {
    let mut data = Vec::with_capacity(BIP_N * (BIP_N - 1) / 2);
    for i in 1..BIP_N {
        for j in 0..i {
            data.push(bipartite_dist(i, j));
        }
    }
    DistanceMatrix::from_condensed(data).unwrap()
}

pub(crate) fn bipartite_k4_dist(i: usize, j: usize) -> f64 {
    let across_bipartite = i < BIP_N && j < BIP_N && (i < BIP_N / 2) != (j < BIP_N / 2);
    let inside_k4 = i >= BIP_N && j >= BIP_N;
    if across_bipartite || inside_k4 {
        1.0
    } else {
        f64::INFINITY
    }
}

pub(crate) fn bipartite_k4_dense() -> DistanceMatrix {
    let n = BIP_N + K4_N;
    let mut data = Vec::with_capacity(n * (n - 1) / 2);
    for i in 1..n {
        for j in 0..i {
            data.push(bipartite_k4_dist(i, j));
        }
    }
    DistanceMatrix::from_condensed(data).unwrap()
}
