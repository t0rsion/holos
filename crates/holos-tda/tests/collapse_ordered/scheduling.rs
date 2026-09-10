use super::*;

#[test]
fn forward_arming() {
    // Attack: a removal arms a previously clean edge that lies inside the
    // retirement span of the window that is running. The armed edge was
    // not a member, so its verdict cannot come from the cache; it must be
    // tested serially at its turn and leave in the same pass.
    let dense = forward_arming_matrix();
    let result = fixture("forward_arming", &dense, Some(2.0), 4, ONE_STAGE);

    let armed = step_for(&result, (0, 1)).expect("the armed edge must be removed");
    assert_eq!(
        armed.position().number(),
        2,
        "the armed edge must leave in the pass that armed it"
    );
    assert_eq!(
        armed.witnesses().to_vec(),
        vec![(1.0, 2)],
        "the armed edge is certified by its only remaining candidate"
    );
    let arming = step_for(&result, (0, 3)).expect("the arming removal must happen");
    assert_eq!(
        arming.position().number(),
        2,
        "the arming removal is in pass 2"
    );
    assert_eq!(
        step_for(&result, (1, 4)).map(|s| s.position().number()),
        Some(1),
        "the pass 1 removal that unblocks (1, 3)"
    );
    assert!(
        result.stats.epochs >= 3,
        "a pass 2 removal needs a third pass, got {}",
        result.stats.epochs
    );

    // The same trace with no speculation at all.
    let serial_windows = collapse_dense_ordered_with_window(&dense, Some(2.0), 8, 1).unwrap();
    assert_same_output("forward_arming: W=1", &serial_windows, &result);
}

#[test]
fn backward_dirtiness() {
    // Attack: a removal dirties an already-retired lower position. The
    // schedule may not revisit it in this pass; it must come back in the
    // next one. Edge (0, 1) is tested first and fails, then the removal
    // of (0, 3) dirties it, and it leaves in pass 2.
    let dense = later_pass_matrix();
    let result = fixture("backward_dirtiness", &dense, Some(1.0), 4, ONE_STAGE);

    let step = step_for(&result, (0, 1)).expect("the dirtied edge must come back");
    assert_eq!(
        step.position().number(),
        2,
        "a backward dirty flag must be served in the next pass"
    );
    assert!(
        result
            .certificate
            .steps()
            .iter()
            .any(|s| s.edge() == (0, 3) && s.position().number() == 1),
        "the removal that dirties (0, 1) must be in pass 1"
    );
    assert!(
        result.stats.epochs >= 3,
        "a pass 2 removal needs a third pass, got {}",
        result.stats.epochs
    );
}

#[test]
fn stale_negative_to_positive() {
    // Attack: a cached verdict of false that becomes true before its turn.
    // At FORM, edge (0, 1) has the non-adjacent candidates 2 and 3 and no
    // apex. The earlier removal of (1, 3) drops candidate 3, so at its
    // turn the edge is dominated by vertex 2 and must leave in pass 1.
    let dense = dense_from_edges(
        4,
        &[
            (0, 1, 1.0),
            (0, 2, 1.0),
            (1, 2, 1.0),
            (0, 3, 1.0),
            (1, 3, 2.0),
        ],
    );
    let result = fixture(
        "stale_negative_to_positive",
        &dense,
        Some(2.0),
        4,
        ONE_STAGE,
    );

    let step = step_for(&result, (0, 1)).expect("the repaired edge must be removed");
    assert_eq!(
        step.position().number(),
        1,
        "the repair must happen inside pass 1"
    );
    assert_eq!(
        step.witnesses().to_vec(),
        vec![(1.0, 2)],
        "the surviving candidate certifies the removal"
    );
    assert_eq!(
        step_for(&result, (1, 3)).map(|s| s.position().number()),
        Some(1),
        "the conflicting removal is the first step"
    );
    assert!(
        result.stats.invalidated_results >= 1,
        "a stale member must be re-evaluated at its turn"
    );
}

