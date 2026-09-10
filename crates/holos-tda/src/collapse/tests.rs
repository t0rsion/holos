use super::*;

fn edges_of(m: &SparseDistanceMatrix) -> Vec<(usize, usize, f64)> {
    m.edges().collect()
}

fn check_invariants(r: &CollapsedRips) {
    let c = &r.certificate;
    assert_eq!(c.algorithm_version(), 1);
    assert_eq!(c.objective(), None);
    assert_eq!(c.completeness(), CollapseCompleteness::CompleteFixedPoint);
    assert_eq!(c.work_limit(), None);
    assert_eq!(c.work_used(), 0);
    assert_eq!(
        c.input_edge_count(),
        c.output_edge_count() + c.steps().len()
    );
    assert_eq!(r.stats.input_edges, c.input_edge_count());
    assert_eq!(r.stats.output_edges, c.output_edge_count());
    assert_eq!(r.stats.removed_edges, c.steps().len());
    assert_eq!(r.stats.output_edges, r.matrix.num_edges());
    assert_eq!(
        r.stats.witness_segments,
        c.steps().iter().map(|s| s.witnesses().len()).sum::<usize>()
    );
    assert!(r.stats.epochs >= 1);
    for s in c.steps() {
        assert!(s.edge().0 < s.edge().1);
        assert!(s.position().number() >= 1);
        assert!(
            s.position().number() < r.stats.epochs,
            "final pass removes nothing"
        );
        assert!(!s.witnesses().is_empty());
        assert_eq!(s.witnesses()[0].0, s.value());
        for w in s.witnesses().windows(2) {
            assert!(w[0].0 < w[1].0);
        }
        assert!(s.witnesses().iter().all(|&(_, w)| w < c.vertex_count()));
    }
}

// Complete graph on 6 vertices with ties, a zero edge, and two entries
// above the enclosing radius.
fn tie_heavy_condensed() -> Vec<f64> {
    vec![
        0.0, // 1-0
        1.0, 1.0, // 2-*
        2.0, 2.0, 1.0, // 3-*
        3.0, 1.0, 2.0, 2.0, // 4-*
        1.0, 3.0, 2.0, 1.0, 2.0, // 5-*
    ]
}

#[test]
fn triangle_collapses_the_first_scheduled_edge() {
    let d = DistanceMatrix::from_condensed(vec![1.0, 1.0, 1.0]).unwrap();
    let r = collapse_dense(&d, None).unwrap();
    check_invariants(&r);
    let steps = r.certificate.steps();
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].edge(), (0, 1));
    assert_eq!(steps[0].value(), 1.0);
    assert_eq!(steps[0].position().number(), 1);
    assert_eq!(steps[0].witnesses(), &[(1.0, 2)]);
    assert_eq!(edges_of(&r.matrix), vec![(0, 2, 1.0), (1, 2, 1.0)]);
    assert_eq!(r.certificate.terminal_level(), 1.0);
    assert_eq!(r.stats.epochs, 2);
}

#[test]
fn chordless_four_cycle_survives() {
    let m = SparseDistanceMatrix::from_triplets(
        4,
        &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)],
    )
    .unwrap();
    let r = collapse_sparse(&m, None).unwrap();
    check_invariants(&r);
    assert!(r.certificate.steps().is_empty());
    assert_eq!(r.matrix.num_edges(), 4);
    assert_eq!(r.stats.epochs, 1);
    assert_eq!(r.stats.edge_tests, 4);
}

// Schedule order on unit K4: (0,1), (0,2), (1,2), then the three edges
// at vertex 3. (0,1) goes first with apex 2 (lowest common neighbor);
// after that removal the only common neighbor of each remaining pair in
// {0,1,2} is 3, so (0,2) and (1,2) fall with apex 3 and the star at 3
// has no removable edge.
#[test]
fn k4_collapses_to_a_spanning_star() {
    let d = DistanceMatrix::from_condensed(vec![1.0; 6]).unwrap();
    let r = collapse_dense(&d, None).unwrap();
    check_invariants(&r);
    let removed: Vec<_> = r.certificate.steps().iter().map(|s| s.edge()).collect();
    assert_eq!(removed, vec![(0, 1), (0, 2), (1, 2)]);
    let witnesses: Vec<_> = r
        .certificate
        .steps()
        .iter()
        .map(|s| s.witnesses().to_vec())
        .collect();
    assert_eq!(
        witnesses,
        vec![vec![(1.0, 2)], vec![(1.0, 3)], vec![(1.0, 3)]]
    );
    assert!(
        r.certificate
            .steps()
            .iter()
            .all(|s| s.position().number() == 1)
    );
    assert_eq!(
        edges_of(&r.matrix),
        vec![(0, 3, 1.0), (1, 3, 1.0), (2, 3, 1.0)]
    );
    assert_eq!(r.stats.epochs, 2);
}

