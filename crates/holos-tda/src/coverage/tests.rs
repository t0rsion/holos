use super::*;
use crate::SparseDistanceMatrix;

fn model() -> PlanarCoverageModel {
    PlanarCoverageModel::new(1.0, 1.0).unwrap()
}

fn wheel() -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(
        5,
        &[
            (0, 1, 1.0),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (0, 3, 1.0),
            (0, 4, 1.0),
            (1, 4, 1.0),
            (2, 4, 1.0),
            (3, 4, 1.0),
        ],
    )
    .unwrap()
}

#[test]
fn a_wheel_fills_its_fence_over_several_fields() {
    let fence = CoverageFence::new(vec![0, 1, 2, 3]).unwrap();
    for modulus in [2, 3, 5] {
        let evaluation = evaluate_planar_coverage(
            &wheel(),
            &[0, 1, 2, 3, 4],
            &fence,
            modulus,
            model(),
            CoverageLimits::default(),
        )
        .unwrap();
        assert!(evaluation.criterion_holds);
        assert_eq!(evaluation.active_triangles, 4);
        assert_eq!(evaluation.witness.len(), 4);
    }
}

#[test]
fn the_unfilled_fence_fails_the_criterion() {
    let evaluation = evaluate_planar_coverage(
        &wheel(),
        &[0, 1, 2, 3],
        &CoverageFence::new(vec![0, 1, 2, 3]).unwrap(),
        2,
        model(),
        CoverageLimits::default(),
    )
    .unwrap();
    assert!(!evaluation.criterion_holds);
    assert!(evaluation.witness.is_empty());
}

#[test]
fn fence_order_has_one_canonical_orientation() {
    let expected = CoverageFence::new(vec![0, 1, 2, 3]).unwrap();
    assert_eq!(expected, CoverageFence::new(vec![2, 3, 0, 1]).unwrap());
    assert_eq!(expected, CoverageFence::new(vec![2, 1, 0, 3]).unwrap());
}

#[test]
fn radius_inequality_uses_exact_dyadic_values() {
    assert!(PlanarCoverageModel::new(1.0, 0.5).is_err());
    assert!(PlanarCoverageModel::new(1.0, 0.6).is_ok());
}

#[test]
fn a_missing_fence_edge_is_rejected() {
    let graph =
        SparseDistanceMatrix::from_triplets(4, &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0)]).unwrap();
    assert!(
        evaluate_planar_coverage(
            &graph,
            &[0, 1, 2, 3],
            &CoverageFence::new(vec![0, 1, 2, 3]).unwrap(),
            2,
            model(),
            CoverageLimits::default(),
        )
        .is_err()
    );
}
