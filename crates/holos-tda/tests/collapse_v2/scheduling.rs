use super::*;
use holos_tda::collapse::{collapse_dense, collapse_sparse};

#[test]
fn overlapping_but_commuting_read_sets() {
    // Two triangles sharing vertex 2. The read sets S((0, 1)) = {0, 1, 2}
    // and S((2, 3)) = {2, 3, 4} share a vertex, but neither edge has both
    // endpoints inside the other set, so the two removals commute and the
    // conflict rule must keep them in the same round. A rule that blocked on
    // any read-set overlap would split them.
    let edges = [
        (0, 1, 1.0),
        (0, 2, 1.0),
        (1, 2, 1.0),
        (2, 3, 1.0),
        (2, 4, 1.0),
        (3, 4, 1.0),
    ];
    let dense = dense_from_edges(5, &edges);
    let adj = adjacency(5, &edges);
    let s01 = common_closed_set(&adj, 0, 1);
    let s23 = common_closed_set(&adj, 2, 3);
    assert_eq!(s01, vec![0, 1, 2], "S((0, 1))");
    assert_eq!(s23, vec![2, 3, 4], "S((2, 3))");
    assert!(
        s01.contains(&2) && s23.contains(&2),
        "the two read sets must share a vertex"
    );
    assert!(
        !(s01.contains(&2) && s01.contains(&3)),
        "edge (2, 3) must not lie inside S((0, 1))"
    );
    assert!(
        !(s23.contains(&0) && s23.contains(&1)),
        "edge (0, 1) must not lie inside S((2, 3))"
    );

    let result = v2_dense("bowtie", &dense, Some(1.0), 2);
    let first = step_for(&result, (0, 1)).expect("edge (0, 1) must be removable");
    let second = step_for(&result, (2, 3)).expect("edge (2, 3) must be removable");
    assert_eq!(
        first.position().number(),
        1,
        "edge (0, 1) must go in round 1"
    );
    assert_eq!(
        second.position().number(),
        1,
        "edge (2, 3) must go in round 1"
    );
    assert_eq!(
        round_widths(&result)[0],
        2,
        "round 1 must take both commuting removals"
    );
    assert_fixture_barcode("bowtie", &dense, Some(1.0), 2);
}

#[test]
fn conflicting_removals_split_rounds() {
    // K4 alone. In the complete graph S(e) is every vertex, so every pair of
    // removable edges conflicts and round 1 can take one edge only. The rest
    // must wait for later rounds, and the schedule must stop at a spanning
    // star, where no edge has a common neighbor.
    let dense = complete_matrix(4);
    let result = v2_dense("k4", &dense, Some(1.0), 2);
    assert_eq!(
        round_widths(&result),
        vec![1, 2, 0],
        "K4: one removal in the conflict clique, then the two that commute"
    );
    assert_eq!(
        result.certificate.steps().len(),
        3,
        "K4 collapses to a spanning tree"
    );
    assert_eq!(result.stats.epochs, 3, "K4: rounds");
    assert!(
        result
            .certificate
            .steps()
            .iter()
            .any(|s| s.position().number() >= 2),
        "the conflicting removals must spread over rounds"
    );

    let survivors = edge_list(&result.matrix);
    assert_eq!(survivors.len(), 3, "three edges survive");
    let hub = (0..4)
        .find(|&h| survivors.iter().all(|&(u, v, _)| u == h || v == h))
        .expect("the fixed point must be a spanning star");
    assert!(hub < 4, "star centre out of range");
    assert_fixture_barcode("k4", &dense, Some(1.0), 2);
}

#[test]
fn conflict_clique_batch_width_one() {
    // A complete graph is one conflict clique: S(e) is every vertex, so the
    // greedy selection can take a single edge however many are removable.
    // K3 stays complete until it stops yielding, so every one of its rounds
    // has width one; the larger cliques must at least start that way.
    for n in 3..=6 {
        let dense = complete_matrix(n);
        let result = v2_dense(&format!("k{n}"), &dense, Some(1.0), 2);
        let widths = round_widths(&result);
        assert_eq!(
            widths[0], 1,
            "K{n}: the conflict clique allows one removal in round 1, got {widths:?}"
        );
    }

    let dense = complete_matrix(3);
    let result = v2_dense("k3", &dense, Some(1.0), 1);
    assert_eq!(
        round_widths(&result),
        vec![1, 0],
        "K3: one removal, then a round that removes nothing"
    );
    assert_eq!(
        result.stats.epochs,
        result.certificate.steps().len() + 1,
        "K3: one round per removal plus the closing round"
    );
    assert_fixture_barcode("k3", &dense, Some(1.0), 2);
}

