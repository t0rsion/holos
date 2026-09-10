use super::*;
use crate::{CertificateLimits, IndexParams, RipsParams, SparseDistanceMatrix};

fn graph(changed: bool) -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(
        6,
        &[
            (0, 1, 0.25),
            (0, 2, if changed { 1.01 } else { 1.0 }),
            (1, 2, 1.5),
            (0, 3, 1.2),
            (1, 3, 1.7),
            (0, 4, 1.1),
            (1, 4, 1.6),
            (0, 5, 1.3),
            (1, 5, 1.8),
        ],
    )
    .unwrap()
}

#[test]
fn warm_delta_contains_only_changed_tree_nodes() {
    let initial = graph(false);
    let params = RipsParams::new(1).with_modulus(3);
    let first = PersistenceIndex::compile(
        &initial,
        &params,
        IndexParams::default(),
        CertificateLimits::default(),
    )
    .unwrap();
    let second = first.transition(&graph(true)).unwrap().index;
    let snapshot = IndexSnapshotProof::from_index(&first).unwrap();
    let delta = IndexDeltaProof::between(&first, &second).unwrap();
    assert_eq!(delta.summary().edge_changes, 1);
    assert!(delta.summary().nodes < snapshot.summary().nodes);
    assert!(snapshot.encode().unwrap().starts_with(SNAPSHOT_MAGIC));
    assert!(delta.encode().unwrap().starts_with(DELTA_MAGIC));
}

#[test]
fn delta_rejects_a_different_envelope() {
    let initial = graph(false);
    let params = RipsParams::new(1);
    let first = PersistenceIndex::compile(
        &initial,
        &params,
        IndexParams::default(),
        CertificateLimits::default(),
    )
    .unwrap();
    let other_graph = SparseDistanceMatrix::from_triplets(2, &[(0, 1, 1.0)]).unwrap();
    let second = PersistenceIndex::compile(
        &other_graph,
        &params,
        IndexParams::default(),
        CertificateLimits::default(),
    )
    .unwrap();
    assert!(IndexDeltaProof::between(&first, &second).is_err());
}
