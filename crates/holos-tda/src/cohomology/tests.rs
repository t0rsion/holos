use super::*;
use crate::{RipsParams, SparseDistanceMatrix, rips_persistence_sparse};

fn cross_polytope(pairs: usize) -> SparseDistanceMatrix {
    let vertices = 2 * pairs;
    let edges: Vec<_> = (0..vertices)
        .flat_map(|u| ((u + 1)..vertices).map(move |v| (u, v)))
        .filter(|&(u, v)| u / 2 != v / 2)
        .map(|(u, v)| (u, v, 1.0))
        .collect();
    SparseDistanceMatrix::from_triplets(vertices, &edges).unwrap()
}

#[test]
fn ranks_match_active_bars_through_h3_over_prime_fields() {
    let cycle = SparseDistanceMatrix::from_triplets(
        4,
        &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)],
    )
    .unwrap();
    let cases = [(cycle, 1), (cross_polytope(3), 2), (cross_polytope(4), 3)];
    for (graph, dimension) in cases {
        for modulus in [2, 3, 5] {
            let space =
                cohomology_space(&graph, dimension, 1.0, modulus, CohomologyLimits::default())
                    .unwrap();
            let diagram =
                rips_persistence_sparse(&graph, &RipsParams::new(dimension).with_modulus(modulus))
                    .unwrap();
            let active = diagram
                .in_dim(dimension)
                .filter(|bar| bar.birth <= 1.0 && 1.0 < bar.death)
                .count();
            assert_eq!(space.rank(), active);
            assert_eq!(space.rank(), 1);
        }
    }
}

#[test]
fn h0_basis_counts_components() {
    let graph = SparseDistanceMatrix::from_triplets(4, &[(0, 1, 1.0), (2, 3, 1.0)]).unwrap();
    let space = cohomology_space(&graph, 0, 1.0, 3, CohomologyLimits::default()).unwrap();
    assert_eq!(space.rank(), 2);
}

#[test]
fn restriction_map_uses_canonical_quotient_coordinates() {
    let subgraph = SparseDistanceMatrix::from_triplets(4, &[(0, 1, 1.0), (2, 3, 1.0)]).unwrap();
    let containing =
        SparseDistanceMatrix::from_triplets(4, &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0)]).unwrap();
    let source = cohomology_space(&containing, 0, 1.0, 5, CohomologyLimits::default()).unwrap();
    let target = cohomology_space(&subgraph, 0, 1.0, 5, CohomologyLimits::default()).unwrap();
    let restriction = cohomology_restriction(&containing, &source, &subgraph, &target).unwrap();
    assert_eq!(restriction.rank, 1);
    assert_eq!(restriction.columns.len(), 1);
    assert_eq!(restriction.columns[0].image.len(), 2);

    let identity = cohomology_restriction(&subgraph, &target, &subgraph, &target).unwrap();
    assert_eq!(identity.rank, 2);
    assert_eq!(identity.columns.len(), 2);
    assert!(
        identity
            .image_contains(&target, target.basis()[0].id)
            .unwrap()
    );
}

#[test]
fn identity_is_an_isomorphism_and_a_filled_sphere_dies() {
    let sphere = cross_polytope(3);
    let old = cohomology_space(&sphere, 2, 1.0, 5, CohomologyLimits::default()).unwrap();
    let identity =
        cohomology_relation(&sphere, &old, &sphere, &old, CohomologyLimits::default()).unwrap();
    assert!(identity.is_isomorphism());
    assert!(
        identity
            .contains_old_class(&old, old.basis()[0].id)
            .unwrap()
    );

    let mut edges: Vec<_> = sphere.edges().collect();
    edges.push((0, 1, 1.0));
    let filled = SparseDistanceMatrix::from_triplets(6, &edges).unwrap();
    let new = cohomology_space(&filled, 2, 1.0, 5, CohomologyLimits::default()).unwrap();
    assert_eq!(new.rank(), 0);
    let relation =
        cohomology_relation(&sphere, &old, &filled, &new, CohomologyLimits::default()).unwrap();
    assert_eq!(relation.relation_rank, 0);
    assert!(
        !relation
            .contains_old_class(&old, old.basis()[0].id)
            .unwrap()
    );
}

