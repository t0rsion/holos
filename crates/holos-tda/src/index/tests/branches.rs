use super::support::*;
use super::*;

#[test]
fn batches_are_atomic_and_alternatives_keep_input_order() {
    let initial = shared_edge_graph(0.25);
    let first = SparseDistanceMatrix::from_triplets(
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
    let second = SparseDistanceMatrix::from_triplets(
        6,
        &[
            (0, 1, 0.25),
            (0, 2, 1.0),
            (1, 2, 1.5),
            (0, 3, 1.2),
            (1, 3, 1.7),
            (0, 4, 1.11),
            (1, 4, 1.6),
            (0, 5, 1.3),
            (1, 5, 1.8),
        ],
    )
    .unwrap();
    let invalid_triplets: Vec<_> = (0..6)
        .flat_map(|u| (u + 1..6).map(move |v| (u, v, 1.0 + (u + v) as f64 / 100.0)))
        .collect();
    let invalid = SparseDistanceMatrix::from_triplets(6, &invalid_triplets).unwrap();
    let mut params = RipsParams::new(1);
    params.threads = 2;
    let limits = CertificateLimits {
        max_triangles: 4,
        ..CertificateLimits::default()
    };
    let mut index =
        PersistenceIndex::compile(&initial, &params, IndexParams::default(), limits).unwrap();
    let original = index.version();
    assert!(index.advance_batch(&[first.clone(), invalid]).is_err());
    assert_eq!(index.version(), original);

    let branches = index.branch(&[first, second]).unwrap();
    assert_eq!(branches.len(), 2);
    assert_eq!(branches[0].index, 0);
    assert_eq!(branches[1].index, 1);
    assert_eq!(index.version(), original);
    assert!(
        branches
            .iter()
            .all(|branch| branch.transition.work.nodes_shared > 0)
    );
}

#[test]
fn disconnected_envelope_recompiles_after_a_bridge_is_added() {
    let initial = SparseDistanceMatrix::from_triplets(
        6,
        &[
            (0, 1, 1.0),
            (0, 2, 1.1),
            (1, 2, 1.2),
            (3, 4, 1.3),
            (3, 5, 1.4),
            (4, 5, 1.5),
        ],
    )
    .unwrap();
    let updated = SparseDistanceMatrix::from_triplets(
        6,
        &[
            (0, 1, 1.0),
            (0, 2, 1.1),
            (1, 2, 1.2),
            (2, 3, 1.25),
            (3, 4, 1.3),
            (3, 5, 1.4),
            (4, 5, 1.5),
        ],
    )
    .unwrap();
    let params = RipsParams::new(1).with_modulus(3);
    let index = PersistenceIndex::compile(
        &initial,
        &params,
        IndexParams::default(),
        CertificateLimits::default(),
    )
    .unwrap();
    assert_eq!(index.summary().component_splits, 1);
    let transition = index.transition(&updated).unwrap();
    assert_eq!(transition.mode, IndexUpdateMode::Recompiled);
    assert!(
        transition
            .events
            .iter()
            .any(|event| event.kind == IndexEventKind::EnvelopeRecompiled)
    );
    let diff = index.diff(&transition.index);
    assert!(!diff.same_envelope);
    assert_eq!(
        transition.index.diagram().bars,
        rips_persistence_sparse(&updated, &params).unwrap().bars
    );
}
