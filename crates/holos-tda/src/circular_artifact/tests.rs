use super::*;
use crate::{
    CircularCoordinateParams, CohomologyLimits, RipsParams, SparseDistanceMatrix,
    circular_coordinate_for_class, continue_circular_coordinate,
    rips_persistence_with_classes_sparse,
};
use holos_tda_check::{CircularProofLimits, verify_circular_coordinate};

fn cycle_graph() -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(
        8,
        &[
            (0, 1, 1.0),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (3, 4, 1.0),
            (4, 5, 1.0),
            (5, 6, 1.0),
            (6, 7, 1.0),
            (0, 7, 1.0),
        ],
    )
    .unwrap()
}

#[test]
fn coordinate_and_identity_continuation_encode() {
    let graph = cycle_graph();
    let explained =
        rips_persistence_with_classes_sparse(&graph, &RipsParams::new(1).with_modulus(47)).unwrap();
    let coordinate = circular_coordinate_for_class(
        &graph,
        explained.classes().next().unwrap(),
        CircularCoordinateParams::default(),
    )
    .unwrap();
    let single = CircularCoordinateArtifact::from_coordinate(&graph, &coordinate).unwrap();
    let single_bytes = single.encode().unwrap();
    assert!(single_bytes.starts_with(MAGIC));
    assert_eq!(single.summary().states, 1);
    let checked =
        verify_circular_coordinate(&single_bytes, CircularProofLimits::default()).unwrap();
    assert_eq!(checked.coordinates, 1);
    assert_eq!(checked.divisibilities, vec![1]);

    let continuation = continue_circular_coordinate(
        &graph,
        &coordinate,
        &graph,
        CircularCoordinateParams::default(),
    )
    .unwrap();
    let pair = CircularCoordinateArtifact::from_continuation(
        &graph,
        &coordinate,
        &graph,
        &continuation,
        CohomologyLimits::default(),
    )
    .unwrap();
    assert_eq!(pair.summary().states, 2);
    assert!(pair.summary().continuation);
    let pair_bytes = pair.encode().unwrap();
    assert!(pair_bytes.len() > single_bytes.len());
    let checked = verify_circular_coordinate(&pair_bytes, CircularProofLimits::default()).unwrap();
    assert_eq!(
        checked.continuation,
        Some(holos_tda_check::VerifiedCircularContinuationKind::Unique)
    );

    let mut mutated = single_bytes;
    let middle = mutated.len() / 2;
    mutated[middle] ^= 1;
    assert!(verify_circular_coordinate(&mutated, CircularProofLimits::default()).is_err());
    let mut strict = CircularProofLimits::default();
    strict.max_tolerance = coordinate.tolerance / 2.0;
    assert!(verify_circular_coordinate(&pair_bytes, strict).is_err());

    let mut excessive_tolerance = single.clone();
    excessive_tolerance.tolerance = 1.0;
    let bytes = excessive_tolerance.encode().unwrap();
    let error = verify_circular_coordinate(&bytes, CircularProofLimits::default()).unwrap_err();
    assert!(error.to_string().contains("tolerance"));

    let mut zero_multiplier = single;
    zero_multiplier.states[0]
        .coordinate
        .as_mut()
        .unwrap()
        .field_multiplier = 0;
    let bytes = zero_multiplier.encode().unwrap();
    let error = verify_circular_coordinate(&bytes, CircularProofLimits::default()).unwrap_err();
    assert!(error.to_string().contains("multiplier"));
}

