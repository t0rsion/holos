use super::*;

#[test]
fn ordered_matches_serial_v1_and_reference() {
    // Small tie-heavy graphs with zeros and absent pairs, over the three
    // threshold shapes, dense and sparse.
    let palette = [0.0, 1.0, 1.0, 2.0, 2.0, 3.0, f64::INFINITY];
    let mut rng = Rng::new(0x0de5_5eed_0001);
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
        // Half the cases take the production window at four workers; the
        // other half force a small window and a drawn worker count, so
        // stages cross the input and the retire walk repairs across
        // stage boundaries.
        let (threads, window) = if it % 2 == 0 {
            (4, None)
        } else {
            let windows = [1, 2, 3, 5];
            let workers = [2, 4, 8];
            (workers[rng.below(3)], Some(windows[rng.below(4)]))
        };
        let name = format!(
            "random {it} (n={n} threshold={threshold:?} threads={threads} window={window:?})"
        );

        removed_total += assert_trace_dense(&name, &dense, threshold, threads, window);
        assert_trace_sparse(&name, &sparse, threshold, threads, window);
    }
    assert!(
        removed_total > 100,
        "the sweep never collapsed anything: {removed_total}"
    );

    // A 76-vertex two-value graph. Some removal here sees a common
    // neighborhood past the marking limit, so the pruning falls back to
    // retesting every live edge and the ordered run invalidates a whole
    // window remainder.
    let mut rng = Rng::new(0x0de5_5eed_0002);
    let n = 76;
    let mut edges = Vec::new();
    for u in 0..n {
        for v in (u + 1)..n {
            if rng.uniform() < 0.95 {
                edges.push((u, v, if rng.uniform() < 0.5 { 1.0 } else { 2.0 }));
            }
        }
    }
    let dense = dense_from_edges(n, &edges);
    assert_trace_dense("dense76", &dense, Some(2.0), 4, None);

    // The mixed-yield fixture: bipartite edges no schedule can touch, plus
    // a K4 that collapses.
    let dense = bipartite_k4_dense();
    assert_trace_dense("k64_64+k4 dense", &dense, None, 4, None);
    let sparse = sparse_from_dense(&dense);
    assert_trace_sparse("k64_64+k4 sparse", &sparse, None, 4, None);
}
