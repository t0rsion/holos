use super::fixtures::*;

#[test]
fn with_edge_collapse_sets_the_flag() {
    assert!(
        !RipsParams::new(1).collapse_edges,
        "edge collapse must be off by default"
    );
    assert!(
        !RipsParams::default().collapse_edges,
        "the default params must not collapse"
    );
    let p = RipsParams::new(2).with_edge_collapse();
    assert!(p.collapse_edges, "with_edge_collapse must set the flag");
    assert_eq!(p.max_dim, 2, "with_edge_collapse must keep max_dim");
    assert_eq!(
        p.collapse_schedule,
        CollapseSchedule::Serial,
        "the serial schedule must be the default"
    );
    for schedule in [
        CollapseSchedule::Ordered,
        CollapseSchedule::Rounds,
        CollapseSchedule::Adaptive,
    ] {
        let p = RipsParams::new(2).with_collapse_schedule(schedule);
        assert!(p.collapse_edges, "with_collapse_schedule must set the flag");
        assert_eq!(p.collapse_schedule, schedule);
    }
}

#[test]
fn every_schedule_gives_the_same_diagram_at_every_thread_count() {
    let fixtures: [(&str, DistanceMatrix); 5] = [
        ("apex", level_dependent_apex_matrix()),
        ("points", battery_points()),
        ("ties", battery_ties()),
        ("zeros", battery_zeros()),
        ("disconnected", battery_disconnected()),
    ];
    let schedules = [
        CollapseSchedule::Serial,
        CollapseSchedule::Ordered,
        CollapseSchedule::Rounds,
        CollapseSchedule::Adaptive,
    ];
    for (name, dense) in &fixtures {
        let sparse = sparse_from_dense(dense);
        for &modulus in &MODULI {
            for threshold in [None, Some(2.0), Some(f64::INFINITY)] {
                for max_dim in 0..=2 {
                    let plain = dense_bars(dense, max_dim, threshold, modulus, 1, ALL_ON, false);
                    let plain_sparse =
                        sparse_bars(&sparse, max_dim, threshold, modulus, 1, ALL_ON, false);
                    for &threads in &THREAD_COUNTS {
                        for schedule in schedules {
                            let label = format!(
                                "{name}: p={modulus} threshold={threshold:?} max_dim={max_dim} \
                                 threads={threads} schedule={schedule:?}"
                            );
                            let p = params(max_dim, threshold, modulus, threads, ALL_ON, false)
                                .with_collapse_schedule(schedule);
                            assert_eq!(
                                canon(&rips_persistence(dense, &p).unwrap()),
                                plain,
                                "{label}: dense"
                            );
                            assert_eq!(
                                canon(&rips_persistence_sparse(&sparse, &p).unwrap()),
                                plain_sparse,
                                "{label}: sparse"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn convenience_path_matches_the_standalone_path() {
    // The flag on RipsParams runs the serial version 1 schedule, whose
    // output equals the serial entry point's exactly; the version 2 entry
    // point follows a different schedule and stops at a different graph.
    // Both must give the same diagram as the pipeline.
    let dense = level_dependent_apex_matrix();
    let sparse = sparse_from_dense(&dense);
    let standalone_bars = |collapsed: &CollapsedRips, max_dim: usize, modulus: u32| {
        let inner = params(
            max_dim,
            Some(collapsed.certificate.terminal_level()),
            modulus,
            1,
            ALL_ON,
            false,
        );
        canon(&rips_persistence_sparse(&collapsed.matrix, &inner).unwrap())
    };
    for &modulus in &MODULI {
        for threshold in [None, Some(2.0), Some(f64::INFINITY)] {
            for max_dim in 0..=2 {
                let label = format!("p={modulus} threshold={threshold:?} max_dim={max_dim}");

                let convenience = dense_bars(&dense, max_dim, threshold, modulus, 1, ALL_ON, true);
                let collapsed = collapse_dense_rounds_parallel(&dense, threshold, 1).unwrap();
                assert_eq!(
                    convenience,
                    standalone_bars(&collapsed, max_dim, modulus),
                    "{label}: dense entry point"
                );
                let serial = collapse_dense(&dense, threshold).unwrap();
                assert_eq!(
                    convenience,
                    standalone_bars(&serial, max_dim, modulus),
                    "{label}: dense serial schedule"
                );

                let convenience =
                    sparse_bars(&sparse, max_dim, threshold, modulus, 1, ALL_ON, true);
                let collapsed = collapse_sparse_rounds_parallel(&sparse, threshold, 1).unwrap();
                assert_eq!(
                    convenience,
                    standalone_bars(&collapsed, max_dim, modulus),
                    "{label}: sparse entry point"
                );
                let serial = collapse_sparse(&sparse, threshold).unwrap();
                assert_eq!(
                    convenience,
                    standalone_bars(&serial, max_dim, modulus),
                    "{label}: sparse serial schedule"
                );
            }
        }
    }
}

#[test]
fn sparse_edges_are_sorted_deduplicated_and_exact() {
    let triplets = [
        (4usize, 1usize, 2.5f64),
        (0, 3, 1.0),
        (2, 0, 0.5),
        (3, 4, 0.0),
        (1, 0, 1.5),
    ];
    let matrix = sparse_from_edges(5, &triplets);
    let edges = edge_list(&matrix);
    assert_eq!(
        edges,
        vec![
            (0, 1, 1.5),
            (0, 2, 0.5),
            (0, 3, 1.0),
            (1, 4, 2.5),
            (3, 4, 0.0),
        ],
        "edges() must yield each pair once, ordered, with exact values"
    );
    assert_eq!(edges.len(), matrix.num_edges(), "one item per stored edge");
    for &(u, v, d) in &edges {
        assert!(u < v, "edge ({u}, {v}) not ordered");
        assert_eq!(
            matrix.get(u, v),
            d,
            "edge ({u}, {v}) value differs from get"
        );
        assert_eq!(matrix.get(v, u), d, "edge ({u}, {v}) is not symmetric");
    }

    // The same iterator on a collapsed matrix: still sorted and unique.
    let collapsed = collapse_dense(&level_dependent_apex_matrix(), Some(2.0)).unwrap();
    let edges = edge_list(&collapsed.matrix);
    assert_eq!(
        edges.len(),
        collapsed.matrix.num_edges(),
        "collapsed matrix: one item per edge"
    );
    for pair in edges.windows(2) {
        assert!(
            (pair[0].0, pair[0].1) < (pair[1].0, pair[1].1),
            "collapsed matrix: edges out of order at {:?}",
            pair
        );
    }
}

#[test]
fn random_certificates_pass_the_independent_verifier() {
    let palette = [0.0, 0.5, 1.0, 1.0, 2.0, 2.5, f64::INFINITY];
    let mut rng = Rng(0xdead_beef_1234_5677);
    let mut removed_total = 0usize;
    for it in 0..300 {
        let n = 2 + rng.below(9);
        let m = n * (n - 1) / 2;
        let data: Vec<f64> = (0..m).map(|_| palette[rng.below(palette.len())]).collect();
        let dense = DistanceMatrix::from_condensed(data).unwrap();
        let threshold = match rng.below(3) {
            0 => None,
            1 => Some(1.5),
            _ => Some(f64::INFINITY),
        };
        let collapsed = collapse_dense(&dense, threshold).unwrap();
        removed_total += collapsed.stats.removed_edges;
        verify_dense(&dense, threshold, &collapsed)
            .unwrap_or_else(|e| panic!("iter {it} dense: {e}"));

        let mut triplets = Vec::new();
        for i in 1..n {
            for j in 0..i {
                let d = dense.get(i, j);
                if d.is_finite() {
                    triplets.push((i, j, d));
                }
            }
        }
        let sparse = SparseDistanceMatrix::from_triplets(n, &triplets).unwrap();
        let st = threshold.or(Some(f64::INFINITY));
        let collapsed_s = collapse_sparse(&sparse, st).unwrap();
        verify_sparse(&sparse, st, &collapsed_s)
            .unwrap_or_else(|e| panic!("iter {it} sparse: {e}"));
    }
    assert!(removed_total > 100, "collapse never fired: {removed_total}");
}
