use super::*;
use holos_tda::DistanceMatrix;

#[test]
fn production_matches_unpruned_v2_reference() {
    // Small tie-heavy graphs: zeros, repeated values, and absent pairs, over
    // the three threshold shapes, dense and sparse, at one and four workers.
    let palette = [0.0, 1.0, 1.0, 2.0, 2.0, 3.0, f64::INFINITY];
    let mut rng = Rng::new(0x2ec0_11a9_5e02);
    let mut removed_total = 0usize;
    for it in 0..250 {
        let n = 2 + rng.below(11);
        let data: Vec<f64> = (0..n * (n - 1) / 2)
            .map(|_| palette[rng.below(palette.len())])
            .collect();
        let dense = DistanceMatrix::from_condensed(data).unwrap();
        let sparse = sparse_from_dense(&dense);
        let threshold = match rng.below(3) {
            0 => None,
            1 => Some(2.0),
            _ => Some(f64::INFINITY),
        };
        let dense_reference = reference_dense(&dense, threshold);
        let sparse_reference = reference_sparse(&sparse, threshold);
        for &threads in &[1usize, 4] {
            let name = format!("random {it} (n={n} threshold={threshold:?} threads={threads})");
            let result = v2_dense(&name, &dense, threshold, threads);
            assert_reference_match(&name, &result, &dense_reference);
            removed_total += result.stats.removed_edges;
            let result = v2_sparse(&name, &sparse, threshold, threads);
            assert_reference_match(&name, &result, &sparse_reference);
        }
    }
    assert!(
        removed_total > 100,
        "the sweep never collapsed anything: {removed_total}"
    );

    // A denser seeded graph on two values: wider candidate sets, more
    // conflicts, and many rounds.
    let dense = random_matrix(0x2ec0_11a9_5e03, 24, 0.7);
    let threshold = Some(2.0);
    let reference = reference_dense(&dense, threshold);
    for &threads in &[1usize, 4] {
        let name = format!("random24 (threads={threads})");
        let result = v2_dense(&name, &dense, threshold, threads);
        assert_reference_match(&name, &result, &reference);
        assert!(result.stats.removed_edges > 0, "{name}: no removal");
    }

    // The marking fallback: the first removal reads a closed common
    // neighborhood of every vertex in the graph, far past the marking limit,
    // so production retests everything in the next round. The reference
    // never prunes, so this is the gate on that fallback.
    let dense = fallback_matrix();
    let threshold = Some(1.0);
    let reference = reference_dense(&dense, threshold);
    let result = v2_dense("fallback", &dense, threshold, 4);
    assert_reference_match("fallback", &result, &reference);
    let widest = widest_round_read_set(FALLBACK_N, &thresholded_dense(&dense, threshold), &result);
    assert!(
        widest > MARK_LIMIT,
        "fallback: widest read set is {widest}, too small to force the retest fallback"
    );

    // The mixed-yield fixture: 4,096 bipartite edges that no schedule can
    // touch, plus a K4 that collapses.
    let dense = bipartite_k4_dense();
    let sparse = sparse_from_dense(&dense);
    let dense_reference = reference_dense(&dense, None);
    let sparse_reference = reference_sparse(&sparse, None);
    for &threads in &[1usize, 4] {
        let name = format!("k64_64+k4 (threads={threads})");
        let result = v2_dense(&name, &dense, None, threads);
        assert_reference_match(&name, &result, &dense_reference);
        let result = v2_sparse(&name, &sparse, None, threads);
        assert_reference_match(&name, &result, &sparse_reference);
    }
}
