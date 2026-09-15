use sha2::{Digest, Sha256};

use super::*;

use crate::{CertificateLimits, RipsParams, SparseDistanceMatrix};
#[cfg(holos_repository_tests)]
use holos_tda_check::{ProofLimits, verify_persistent_class};

fn square(with_diagonals: bool) -> SparseDistanceMatrix {
    weighted_square(1.0, with_diagonals.then_some(2.0), 4)
}

fn weighted_square(
    edge_weight: f64,
    diagonal_weight: Option<f64>,
    vertex_count: usize,
) -> SparseDistanceMatrix {
    let mut edges = vec![
        (0, 1, edge_weight),
        (1, 2, edge_weight),
        (2, 3, edge_weight),
        (0, 3, edge_weight),
    ];
    if let Some(weight) = diagonal_weight {
        edges.extend([(0, 2, weight), (1, 3, weight)]);
    }
    SparseDistanceMatrix::from_triplets(vertex_count, &edges).unwrap()
}

#[cfg(holos_repository_tests)]
fn theta_graph() -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(
        6,
        &[
            (0, 1, 1.0),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (0, 3, 1.0),
            (1, 4, 1.0),
            (2, 5, 1.0),
            (4, 5, 1.0),
        ],
    )
    .unwrap()
}

#[test]
fn finite_artifact_contains_a_normalized_cycle_and_chain() {
    let input = square(true);
    let params = RipsParams::new(1).with_threshold(2.0).with_modulus(5);
    let artifact =
        PersistentClassArtifact::build(&input, &params, 0, 0, CertificateLimits::default())
            .unwrap();
    assert_eq!(artifact.class().interval.birth.to_bits(), 1.0f64.to_bits());
    assert_eq!(artifact.class().interval.death.to_bits(), 2.0f64.to_bits());
    assert!(!artifact.cycle().is_empty());
    assert!(!artifact.bounding_chain().is_empty());
    assert!(artifact.cycle().windows(2).all(|pair| pair[0] < pair[1]));
    assert!(
        artifact
            .bounding_chain()
            .windows(2)
            .all(|pair| pair[0] < pair[1])
    );
    assert_eq!(artifact.critical_pair().death.as_ref().unwrap().value, 2.0);
}

#[test]
fn essential_artifact_omits_the_bounding_chain() {
    let input = square(false);
    let params = RipsParams::new(1).with_threshold(1.0).with_modulus(5);
    let artifact =
        PersistentClassArtifact::build(&input, &params, 0, 0, CertificateLimits::default())
            .unwrap();
    assert!(artifact.class().interval.death.is_infinite());
    assert!(artifact.critical_pair().death.is_none());
    assert!(artifact.bounding_chain().is_empty());
    assert!(!artifact.cycle().is_empty());
}

#[test]
fn encoded_artifact_hashes_every_preceding_byte() {
    let input = square(true);
    let params = RipsParams::new(1).with_threshold(2.0);
    let artifact =
        PersistentClassArtifact::build(&input, &params, 0, 0, CertificateLimits::default())
            .unwrap();
    let bytes = artifact.encode(CertificateLimits::default()).unwrap();
    assert_eq!(&bytes[..8], b"HOLOSPC\0");
    let payload_len = bytes.len() - 32;
    let expected: [u8; 32] = Sha256::digest(&bytes[..payload_len]).into();
    assert_eq!(&bytes[payload_len..], expected.as_slice());
    let limits = CertificateLimits {
        max_bytes: bytes.len() - 1,
        ..CertificateLimits::default()
    };
    assert!(artifact.encode(limits).is_err());
}

#[test]
#[cfg(holos_repository_tests)]
fn encoded_finite_and_essential_artifacts_replay_with_all_claim_fields() {
    for (input, params) in [
        (
            square(true),
            RipsParams::new(1).with_threshold(2.0).with_modulus(2),
        ),
        (
            square(false),
            RipsParams::new(1).with_threshold(1.0).with_modulus(2),
        ),
    ] {
        let artifact =
            PersistentClassArtifact::build(&input, &params, 0, 0, CertificateLimits::default())
                .unwrap();
        assert_checker_matches(&artifact);
    }
}

#[test]
#[cfg(holos_repository_tests)]
fn repeated_interval_basis_classes_replay_individually() {
    let input = SparseDistanceMatrix::from_triplets(
        8,
        &[
            (0, 1, 1.0),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (0, 3, 1.0),
            (0, 2, 2.0),
            (1, 3, 2.0),
            (4, 5, 1.0),
            (5, 6, 1.0),
            (6, 7, 1.0),
            (4, 7, 1.0),
            (4, 6, 2.0),
            (5, 7, 2.0),
        ],
    )
    .unwrap();
    let params = RipsParams::new(1).with_threshold(2.0).with_modulus(2);
    let explained = crate::rips_persistence_with_classes_sparse(&input, &params).unwrap();
    assert_eq!(explained.spaces.len(), 1);
    assert_eq!(explained.spaces[0].basis.len(), 2);
    for basis_index in 0..explained.spaces[0].basis.len() {
        let artifact = PersistentClassArtifact::build(
            &input,
            &params,
            0,
            basis_index,
            CertificateLimits::default(),
        )
        .unwrap();
        assert_checker_matches(&artifact);
    }
}