#[test]
fn relation_keeps_restriction_kernels() {
    let cycle = SparseDistanceMatrix::from_triplets(
        4,
        &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)],
    )
    .unwrap();
    let path =
        SparseDistanceMatrix::from_triplets(4, &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0)]).unwrap();
    let old = cohomology_space(&cycle, 1, 1.0, 5, CohomologyLimits::default()).unwrap();
    let new = cohomology_space(&path, 1, 1.0, 5, CohomologyLimits::default()).unwrap();
    let relation =
        cohomology_relation(&cycle, &old, &path, &new, CohomologyLimits::default()).unwrap();
    assert_eq!(relation.old_rank, 1);
    assert_eq!(relation.new_rank, 0);
    assert_eq!(relation.old_image_rank, 0);
    assert_eq!(relation.old_kernel_rank, 1);
    assert_eq!(relation.new_kernel_rank, 0);
    assert_eq!(relation.relation_rank, 1);
    assert_eq!(relation.basis[0].old.len(), 1);
    assert!(relation.basis[0].new.is_empty());
    assert!(
        relation
            .contains_old_class(&old, old.basis()[0].id)
            .unwrap()
    );
}

#[test]
fn cocycle_coordinates_round_trip() {
    let cycle = SparseDistanceMatrix::from_triplets(
        4,
        &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)],
    )
    .unwrap();
    let space = cohomology_space(&cycle, 1, 1.0, 5, CohomologyLimits::default()).unwrap();
    let terms = space.cocycle_from_coordinates(&[(0, 3)]).unwrap();
    assert_eq!(space.coordinates_of_cocycle(&terms).unwrap(), vec![(0, 3)]);
    assert!(space.coordinates_of_cocycle(&[]).unwrap().is_empty());
}

#[test]
fn continuation_classifies_every_affine_fiber_shape() {
    let cycle = SparseDistanceMatrix::from_triplets(
        8,
        &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)],
    )
    .unwrap();
    let old = cohomology_space(&cycle, 1, 1.0, 5, CohomologyLimits::default()).unwrap();
    let identity =
        cohomology_relation(&cycle, &old, &cycle, &old, CohomologyLimits::default()).unwrap();
    let continued = cohomology_continuation(&old, &old, &identity, &[(0, 1)]).unwrap();
    assert_eq!(continued.kind, CohomologyContinuationKind::Unique);
    assert_eq!(continued.new.len(), 1);

    let path =
        SparseDistanceMatrix::from_triplets(8, &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0)]).unwrap();
    let path_space = cohomology_space(&path, 1, 1.0, 5, CohomologyLimits::default()).unwrap();
    let zero_relation = cohomology_relation(
        &cycle,
        &old,
        &path,
        &path_space,
        CohomologyLimits::default(),
    )
    .unwrap();
    let continued = cohomology_continuation(&old, &path_space, &zero_relation, &[(0, 1)]).unwrap();
    assert_eq!(
        continued.kind,
        CohomologyContinuationKind::NoNonzeroContinuation
    );

    let filled = SparseDistanceMatrix::from_triplets(
        8,
        &[
            (0, 1, 1.0),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (0, 3, 1.0),
            (0, 2, 1.0),
        ],
    )
    .unwrap();
    let filled_space = cohomology_space(&filled, 1, 1.0, 5, CohomologyLimits::default()).unwrap();
    let absent_relation = cohomology_relation(
        &cycle,
        &old,
        &filled,
        &filled_space,
        CohomologyLimits::default(),
    )
    .unwrap();
    let continued =
        cohomology_continuation(&old, &filled_space, &absent_relation, &[(0, 1)]).unwrap();
    assert_eq!(continued.kind, CohomologyContinuationKind::NoExtension);

    let disjoint_cycle = SparseDistanceMatrix::from_triplets(
        8,
        &[(4, 5, 1.0), (5, 6, 1.0), (6, 7, 1.0), (4, 7, 1.0)],
    )
    .unwrap();
    let disjoint =
        cohomology_space(&disjoint_cycle, 1, 1.0, 5, CohomologyLimits::default()).unwrap();
    let ambiguous_relation = cohomology_relation(
        &cycle,
        &old,
        &disjoint_cycle,
        &disjoint,
        CohomologyLimits::default(),
    )
    .unwrap();
    let continued =
        cohomology_continuation(&old, &disjoint, &ambiguous_relation, &[(0, 1)]).unwrap();
    assert_eq!(continued.kind, CohomologyContinuationKind::Ambiguous);
    assert_eq!(continued.ambiguity.len(), 1);
}

