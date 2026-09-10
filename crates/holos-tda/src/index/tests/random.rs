use super::*;

#[test]
fn random_fixed_envelope_versions_match_clean_reduction() {
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    for case in 0..48 {
        let vertices = 4 + case % 5;
        let mut initial_triplets = Vec::new();
        let mut updated_triplets = Vec::new();
        for v in 1..vertices {
            for u in 0..v {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                if state % 4 == 0 {
                    continue;
                }
                let first = 0.25 * (1 + state % 12) as f64;
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let second = 0.25 * (1 + state % 12) as f64;
                initial_triplets.push((u, v, first));
                updated_triplets.push((u, v, second));
            }
        }
        let initial = SparseDistanceMatrix::from_triplets(vertices, &initial_triplets).unwrap();
        let updated = SparseDistanceMatrix::from_triplets(vertices, &updated_triplets).unwrap();
        for modulus in [2, 3, 5] {
            let mut params = RipsParams::new(case % 3).with_modulus(modulus);
            params.threshold = (case % 2 == 0).then_some(2.0);
            let index = PersistenceIndex::compile(
                &initial,
                &params,
                IndexParams::default(),
                CertificateLimits::default(),
            )
            .unwrap();
            assert_eq!(
                index.diagram().bars,
                rips_persistence_sparse(&initial, &params).unwrap().bars,
                "initial case {case}, modulus {modulus}, edges {:?}, interfaces {:?}",
                initial.edges().collect::<Vec<_>>(),
                index.interfaces(),
            );
            let transition = index.transition(&updated).unwrap();
            assert_eq!(
                transition.index.diagram().bars,
                rips_persistence_sparse(&updated, &params).unwrap().bars,
                "updated case {case}, modulus {modulus}, edges {:?}, interfaces {:?}",
                updated.edges().collect::<Vec<_>>(),
                transition.index.interfaces(),
            );
        }
    }
}

#[test]
fn relative_index_keeps_ancestor_separators_and_updates_one_route() {
    let graph = SparseDistanceMatrix::from_triplets(
        7,
        &[
            (0, 1, 0.75),
            (0, 3, 0.5),
            (0, 5, 2.5),
            (0, 6, 1.75),
            (1, 2, 3.0),
            (1, 3, 2.5),
            (1, 4, 1.5),
            (1, 5, 3.0),
            (2, 3, 1.0),
            (2, 4, 0.75),
            (2, 5, 0.5),
            (2, 6, 0.5),
            (3, 4, 2.75),
            (3, 5, 2.75),
            (4, 5, 3.0),
            (4, 6, 0.5),
            (5, 6, 2.0),
        ],
    )
    .unwrap();
    let mut params = RipsParams::new(2).with_modulus(3);
    params.threshold = Some(2.0);
    let index = PersistenceIndex::compile(
        &graph,
        &params,
        IndexParams::default(),
        CertificateLimits::default(),
    )
    .unwrap();
    assert_eq!(index.interfaces()[0].mode, InterfaceMode::Relative);
    assert_eq!(
        index.diagram().bars,
        rips_persistence_sparse(&graph, &params).unwrap().bars
    );
    let root_separator = index.interfaces()[0].separator.clone();
    for interface in index.interfaces().into_iter().skip(1) {
        for vertex in &root_separator {
            if interface.vertices.contains(vertex) {
                assert!(interface.protected_vertices.contains(vertex));
            }
        }
    }

    let transition = index
        .transition_edits(&[IndexEdit::set_weight(0, 1, 0.8)])
        .unwrap();
    assert_eq!(transition.mode, IndexUpdateMode::Relative);
    assert!(transition.work.relative_nodes_rebuilt > 0);
    assert!(transition.work.relative_nodes_composed > 0);
    assert!(transition.work.nodes_shared > 0);
    assert_eq!(
        transition.index.diagram().bars,
        rips_persistence_sparse(transition.index.graph(), &params)
            .unwrap()
            .bars
    );
}