#[test]
fn checker_rejects_each_coordinate_claim_family() {
    let graph = cycle_graph();
    let explained =
        rips_persistence_with_classes_sparse(&graph, &RipsParams::new(1).with_modulus(47)).unwrap();
    let coordinate = circular_coordinate_for_class(
        &graph,
        explained.classes().next().unwrap(),
        CircularCoordinateParams::default(),
    )
    .unwrap();
    let artifact = CircularCoordinateArtifact::from_coordinate(&graph, &coordinate).unwrap();
    let rejects = |candidate: CircularCoordinateArtifact| {
        assert!(
            verify_circular_coordinate(
                &candidate.encode().unwrap(),
                CircularProofLimits::default(),
            )
            .is_err()
        );
    };

    let mut candidate = artifact.clone();
    candidate.modulus = 4;
    rejects(candidate);
    let mut candidate = artifact.clone();
    candidate.scale = f64::NAN;
    rejects(candidate);
    let mut candidate = artifact.clone();
    candidate.tolerance = 1.0;
    rejects(candidate);
    let mut candidate = artifact.clone();
    candidate.states[0].vertex_count += 1;
    rejects(candidate);
    let mut candidate = artifact.clone();
    candidate.states[0].edges.pop();
    rejects(candidate);

    fn coordinate_of(candidate: &mut CircularCoordinateArtifact) -> &mut CircularCoordinate {
        candidate.states[0].coordinate.as_mut().unwrap()
    }
    let mut candidate = artifact.clone();
    let coordinate = coordinate_of(&mut candidate);
    let mut bytes = *coordinate.space.as_bytes();
    bytes[0] ^= 1;
    coordinate.space = crate::CohomologySpaceId::from_bytes(bytes);
    rejects(candidate);
    let mut candidate = artifact.clone();
    coordinate_of(&mut candidate).field_multiplier = 2;
    rejects(candidate);
    let mut candidate = artifact.clone();
    coordinate_of(&mut candidate).divisibility += 1;
    rejects(candidate);
    let mut candidate = artifact.clone();
    coordinate_of(&mut candidate).source[0].coefficient = 2;
    rejects(candidate);
    let mut candidate = artifact.clone();
    coordinate_of(&mut candidate).integral[0].coefficient += 1;
    rejects(candidate);
    let mut candidate = artifact.clone();
    coordinate_of(&mut candidate).class[0].coefficient = 2;
    rejects(candidate);
    let mut candidate = artifact;
    coordinate_of(&mut candidate).potential[0] = 0.25;
    rejects(candidate);
}

#[test]
fn checker_enforces_each_circular_collection_limit() {
    let graph = SparseDistanceMatrix::from_triplets(
        8,
        &[
            (0, 1, 1.0),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (0, 3, 1.0),
            (4, 5, 1.0),
            (5, 6, 1.0),
            (4, 6, 1.0),
        ],
    )
    .unwrap();
    let explained =
        rips_persistence_with_classes_sparse(&graph, &RipsParams::new(1).with_modulus(47)).unwrap();
    let coordinate = circular_coordinate_for_class(
        &graph,
        explained.classes().next().unwrap(),
        CircularCoordinateParams::default(),
    )
    .unwrap();
    let bytes = CircularCoordinateArtifact::from_coordinate(&graph, &coordinate)
        .unwrap()
        .encode()
        .unwrap();
    let rejects = |limits: CircularProofLimits| {
        assert!(verify_circular_coordinate(&bytes, limits).is_err());
    };

    let mut limits = CircularProofLimits::default();
    limits.proof.max_bytes = bytes.len() - 1;
    rejects(limits);
    let mut limits = CircularProofLimits::default();
    limits.proof.max_snapshots = 0;
    rejects(limits);
    let mut limits = CircularProofLimits::default();
    limits.proof.max_vertices = graph.len() - 1;
    rejects(limits);
    let mut limits = CircularProofLimits::default();
    limits.proof.max_edges = graph.edges().count() - 1;
    rejects(limits);
    let mut limits = CircularProofLimits::default();
    limits.proof.max_triangles = 0;
    rejects(limits);
    let mut limits = CircularProofLimits::default();
    limits.proof.max_terms = 0;
    rejects(limits);
    let mut limits = CircularProofLimits::default();
    limits.max_tolerance = coordinate.tolerance / 2.0;
    rejects(limits);
}

#[test]
fn checker_accepts_cycle_artifacts_across_small_sizes() {
    for vertices in 4..=32 {
        let mut edges = (0..vertices - 1)
            .map(|u| (u, u + 1, 1.0))
            .collect::<Vec<_>>();
        edges.push((0, vertices - 1, 1.0));
        let graph = SparseDistanceMatrix::from_triplets(vertices, &edges).unwrap();
        let explained =
            rips_persistence_with_classes_sparse(&graph, &RipsParams::new(1).with_modulus(47))
                .unwrap();
        let coordinate = circular_coordinate_for_class(
            &graph,
            explained.classes().next().unwrap(),
            CircularCoordinateParams::default(),
        )
        .unwrap();
        let bytes = CircularCoordinateArtifact::from_coordinate(&graph, &coordinate)
            .unwrap()
            .encode()
            .unwrap();
        verify_circular_coordinate(&bytes, CircularProofLimits::default()).unwrap();
    }
}

