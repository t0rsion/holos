use super::GradedReductionCertificate;
use crate::certificate::CertificateLimits;
use crate::{RipsParams, SparseDistanceMatrix, rips_persistence_sparse};

fn octahedron() -> SparseDistanceMatrix {
    let opposite = [(0, 1), (2, 3), (4, 5)];
    let edges = (0..6)
        .flat_map(|u| (u + 1..6).map(move |v| (u, v)))
        .filter(|edge| !opposite.contains(edge))
        .map(|(u, v)| (u, v, 1.0 + (u + v) as f64 / 100.0))
        .collect::<Vec<_>>();
    SparseDistanceMatrix::from_triplets(6, &edges).unwrap()
}

fn cross_polytope(pair_count: usize) -> SparseDistanceMatrix {
    let edges = (0..2 * pair_count)
        .flat_map(|u| (u + 1..2 * pair_count).map(move |v| (u, v)))
        .filter(|&(u, v)| u / 2 != v / 2)
        .map(|(u, v)| (u, v, 1.0 + (u + v) as f64 / 100.0))
        .collect::<Vec<_>>();
    SparseDistanceMatrix::from_triplets(2 * pair_count, &edges).unwrap()
}

#[test]
fn certifies_and_repairs_h2_over_prime_fields() {
    let current = octahedron();
    let mut edges: Vec<_> = current.edges().collect();
    edges[0].2 += 0.001;
    let updated = SparseDistanceMatrix::from_triplets(6, &edges).unwrap();
    for modulus in [2, 3, 5] {
        let params = RipsParams::new(2).with_modulus(modulus);
        let certificate =
            GradedReductionCertificate::build(&current, &params, CertificateLimits::default())
                .unwrap();
        assert!(certificate.diagram().in_dim(2).next().is_some());
        assert_eq!(
            certificate
                .verify(&current, CertificateLimits::default())
                .unwrap()
                .bars,
            rips_persistence_sparse(&current, &params).unwrap().bars
        );
        let repaired = certificate
            .repair(&current, &updated, CertificateLimits::default())
            .unwrap();
        assert_eq!(
            repaired.certificate().diagram().bars,
            rips_persistence_sparse(&updated, &params).unwrap().bars
        );
    }
}

#[test]
fn graded_certificate_extends_through_h3() {
    let graph = cross_polytope(4);
    for modulus in [2, 3, 5] {
        let params = RipsParams::new(3).with_modulus(modulus);
        let certificate =
            GradedReductionCertificate::build(&graph, &params, CertificateLimits::default())
                .unwrap();
        assert_eq!(certificate.graded_columns().len(), 4);
        assert_eq!(certificate.diagram().in_dim(3).count(), 1);
        assert_eq!(
            certificate.diagram().bars,
            rips_persistence_sparse(&graph, &params).unwrap().bars
        );
    }
}

#[test]
fn absent_edges_never_enter_at_an_infinite_threshold() {
    let graph =
        SparseDistanceMatrix::from_triplets(4, &[(0, 1, 1.0), (1, 2, 1.1), (2, 3, 1.2)]).unwrap();
    let params = RipsParams::new(2);
    let certificate =
        GradedReductionCertificate::build(&graph, &params, CertificateLimits::default()).unwrap();
    assert_eq!(certificate.columns(1).unwrap().len(), 3);
    assert!(certificate.columns(2).unwrap().is_empty());
    assert!(certificate.columns(3).unwrap().is_empty());
}