#[test]
fn many_disjoint_k4s() {
    // Eight K4 components. Conflicts never cross a component, so the batch
    // width is the component count: round 1 must take exactly one edge from
    // each K4, which is what makes the round structure scale at all.
    let blocks = 8;
    let dense = disjoint_k4_matrix(blocks);
    let result = v2_dense("k4x8", &dense, Some(1.0), 4);
    assert_eq!(
        round_widths(&result),
        vec![blocks, 2 * blocks, 0],
        "eight independent K4 schedules must run in lockstep"
    );
    assert_eq!(
        result.certificate.steps().len(),
        3 * blocks,
        "each K4 gives up three edges"
    );

    let mut round1: Vec<usize> = result
        .certificate
        .steps()
        .iter()
        .filter(|s| s.position().number() == 1)
        .map(|s| s.edge().0 / 4)
        .collect();
    round1.sort_unstable();
    assert_eq!(
        round1,
        (0..blocks).collect::<Vec<_>>(),
        "round 1 must take one edge from every component"
    );
    for step in result.certificate.steps() {
        let (u, v) = step.edge();
        assert_eq!(u / 4, v / 4, "removal of ({u}, {v}) crossed a component");
    }
    assert_fixture_barcode("k4x8", &dense, Some(1.0), 2);
}

#[test]
fn later_round_removability() {
    // Edge (0, 1) has two candidates, 2 and 3, and they are not adjacent, so
    // it fails against the round 1 snapshot. Round 1 removes (0, 3), which
    // drops candidate 3, and (0, 1) leaves in round 2. A schedule that only
    // ever read the first snapshot would keep it forever.
    let dense = later_round_matrix();
    let result = v2_dense("later_round", &dense, Some(1.0), 2);
    let step = step_for(&result, (0, 1)).expect("edge (0, 1) must be removable in a later round");
    assert_eq!(
        step.position().number(),
        2,
        "edge (0, 1) must survive round 1 and leave in round 2"
    );
    assert!(
        result
            .certificate
            .steps()
            .iter()
            .any(|s| s.position().number() >= 2),
        "no removal happened after the first round"
    );
    assert_eq!(
        round_widths(&result),
        vec![3, 1, 0],
        "three commuting removals, then the edge they unlocked"
    );
    assert!(
        result.stats.epochs >= 3,
        "a removal in round 2 needs a third, empty round, got {}",
        result.stats.epochs
    );
    assert_fixture_barcode("later_round", &dense, Some(1.0), 2);
}

#[test]
fn v1_v2_schedules_diverge() {
    // Two triangles sharing the edge (1, 2), with the pair (0, 3) absent.
    // Version 1 removes (0, 1) and then sees a graph where (1, 2) has become
    // removable, so it takes that. Version 2 tests everything against the
    // round 1 snapshot, where (1, 2) fails and (1, 3) succeeds, and (1, 3)
    // does not conflict with (0, 1). The two schedules therefore delete
    // different edges and stop at different fixed points. Only the barcode
    // has to agree.
    let dense = diamond_matrix();
    let sparse = sparse_from_dense(&dense);
    let threshold = Some(1.0);
    let v1 = collapse_dense(&dense, threshold).unwrap();
    let v2 = v2_dense("diamond", &dense, threshold, 2);

    assert_eq!(v1.certificate.algorithm_version(), 1, "version 1 tag");
    assert_eq!(v2.certificate.algorithm_version(), 2, "version 2 tag");
    assert_ne!(
        removed_set(&v1),
        removed_set(&v2),
        "the two schedules must delete different edge sets here"
    );
    assert_ne!(
        v1.certificate, v2.certificate,
        "the certificates must differ"
    );
    assert_ne!(
        edge_list(&v1.matrix),
        edge_list(&v2.matrix),
        "the two fixed points must differ"
    );
    assert_eq!(
        v1.certificate.steps().len(),
        v2.certificate.steps().len(),
        "both schedules remove two edges here"
    );

    let v1_sparse = collapse_sparse(&sparse, threshold).unwrap();
    let v2_sparse_result = v2_sparse("diamond", &sparse, threshold, 2);
    for &modulus in &MODULI {
        for max_dim in 0..=2 {
            let plain = dense_bars(&dense, max_dim, threshold, modulus, 1, ALL_ON, false);
            let label = format!("diamond p={modulus} max_dim={max_dim}");
            assert_eq!(
                plain,
                collapsed_bars(&v1, max_dim, modulus, 1, ALL_ON),
                "{label}: version 1 dense"
            );
            assert_eq!(
                plain,
                collapsed_bars(&v2, max_dim, modulus, 1, ALL_ON),
                "{label}: version 2 dense"
            );
            assert_eq!(
                plain,
                collapsed_bars(&v1_sparse, max_dim, modulus, 1, ALL_ON),
                "{label}: version 1 sparse"
            );
            assert_eq!(
                plain,
                collapsed_bars(&v2_sparse_result, max_dim, modulus, 1, ALL_ON),
                "{label}: version 2 sparse"
            );
            assert_eq!(
                plain,
                oracle_bars(&dense, max_dim, threshold, modulus),
                "{label}: oracle"
            );
        }
    }
}
