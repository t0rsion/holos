#![cfg(holos_repository_tests)]

use holos_tda::{
    CertificateLimits, RelativeInterfaceCertificate, RipsParams, SparseDistanceMatrix,
    rips_persistence_sparse,
};
use holos_tda_check::{ProofLimits, verify_relative_interface};

fn graph(vertex_count: usize, edges: &[(usize, usize, f64)]) -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(vertex_count, edges).unwrap()
}

#[test]
fn independent_checker_accepts_noncontractible_interface() {
    let input = graph(
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
        let certificate = RelativeInterfaceCertificate::build(
            &input,
            &params,
            &[0, 1, 2, 3],
            CertificateLimits::default(),
        )
        .unwrap();
        let bytes = certificate.encode(CertificateLimits::default()).unwrap();
        let decoded =
            RelativeInterfaceCertificate::decode(&bytes, CertificateLimits::default()).unwrap();
        assert_eq!(decoded.encode(CertificateLimits::default()).unwrap(), bytes);
        let checked = verify_relative_interface(&bytes, ProofLimits::default()).unwrap();
        assert_eq!(checked.digest, *certificate.digest());
        assert_eq!(
            checked.bars,
            rips_persistence_sparse(&input, &params).unwrap().bars.len()
        );
    }
}

#[test]
fn independent_checker_rejects_mutations_and_truncations() {
    let input = graph(4, &[(0, 1, 0.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)]);
    let certificate = RelativeInterfaceCertificate::build(
        &input,
        &RipsParams::new(1),
        &[0, 1],
        CertificateLimits::default(),
    )
    .unwrap();
    let bytes = certificate.encode(CertificateLimits::default()).unwrap();
    for end in 0..bytes.len() {
        assert!(verify_relative_interface(&bytes[..end], ProofLimits::default()).is_err());
    }
    let mut mutation = bytes;
    let last = mutation.len() - 1;
    mutation[last] ^= 1;
    assert!(verify_relative_interface(&mutation, ProofLimits::default()).is_err());
}

#[test]
fn composes_h3_through_a_flag_sphere_separator() {
    let separator_edges: Vec<_> = (0..6)
        .flat_map(|u| (u + 1..6).map(move |v| (u, v)))
        .filter(|&(u, v)| u / 2 != v / 2)
        .map(|(u, v)| (u, v, 1.0))
        .collect();
    let mut child_edges = separator_edges.clone();
    child_edges.extend((0..6).map(|vertex| (vertex, 6, 2.0)));
    let child = graph(7, &child_edges);
    let mut full_edges = separator_edges;
    full_edges.extend((0..6).map(|vertex| (vertex, 6, 2.0)));
    full_edges.extend((0..6).map(|vertex| (vertex, 7, 2.0)));
    let full = graph(8, &full_edges);
    for modulus in [2, 3, 5] {
        let params = RipsParams::new(3).with_modulus(modulus);
        let left = RelativeInterfaceCertificate::build_labeled(
            &child,
            &[0, 1, 2, 3, 4, 5, 6],
            &params,
            &[0, 1, 2, 3, 4, 5],
            CertificateLimits::default(),
        )
        .unwrap();
        let right = RelativeInterfaceCertificate::build_labeled(
            &child,
            &[0, 1, 2, 3, 4, 5, 7],
            &params,
            &[0, 1, 2, 3, 4, 5],
            CertificateLimits::default(),
        )
        .unwrap();
        let composed = RelativeInterfaceCertificate::compose(
            &[&left, &right],
            &[],
            CertificateLimits::default(),
        )
        .unwrap();
        assert_eq!(composed.diagram().in_dim(3).count(), 1);
        assert_eq!(
            composed.diagram().bars,
            rips_persistence_sparse(&full, &params).unwrap().bars
        );
        let checked = verify_relative_interface(
            &composed.encode(CertificateLimits::default()).unwrap(),
            ProofLimits::default(),
        )
        .unwrap();
        assert_eq!(checked.max_dim, 3);
    }
}

#[test]
fn deterministic_graph_sweep_matches_exact_persistence() {
    let pairs: Vec<_> = (0..5)
        .flat_map(|u| (u + 1..5).map(move |v| (u, v)))
        .collect();
    for mask in 0u16..128 {
        let edges: Vec<_> = pairs
            .iter()
            .enumerate()
            .filter(|(position, _)| mask & (1 << position) != 0)
            .map(|(position, &(u, v))| (u, v, 1.0 + (position % 3) as f64))
            .collect();
        let input = graph(5, &edges);
        for modulus in [2, 3, 5] {
            let params = RipsParams::new(2).with_modulus(modulus);
            let certificate = RelativeInterfaceCertificate::build(
                &input,
                &params,
                &[0, 1],
                CertificateLimits::default(),
            )
            .unwrap();
            assert_eq!(
                certificate.diagram().bars,
                rips_persistence_sparse(&input, &params).unwrap().bars
            );
        }
    }
}