#[test]
fn stale_positive_to_negative() {
    // Attack: a cached verdict of true, with witnesses, that a conflicting
    // removal destroys. At FORM, edge (0, 1) is dominated by vertex 2 at
    // both levels because f(2, 3) = 2. The earlier removal of (2, 3) ends
    // that domination, and the repaired verdict must keep the edge.
    // Vertices 4 and 5 block (0, 3) and (1, 3) through pass 1.
    let dense = dense_from_edges(
        6,
        &[
            (0, 1, 1.0),
            (0, 2, 1.0),
            (1, 2, 1.0),
            (0, 3, 2.0),
            (1, 3, 2.0),
            (2, 3, 2.0),
            (0, 4, 2.0),
            (3, 4, 2.0),
            (1, 5, 2.0),
            (3, 5, 2.0),
        ],
    );
    let result = fixture(
        "stale_positive_to_negative",
        &dense,
        Some(2.0),
        4,
        ONE_STAGE,
    );

    assert!(
        step_for(&result, (0, 1)).is_none(),
        "the stale positive must not survive the repair"
    );
    assert_eq!(
        step_for(&result, (2, 3)).map(|s| s.position().number()),
        Some(1),
        "the conflicting removal is in pass 1"
    );
    assert!(
        result.stats.invalidated_results >= 1,
        "the stale member must be re-evaluated at its turn"
    );
}

#[test]
fn changed_witness_same_verdict() {
    // Attack: a conflicting removal that changes the witnesses and leaves
    // the verdict alone. At FORM, edge (0, 1) has the candidates 2, 3, and
    // 4, and needs two segments: vertex 2 from level 1, then vertex 3 from
    // level 2, because f(2, 4) is absent. The earlier removal of (1, 4)
    // drops candidate 4, so one segment with apex 2 now covers the whole
    // range. Reusing the cache would record two segments.
    let mut edges = vec![
        (0, 1, 1.0),
        (0, 2, 1.0),
        (1, 2, 1.0),
        (0, 3, 2.0),
        (1, 3, 2.0),
        (0, 4, 2.0),
        (1, 4, 2.0),
        (2, 3, 2.0),
        (3, 4, 2.0),
    ];
    // Private blockers: each keeps one value 2 edge alive through pass 1,
    // so only (1, 4) fires before (0, 1) is retired.
    for &(u, v) in &[
        (0, 5),
        (3, 5),
        (1, 6),
        (3, 6),
        (2, 7),
        (3, 7),
        (0, 8),
        (4, 8),
        (3, 9),
        (4, 9),
    ] {
        edges.push((u, v, 2.0));
    }
    let dense = dense_from_edges(10, &edges);
    let result = fixture(
        "changed_witness_same_verdict",
        &dense,
        Some(2.0),
        4,
        ONE_STAGE,
    );

    let step = step_for(&result, (0, 1)).expect("the repaired edge must still be removed");
    assert_eq!(
        step.position().number(),
        1,
        "the verdict is unchanged, so the pass is"
    );
    assert_eq!(
        step.witnesses().to_vec(),
        vec![(1.0, 2)],
        "the repair must record the witnesses of the graph at the turn"
    );
    assert_eq!(
        step_for(&result, (1, 4)).map(|s| s.position().number()),
        Some(1),
        "the conflicting removal is in pass 1"
    );
    assert!(
        result.stats.invalidated_results >= 1,
        "the changed witnesses must come from a repair"
    );
}

#[test]
fn one_repair_for_many_invalidations() {
    // Attack: two removals conflict with the same cached result. The spine
    // (0, 1) is the last position, and both (1, 2) and (1, 4) mark it
    // through their affected sets {0, 1, 2} and {0, 1, 4}. Staleness is a
    // property of the slot, not a counter, so the whole run must show one
    // invalidation and one repair. No other member is ever stale: every
    // other edge of an affected set is already retired.
    let dense = book_matrix(1.0);
    let result = fixture(
        "one_repair_for_many_invalidations",
        &dense,
        Some(2.0),
        4,
        ONE_STAGE,
    );

    let removals: Vec<((usize, usize), usize)> = result
        .certificate
        .steps()
        .iter()
        .map(|s| (s.edge(), s.position().number()))
        .collect();
    assert_eq!(
        removals,
        vec![((1, 2), 1), ((1, 4), 1), ((0, 2), 2), ((0, 4), 2),],
        "hand-simulated removal sequence"
    );
    assert!(
        step_for(&result, (0, 1)).is_none(),
        "the spine loses both candidates and survives"
    );
    assert_eq!(
        result.stats.invalidated_results, 1,
        "two conflicting removals may stale one slot once"
    );
    assert_eq!(
        result.stats.invalidated_results, 1,
        "one stale slot costs one repair"
    );
    assert_eq!(
        result.stats.global_invalidations, 0,
        "no affected set here reaches the marking limit"
    );
}

