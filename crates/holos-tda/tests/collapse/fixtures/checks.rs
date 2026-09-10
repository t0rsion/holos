use super::*;

/// Header, partition, and witness-shape checks that do not depend on the
/// input kind. `resolved` is the threshold after the input's own rule.
pub(crate) fn check_certificate(
    name: &str,
    n: usize,
    input: &[(usize, usize, f64)],
    threshold: Option<f64>,
    resolved: f64,
    result: &CollapsedRips,
) {
    let cert = &result.certificate;
    let output = edge_list(&result.matrix);

    assert_eq!(cert.algorithm_version(), 1, "{name}: algorithm version");
    assert_eq!(cert.vertex_count(), n, "{name}: vertex count");
    assert_eq!(result.matrix.len(), n, "{name}: output vertex count");
    assert_eq!(
        cert.requested_threshold(),
        threshold,
        "{name}: requested threshold must be verbatim"
    );

    let terminal = if resolved.is_finite() {
        resolved
    } else {
        input.iter().map(|e| e.2).fold(0.0, f64::max)
    };
    assert_eq!(cert.terminal_level(), terminal, "{name}: terminal level");

    assert_eq!(cert.input_edge_count(), input.len(), "{name}: input edges");
    assert_eq!(
        cert.output_edge_count(),
        output.len(),
        "{name}: output edges"
    );
    assert_eq!(
        cert.input_edge_count(),
        cert.output_edge_count() + cert.steps().len(),
        "{name}: input must equal output plus steps"
    );

    // Removed and surviving edges partition the thresholded input, values
    // included. A duplicate or an altered value breaks this compare.
    let mut merged = output.clone();
    merged.extend(removed_edges(result));
    merged.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    assert_eq!(
        merged, input,
        "{name}: output plus removals must reconstruct the thresholded input"
    );

    for &(u, v, d) in &output {
        assert!(u < v, "{name}: output edge ({u}, {v}) not ordered");
        assert!(
            d <= terminal,
            "{name}: output edge ({u}, {v}) above terminal"
        );
    }

    let mut last_pass = 0;
    for (i, step) in cert.steps().iter().enumerate() {
        let (u, v) = step.edge();
        assert!(u < v, "{name}: step {i} endpoints not ordered");
        assert!(v < n, "{name}: step {i} endpoint out of range");
        assert!(
            step.position().number() >= 1,
            "{name}: step {i} pass is not 1-based"
        );
        assert!(
            step.position().number() >= last_pass,
            "{name}: step {i} pass number decreased"
        );
        last_pass = step.position().number();

        let w = step.witnesses();
        assert!(!w.is_empty(), "{name}: step {i} has no witness segment");
        assert_eq!(
            w[0].0,
            step.value(),
            "{name}: step {i} first segment must start at the edge value"
        );
        for pair in w.windows(2) {
            assert!(
                pair[0].0 < pair[1].0,
                "{name}: step {i} segment starts must strictly increase"
            );
        }
        for &(start, apex) in w {
            assert!(apex < n, "{name}: step {i} apex {apex} out of range");
            assert!(
                apex != u && apex != v,
                "{name}: step {i} apex is an endpoint"
            );
            assert!(
                start <= terminal,
                "{name}: step {i} segment starts above the terminal level"
            );
        }
    }

    assert_eq!(
        result.stats.input_edges,
        cert.input_edge_count(),
        "{name}: stats input_edges"
    );
    assert_eq!(
        result.stats.output_edges,
        cert.output_edge_count(),
        "{name}: stats output_edges"
    );
    assert_eq!(
        result.stats.removed_edges,
        cert.steps().len(),
        "{name}: stats removed_edges"
    );
    assert!(result.stats.epochs >= 1, "{name}: stats passes");
    assert_eq!(
        result.stats.witness_segments,
        cert.steps()
            .iter()
            .map(|s| s.witnesses().len())
            .sum::<usize>(),
        "{name}: stats witness_segments"
    );
    if let Some(last) = cert.steps().last() {
        assert!(
            result.stats.epochs >= last.position().number(),
            "{name}: stats passes must cover the last removal"
        );
    }
}

