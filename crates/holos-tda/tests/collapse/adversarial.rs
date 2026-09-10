use super::fixtures::*;

#[test]
fn positive_scaling_preserves_removals_and_scales_witnesses() {
    // Scaling by 3 is exact on these values, so the schedule order and every
    // comparison in the predicate survive unchanged. Only the recorded
    // breakpoints move, by the same factor.
    let dense = level_dependent_apex_matrix();
    let scaled = scale_dense(&dense, 3.0);
    let base = collapse_and_check_dense("scaling base", &dense, Some(2.0));
    let big = collapse_and_check_dense("scaling scaled", &scaled, Some(6.0));

    assert_eq!(
        big.certificate.terminal_level(),
        3.0 * base.certificate.terminal_level(),
        "terminal level must scale"
    );
    let base_steps = base.certificate.steps();
    let big_steps = big.certificate.steps();
    assert_eq!(
        base_steps.len(),
        big_steps.len(),
        "scaling changed the number of removals"
    );
    for (i, (a, b)) in base_steps.iter().zip(big_steps).enumerate() {
        assert_eq!(a.edge(), b.edge(), "step {i}: scaling changed the edge");
        assert_eq!(
            a.position().number(),
            b.position().number(),
            "step {i}: scaling changed the pass"
        );
        assert_eq!(
            b.value(),
            3.0 * a.value(),
            "step {i}: value must scale exactly"
        );
        assert_eq!(
            a.witnesses().len(),
            b.witnesses().len(),
            "step {i}: scaling changed the segment count"
        );
        for (j, (wa, wb)) in a.witnesses().iter().zip(b.witnesses()).enumerate() {
            assert_eq!(wa.1, wb.1, "step {i} segment {j}: apex changed");
            assert_eq!(
                wb.0,
                3.0 * wa.0,
                "step {i} segment {j}: start must scale exactly"
            );
        }
    }
}

#[test]
fn vertex_permutation_preserves_the_barcode_only() {
    // The reduced graph is not canonical, so nothing is asserted about which
    // edges survive a relabeling. The barcode is the invariant.
    let dense = level_dependent_apex_matrix();
    let mut rng = Rng::new(0x9e37_79b9);
    let n = dense.len();
    let mut perm: Vec<usize> = (0..n).collect();
    for i in (1..n).rev() {
        perm.swap(i, rng.below(i + 1));
    }
    let permuted = permute_dense(&dense, &perm);
    collapse_and_check_dense("permuted", &permuted, Some(2.0));
    for &modulus in &MODULI {
        let base = dense_bars(&dense, 2, Some(2.0), modulus, 1, ALL_ON, true);
        let moved = dense_bars(&permuted, 2, Some(2.0), modulus, 1, ALL_ON, true);
        assert_eq!(base, moved, "permutation changed the barcode (p={modulus})");
    }
}

#[test]
fn empty_graph_collapses_to_nothing() {
    // No edges at all: the terminal level falls back to 0 and the schedule
    // still runs exactly one pass.
    let empty = sparse_from_edges(5, &[]);
    let result = collapse_and_check_sparse("empty", &empty, None);
    assert_eq!(result.certificate.terminal_level(), 0.0, "terminal level");
    assert_eq!(result.certificate.input_edge_count(), 0, "input edges");
    assert_eq!(result.stats.epochs, 1, "passes");
    assert_eq!(result.stats.max_common_neighborhood, 0, "neighborhood");
}

#[test]
fn level_dependent_apex() {
    let dense = level_dependent_apex_matrix();
    let result = assert_fixture("level_dependent_apex", &dense, Some(2.0), 2, true);
    let step = step_for(&result, (0, 1)).expect("edge (0, 1) must be removable");
    // Expected witness function: vertex 2 from level 1, vertex 4 from level 2.
    assert!(
        step.witnesses().len() >= 2,
        "edge (0, 1) needs a piecewise apex, got {:?}",
        step.witnesses()
    );
    assert_eq!(
        step.witnesses()[0],
        (1.0, 2),
        "first segment must start at the edge value with the only low candidate"
    );
    assert!(
        step.witnesses().iter().any(|&(_, apex)| apex == 4),
        "the upper level needs vertex 4 as apex, got {:?}",
        step.witnesses()
    );
    assert!(
        result
            .certificate
            .steps()
            .iter()
            .any(|s| s.witnesses().len() >= 2),
        "no removal recorded more than one segment"
    );
}