#[test]
fn isolated_edge_survives() {
    let m = SparseDistanceMatrix::from_triplets(
        5,
        &[(0, 1, 1.0), (0, 2, 1.0), (1, 2, 1.0), (3, 4, 1.0)],
    )
    .unwrap();
    let r = collapse_sparse(&m, None).unwrap();
    check_invariants(&r);
    let removed: Vec<_> = r.certificate.steps().iter().map(|s| s.edge()).collect();
    assert_eq!(removed, vec![(0, 1)]);
    assert!(edges_of(&r.matrix).contains(&(3, 4, 1.0)));
}

#[test]
fn threshold_drops_edges_before_collapse() {
    let d = DistanceMatrix::from_condensed(vec![1.0, 1.0, 5.0]).unwrap();
    let r = collapse_dense(&d, Some(2.0)).unwrap();
    check_invariants(&r);
    assert_eq!(r.certificate.requested_threshold(), Some(2.0));
    assert_eq!(r.certificate.terminal_level(), 2.0);
    assert_eq!(r.certificate.input_edge_count(), 2);
    assert!(r.certificate.steps().is_empty());
    assert_eq!(edges_of(&r.matrix), vec![(0, 1, 1.0), (0, 2, 1.0)]);
    assert_eq!(r.stats.epochs, 1);
}

#[test]
fn infinite_dense_entries_are_absent_edges() {
    let d = DistanceMatrix::from_condensed(vec![1.0, 1.0, f64::INFINITY]).unwrap();
    let r = collapse_dense(&d, Some(f64::INFINITY)).unwrap();
    check_invariants(&r);
    assert_eq!(r.certificate.input_edge_count(), 2);
    assert_eq!(r.certificate.terminal_level(), 1.0);
    assert!(r.certificate.steps().is_empty());
}

#[test]
fn reruns_are_identical() {
    let d = DistanceMatrix::from_condensed(tie_heavy_condensed()).unwrap();
    let a = collapse_dense(&d, None).unwrap();
    let b = collapse_dense(&d, None).unwrap();
    assert_eq!(a.certificate, b.certificate);
    assert_eq!(a.stats, b.stats);
    assert_eq!(edges_of(&a.matrix), edges_of(&b.matrix));
    assert!(a.stats.removed_edges > 0);
    check_invariants(&a);
}

#[test]
fn dense_and_sparse_agree() {
    let condensed = tie_heavy_condensed();
    let d = DistanceMatrix::from_condensed(condensed.clone()).unwrap();
    let mut triplets = Vec::new();
    let mut k = 0;
    for i in 1..6 {
        for j in 0..i {
            triplets.push((i, j, condensed[k]));
            k += 1;
        }
    }
    let s = SparseDistanceMatrix::from_triplets(6, &triplets).unwrap();
    let rd = collapse_dense(&d, Some(2.0)).unwrap();
    let rs = collapse_sparse(&s, Some(2.0)).unwrap();
    check_invariants(&rd);
    assert_eq!(rd.certificate, rs.certificate);
    assert_eq!(rd.stats, rs.stats);
    assert_eq!(edges_of(&rd.matrix), edges_of(&rs.matrix));
    assert!(rd.stats.removed_edges > 0);
}

// Exact counters for unit K4: pass 1 tests 6 edges and removes 3, pass
// 2 tests the 3 survivors, every removal has one segment, and the
// largest C is the pair {2, 3} seen by edge (0,1).
#[test]
fn stats_match_the_certificate() {
    let d = DistanceMatrix::from_condensed(vec![1.0; 6]).unwrap();
    let r = collapse_dense(&d, None).unwrap();
    check_invariants(&r);
    assert_eq!(r.stats.input_edges, 6);
    assert_eq!(r.stats.output_edges, 3);
    assert_eq!(r.stats.removed_edges, 3);
    assert_eq!(r.stats.epochs, 2);
    // Every edge is tested once in pass 1. The three survivors are last
    // dirtied before their own pass-1 tests, so pass 2 retests nothing.
    assert_eq!(r.stats.edge_tests, 6);
    assert_eq!(r.stats.witness_segments, 3);
    assert_eq!(r.stats.max_common_neighborhood, 2);
}

#[test]
fn empty_inputs_yield_empty_certificates() {
    let d0 = DistanceMatrix::from_points(&[]).unwrap();
    let d1 = DistanceMatrix::from_condensed(vec![]).unwrap();
    let s0 = SparseDistanceMatrix::from_triplets(0, &[]).unwrap();
    for r in [
        collapse_dense(&d0, None).unwrap(),
        collapse_dense(&d1, None).unwrap(),
        collapse_sparse(&s0, None).unwrap(),
    ] {
        check_invariants(&r);
        assert_eq!(r.certificate.input_edge_count(), 0);
        assert_eq!(r.certificate.terminal_level(), 0.0);
        assert!(r.certificate.steps().is_empty());
        assert_eq!(r.stats.epochs, 1);
        assert_eq!(r.stats.edge_tests, 0);
    }
}

#[test]
fn invalid_thresholds_are_rejected() {
    let d = DistanceMatrix::from_condensed(vec![1.0]).unwrap();
    assert!(collapse_dense(&d, Some(-1.0)).is_err());
    assert!(collapse_dense(&d, Some(f64::NAN)).is_err());
}
