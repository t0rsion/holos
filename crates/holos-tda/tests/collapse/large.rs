use super::fixtures::*;

#[test]
fn k64_64_zero_yield() {
    let dense = bipartite_dense();
    let result = collapse_and_check_dense("k64_64", &dense, None);
    assert!(
        result.certificate.steps().is_empty(),
        "a triangle-free graph has no removable edge"
    );
    assert_eq!(result.certificate.input_edge_count(), 4096, "input edges");
    assert_eq!(result.certificate.output_edge_count(), 4096, "output edges");
    assert_eq!(result.matrix.num_edges(), 4096, "surviving edges");
    assert_eq!(result.stats.epochs, 1, "zero yield must take one pass");
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

    for &modulus in &[2u32, 3] {
        for &threads in &[1usize, 2] {
            let plain = dense_bars(&dense, 1, None, modulus, threads, ALL_ON, false);
            let collapsed = dense_bars(&dense, 1, None, modulus, threads, ALL_ON, true);
            assert_eq!(
                plain, collapsed,
                "K64,64: collapse changed the diagram (p={modulus} threads={threads})"
            );
            assert_eq!(
                essential_count(&collapsed, 0),
                1,
                "K64,64 is connected (p={modulus})"
            );
            assert_eq!(
                essential_count(&collapsed, 1),
                3969,
                "K64,64 cycle rank (p={modulus})"
            );
            assert_eq!(finite_count(&collapsed, 1), 0, "no finite H1 (p={modulus})");
        }
    }
}

#[test]
fn k64_64_plus_k4_mixed_yield() {
    // Only the K4 can yield: every bipartite edge is triangle-free and must
    // survive, so every removal step must sit inside the K4.
    let dense = bipartite_k4_dense();
    let result = collapse_and_check_dense("k64_64+k4", &dense, None);
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

    for &modulus in &[2u32, 3] {
        for &threads in &[1usize, 2] {
            let plain = dense_bars(&dense, 1, None, modulus, threads, ALL_ON, false);
            let collapsed = dense_bars(&dense, 1, None, modulus, threads, ALL_ON, true);
            assert_eq!(
                plain, collapsed,
                "K64,64+K4: collapse changed the diagram (p={modulus} threads={threads})"
            );
            assert_eq!(
                essential_count(&collapsed, 0),
                2,
                "two components (p={modulus})"
            );
            assert_eq!(
                essential_count(&collapsed, 1),
                3969,
                "bipartite cycle rank (p={modulus})"
            );
        }
    }
}