#[test]
fn later_pass_removability() {
    let dense = later_pass_matrix();
    let result = assert_fixture("later_pass_removability", &dense, Some(1.0), 2, true);
    let step = step_for(&result, (0, 1)).expect("edge (0, 1) must be removable in a later pass");
    assert_eq!(
        step.position().number(),
        2,
        "edge (0, 1) must survive pass 1 and leave in pass 2"
    );
    assert!(
        result
            .certificate
            .steps()
            .iter()
            .any(|s| s.position().number() == 2),
        "no removal happened after the first pass"
    );
    assert!(
        result.stats.epochs >= 3,
        "a removal in pass 2 needs a third, empty pass, got {}",
        result.stats.epochs
    );
}

#[test]
fn ties_le_vs_lt() {
    // Every comparison that decides edge (0, 1) is an equality: the candidate
    // birth b(x) equals the terminal level, the domination test f(w, x) <= t
    // holds with f(2, 3) == t, and the level is the edge's own value. A strict
    // comparison anywhere in the predicate loses this removal.
    let tied = dense_from_edges(
        4,
        &[
            (0, 1, 2.0),
            (0, 2, 2.0),
            (1, 2, 2.0),
            (0, 3, 2.0),
            (1, 3, 2.0),
            (2, 3, 2.0),
        ],
    );
    let result = assert_fixture("ties_le_vs_lt", &tied, Some(2.0), 2, true);
    let step = step_for(&result, (0, 1)).expect("the tied edge (0, 1) must be removable");
    assert_eq!(
        step.witnesses().to_vec(),
        vec![(2.0, 2)],
        "the tie must be certified by the first candidate at the edge value"
    );
    assert_eq!(
        step.position().number(),
        1,
        "the tied edge must go in pass 1"
    );

    // Same graph without the (2, 3) tie: the two candidates are not adjacent,
    // so (0, 1) is never removable. This isolates the tie as the deciding
    // comparison above.
    let untied = dense_from_edges(
        4,
        &[
            (0, 1, 2.0),
            (0, 2, 2.0),
            (1, 2, 2.0),
            (0, 3, 2.0),
            (1, 3, 2.0),
        ],
    );
    let result = assert_fixture("ties_le_vs_lt_untied", &untied, Some(2.0), 2, true);
    assert!(
        step_for(&result, (0, 1)).is_none(),
        "without the tie, edge (0, 1) has no dominating apex"
    );
}

#[test]
fn chordless_4cycle() {
    // No edge of a chordless cycle has a common neighbor, so nothing is
    // removable and the H1 class must survive untouched.
    let dense = dense_from_edges(4, &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)]);
    let result = assert_fixture("chordless_4cycle", &dense, Some(1.0), 2, true);
    assert!(
        result.certificate.steps().is_empty(),
        "a chordless 4-cycle has no removable edge"
    );
    assert_eq!(result.stats.epochs, 1, "zero yield must take one pass");
    assert_eq!(result.matrix.num_edges(), 4, "all four edges must survive");

    let bars = dense_bars(&dense, 2, Some(1.0), 2, 1, ALL_ON, true);
    assert_eq!(essential_count(&bars, 1), 1, "the loop must stay essential");
}

