use super::*;
use crate::{
    FilteredSimplex, FlagComplexParams, RipsParams, SparseDistanceMatrix, rips_persistence_sparse,
};

fn cycle_complex() -> FilteredSimplicialComplex<ScalarGrade> {
    let zero = ScalarGrade::new(0.0).unwrap();
    let one = ScalarGrade::new(1.0).unwrap();
    FilteredSimplicialComplex::new(
        vec![0, 1, 2, 3],
        vec![
            (0..4)
                .map(|vertex| FilteredSimplex::new(vec![vertex], zero))
                .collect(),
            vec![
                FilteredSimplex::new(vec![0, 1], one),
                FilteredSimplex::new(vec![0, 3], one),
                FilteredSimplex::new(vec![1, 2], one),
                FilteredSimplex::new(vec![2, 3], one),
            ],
            Vec::new(),
        ],
    )
    .unwrap()
}

#[test]
fn explicit_cycle_has_one_essential_h1_class() {
    let certificate =
        ExplicitReductionCertificate::build(&cycle_complex(), 1, 3, CertificateLimits::default())
            .unwrap();
    assert_eq!(certificate.diagram().in_dim(1).count(), 1);
    assert!(
        certificate
            .diagram()
            .in_dim(1)
            .next()
            .unwrap()
            .is_essential()
    );
    certificate.verify(CertificateLimits::default()).unwrap();
}

#[test]
fn explicit_flag_certificate_matches_the_implicit_engine() {
    let graph = SparseDistanceMatrix::from_triplets(
        4,
        &[
            (0, 1, 1.0),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (0, 3, 1.0),
            (0, 2, 2.0),
            (1, 3, 2.0),
        ],
    )
    .unwrap();
    let complex = FilteredSimplicialComplex::from_flag_graph(
        &graph,
        &[0, 1, 2, 3],
        FlagComplexParams {
            max_dimension: 2,
            threshold: None,
            limits: crate::ComplexLimits::default(),
        },
    )
    .unwrap();
    let explicit =
        ExplicitReductionCertificate::build(&complex, 1, 5, CertificateLimits::default()).unwrap();
    let implicit = rips_persistence_sparse(&graph, &RipsParams::new(1).with_modulus(5)).unwrap();
    assert!(diagrams_equal(explicit.diagram(), &implicit));
}

#[test]
fn explicit_artifact_round_trips_and_rejects_mutation() {
    let certificate =
        ExplicitReductionCertificate::build(&cycle_complex(), 1, 2, CertificateLimits::default())
            .unwrap();
    let mut bytes = certificate.encode(CertificateLimits::default()).unwrap();
    let decoded =
        ExplicitReductionCertificate::decode(&bytes, CertificateLimits::default()).unwrap();
    assert!(diagrams_equal(certificate.diagram(), decoded.diagram()));
    let checked = holos_tda_check::verify_explicit_persistence(
        &bytes,
        holos_tda_check::ProofLimits::default(),
    )
    .unwrap();
    assert_eq!(checked.max_homology_dimension, 1);
    assert_eq!(checked.modulus, 2);
    assert_eq!(checked.bars.len(), certificate.diagram().bars.len());
    bytes[20] ^= 1;
    assert!(ExplicitReductionCertificate::decode(&bytes, CertificateLimits::default()).is_err());
}
