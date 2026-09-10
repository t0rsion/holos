use super::*;

pub(crate) fn params(
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    toggles: (bool, bool, bool),
    collapse: bool,
) -> RipsParams {
    let mut p = RipsParams::new(max_dim)
        .with_modulus(modulus)
        .with_threads(threads);
    p.threshold = threshold;
    p.use_clearing = toggles.0;
    p.use_emergent_pairs = toggles.1;
    p.use_apparent_pairs = toggles.2;
    if collapse {
        p = p.with_collapse_schedule(CollapseSchedule::Ordered);
    }
    p
}

pub(crate) fn canon(diagram: &Diagram) -> Vec<Bar> {
    let mut d = diagram.clone();
    d.canonicalize();
    d.bars
}

pub(crate) fn dense_bars(
    dist: &DistanceMatrix,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    toggles: (bool, bool, bool),
    collapse: bool,
) -> Vec<Bar> {
    let p = params(max_dim, threshold, modulus, threads, toggles, collapse);
    canon(&rips_persistence(dist, &p).unwrap())
}

pub(crate) fn sparse_bars(
    dist: &SparseDistanceMatrix,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    toggles: (bool, bool, bool),
    collapse: bool,
) -> Vec<Bar> {
    let p = params(max_dim, threshold, modulus, threads, toggles, collapse);
    canon(&rips_persistence_sparse(dist, &p).unwrap())
}

pub(crate) fn oracle_bars(
    dist: &DistanceMatrix,
    max_dim: usize,
    threshold: Option<f64>,
    modulus: u32,
) -> Vec<Bar> {
    canon(&rips_persistence_oracle_mod(
        dist, max_dim, threshold, modulus,
    ))
}

/// The engine on an ordered collapsed graph, at the certificate's terminal
/// level: what a caller does by hand with the standalone entry point.
pub(crate) fn standalone_bars(
    collapsed: &CollapsedRips,
    max_dim: usize,
    modulus: u32,
    threads: usize,
    toggles: (bool, bool, bool),
) -> Vec<Bar> {
    let p = params(
        max_dim,
        Some(collapsed.certificate.terminal_level()),
        modulus,
        threads,
        toggles,
        false,
    );
    canon(&rips_persistence_sparse(&collapsed.matrix, &p).unwrap())
}

/// Bar-for-bar equality of the uncollapsed engine, the convenience path,
/// the standalone ordered path, and the oracle, over the modulus x
/// threshold x max_dim x reducer threads x toggles cross.
pub(crate) fn assert_ordered_preserves_diagram(name: &str, dense: &DistanceMatrix, mid: f64) {
    let sparse = sparse_from_dense(dense);
    for threshold in [None, Some(mid), Some(f64::INFINITY)] {
        let od = collapse_dense_ordered_parallel(dense, threshold, 4).unwrap();
        verify_dense(dense, threshold, &od)
            .unwrap_or_else(|e| panic!("{name}: verifier rejected the dense certificate: {e}"));
        let os = collapse_sparse_ordered_parallel(&sparse, threshold, 4).unwrap();
        verify_sparse(&sparse, threshold, &os)
            .unwrap_or_else(|e| panic!("{name}: verifier rejected the sparse certificate: {e}"));

        for &modulus in &MODULI {
            for max_dim in 0..=2 {
                let oracle = oracle_bars(dense, max_dim, threshold, modulus);
                for &threads in &[1usize, 4] {
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
                            standalone_bars(&od, max_dim, modulus, threads, toggles),
                            "{label}: dense standalone ordered path"
                        );
                        assert_eq!(plain, oracle, "{label}: dense oracle");

                        let plain = sparse_bars(
                            &sparse, max_dim, threshold, modulus, threads, toggles, false,
                        );
                        let convenience = sparse_bars(
                            &sparse, max_dim, threshold, modulus, threads, toggles, true,
                        );
                        assert_eq!(plain, convenience, "{label}: sparse convenience path");
                        assert_eq!(
                            plain,
                            standalone_bars(&os, max_dim, modulus, threads, toggles),
                            "{label}: sparse standalone ordered path"
                        );
                    }
                }
            }
        }
    }
}
