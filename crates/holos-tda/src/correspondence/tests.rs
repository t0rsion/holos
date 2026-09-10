use super::*;
use crate::{RipsParams, rips_persistence_with_classes_sparse};

fn two_squares() -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(
        7,
        &[
            (0, 1, 1.0),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (0, 3, 1.0),
            (0, 2, 2.0),
            (1, 3, 2.0),
            (3, 4, 1.0),
            (4, 5, 1.0),
            (5, 6, 1.0),
            (3, 6, 1.0),
            (3, 5, 2.0),
            (4, 6, 2.0),
        ],
    )
    .unwrap()
}

#[test]
fn identity_update_proves_the_full_rank_two_space() {
    let graph = two_squares();
    for modulus in [2, 3, 5] {
        let explained =
            rips_persistence_with_classes_sparse(&graph, &RipsParams::new(1).with_modulus(modulus))
                .unwrap();
        let relation = class_correspondences(
            &graph,
            &explained.spaces,
            &graph,
            &explained.spaces,
            modulus,
        )
        .unwrap();
        assert_eq!(relation.len(), 1);
        assert_eq!(relation[0].old_rank, 2);
        assert_eq!(relation[0].relation_rank, 2);
        assert!(relation[0].is_isomorphism());
    }
}

#[test]
fn disjoint_active_subgraphs_do_not_invent_a_relation() {
    let old = two_squares();
    let new = SparseDistanceMatrix::from_triplets(
        7,
        &[
            (0, 1, 3.0),
            (1, 2, 3.0),
            (2, 3, 3.0),
            (0, 3, 3.0),
            (0, 2, 4.0),
            (1, 3, 4.0),
            (3, 4, 3.0),
            (4, 5, 3.0),
            (5, 6, 3.0),
            (3, 6, 3.0),
            (3, 5, 4.0),
            (4, 6, 4.0),
        ],
    )
    .unwrap();
    let params = RipsParams::new(1);
    let old_result = rips_persistence_with_classes_sparse(&old, &params).unwrap();
    let new_result = rips_persistence_with_classes_sparse(&new, &params).unwrap();
    let relation =
        class_correspondences(&old, &old_result.spaces, &new, &new_result.spaces, 2).unwrap();
    assert!(relation.is_empty());
}
