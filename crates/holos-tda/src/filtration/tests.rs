use super::*;
use crate::SparseDistanceMatrix;

#[test]
fn product_order_keeps_incomparable_grades_incomparable() {
    let left = ProductGrade::new([1.0, 3.0]).unwrap();
    let right = ProductGrade::new([2.0, 2.0]).unwrap();
    assert!(!left.precedes(&right));
    assert!(!right.precedes(&left));
    assert_eq!(
        CoordinateProjection::new(0).project(&left).unwrap().value(),
        1.0
    );
}

#[test]
fn explicit_complex_rejects_a_late_face() {
    let complex = FilteredSimplicialComplex::new(
        vec![0, 1],
        vec![
            vec![
                FilteredSimplex::new(vec![0], ScalarGrade::new(0.0).unwrap()),
                FilteredSimplex::new(vec![1], ScalarGrade::new(2.0).unwrap()),
            ],
            vec![FilteredSimplex::new(
                vec![0, 1],
                ScalarGrade::new(1.0).unwrap(),
            )],
        ],
    );
    assert_eq!(
        complex.unwrap_err().message(),
        "face [1] appears after its coface [0, 1]"
    );
}

#[test]
fn flag_builder_uses_global_labels_and_clique_diameters() {
    let graph =
        SparseDistanceMatrix::from_triplets(3, &[(0, 1, 1.0), (0, 2, 2.0), (1, 2, 1.5)]).unwrap();
    let complex = FilteredSimplicialComplex::from_flag_graph(
        &graph,
        &[4, 8, 9],
        FlagComplexParams {
            max_dimension: 2,
            threshold: Some(2.0),
            limits: ComplexLimits {
                max_vertices: 10,
                max_edges: 10,
                max_triangles: 10,
                max_higher_simplices: 10,
            },
        },
    )
    .unwrap();
    assert_eq!(complex.simplices()[2][0].vertices(), &[4, 8, 9]);
    assert_eq!(complex.simplices()[2][0].grade().value(), 2.0);
}
