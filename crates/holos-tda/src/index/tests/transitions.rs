use super::support::*;
use super::*;

#[test]
fn local_transition_shares_untouched_subtrees() {
    let initial = shared_edge_graph(0.25);
    let updated = SparseDistanceMatrix::from_triplets(
        6,
        &[
            (0, 1, 0.25),
            (0, 2, 1.01),
            (1, 2, 1.5),
            (0, 3, 1.2),
            (1, 3, 1.7),
            (0, 4, 1.1),
            (1, 4, 1.6),
            (0, 5, 1.3),
            (1, 5, 1.8),
        ],
    )
    .unwrap();
    let params = RipsParams::new(1).with_modulus(3);
    let index = PersistenceIndex::compile(
        &initial,
        &params,
        compose_index_params(),
        CertificateLimits::default(),
    )
    .unwrap();
    let transition = index.transition(&updated).unwrap();
    let expected = rips_persistence_sparse(&updated, &params).unwrap();
    assert_eq!(transition.index.diagram().bars, expected.bars);
    assert!(transition.work.nodes_shared > 0);
    assert_eq!(
        index.shared_nodes_with(&transition.index),
        transition.work.nodes_shared
    );
}

#[test]
fn threshold_crossing_keeps_the_tree_and_rebuilds_reductions() {
    let initial = shared_edge_graph(0.25);
    let updated = shared_edge_graph(2.5);
    let mut params = RipsParams::new(1).with_modulus(5);
    params.threshold = Some(2.0);
    let index = PersistenceIndex::compile(
        &initial,
        &params,
        compose_index_params(),
        CertificateLimits::default(),
    )
    .unwrap();
    let transition = index.transition(&updated).unwrap();
    assert_eq!(transition.mode, IndexUpdateMode::Rebuilt);
    assert!(
        transition
            .events
            .iter()
            .any(|event| event.kind == IndexEventKind::ThresholdCrossing)
    );
    assert_eq!(index.summary(), transition.index.summary());
    assert_eq!(
        transition.index.diagram().bars,
        rips_persistence_sparse(&updated, &params).unwrap().bars
    );
}

#[test]
fn exact_relations_remain_opt_in() {
    let initial = SparseDistanceMatrix::from_triplets(
        4,
        &[(0, 1, 1.0), (1, 2, 1.1), (2, 3, 1.2), (0, 3, 1.3)],
    )
    .unwrap();
    let updated = SparseDistanceMatrix::from_triplets(
        4,
        &[(0, 1, 1.01), (1, 2, 1.1), (2, 3, 1.2), (0, 3, 1.3)],
    )
    .unwrap();
    let params = RipsParams::new(1).with_modulus(3);
    let index = PersistenceIndex::compile(
        &initial,
        &params,
        compose_index_params(),
        CertificateLimits::default(),
    )
    .unwrap();
    let transition = index
        .transition_with(&updated, CorrespondenceMode::Exact)
        .unwrap();
    assert!(!transition.correspondence.is_empty());
}

#[test]
fn edits_cross_the_threshold_without_changing_the_envelope() {
    let initial = shared_edge_graph(0.25);
    let mut params = RipsParams::new(1);
    params.threshold = Some(2.0);
    let index = PersistenceIndex::compile(
        &initial,
        &params,
        compose_index_params(),
        CertificateLimits::default(),
    )
    .unwrap();
    let transition = index
        .transition_edits(&[IndexEdit::deactivate(0, 1)])
        .unwrap();
    assert_eq!(transition.index.graph().num_edges(), initial.num_edges());
    assert_eq!(transition.work.edges_checked, 1);
    assert_eq!(transition.mode, IndexUpdateMode::Rebuilt);
    assert!(
        transition
            .events
            .iter()
            .any(|event| event.kind == IndexEventKind::ThresholdCrossing)
    );
}
