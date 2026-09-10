use super::model::*;
use super::verify::diagram_bits_equal;
use crate::{RipsParams, SparseDistanceMatrix, rips_persistence_sparse};
use proptest::prelude::*;

fn square() -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(
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
    .unwrap()
}

#[test]
fn square_certificate_verifies_over_several_fields() {
    let input = square();
    for modulus in [2, 3, 5, 7] {
        let params = RipsParams::new(1).with_modulus(modulus);
        let certificate =
            ReductionCertificate::build(&input, &params, CertificateLimits::default()).unwrap();
        let bytes = certificate.encode().unwrap();
        let certificate =
            ReductionCertificate::decode(&bytes, CertificateLimits::default()).unwrap();
        assert_eq!(certificate.encode().unwrap(), bytes);
        let diagram = certificate
            .verify(&input, CertificateLimits::default())
            .unwrap();
        let expected = rips_persistence_sparse(&input, &params).unwrap();
        assert!(diagram_bits_equal(&diagram, &expected));
    }
}

#[test]
fn repair_retains_the_stable_reduction_prefix() {
    let current = SparseDistanceMatrix::from_triplets(
        4,
        &[
            (0, 1, 1.0),
            (0, 2, 2.0),
            (0, 3, 3.0),
            (1, 2, 4.0),
            (1, 3, 5.0),
            (2, 3, 6.0),
        ],
    )
    .unwrap();
    let updated = SparseDistanceMatrix::from_triplets(
        4,
        &[
            (0, 1, 1.0),
            (0, 2, 2.0),
            (0, 3, 3.0),
            (1, 2, 4.0),
            (1, 3, 6.5),
            (2, 3, 6.0),
        ],
    )
    .unwrap();
    for modulus in [2, 3, 5] {
        let params = RipsParams::new(1).with_modulus(modulus);
        let certificate =
            ReductionCertificate::build(&current, &params, CertificateLimits::default()).unwrap();
        let repair = certificate
            .repair(&current, &updated, CertificateLimits::default())
            .unwrap();
        assert_eq!(repair.mode(), ReductionRepairMode::SuffixRepaired);
        assert_eq!(repair.work().edge_columns_reused, 6);
        assert_eq!(repair.work().edge_columns_reduced, 0);
        assert!(repair.work().triangle_columns_reused > 0);
        assert!(repair.work().triangle_columns_reduced > 0);
        let actual = repair
            .certificate()
            .verify(&updated, CertificateLimits::default())
            .unwrap();
        let expected = rips_persistence_sparse(&updated, &params).unwrap();
        assert!(diagram_bits_equal(&actual, &expected));
    }
}

#[test]
fn repair_rejects_topology_and_threshold_changes() {
    let current = square();
    let params = RipsParams::new(1).with_threshold(1.5);
    let certificate =
        ReductionCertificate::build(&current, &params, CertificateLimits::default()).unwrap();
    let topology = SparseDistanceMatrix::from_triplets(
        4,
        &[
            (0, 1, 1.0),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (0, 3, 1.0),
            (0, 2, 2.0),
        ],
    )
    .unwrap();
    assert!(
        certificate
            .repair(&current, &topology, CertificateLimits::default())
            .is_err()
    );
    let crossing = SparseDistanceMatrix::from_triplets(
        4,
        &[
            (0, 1, 1.0),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (0, 3, 2.0),
            (0, 2, 2.0),
            (1, 3, 2.0),
        ],
    )
    .unwrap();
    assert!(
        certificate
            .repair(&current, &crossing, CertificateLimits::default())
            .is_err()
    );
}

#[test]
fn dependency_frontier_repairs_random_edge_swaps_over_prime_fields() {
    let mut state = 0x71f3_2c85_9a40_b6d1u64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut saw_reuse = false;
    for case in 0..24 {
        let n = 5 + next() as usize % 4;
        let mut endpoints = Vec::new();
        for u in 0..n {
            for v in u + 1..n {
                endpoints.push((u, v));
            }
        }
        let mut order: Vec<_> = (0..endpoints.len()).map(|index| (next(), index)).collect();
        order.sort_unstable();
        let mut rank = vec![0usize; endpoints.len()];
        for (position, &(_, index)) in order.iter().enumerate() {
            rank[index] = position;
        }
        let triplets: Vec<_> = endpoints
            .iter()
            .enumerate()
            .map(|(index, &(u, v))| (u, v, 1.0 + rank[index] as f64))
            .collect();
        let left = endpoints.len() - 2 - next() as usize % 3.min(endpoints.len() - 1);
        let right = left + 1;
        let mut changed = triplets.clone();
        changed[left].2 = triplets[right].2;
        changed[right].2 = triplets[left].2;
        let current = SparseDistanceMatrix::from_triplets(n, &triplets).unwrap();
        let updated = SparseDistanceMatrix::from_triplets(n, &changed).unwrap();
        for modulus in [2, 3, 5] {
            let params = RipsParams::new(1).with_modulus(modulus);
            let certificate =
                ReductionCertificate::build(&current, &params, CertificateLimits::default())
                    .unwrap_or_else(|error| panic!("case {case}, Z/{modulus}: {error}"));
            let repair = certificate
                .repair(&current, &updated, CertificateLimits::default())
                .unwrap_or_else(|error| panic!("case {case}, Z/{modulus}: {error}"));
            saw_reuse |= repair.work().columns_reused() > 0;
            let actual = repair
                .certificate()
                .verify(&updated, CertificateLimits::default())
                .unwrap();
            let expected = rips_persistence_sparse(&updated, &params).unwrap();
            assert!(diagram_bits_equal(&actual, &expected));
        }
    }
    assert!(saw_reuse);
}