#[test]
fn octahedral_sphere() {
    // The octahedron is a flag 2-sphere. Every edge has exactly two common
    // neighbors and they are antipodal, so no apex dominates and the H2 class
    // cannot be collapsed away.
    let n = 6;
    let mut edges = Vec::new();
    for u in 0..n {
        for v in (u + 1)..n {
            let antipodal = u / 2 == v / 2;
            if !antipodal {
                edges.push((u, v, 1.0));
            }
        }
    }
    let dense = dense_from_edges(n, &edges);
    let result = assert_fixture("octahedral_sphere", &dense, Some(1.0), 2, true);
    assert!(
        result.certificate.steps().is_empty(),
        "the octahedron has no removable edge"
    );
    assert_eq!(result.stats.epochs, 1, "zero yield must take one pass");
    assert_eq!(
        result.matrix.num_edges(),
        12,
        "all twelve edges must survive"
    );

    for &modulus in &MODULI {
        let bars = dense_bars(&dense, 2, Some(1.0), modulus, 1, ALL_ON, true);
        assert_eq!(
            essential_count(&bars, 2),
            1,
            "the sphere class must survive at p={modulus}"
        );
        assert_eq!(essential_count(&bars, 1), 0, "no H1 at p={modulus}");
    }
}

#[test]
fn zero_distance_clusters() {
    // Duplicate points glue into clusters at distance exactly 0. Zero-value
    // edges are born at the bottom of the filtration, where the candidate set
    // is at its largest.
    let sites = [[0.0, 0.0], [1.0, 0.0], [0.4, 0.9]];
    let mut points = Vec::new();
    for site in sites {
        for _ in 0..3 {
            points.push(site.to_vec());
        }
    }
    let dense = DistanceMatrix::from_points(&points).unwrap();
    let result = assert_fixture("zero_distance_clusters", &dense, None, 2, true);
    for step in result.certificate.steps() {
        if step.value() == 0.0 {
            assert_eq!(
                step.witnesses()[0].0,
                0.0,
                "a zero-value edge must be certified from level 0"
            );
        }
    }
    // The engine follows ripser and drops zero-persistence pairs, so the six
    // within-cluster [0, 0) bars never appear: two finite H0 bars remain.
    let bars = dense_bars(&dense, 2, None, 2, 1, ALL_ON, true);
    assert_eq!(finite_count(&bars, 0), 2, "two cluster merges must die");
}

#[test]
fn tie_heavy_grid() {
    // L1 distances on a 4x4 grid: only integer values 1 through 6, so almost
    // every candidate shares a birth level with its neighbors and the
    // critical-value sweep runs on large tied blocks.
    let side = 4i64;
    let coords: Vec<(i64, i64)> = (0..side)
        .flat_map(|x| (0..side).map(move |y| (x, y)))
        .collect();
    let mut condensed = Vec::new();
    for i in 1..coords.len() {
        for j in 0..i {
            let d = (coords[i].0 - coords[j].0).abs() + (coords[i].1 - coords[j].1).abs();
            condensed.push(d as f64);
        }
    }
    let dense = DistanceMatrix::from_condensed(condensed).unwrap();
    assert_fixture("tie_heavy_grid", &dense, None, 1, true);
}

#[test]
fn dense_near_clique() {
    // K8 minus one edge: the candidate sets are as large as they get for this
    // size, and the missing edge is the only obstruction the predicate can
    // find.
    let n = 8;
    let mut edges = Vec::new();
    for u in 0..n {
        for v in (u + 1)..n {
            if (u, v) != (0, 1) {
                edges.push((u, v, 1.0));
            }
        }
    }
    let dense = dense_from_edges(n, &edges);
    let result = assert_fixture("dense_near_clique", &dense, Some(1.0), 2, true);
    assert!(
        !result.certificate.steps().is_empty(),
        "a near-clique must yield removals"
    );
    assert!(
        result.stats.max_common_neighborhood >= 5,
        "expected a large common neighborhood, got {}",
        result.stats.max_common_neighborhood
    );
}

#[test]
fn disconnected_plus_inf() {
    // Three components joined by nothing: the enclosing radius is +inf, so the
    // terminal level comes from the largest finite edge, and the three
    // essential H0 classes must survive the collapse.
    let edges = [
        (0, 1, 1.0),
        (0, 2, 1.0),
        (1, 2, 1.0),
        (3, 4, 1.0),
        (3, 5, 2.0),
        (4, 5, 2.0),
        (6, 7, 2.0),
        (6, 8, 2.0),
        (7, 8, 1.0),
    ];
    let dense = dense_from_edges(9, &edges);
    let result = assert_fixture("disconnected_plus_inf", &dense, None, 2, true);
    assert_eq!(
        result.certificate.terminal_level(),
        2.0,
        "terminal level must fall back to the largest finite edge"
    );
    for &modulus in &MODULI {
        let bars = dense_bars(&dense, 2, None, modulus, 1, ALL_ON, true);
        assert_eq!(
            essential_count(&bars, 0),
            3,
            "three components must stay separate at p={modulus}"
        );
    }
}

