use std::collections::BTreeMap;

use super::{ImageVector, SparseRow, image_intersection, independent_image};

#[test]
fn independent_image_checks_its_dense_coefficient_bound() {
    let rows = vec![SparseRow(BTreeMap::from([(0, 1)])); 2];
    let error = independent_image(&rows, 2, 3).expect_err("the relation limit was ignored");
    assert_eq!(
        error.message(),
        "correspondence image matrix exceeds the relation-cell limit"
    );
}

#[test]
fn image_intersection_checks_equation_and_nullspace_bounds() {
    let image = ImageVector {
        row: SparseRow(BTreeMap::from([(0, 1)])),
        coefficients: vec![1],
    };
    let error = image_intersection(
        std::slice::from_ref(&image),
        std::slice::from_ref(&image),
        2,
        2,
        5,
    )
    .expect_err("the relation limit was ignored");
    assert_eq!(
        error.message(),
        "correspondence relation matrix exceeds the relation-cell limit"
    );
}