/// Collapse a dense input and run every input-independent certificate check:
/// partition, subset with unchanged values, the independent verifier,
/// idempotence, and a byte-identical rerun.
pub(crate) fn collapse_and_check_dense(
    name: &str,
    dist: &DistanceMatrix,
    threshold: Option<f64>,
) -> CollapsedRips {
    let result = collapse_dense(dist, threshold).unwrap();
    let resolved = threshold.unwrap_or_else(|| dist.enclosing_radius());
    let input = thresholded_dense(dist, threshold);
    check_certificate(name, dist.len(), &input, threshold, resolved, &result);

    for (u, v, d) in edge_list(&result.matrix) {
        assert_eq!(
            dist.get(u, v),
            d,
            "{name}: surviving edge ({u}, {v}) changed value"
        );
    }
    for step in result.certificate.steps() {
        let (u, v) = step.edge();
        assert_eq!(
            dist.get(u, v),
            step.value(),
            "{name}: step for ({u}, {v}) changed value"
        );
    }

    verify_dense(dist, threshold, &result)
        .unwrap_or_else(|e| panic!("{name}: verifier rejected the certificate: {e}"));

    check_idempotent(name, &result);

    let rerun = collapse_dense(dist, threshold).unwrap();
    assert_eq!(
        rerun.certificate, result.certificate,
        "{name}: rerun changed the certificate"
    );
    assert_eq!(
        edge_list(&rerun.matrix),
        edge_list(&result.matrix),
        "{name}: rerun changed the output matrix"
    );
    result
}

pub(crate) fn collapse_and_check_sparse(
    name: &str,
    dist: &SparseDistanceMatrix,
    threshold: Option<f64>,
) -> CollapsedRips {
    let result = collapse_sparse(dist, threshold).unwrap();
    let resolved = threshold.unwrap_or(f64::INFINITY);
    let input = thresholded_sparse(dist, threshold);
    check_certificate(name, dist.len(), &input, threshold, resolved, &result);

    for (u, v, d) in edge_list(&result.matrix) {
        assert_eq!(
            dist.get(u, v),
            d,
            "{name}: surviving edge ({u}, {v}) changed value"
        );
    }

    verify_sparse(dist, threshold, &result)
        .unwrap_or_else(|e| panic!("{name}: verifier rejected the certificate: {e}"));

    check_idempotent(name, &result);

    let rerun = collapse_sparse(dist, threshold).unwrap();
    assert_eq!(
        rerun.certificate, result.certificate,
        "{name}: rerun changed the certificate"
    );
    assert_eq!(
        edge_list(&rerun.matrix),
        edge_list(&result.matrix),
        "{name}: rerun changed the output matrix"
    );
    result
}

/// Collapsing the collapsed graph must remove nothing in a single pass.
pub(crate) fn check_idempotent(name: &str, result: &CollapsedRips) {
    let terminal = Some(result.certificate.terminal_level());
    let again = collapse_sparse(&result.matrix, terminal).unwrap();
    assert!(
        again.certificate.steps().is_empty(),
        "{name}: collapsing the collapsed graph removed {} edges",
        again.certificate.steps().len()
    );
    assert_eq!(
        again.stats.epochs, 1,
        "{name}: idempotent run needs one pass"
    );
    assert_eq!(
        again.stats.removed_edges, 0,
        "{name}: idempotent run removed edges"
    );
    assert_eq!(
        edge_list(&again.matrix),
        edge_list(&result.matrix),
        "{name}: idempotent run changed the graph"
    );
    verify_sparse(&result.matrix, terminal, &again)
        .unwrap_or_else(|e| panic!("{name}: verifier rejected the idempotent run: {e}"));
}