#[test]
#[cfg(holos_repository_tests)]
fn same_component_equal_interval_group_replays_at_odd_prime() {
    let input = theta_graph();
    let params = RipsParams::new(1).with_threshold(1.0).with_modulus(47);
    let explained = crate::rips_persistence_with_classes_sparse(&input, &params).unwrap();
    assert_eq!(explained.spaces.len(), 1);
    assert_eq!(explained.spaces[0].basis.len(), 2);
    assert_eq!(
        explained.spaces[0].basis[0].group_id,
        explained.spaces[0].basis[1].group_id
    );
    assert_ne!(
        explained.spaces[0].basis[0].id,
        explained.spaces[0].basis[1].id
    );
    for basis_index in 0..explained.spaces[0].basis.len() {
        let artifact = PersistentClassArtifact::build(
            &input,
            &params,
            0,
            basis_index,
            CertificateLimits::default(),
        )
        .unwrap();
        assert_checker_matches(&artifact);
    }
}

#[test]
#[cfg(holos_repository_tests)]
fn producer_keeps_an_above_threshold_destroyer_out_of_an_essential_pair() {
    let input = weighted_square(1.0, Some(3.0), 4);
    let params = RipsParams::new(1).with_threshold(2.0).with_modulus(47);
    let artifact =
        PersistentClassArtifact::build(&input, &params, 0, 0, CertificateLimits::default())
            .unwrap();
    assert_eq!(artifact.source().num_edges(), 6);
    assert!(artifact.class().interval.death.is_infinite());
    assert!(artifact.critical_pair().death.is_none());
    assert_checker_matches(&artifact);
}

#[test]
#[cfg(holos_repository_tests)]
fn producer_binds_isolates_and_canonical_threshold_forms() {
    for (input, threshold, birth, vertices) in [
        (weighted_square(1.0, None, 6), 1.0f64, 1.0f64, 6),
        (weighted_square(0.0, None, 4), 0.0f64, 0.0f64, 4),
        (weighted_square(1.0, None, 4), f64::INFINITY, 1.0f64, 4),
    ] {
        let params = RipsParams::new(1)
            .with_threshold(threshold)
            .with_modulus(47);
        let artifact =
            PersistentClassArtifact::build(&input, &params, 0, 0, CertificateLimits::default())
                .unwrap();
        assert_eq!(artifact.source().len(), vertices);
        assert_eq!(artifact.class().interval.birth.to_bits(), birth.to_bits());
        assert!(artifact.class().interval.death.is_infinite());
        assert_checker_matches(&artifact);

        if threshold == 1.0 && vertices == 6 {
            let mut bytes = artifact.encode(CertificateLimits::default()).unwrap();
            let payload_len = bytes.len() - 32;
            let scale_offset = 31 + artifact.source().num_edges() * 24 + 9 + 32 + 32 + 8 + 8 + 8;
            bytes[scale_offset..scale_offset + 8]
                .copy_from_slice(&(1.0 + f64::EPSILON).to_bits().to_be_bytes());
            reseal(&mut bytes);
            assert_eq!(bytes.len() - 32, payload_len);
            assert!(verify_persistent_class(&bytes, ProofLimits::default()).is_err());
        }
    }
}

#[test]
#[cfg(holos_repository_tests)]
fn sampled_small_graphs_cover_every_native_class() {
    let mut state = 0x72a4_16d3_9b8c_50efu64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for n in 3..=5 {
        for _ in 0..24 {
            let mut triplets = Vec::new();
            for u in 0..n {
                for v in u + 1..n {
                    if next() % 3 != 0 {
                        triplets.push((u, v, (1 + next() % 3) as f64));
                    }
                }
            }
            let input = SparseDistanceMatrix::from_triplets(n, &triplets).unwrap();
            let params = RipsParams::new(1).with_threshold(3.0).with_modulus(3);
            let explained = crate::rips_persistence_with_classes_sparse(&input, &params).unwrap();
            for (space_index, space) in explained.spaces.iter().enumerate() {
                for basis_index in 0..space.basis.len() {
                    let artifact = PersistentClassArtifact::build(
                        &input,
                        &params,
                        space_index,
                        basis_index,
                        CertificateLimits::default(),
                    )
                    .unwrap();
                    assert_checker_matches(&artifact);
                }
            }
        }
    }
}

