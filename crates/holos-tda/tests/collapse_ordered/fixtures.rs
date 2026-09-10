use super::*;

// K64,64 with every present edge at distance 1, plus a disjoint K4. The
// bipartite block is triangle-free, so every removal must sit in the K4.
pub(crate) const BIP_N: usize = 128;
pub(crate) const K4_N: usize = 4;

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

/// Inputs with different pass shapes, yields, and densities.
pub(crate) fn invariance_inputs() -> Vec<(String, DistanceMatrix, Option<f64>)> {
    let mut rng = Rng::new(0x1ab5_e1ce_0001);
    let points: Vec<Vec<f64>> = (0..7).map(|_| vec![rng.uniform(), rng.uniform()]).collect();
    let cloud = DistanceMatrix::from_points(&points).unwrap();

    let palette = [1.0, 2.0, f64::INFINITY];
    let n = 24;
    let data: Vec<f64> = (0..n * (n - 1) / 2)
        .map(|_| palette[rng.below(palette.len())])
        .collect();
    let mixed = DistanceMatrix::from_condensed(data).unwrap();

    let mut clique = Vec::new();
    for u in 0..16 {
        for v in (u + 1)..16 {
            clique.push((u, v, 1.0));
        }
    }
    let clique = dense_from_edges(16, &clique);

    vec![
        ("cloud".to_string(), cloud, None),
        ("ties/inf".to_string(), battery_ties(), Some(f64::INFINITY)),
        ("mixed".to_string(), mixed, Some(2.0)),
        ("clique16".to_string(), clique, Some(1.0)),
        ("later_pass".to_string(), later_pass_matrix(), Some(1.0)),
        (
            "forward_arming".to_string(),
            forward_arming_matrix(),
            Some(2.0),
        ),
        ("book".to_string(), book_matrix(1.0), Some(2.0)),
    ]
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

/// Run one fixture through the ordered path and gate it against both
/// references, then return the ordered result for the fixture's own
/// counter checks.
pub(crate) fn fixture(
    name: &str,
    dense: &DistanceMatrix,
    threshold: Option<f64>,
    threads: usize,
    window: usize,
) -> CollapsedRips {
    let ordered = collapse_dense_ordered_with_window(dense, threshold, threads, window).unwrap();
    let serial = collapse_dense(dense, threshold).unwrap();
    assert_matches_serial(name, &ordered, &serial);
    assert_matches_reference(name, &ordered, &reference_dense(dense, threshold));
    verify_dense(dense, threshold, &ordered)
        .unwrap_or_else(|e| panic!("{name}: verifier rejected the certificate: {e}"));
    ordered
}

/// Edge (0, 1) has two candidates, 2 and 3, that are not adjacent, so no
/// apex dominates it in pass 1. Edge (0, 3) leaves in pass 1, which drops
/// candidate 3 and makes (0, 1) removable in pass 2.
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

/// Component 1: edge (0, 1) is blocked by the non-adjacent candidates 2
/// and 3; vertices 4 and 5 block (1, 3) and (0, 3) through pass 1, and 6
/// and 7 block (0, 2) and (1, 2). Pass 1 removes (1, 4), (0, 5), (0, 6),
/// and (1, 7), which leaves (0, 3) due in pass 2 and (0, 1) clean.
/// Component 2 is a copy of the later-pass gadget at value 0.5 on
/// vertices 8 to 13; its (8, 9) is also due in pass 2 and sits after
/// (0, 1) in the schedule, so (0, 1) is armed inside a retirement span.
pub(crate) fn forward_arming_matrix() -> DistanceMatrix {
    let mut edges = vec![
        (0, 1, 1.0),
        (0, 2, 1.0),
        (1, 2, 1.0),
        (0, 3, 2.0),
        (1, 3, 2.0),
        (1, 4, 2.0),
        (3, 4, 2.0),
        (0, 5, 2.0),
        (3, 5, 2.0),
        (0, 6, 1.0),
        (2, 6, 1.0),
        (1, 7, 1.0),
        (2, 7, 1.0),
    ];
    for &(u, v) in &[
        (8, 9),
        (8, 10),
        (9, 10),
        (8, 11),
        (9, 11),
        (8, 12),
        (10, 12),
        (9, 13),
        (10, 13),
    ] {
        edges.push((u, v, 0.5));
    }
    dense_from_edges(14, &edges)
}

/// Two disjoint gadgets on a shared spine (0, 1). In each gadget the
/// spoke (0, x) is blocked by the non-adjacent pair {1, z} and the spoke
/// (1, x) has the single candidate 0, so exactly (1, 2) and (1, 4) leave
/// in pass 1. The value 1.8 and 1.5 edges keep their only candidate born
/// above their own value, so they never fire in pass 1.
///
/// `spine` places the spine edge in the schedule: below 2.0 puts it last,
/// where both removals conflict with it; at 2.0 it retires first, where
/// neither does.
pub(crate) fn book_matrix(spine: f64) -> DistanceMatrix {
    let edges = [
        (0, 1, spine),
        (0, 2, 2.0),
        (1, 2, 2.0),
        (2, 3, 1.8),
        (0, 3, 1.5),
        (0, 4, 2.0),
        (1, 4, 2.0),
        (4, 5, 1.8),
        (0, 5, 1.5),
    ];
    dense_from_edges(6, &edges)
}
