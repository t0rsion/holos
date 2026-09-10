use super::common::Rng;

use super::super::matrix::DistanceMatrix;
use super::super::point_cloud::{PointCloudGraph, PointCloudParams, PointCloudStrategy};
use super::super::sparse::SparseDistanceMatrix;

fn edge_bits(matrix: &SparseDistanceMatrix) -> Vec<(usize, usize, u64)> {
    matrix
        .edges()
        .map(|(u, v, distance)| (u, v, distance.to_bits()))
        .collect()
}

#[test]
fn threshold_point_kernels_match_the_dense_constructor() {
    let mut rng = Rng::new(0x8d47_2016_7f31);
    for dimensions in [0usize, 1, 2, 3, 7, 13] {
        let mut points = Vec::new();
        for i in 0..47 {
            let point = (0..dimensions)
                .map(|axis| {
                    let raw = (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
                    if i % 11 == 0 {
                        axis as f64 * 0.125
                    } else {
                        raw.mul_add(4.0, -2.0)
                    }
                })
                .collect();
            points.push(point);
        }
        let dense = DistanceMatrix::from_points(&points).unwrap();
        for threshold in [0.0, 0.25, 1.0, 4.0, f64::INFINITY] {
            let expected = edge_bits(&dense.to_sparse_at(threshold).unwrap());
            for strategy in [PointCloudStrategy::KdTree, PointCloudStrategy::Exhaustive] {
                let serial = PointCloudGraph::build(
                    &points,
                    PointCloudParams::new(threshold).with_strategy(strategy),
                )
                .unwrap();
                let parallel = PointCloudGraph::build(
                    &points,
                    PointCloudParams::new(threshold)
                        .with_strategy(strategy)
                        .with_threads(3),
                )
                .unwrap();
                assert_eq!(
                    edge_bits(serial.matrix()),
                    expected,
                    "dimensions {dimensions}, threshold {threshold}, strategy {strategy:?}"
                );
                assert_eq!(edge_bits(parallel.matrix()), expected);
                assert_eq!(serial.stats(), parallel.stats());
            }
        }
    }
}

#[test]
fn threshold_point_constructor_preserves_extreme_distance_bits() {
    let points = vec![
        vec![0.0, 0.0],
        vec![1e-200, 0.0],
        vec![0.0, 1e200],
        vec![1e308, 0.0],
        vec![-1e308, 0.0],
    ];
    let dense = DistanceMatrix::from_points(&points).unwrap();
    for threshold in [0.0, 1e-200, 1e200, f64::MAX, f64::INFINITY] {
        let expected = edge_bits(&dense.to_sparse_at(threshold).unwrap());
        let graph = PointCloudGraph::build(&points, PointCloudParams::new(threshold)).unwrap();
        assert_eq!(edge_bits(graph.matrix()), expected, "threshold {threshold}");
    }
}

#[test]
fn automatic_point_route_is_frozen() {
    let low = vec![vec![0.0; 12], vec![1.0; 12]];
    let high = vec![vec![0.0; 13], vec![1.0; 13]];
    assert_eq!(
        PointCloudGraph::build(&low, PointCloudParams::new(1.0))
            .unwrap()
            .stats()
            .strategy,
        PointCloudStrategy::KdTree
    );
    assert_eq!(
        PointCloudGraph::build(&high, PointCloudParams::new(1.0))
            .unwrap()
            .stats()
            .strategy,
        PointCloudStrategy::Exhaustive
    );
    assert_eq!(
        PointCloudGraph::build(&low, PointCloudParams::new(f64::INFINITY))
            .unwrap()
            .stats()
            .strategy,
        PointCloudStrategy::Exhaustive
    );
}

#[test]
fn overflowing_difference_is_an_absent_edge() {
    let d = DistanceMatrix::from_points(&[vec![1e308], vec![-1e308]]).unwrap();
    assert_eq!(d.get(0, 1), f64::INFINITY);
}

#[test]
fn non_finite_coordinates_are_rejected() {
    assert!(DistanceMatrix::from_points(&[vec![f64::INFINITY], vec![0.0]]).is_err());
    assert!(DistanceMatrix::from_points(&[vec![f64::NAN], vec![0.0]]).is_err());
}

#[test]
fn negative_zero_entries_are_normalized() {
    let d = DistanceMatrix::from_condensed(vec![-0.0]).unwrap();
    assert!(d.get(0, 1).is_sign_positive());
}

#[test]
fn validation_errors_carry_the_condensed_index() {
    let err = DistanceMatrix::from_condensed(vec![1.0, f64::NAN, 1.0]).unwrap_err();
    assert!(err.to_string().contains("index 1"), "{err}");
    let err = DistanceMatrix::from_condensed(vec![1.0, 1.0, -2.0]).unwrap_err();
    assert!(err.to_string().contains("index 2"), "{err}");
}

#[test]
fn empty_condensed_means_one_point() {
    assert_eq!(DistanceMatrix::from_condensed(vec![]).unwrap().len(), 1);
}