#[test]
fn checker_reconstructs_each_continuation_status() {
    let old_graph = SparseDistanceMatrix::from_triplets(
        8,
        &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)],
    )
    .unwrap();
    let explained =
        rips_persistence_with_classes_sparse(&old_graph, &RipsParams::new(1).with_modulus(5))
            .unwrap();
    let old_coordinate = circular_coordinate_for_class(
        &old_graph,
        explained.classes().next().unwrap(),
        CircularCoordinateParams::default(),
    )
    .unwrap();
    let cases = [
        (
            SparseDistanceMatrix::from_triplets(8, &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0)])
                .unwrap(),
            holos_tda_check::VerifiedCircularContinuationKind::NoNonzeroContinuation,
        ),
        (
            SparseDistanceMatrix::from_triplets(
                8,
                &[
                    (0, 1, 1.0),
                    (1, 2, 1.0),
                    (2, 3, 1.0),
                    (0, 3, 1.0),
                    (0, 2, 1.0),
                ],
            )
            .unwrap(),
            holos_tda_check::VerifiedCircularContinuationKind::NoExtension,
        ),
        (
            SparseDistanceMatrix::from_triplets(
                8,
                &[(4, 5, 1.0), (5, 6, 1.0), (6, 7, 1.0), (4, 7, 1.0)],
            )
            .unwrap(),
            holos_tda_check::VerifiedCircularContinuationKind::Ambiguous,
        ),
    ];
    for (new_graph, expected) in cases {
        let continuation = continue_circular_coordinate(
            &old_graph,
            &old_coordinate,
            &new_graph,
            CircularCoordinateParams::default(),
        )
        .unwrap();
        let artifact = CircularCoordinateArtifact::from_continuation(
            &old_graph,
            &old_coordinate,
            &new_graph,
            &continuation,
            CohomologyLimits::default(),
        )
        .unwrap();
        let checked =
            verify_circular_coordinate(&artifact.encode().unwrap(), CircularProofLimits::default())
                .unwrap();
        assert_eq!(checked.continuation, Some(expected));
        assert_eq!(checked.coordinates, 1);
        assert_eq!(
            checked.ambiguity_rank > 0,
            expected == holos_tda_check::VerifiedCircularContinuationKind::Ambiguous
        );
        let mut changed = artifact.clone();
        changed.continuation.as_mut().unwrap().kind = CohomologyContinuationKind::Unique;
        assert!(
            verify_circular_coordinate(&changed.encode().unwrap(), CircularProofLimits::default(),)
                .is_err()
        );
        if expected == holos_tda_check::VerifiedCircularContinuationKind::Ambiguous {
            let mut changed = artifact;
            changed.continuation.as_mut().unwrap().ambiguity.clear();
            assert!(
                verify_circular_coordinate(
                    &changed.encode().unwrap(),
                    CircularProofLimits::default(),
                )
                .is_err()
            );
        }
    }
}

#[test]
fn checker_accepts_a_nonunit_continued_class_vector() {
    let graph = cycle_graph();
    let explained =
        rips_persistence_with_classes_sparse(&graph, &RipsParams::new(1).with_modulus(47)).unwrap();
    let mut coordinate = circular_coordinate_for_class(
        &graph,
        explained.classes().next().unwrap(),
        CircularCoordinateParams::default(),
    )
    .unwrap();
    for term in &mut coordinate.source {
        term.coefficient = term.coefficient * 2 % 47;
    }
    for term in &mut coordinate.integral {
        term.coefficient *= 2;
    }
    for term in &mut coordinate.class {
        term.coefficient = term.coefficient * 2 % 47;
    }
    for value in &mut coordinate.potential {
        *value *= 2.0;
    }
    coordinate.divisibility *= 2;
    let continuation = continue_circular_coordinate(
        &graph,
        &coordinate,
        &graph,
        CircularCoordinateParams::default(),
    )
    .unwrap();
    let artifact = CircularCoordinateArtifact::from_continuation(
        &graph,
        &coordinate,
        &graph,
        &continuation,
        CohomologyLimits::default(),
    )
    .unwrap();
    let checked =
        verify_circular_coordinate(&artifact.encode().unwrap(), CircularProofLimits::default())
            .unwrap();
    assert_eq!(checked.coordinates, 2);
    assert_eq!(checked.divisibilities, vec![2, 2]);
    assert_eq!(
        checked.continuation,
        Some(holos_tda_check::VerifiedCircularContinuationKind::Unique)
    );
}