#[test]
#[cfg(holos_repository_tests)]
fn rehashed_source_mutation_does_not_validate_the_old_class() {
    let artifact = PersistentClassArtifact::build(
        &square(true),
        &RipsParams::new(1).with_threshold(2.0),
        0,
        0,
        CertificateLimits::default(),
    )
    .unwrap();
    let mut bytes = artifact.encode(CertificateLimits::default()).unwrap();
    let payload_len = bytes.len() - 32;
    let source_first_weight = 31 + 16;
    bytes[source_first_weight..source_first_weight + 8]
        .copy_from_slice(&3.0f64.to_bits().to_be_bytes());
    reseal(&mut bytes);
    assert_eq!(bytes.len() - 32, payload_len);
    assert!(verify_persistent_class(&bytes, ProofLimits::default()).is_err());
}

#[test]
fn preflight_rejects_collapse_and_source_limits() {
    let input = square(true);
    let collapse = RipsParams::new(1).with_edge_collapse();
    assert!(
        PersistentClassArtifact::build(&input, &collapse, 0, 0, CertificateLimits::default(),)
            .is_err()
    );

    let limits = CertificateLimits {
        max_edges: 1,
        ..CertificateLimits::default()
    };
    assert!(PersistentClassArtifact::build(&input, &RipsParams::new(1), 0, 0, limits,).is_err());
}

#[cfg(holos_repository_tests)]
fn reseal(bytes: &mut [u8]) {
    let payload_len = bytes.len() - 32;
    let digest: [u8; 32] = Sha256::digest(&bytes[..payload_len]).into();
    bytes[payload_len..].copy_from_slice(&digest);
}

#[cfg(holos_repository_tests)]
fn assert_checker_matches(artifact: &PersistentClassArtifact) {
    let bytes = artifact.encode(CertificateLimits::default()).unwrap();
    let checked = verify_persistent_class(&bytes, ProofLimits::default()).unwrap();
    assert_eq!(checked.vertex_count(), artifact.source().len());
    assert_eq!(checked.threshold(), artifact.threshold());
    assert_eq!(checked.modulus(), artifact.class().cocycle.modulus);
    assert_eq!(checked.group_id(), artifact.class().group_id.as_bytes());
    assert_eq!(checked.class_id(), artifact.class().id.as_bytes());
    assert_eq!(checked.basis_index(), artifact.class().basis_index);
    assert_eq!(
        checked.interval().birth.to_bits(),
        artifact.class().interval.birth.to_bits()
    );
    assert_eq!(
        checked.interval().death.to_bits(),
        artifact.class().interval.death.to_bits()
    );
    assert_eq!(
        checked.scale().to_bits(),
        artifact.class().cocycle.scale.to_bits()
    );
    let source: Vec<_> = artifact.source().edges().collect();
    assert_eq!(checked.source().len(), source.len());
    for (edge, &(u, v, value)) in checked.source().iter().zip(&source) {
        assert_eq!((edge.u(), edge.v()), (u, v));
        assert_eq!(edge.value().to_bits(), value.to_bits());
    }
    assert_eq!(
        checked.cocycle().len(),
        artifact.class().cocycle.terms.len()
    );
    for (actual, expected) in checked
        .cocycle()
        .iter()
        .zip(&artifact.class().cocycle.terms)
    {
        assert_eq!(
            (actual.u(), actual.v(), actual.coefficient()),
            (expected.u, expected.v, expected.coefficient)
        );
    }
    let pair = checked.critical_pair();
    let expected_birth: [usize; 2] = artifact
        .critical_pair()
        .birth
        .vertices
        .as_slice()
        .try_into()
        .unwrap();
    assert_eq!(pair.birth(), expected_birth);
    let expected_death: Option<[usize; 3]> = artifact
        .critical_pair()
        .death
        .as_ref()
        .map(|simplex| simplex.vertices.as_slice().try_into().unwrap());
    assert_eq!(pair.death(), expected_death);
    assert_eq!(checked.cycle().len(), artifact.cycle().len());
    for (actual, expected) in checked.cycle().iter().zip(artifact.cycle()) {
        assert_eq!(
            (actual.u(), actual.v(), actual.coefficient()),
            (expected.u, expected.v, expected.coefficient)
        );
    }
    assert_eq!(
        checked.bounding_chain().len(),
        artifact.bounding_chain().len()
    );
    for (actual, expected) in checked
        .bounding_chain()
        .iter()
        .zip(artifact.bounding_chain())
    {
        assert_eq!(actual.vertices(), expected.vertices);
        assert_eq!(actual.coefficient(), expected.coefficient);
    }
}

#[test]
fn h1_selection_does_not_expand_the_requested_dimension() {
    let input = square(true);
    let params = RipsParams::new(usize::MAX).with_threshold(2.0);
    let artifact =
        PersistentClassArtifact::build(&input, &params, 0, 0, CertificateLimits::default())
            .unwrap();
    assert_eq!(artifact.class().interval.dim, 1);
}