#[test]
fn nonconflicting_reuse() {
    // Attack: an unrelated removal between FORM and a member's turn. The
    // same two gadgets, with the spine raised to 2.0 so it retires first.
    // The removal of (1, 2) then lies between FORM and the turn of (1, 4),
    // and its affected set {0, 1, 2} holds no later member, so the cached
    // verdict of (1, 4) must be reused unchanged.
    let dense = book_matrix(2.0);
    let result = fixture("nonconflicting_reuse", &dense, Some(2.0), 4, ONE_STAGE);

    assert!(
        result.stats.removed_edges >= 2,
        "the fixture needs removals to reuse around, got {}",
        result.stats.removed_edges
    );
    assert_eq!(
        step_for(&result, (1, 2)).map(|s| s.position().number()),
        Some(1),
        "the unrelated removal is in pass 1"
    );
    assert_eq!(
        step_for(&result, (1, 4)).map(|s| s.position().number()),
        Some(1),
        "the reusing member leaves in the same pass"
    );
    assert_eq!(
        result.stats.invalidated_results, 0,
        "no removal here conflicts with a later member"
    );
    assert_eq!(
        result.stats.invalidated_results, 0,
        "nothing stale means nothing to repair"
    );
    assert_eq!(
        result.stats.edge_tests, result.stats.logical_tests,
        "with no repair, every physical test is a logical one"
    );
}

#[test]
fn large_s_global_invalidation() {
    // Attack: a removal whose affected set passes the marking limit in the
    // middle of a window. Fine marking bails, so the rest of the window
    // loses its cache and the next pass retests every live edge. K68 makes
    // the first removal bail: its affected set is all 68 vertices.
    let n = 68;
    let mut edges = Vec::new();
    for u in 0..n {
        for v in (u + 1)..n {
            edges.push((u, v, 1.0));
        }
    }
    let dense = dense_from_edges(n, &edges);
    let ordered = collapse_dense_ordered_with_window(&dense, Some(1.0), 4, ONE_STAGE).unwrap();
    let serial = collapse_dense(&dense, Some(1.0)).unwrap();
    assert_matches_serial("large_s_global_invalidation", &ordered, &serial);
    verify_dense(&dense, Some(1.0), &ordered)
        .unwrap_or_else(|e| panic!("large_s_global_invalidation: verifier: {e}"));

    assert!(
        ordered.stats.global_invalidations >= 1,
        "the marking limit must be reached at least once"
    );
    assert!(
        ordered.stats.invalidated_results >= 1,
        "a bail must stale the window remainder"
    );
}

#[test]
fn underfilled_final_pass() {
    // Attack: the final pass, whose due set is smaller than the window. It
    // removes nothing and must still be counted. The octahedron is a flag
    // 2-sphere with no removable edge, so its whole run is that one pass.
    let n = 6;
    let mut edges = Vec::new();
    for u in 0..n {
        for v in (u + 1)..n {
            if u / 2 != v / 2 {
                edges.push((u, v, 1.0));
            }
        }
    }
    let octahedron = dense_from_edges(n, &edges);
    let result = fixture(
        "underfilled_final_pass",
        &octahedron,
        Some(1.0),
        8,
        ONE_STAGE,
    );
    assert!(
        result.certificate.steps().is_empty(),
        "the octahedron has no removable edge"
    );
    assert_eq!(result.stats.epochs, 1, "zero yield must take one pass");
    assert_eq!(
        result.stats.logical_tests, 12,
        "one logical test per edge in the only pass"
    );

    // A run that does remove: the last pass has no due edge at all and is
    // still counted, and no removal carries the last pass number.
    let dense = later_pass_matrix();
    let result = fixture(
        "underfilled_final_pass tail",
        &dense,
        Some(1.0),
        8,
        ONE_STAGE,
    );
    let last = result.stats.epochs;
    assert!(
        result
            .certificate
            .steps()
            .iter()
            .all(|s| s.position().number() < last),
        "the final pass must remove nothing"
    );
}

#[test]
fn oversized_window_and_workers() {
    // Attack: a window larger than the input and more workers than there
    // is work. Both are legal and neither may reach the output.
    let dense = later_pass_matrix();
    let base = fixture("oversized", &dense, Some(1.0), 8, 100_000);
    assert_eq!(
        base.certificate.input_edge_count(),
        9,
        "the fixture is smaller than the window"
    );
    for &workers in &[8usize, 64] {
        let got = collapse_dense_ordered_with_window(&dense, Some(1.0), workers, 100_000).unwrap();
        assert_same_output(&format!("oversized: workers={workers}"), &got, &base);
        assert_invariant_stats(&format!("oversized: workers={workers}"), &got, &base);
    }
}
