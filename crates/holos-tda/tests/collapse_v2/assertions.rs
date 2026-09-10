use super::*;
use holos_tda::DistanceMatrix;
use holos_tda::collapse::{collapse_dense, collapse_sparse};

/// Every worker count must give the same certificate, the same matrix, and
/// the same counters, `edge_tests` included: the pruning rule is a function
/// of the schedule, not of the worker count.
pub(crate) fn assert_thread_invariant(name: &str, dense: &DistanceMatrix, threshold: Option<f64>) {
    let sparse = sparse_from_dense(dense);
    let base_dense = v2_dense(name, dense, threshold, COLLAPSE_THREADS[0]);
    let base_sparse = v2_sparse(name, &sparse, threshold, COLLAPSE_THREADS[0]);
    for &threads in &COLLAPSE_THREADS[1..] {
        let got = v2_dense(name, dense, threshold, threads);
        assert_eq!(
            got.certificate, base_dense.certificate,
            "{name}: dense certificate changed at {threads} workers"
        );
        assert_eq!(
            step_bits(&got.certificate),
            step_bits(&base_dense.certificate),
            "{name}: dense certificate bits changed at {threads} workers"
        );
        assert_eq!(
            edge_list(&got.matrix),
            edge_list(&base_dense.matrix),
            "{name}: dense matrix changed at {threads} workers"
        );
        assert_eq!(
            got.stats, base_dense.stats,
            "{name}: dense stats changed at {threads} workers"
        );

        let got = v2_sparse(name, &sparse, threshold, threads);
        assert_eq!(
            got.certificate, base_sparse.certificate,
            "{name}: sparse certificate changed at {threads} workers"
        );
        assert_eq!(
            step_bits(&got.certificate),
            step_bits(&base_sparse.certificate),
            "{name}: sparse certificate bits changed at {threads} workers"
        );
        assert_eq!(
            edge_list(&got.matrix),
            edge_list(&base_sparse.matrix),
            "{name}: sparse matrix changed at {threads} workers"
        );
        assert_eq!(
            got.stats, base_sparse.stats,
            "{name}: sparse stats changed at {threads} workers"
        );
    }
}

/// Bar-for-bar equality of five paths: the uncollapsed engine, the
/// convenience path, the standalone version 1 collapse, the standalone
/// version 2 collapse, and the oracle. The schedules differ; the barcode
/// does not.
pub(crate) fn assert_v2_preserves_the_diagram(
    name: &str,
    dense: &DistanceMatrix,
    mid: f64,
    check_oracle: bool,
) {
    let sparse = sparse_from_dense(dense);
    for threshold in [None, Some(mid), Some(f64::INFINITY)] {
        // The collapse depends on the input and the threshold only, so one
        // run of each schedule serves the whole cross below.
        let v1_dense = collapse_dense(dense, threshold).unwrap();
        let v1_sparse = collapse_sparse(&sparse, threshold).unwrap();
        let v2d = v2_dense(name, dense, threshold, 4);
        let v2s = v2_sparse(name, &sparse, threshold, 4);
        for &modulus in &MODULI {
            for max_dim in 0..=2 {
                for &threads in &REDUCER_THREADS {
                    for &toggles in &[ALL_ON, ALL_OFF] {
                        let label = format!(
                            "{name}: p={modulus} threshold={threshold:?} max_dim={max_dim} \
                             threads={threads} toggles={toggles:?}"
                        );
                        let plain =
                            dense_bars(dense, max_dim, threshold, modulus, threads, toggles, false);
                        let convenience =
                            dense_bars(dense, max_dim, threshold, modulus, threads, toggles, true);
                        assert_eq!(plain, convenience, "{label}: dense convenience path");
                        assert_eq!(
                            plain,
                            collapsed_bars(&v1_dense, max_dim, modulus, threads, toggles),
                            "{label}: dense standalone version 1"
                        );
                        assert_eq!(
                            plain,
                            collapsed_bars(&v2d, max_dim, modulus, threads, toggles),
                            "{label}: dense standalone version 2"
                        );

                        let plain = sparse_bars(
                            &sparse, max_dim, threshold, modulus, threads, toggles, false,
                        );
                        let convenience = sparse_bars(
                            &sparse, max_dim, threshold, modulus, threads, toggles, true,
                        );
                        assert_eq!(plain, convenience, "{label}: sparse convenience path");
                        assert_eq!(
                            plain,
                            collapsed_bars(&v1_sparse, max_dim, modulus, threads, toggles),
                            "{label}: sparse standalone version 1"
                        );
                        assert_eq!(
                            plain,
                            collapsed_bars(&v2s, max_dim, modulus, threads, toggles),
                            "{label}: sparse standalone version 2"
                        );
                    }
                }
            }
            if check_oracle {
                assert_eq!(
                    collapsed_bars(&v2d, 2, modulus, 1, ALL_ON),
                    oracle_bars(dense, 2, threshold, modulus),
                    "{name}: version 2 diagram differs from the oracle \
                     (p={modulus} threshold={threshold:?})"
                );
            }
        }
    }
}

/// Collapse on against collapse off at a few fields, dense and sparse. The
/// fixtures below use this to keep their round-structure claims tied to a
/// preserved barcode.
pub(crate) fn assert_fixture_barcode(
    name: &str,
    dense: &DistanceMatrix,
    threshold: Option<f64>,
    max_dim: usize,
) {
    let sparse = sparse_from_dense(dense);
    for &modulus in &MODULI {
        let plain = dense_bars(dense, max_dim, threshold, modulus, 1, ALL_ON, false);
        let collapsed = dense_bars(dense, max_dim, threshold, modulus, 2, ALL_ON, true);
        assert_eq!(
            plain, collapsed,
            "{name}: dense collapse changed the diagram (p={modulus})"
        );
        let plain = sparse_bars(&sparse, max_dim, threshold, modulus, 1, ALL_ON, false);
        let collapsed = sparse_bars(&sparse, max_dim, threshold, modulus, 2, ALL_ON, true);
        assert_eq!(
            plain, collapsed,
            "{name}: sparse collapse changed the diagram (p={modulus})"
        );
    }
}