/// The equality battery: collapse on and off must agree bar for bar over the
/// modulus x threshold x threads x max_dim x toggles cross, on the dense input
/// and on its sparse equivalent. The toggle arm is the full eight-way cross at
/// threads {1, 4} and the two extremes at the other thread counts. Small
/// inputs also face the oracle.
pub(crate) fn assert_collapse_preserves_diagram(
    name: &str,
    dense: &DistanceMatrix,
    mid: f64,
    check_oracle: bool,
) {
    let sparse = sparse_from_dense(dense);
    let thresholds = [None, Some(mid), Some(f64::INFINITY)];
    for &modulus in &MODULI {
        for threshold in thresholds {
            for &threads in &THREAD_COUNTS {
                // Toggle coverage: the full eight-way cross at one and four
                // threads, the two extremes at every thread count. The
                // crossed pair is modulus x threshold x max_dim x toggles at
                // threads {1, 4}, and modulus x threshold x max_dim x
                // {all on, all off} at threads {1, 2, 4, 8}.
                let toggle_set: &[(bool, bool, bool)] = match threads {
                    1 | 4 => &TOGGLE_CROSS,
                    _ => &[ALL_ON, ALL_OFF],
                };
                for max_dim in 0..=2 {
                    for &toggles in toggle_set {
                        let label = format!(
                            "{name}: p={modulus} threshold={threshold:?} threads={threads} \
                             max_dim={max_dim} toggles={toggles:?}"
                        );
                        let plain =
                            dense_bars(dense, max_dim, threshold, modulus, threads, toggles, false);
                        let collapsed =
                            dense_bars(dense, max_dim, threshold, modulus, threads, toggles, true);
                        assert_eq!(plain, collapsed, "{label}: dense");

                        let plain = sparse_bars(
                            &sparse, max_dim, threshold, modulus, threads, toggles, false,
                        );
                        let collapsed = sparse_bars(
                            &sparse, max_dim, threshold, modulus, threads, toggles, true,
                        );
                        assert_eq!(plain, collapsed, "{label}: sparse");
                    }
                }
            }
            if check_oracle {
                let collapsed = dense_bars(dense, 2, threshold, modulus, 1, ALL_ON, true);
                assert_eq!(
                    collapsed,
                    oracle_bars(dense, 2, threshold, modulus),
                    "{name}: collapsed diagram differs from the oracle \
                     (p={modulus} threshold={threshold:?})"
                );
            }
        }
    }
}

/// Per-fixture gate: full certificate checks plus collapse-on against
/// collapse-off at several fields and thread counts, dense and sparse.
pub(crate) fn assert_fixture(
    name: &str,
    dense: &DistanceMatrix,
    threshold: Option<f64>,
    max_dim: usize,
    check_oracle: bool,
) -> CollapsedRips {
    let result = collapse_and_check_dense(name, dense, threshold);
    let sparse = sparse_from_dense(dense);
    for &modulus in &MODULI {
        for &threads in &[1usize, 2] {
            let plain = dense_bars(dense, max_dim, threshold, modulus, threads, ALL_ON, false);
            let collapsed = dense_bars(dense, max_dim, threshold, modulus, threads, ALL_ON, true);
            assert_eq!(
                plain, collapsed,
                "{name}: dense collapse changed the diagram (p={modulus} threads={threads})"
            );
            let plain_sparse =
                sparse_bars(&sparse, max_dim, threshold, modulus, threads, ALL_ON, false);
            let collapsed_sparse =
                sparse_bars(&sparse, max_dim, threshold, modulus, threads, ALL_ON, true);
            assert_eq!(
                plain_sparse, collapsed_sparse,
                "{name}: sparse collapse changed the diagram (p={modulus} threads={threads})"
            );
            if check_oracle && threads == 1 {
                assert_eq!(
                    collapsed,
                    oracle_bars(dense, max_dim, threshold, modulus),
                    "{name}: collapsed diagram differs from the oracle (p={modulus})"
                );
            }
        }
    }
    result
}