#[test]
fn changed_basis_term_and_graph_are_rejected() {
    let input = square();
    let params = RipsParams::new(1).with_modulus(3);
    let mut certificate =
        ReductionCertificate::build(&input, &params, CertificateLimits::default()).unwrap();
    certificate.triangle_columns[0].terms[0].coefficient = 0;
    assert!(
        certificate
            .verify(&input, CertificateLimits::default())
            .is_err()
    );

    let other =
        SparseDistanceMatrix::from_triplets(4, &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0)]).unwrap();
    assert!(
        ReductionCertificate::build(&input, &params, CertificateLimits::default())
            .unwrap()
            .verify(&other, CertificateLimits::default())
            .is_err()
    );
}

#[test]
fn random_certificates_match_the_implicit_solver() {
    let mut state = 0x57c8_02ed_4a91_b36fu64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for case in 0..96 {
        let n = 3 + next() as usize % 8;
        let mut triplets = Vec::new();
        for u in 0..n {
            for v in u + 1..n {
                if next() % 5 < 3 {
                    triplets.push((u, v, (next() % 6) as f64));
                }
            }
        }
        let input = SparseDistanceMatrix::from_triplets(n, &triplets).unwrap();
        for modulus in [2, 3, 5] {
            let params = RipsParams::new(1)
                .with_modulus(modulus)
                .with_threshold((next() % 7) as f64);
            let certificate =
                ReductionCertificate::build(&input, &params, CertificateLimits::default())
                    .unwrap_or_else(|error| panic!("case {case}, modulus {modulus}: {error}"));
            certificate
                .verify(&input, CertificateLimits::default())
                .unwrap_or_else(|error| panic!("case {case}, modulus {modulus}: {error}"));
        }
    }
}

#[test]
fn random_accepted_reweightings_match_exact_reduction() {
    let mut state = 0xa314_56f0_7c2d_98ebu64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut accepted = 0usize;
    for case in 0..48 {
        let n = 5 + next() as usize % 6;
        let mut triplets = Vec::new();
        for u in 0..n {
            for v in u + 1..n {
                if next() % 5 < 3 {
                    triplets.push((u, v, (1 + next() % 20) as f64));
                }
            }
        }
        let input = SparseDistanceMatrix::from_triplets(n, &triplets).unwrap();
        for modulus in [2, 3, 5] {
            let params = RipsParams::new(1).with_modulus(modulus);
            let certificate =
                ReductionCertificate::build(&input, &params, CertificateLimits::default()).unwrap();
            let region = certificate
                .compile_region(&input, CertificateLimits::default())
                .unwrap();
            for attempt in 0..8 {
                let updated_triplets: Vec<_> = triplets
                    .iter()
                    .map(|&(u, v, value)| {
                        let delta = (next() % 7) as f64 * 0.01 * (attempt + 1) as f64;
                        (u, v, value + delta)
                    })
                    .collect();
                let updated = SparseDistanceMatrix::from_triplets(n, &updated_triplets).unwrap();
                if region.violations(&updated).is_empty() {
                    accepted += 1;
                    let actual = region.evaluate(&updated).unwrap();
                    let expected = rips_persistence_sparse(&updated, &params).unwrap();
                    assert!(
                        diagram_bits_equal(actual.diagram(), &expected),
                        "case {case}, modulus {modulus}, attempt {attempt}"
                    );
                }
            }
        }
    }
    assert!(accepted > 100);
}

proptest! {
    #[test]
    fn arbitrary_short_envelopes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let _ = ReductionCertificate::decode(&bytes, CertificateLimits::default());
    }
}