#[test]
fn subspace_generators_are_canonical_and_intersect_restriction_images() {
    let base = SparseDistanceMatrix::from_triplets(4, &[(0, 1, 1.0), (2, 3, 1.0)]).unwrap();
    let joined =
        SparseDistanceMatrix::from_triplets(4, &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0)]).unwrap();
    let target = cohomology_space(&base, 0, 1.0, 5, CohomologyLimits::default()).unwrap();
    let source = cohomology_space(&joined, 0, 1.0, 5, CohomologyLimits::default()).unwrap();
    let restriction = cohomology_restriction(&joined, &source, &base, &target).unwrap();
    let full = target.full_subspace();
    assert_eq!(full.rank(), 2);
    assert_eq!(
        restriction.image_intersection_rank(&target, &full).unwrap(),
        1
    );

    let line = target
        .subspace_from_coordinates(&[vec![(0, 2)], vec![(0, 1)]])
        .unwrap();
    assert_eq!(line.rank(), 1);
    assert!(restriction.image_intersection_rank(&target, &line).unwrap() <= 1);
    assert!(
        target
            .subspace_from_coordinates(&[vec![(0, 1), (0, 2)]])
            .is_err()
    );
}

#[test]
fn malformed_continuation_relation_returns_invalid_input() {
    let cycle = SparseDistanceMatrix::from_triplets(
        4,
        &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)],
    )
    .unwrap();
    let old = cohomology_space(&cycle, 1, 1.0, 5, CohomologyLimits::default()).unwrap();
    let relation =
        cohomology_relation(&cycle, &old, &cycle, &old, CohomologyLimits::default()).unwrap();
    let foreign_graph = SparseDistanceMatrix::from_triplets(
        8,
        &[(4, 5, 1.0), (5, 6, 1.0), (6, 7, 1.0), (4, 7, 1.0)],
    )
    .unwrap();
    let foreign = cohomology_space(&foreign_graph, 1, 1.0, 5, CohomologyLimits::default()).unwrap();

    let mut unknown = relation.clone();
    unknown.basis[0].old[0].class = foreign.basis()[0].id;
    assert!(matches!(
        cohomology_continuation(&old, &old, &unknown, &[(0, 1)]),
        Err(crate::Error::InvalidInput(_))
    ));

    let mut coefficient = relation.clone();
    coefficient.basis[0].old[0].coefficient = 0;
    assert!(matches!(
        cohomology_continuation(&old, &old, &coefficient, &[(0, 1)]),
        Err(crate::Error::InvalidInput(_))
    ));

    let mut duplicate = relation.clone();
    let term = duplicate.basis[0].old[0];
    duplicate.basis[0].old.push(term);
    assert!(matches!(
        cohomology_continuation(&old, &old, &duplicate, &[(0, 1)]),
        Err(crate::Error::InvalidInput(_))
    ));

    let mut rank = relation.clone();
    rank.relation_rank += 1;
    assert!(matches!(
        cohomology_continuation(&old, &old, &rank, &[(0, 1)]),
        Err(crate::Error::InvalidInput(_))
    ));

    let mut dimension = relation.clone();
    dimension.dimension += 1;
    assert!(matches!(
        cohomology_continuation(&old, &old, &dimension, &[(0, 1)]),
        Err(crate::Error::InvalidInput(_))
    ));

    let mut scale = relation.clone();
    scale.scale = f64::NAN;
    assert!(matches!(
        cohomology_continuation(&old, &old, &scale, &[(0, 1)]),
        Err(crate::Error::InvalidInput(_))
    ));

    for modulus in [0, 1, 4] {
        let mut malformed = relation.clone();
        malformed.modulus = modulus;
        assert!(matches!(
            malformed.contains_old_class(&old, old.basis()[0].id),
            Err(crate::Error::InvalidInput(_))
        ));
    }

    for coefficient in [0, 5] {
        let mut malformed = relation.clone();
        malformed.basis[0].old[0].coefficient = coefficient;
        assert!(matches!(
            malformed.contains_old_class(&old, old.basis()[0].id),
            Err(crate::Error::InvalidInput(_))
        ));
    }

    let mut projection = relation.clone();
    projection.basis[0].old.clear();
    assert!(matches!(
        projection.contains_old_class(&old, old.basis()[0].id),
        Err(crate::Error::InvalidInput(_))
    ));

    assert!(matches!(
        relation.contains_old_class(&old, foreign.basis()[0].id),
        Err(crate::Error::InvalidInput(_))
    ));
    let mut foreign_term = relation.clone();
    foreign_term.basis[0].old[0].class = foreign.basis()[0].id;
    assert!(matches!(
        foreign_term.contains_old_class(&old, old.basis()[0].id),
        Err(crate::Error::InvalidInput(_))
    ));
}

