use crate::{
    CertificateLimits, CorrespondenceMode, EdgeKey, RipsParams, SparseDistanceMatrix,
    rips_persistence_sparse,
};

use super::*;

mod branches;
mod interfaces;
mod random;
mod support;
mod transitions;

#[test]
fn compile_rejects_over_limit_vertices_before_building_scope() {
    let input = SparseDistanceMatrix::from_triplets(3, &[(0, 1, 1.0)]).unwrap();
    let limits = CertificateLimits {
        max_vertices: 2,
        ..CertificateLimits::default()
    };
    let error =
        PersistenceIndex::compile(&input, &RipsParams::new(1), IndexParams::default(), limits)
            .unwrap_err();
    assert!(error.to_string().contains("vertices exceed the limit 2"));
}
