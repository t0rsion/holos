use super::*;

#[test]
fn large_neighborhood_fallback() {
    // Vertices 0 and 1 share every other vertex, and vertex 2 dominates all
    // of them, so (0, 1) goes first and its read set is the whole 76-vertex
    // graph. That is far past the marking limit, so production must abandon
    // fine marking and retest every live edge in round 2. The certificate
    // may not notice: only the test counter moves.
    let dense = fallback_matrix();
    let threshold = Some(1.0);
    let input = thresholded_dense(&dense, threshold);
    let result = v2_dense("fallback", &dense, threshold, 4);

    let step = step_for(&result, (0, 1)).expect("edge (0, 1) must be removable");
    assert_eq!(
        step.position().number(),
        1,
        "edge (0, 1) leads the schedule"
    );
    assert_eq!(
        step.witnesses().to_vec(),
        vec![(1.0, 2)],
        "the hub is the first dominating vertex at the only critical value"
    );
    assert_eq!(
        round_widths(&result)[0],
        1,
        "the read set covers the graph, so round 1 takes one edge"
    );
    let widest = widest_round_read_set(FALLBACK_N, &input, &result);
    assert_eq!(
        widest, FALLBACK_N,
        "edge (0, 1) must read the whole graph, got {widest}"
    );
    assert!(
        widest > MARK_LIMIT,
        "the read set must exceed the marking limit"
    );
    assert_thread_invariant("fallback", &dense, threshold);
    assert_fixture_barcode("fallback", &dense, threshold, 1);
}

#[test]
fn k64_64_zero_yield_v2() {
    // A triangle-free graph has no candidate anywhere, so the first round
    // finds nothing and the schedule stops. One round, an empty certificate,
    // and the input returned untouched.
    let dense = bipartite_dense();
    let result = v2_dense("k64_64", &dense, None, 4);
    assert_eq!(
        result.certificate.algorithm_version(),
        2,
        "the parallel entry point must tag version 2"
    );
    assert!(
        result.certificate.steps().is_empty(),
        "a triangle-free graph has no removable edge"
    );
    assert_eq!(result.stats.epochs, 1, "zero yield must take one round");
    assert_eq!(result.certificate.input_edge_count(), 4096, "input edges");
    assert_eq!(result.certificate.output_edge_count(), 4096, "output edges");
    assert_eq!(result.matrix.num_edges(), 4096, "surviving edges");
    assert_eq!(result.stats.witness_segments, 0, "no witness segments");
    assert_eq!(
        result.stats.max_common_neighborhood, 0,
        "no edge has a common neighbor"
    );
    assert_eq!(
        result.certificate.terminal_level(),
        1.0,
        "terminal level is the only edge value"
    );
    assert_reference_match("k64_64", &result, &reference_dense(&dense, None));

    let plain = dense_bars(&dense, 1, None, 2, 1, ALL_ON, false);
    let collapsed = dense_bars(&dense, 1, None, 2, 4, ALL_ON, true);
    assert_eq!(plain, collapsed, "K64,64: collapse changed the diagram");
    assert_eq!(essential_count(&collapsed, 0), 1, "K64,64 is connected");
    assert_eq!(essential_count(&collapsed, 1), 3969, "K64,64 cycle rank");
}

#[test]
fn k64_64_plus_k4_mixed_yield_v2() {
    // Only the K4 can yield: every bipartite edge is triangle-free and must
    // survive, so every removal step must sit inside the K4, and the round
    // structure must be the K4's own.
    let dense = bipartite_k4_dense();
    let result = v2_dense("k64_64+k4", &dense, None, 4);
    assert_eq!(
        result.certificate.input_edge_count(),
        4096 + 6,
        "input edges"
    );
    assert!(
        !result.certificate.steps().is_empty(),
        "the K4 must yield removals"
    );
    for step in result.certificate.steps() {
        let (u, v) = step.edge();
        assert!(
            u >= BIP_N && v >= BIP_N,
            "removal of ({u}, {v}) escaped the K4"
        );
    }
    let surviving_bipartite = result
        .matrix
        .edges()
        .filter(|&(u, v, _)| u < BIP_N && v < BIP_N)
        .count();
    assert_eq!(
        surviving_bipartite, 4096,
        "every bipartite edge must survive"
    );
    assert_eq!(
        round_widths(&result),
        vec![1, 2, 0],
        "the K4 keeps its own round structure inside the larger graph"
    );

    let plain = dense_bars(&dense, 1, None, 2, 1, ALL_ON, false);
    let collapsed = dense_bars(&dense, 1, None, 2, 4, ALL_ON, true);
    assert_eq!(plain, collapsed, "K64,64+K4: collapse changed the diagram");
    assert_eq!(essential_count(&collapsed, 0), 2, "two components");
    assert_eq!(essential_count(&collapsed, 1), 3969, "bipartite cycle rank");
}