#[test]
fn sparse_hub() {
    // A star with a few cross edges, given directly as a sparse matrix. The
    // hub sits in every candidate set, and the leaves have degree 1 or 2.
    let n = 7;
    let mut edges: Vec<(usize, usize, f64)> = (1..n).map(|v| (0, v, 1.0)).collect();
    edges.push((1, 2, 1.0));
    edges.push((3, 4, 2.0));
    edges.push((5, 6, 1.5));
    let sparse = sparse_from_edges(n, &edges);
    let result = collapse_and_check_sparse("sparse_hub", &sparse, None);
    assert_eq!(
        result.certificate.terminal_level(),
        2.0,
        "an unthresholded sparse input ends at its largest edge"
    );
    for &modulus in &MODULI {
        for &threads in &[1usize, 2] {
            for threshold in [None, Some(1.5), Some(f64::INFINITY)] {
                let plain = sparse_bars(&sparse, 2, threshold, modulus, threads, ALL_ON, false);
                let collapsed = sparse_bars(&sparse, 2, threshold, modulus, threads, ALL_ON, true);
                assert_eq!(
                    plain, collapsed,
                    "sparse_hub: collapse changed the diagram \
                     (p={modulus} threads={threads} threshold={threshold:?})"
                );
            }
        }
    }
    for threshold in [None, Some(1.5)] {
        collapse_and_check_sparse("sparse_hub", &sparse, threshold);
    }
}

#[test]
fn non_metric_domination_flip() {
    // d(0,1) = 10 while d(0,2) = d(1,2) = 1: a gross triangle-inequality
    // violation. Domination is a graph property, so the long edge is still
    // dominated and must be removed. The predicate may not assume a metric.
    let dense = dense_from_edges(
        4,
        &[
            (0, 1, 10.0),
            (0, 2, 1.0),
            (1, 2, 1.0),
            (0, 3, 5.0),
            (1, 3, 5.0),
            (2, 3, 0.5),
        ],
    );
    let result = assert_fixture("non_metric_domination_flip", &dense, Some(10.0), 2, true);
    let step = step_for(&result, (0, 1)).expect("the long edge must be dominated");
    assert_eq!(
        step.witnesses().to_vec(),
        vec![(10.0, 2)],
        "the first candidate covers the single critical value"
    );
}

#[test]
fn projective_plane_torsion() {
    // The 13-vertex RP^2 triangulation: H1 and H2 are Z/2, visible only at
    // p = 2. The collapse must preserve the torsion answer at every modulus.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/projective_plane.lower_distance_matrix");
    let dense = holos_tda::io::read_lower_distance_matrix(&path, 1).unwrap();
    collapse_and_check_dense("projective_plane", &dense, None);

    let intervals = |bars: &[Bar], dim: usize| -> Vec<(f64, f64)> {
        bars.iter()
            .filter(|b| b.dim == dim)
            .map(|b| (b.birth, b.death))
            .collect()
    };
    for &modulus in &MODULI {
        for &threads in &[1usize, 2] {
            let plain = dense_bars(&dense, 2, None, modulus, threads, ALL_ON, false);
            let collapsed = dense_bars(&dense, 2, None, modulus, threads, ALL_ON, true);
            assert_eq!(
                plain, collapsed,
                "collapse changed the RP^2 diagram (p={modulus} threads={threads})"
            );
            let expected = if modulus == 2 {
                vec![(1.0, 2.0)]
            } else {
                vec![]
            };
            assert_eq!(
                intervals(&collapsed, 1),
                expected,
                "collapsed H1 at p={modulus}"
            );
            assert_eq!(
                intervals(&collapsed, 2),
                expected,
                "collapsed H2 at p={modulus}"
            );
        }
    }
}
