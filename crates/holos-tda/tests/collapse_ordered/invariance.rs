use super::*;

#[test]
fn ordered_is_invariant_across_workers_and_windows() {
    for (name, dense, threshold) in invariance_inputs() {
        let sparse = sparse_from_dense(&dense);
        let serial_dense = collapse_dense(&dense, threshold).unwrap();
        let serial_sparse = collapse_sparse(&sparse, threshold).unwrap();
        // The baseline is a real speculative run, two workers at window
        // one; zero workers would delegate to the serial implementation
        // and compare it with itself.
        let base_dense = collapse_dense_ordered_with_window(&dense, threshold, 2, 1).unwrap();
        let base_sparse = collapse_sparse_ordered_with_window(&sparse, threshold, 2, 1).unwrap();
        assert_matches_serial(
            &format!("{name}: baseline dense"),
            &base_dense,
            &serial_dense,
        );
        assert_matches_serial(
            &format!("{name}: baseline sparse"),
            &base_sparse,
            &serial_sparse,
        );

        for &workers in &WORKERS {
            // The production window enters through the entry point that
            // takes no window; the rest are forced.
            let mut runs = vec![(
                "production".to_string(),
                collapse_dense_ordered_parallel(&dense, threshold, workers).unwrap(),
                collapse_sparse_ordered_parallel(&sparse, threshold, workers).unwrap(),
            )];
            for &window in &WINDOWS {
                runs.push((
                    format!("W={window}"),
                    collapse_dense_ordered_with_window(&dense, threshold, workers, window).unwrap(),
                    collapse_sparse_ordered_with_window(&sparse, threshold, workers, window)
                        .unwrap(),
                ));
            }
            for (label, got_dense, got_sparse) in runs {
                let label = format!("{name}: workers={workers} {label}");
                // Structural fields are invariant; edge_tests,
                // max_common_neighborhood, and the scheduling counters are
                // exempt and are never compared across configurations.
                assert_same_output(&format!("{label} dense"), &got_dense, &base_dense);
                assert_invariant_stats(&format!("{label} dense"), &got_dense, &base_dense);
                assert_work_bound(&format!("{label} dense"), &got_dense);
                assert_same_output(&format!("{label} sparse"), &got_sparse, &base_sparse);
                assert_invariant_stats(&format!("{label} sparse"), &got_sparse, &base_sparse);
                assert_work_bound(&format!("{label} sparse"), &got_sparse);
            }
        }
    }
}

#[test]
fn ordered_preserves_the_diagram() {
    let mut rng = Rng::new(0x0d1a_6a20_0001);
    let points: Vec<Vec<f64>> = (0..7).map(|_| vec![rng.uniform(), rng.uniform()]).collect();
    let cloud = DistanceMatrix::from_points(&points).unwrap();
    assert_ordered_preserves_diagram("points", &cloud, 0.7);
    assert_ordered_preserves_diagram("ties", &battery_ties(), 1.0);

    // Coincident points and absent pairs: zero-value edges at the bottom
    // of the filtration, +inf as a missing edge.
    let sites = [[0.0, 0.0], [1.0, 0.0], [0.5, 0.9]];
    let mut points = Vec::new();
    for site in sites {
        points.push(site.to_vec());
        points.push(site.to_vec());
    }
    let zeros = DistanceMatrix::from_points(&points).unwrap();
    assert_ordered_preserves_diagram("zeros", &zeros, 0.6);

    // A gross triangle-inequality violation: domination is a graph
    // property, so the long edge still goes.
    let non_metric = dense_from_edges(
        5,
        &[
            (0, 1, 10.0),
            (0, 2, 1.0),
            (1, 2, 1.0),
            (0, 3, 5.0),
            (1, 3, 5.0),
            (2, 3, 0.5),
            (2, 4, 3.0),
            (3, 4, 3.0),
        ],
    );
    assert_ordered_preserves_diagram("non_metric", &non_metric, 5.0);
}