#[test]
fn malformed_restriction_returns_invalid_input() {
    let target_graph = SparseDistanceMatrix::from_triplets(4, &[(0, 1, 1.0), (2, 3, 1.0)]).unwrap();
    let source_graph =
        SparseDistanceMatrix::from_triplets(4, &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0)]).unwrap();
    let target = cohomology_space(&target_graph, 0, 1.0, 5, CohomologyLimits::default()).unwrap();
    let source = cohomology_space(&source_graph, 0, 1.0, 5, CohomologyLimits::default()).unwrap();
    let restriction =
        cohomology_restriction(&source_graph, &source, &target_graph, &target).unwrap();
    let foreign_graph = SparseDistanceMatrix::from_triplets(4, &[(0, 1, 1.0)]).unwrap();
    let foreign = cohomology_space(&foreign_graph, 0, 1.0, 5, CohomologyLimits::default()).unwrap();
    let full = target.full_subspace();

    let mut unknown = restriction.clone();
    unknown.columns[0].image[0].class = foreign.basis()[0].id;
    assert!(matches!(
        unknown.image_intersection_rank(&target, &full),
        Err(crate::Error::InvalidInput(_))
    ));

    let mut coefficient = restriction.clone();
    coefficient.columns[0].image[0].coefficient = 0;
    assert!(matches!(
        coefficient.image_intersection_rank(&target, &full),
        Err(crate::Error::InvalidInput(_))
    ));

    let mut duplicate = restriction.clone();
    let term = duplicate.columns[0].image[0];
    duplicate.columns[0].image.push(term);
    assert!(matches!(
        duplicate.image_intersection_rank(&target, &full),
        Err(crate::Error::InvalidInput(_))
    ));

    let mut source_duplicate = restriction.clone();
    source_duplicate
        .columns
        .push(source_duplicate.columns[0].clone());
    assert!(matches!(
        source_duplicate.image_intersection_rank(&target, &full),
        Err(crate::Error::InvalidInput(_))
    ));

    let mut rank = restriction.clone();
    rank.rank += 1;
    assert!(matches!(
        rank.image_intersection_rank(&target, &full),
        Err(crate::Error::InvalidInput(_))
    ));

    let mut dimension = restriction.clone();
    dimension.dimension += 1;
    assert!(matches!(
        dimension.image_intersection_rank(&target, &full),
        Err(crate::Error::InvalidInput(_))
    ));

    let mut scale = restriction.clone();
    scale.scale = f64::NAN;
    assert!(matches!(
        scale.image_intersection_rank(&target, &full),
        Err(crate::Error::InvalidInput(_))
    ));

    assert!(matches!(
        restriction.image_contains(&target, foreign.basis()[0].id),
        Err(crate::Error::InvalidInput(_))
    ));
}
