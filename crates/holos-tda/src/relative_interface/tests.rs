use crate::certificate::CertificateLimits;
use crate::{RipsParams, SparseDistanceMatrix, rips_persistence_sparse};

use super::RelativeInterfaceCertificate;

fn graph(vertex_count: usize, edges: &[(usize, usize, f64)]) -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(vertex_count, edges).unwrap()
}

#[test]
fn relative_cancellation_matches_the_implicit_engine() {
    let input = graph(
        6,
        &[
            (0, 1, 0.0),
            (1, 2, 0.0),
            (0, 2, 0.0),
            (2, 3, 1.0),
            (3, 4, 1.0),
            (2, 4, 1.0),
            (0, 5, 2.0),
        ],
    );
    for modulus in [2, 3, 5] {
        let params = RipsParams::new(2).with_modulus(modulus);
        let certificate = RelativeInterfaceCertificate::build(
            &input,
            &params,
            &[0, 1, 2],
            CertificateLimits::default(),
        )
        .unwrap();
        assert_eq!(
            certificate
                .verify(CertificateLimits::default())
                .unwrap()
                .bars,
            rips_persistence_sparse(&input, &params).unwrap().bars
        );
        assert!(
            certificate.core_cells()[0]
                .iter()
                .any(|cell| cell.vertices == [0])
        );
    }
}

#[test]
fn composes_through_a_noncontractible_filtered_separator() {
    let left = graph(
        5,
        &[
            (0, 1, 1.0),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (0, 3, 1.0),
            (0, 4, 2.0),
            (1, 4, 2.0),
        ],
    );
    let right = graph(
        5,
        &[
            (0, 1, 1.0),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (0, 3, 1.0),
            (2, 4, 2.5),
            (3, 4, 2.5),
        ],
    );
    let complete = graph(
        6,
        &[
            (0, 1, 1.0),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (0, 3, 1.0),
            (0, 4, 2.0),
            (1, 4, 2.0),
            (2, 5, 2.5),
            (3, 5, 2.5),
        ],
    );
    for modulus in [2, 3, 5] {
        let params = RipsParams::new(2).with_modulus(modulus);
        let a = RelativeInterfaceCertificate::build_labeled(
            &left,
            &[0, 1, 2, 3, 4],
            &params,
            &[0, 1, 2, 3],
            CertificateLimits::default(),
        )
        .unwrap();
        let b = RelativeInterfaceCertificate::build_labeled(
            &right,
            &[0, 1, 2, 3, 5],
            &params,
            &[0, 1, 2, 3],
            CertificateLimits::default(),
        )
        .unwrap();
        let composed =
            RelativeInterfaceCertificate::compose(&[&a, &b], &[], CertificateLimits::default())
                .unwrap();
        assert_eq!(
            composed.diagram().bars,
            rips_persistence_sparse(&complete, &params).unwrap().bars
        );
        assert_eq!(composed.diagram().in_dim(1).count(), 1);
    }
}
