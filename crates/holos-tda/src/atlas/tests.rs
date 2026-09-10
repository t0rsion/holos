use super::point::scaled_difference_norm;
use super::support::diagram_bits_equal;
use super::*;
use crate::{
    PointCloudGraph, PointCloudParams, RipsParams, SparseDistanceMatrix, rips_persistence_sparse,
};

fn square(diagonal: f64) -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(
        4,
        &[
            (0, 1, 1.0),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (0, 3, 1.0),
            (0, 2, diagonal),
            (1, 3, diagonal),
        ],
    )
    .unwrap()
}

#[test]
fn atlas_reuses_a_weak_order_and_recomputes_at_an_event() {
    let input = square(2.0);
    let atlas = PersistenceAtlas::build(&input, &RipsParams::new(1)).unwrap();
    let scaled = SparseDistanceMatrix::from_triplets(
        4,
        &[
            (0, 1, 2.0),
            (1, 2, 2.0),
            (2, 3, 2.0),
            (0, 3, 2.0),
            (0, 2, 5.0),
            (1, 3, 5.0),
        ],
    )
    .unwrap();
    let update = atlas.update(&scaled).unwrap();
    assert_eq!(update.mode, UpdateMode::Reused);
    assert!(update.events.is_empty());
    let h1: Vec<_> = update.evaluation.diagram.in_dim(1).collect();
    assert_eq!(h1.len(), 1);
    assert_eq!((h1[0].birth, h1[0].death), (2.0, 5.0));
    assert_eq!(
        update.evaluation.spaces[0].lineage,
        atlas.evaluate(&input).unwrap().spaces[0].lineage
    );

    let split = SparseDistanceMatrix::from_triplets(
        4,
        &[
            (0, 1, 1.0),
            (1, 2, 1.1),
            (2, 3, 1.0),
            (0, 3, 1.0),
            (0, 2, 2.0),
            (1, 3, 2.0),
        ],
    )
    .unwrap();
    let update = atlas.update(&split).unwrap();
    assert_eq!(update.mode, UpdateMode::Recomputed);
    assert!(
        update
            .events
            .iter()
            .any(|event| event.kind == TopologyEventKind::EqualitySplit)
    );
    let exact = rips_persistence_sparse(&split, &RipsParams::new(1)).unwrap();
    assert!(diagram_bits_equal(&update.evaluation.diagram, &exact));
}

#[test]
fn endpoint_gradients_name_unique_and_tied_edges() {
    let atlas = PersistenceAtlas::build(&square(2.0), &RipsParams::new(1)).unwrap();
    let sensitivity = &atlas.evaluate(&square(2.0)).unwrap().sensitivities[0];
    assert!(matches!(sensitivity.birth, EndpointGradient::Edge(_)));
    assert!(matches!(sensitivity.death, EndpointGradient::Edge(_)));
}

#[test]
fn point_radius_reuses_small_changes_and_rejects_its_boundary() {
    let points = vec![
        vec![0.0, 0.0],
        vec![1.0, 0.1],
        vec![1.2, 1.1],
        vec![0.0, 0.9],
    ];
    let params = RipsParams::new(1).with_threshold(2.0);
    let atlas = PointPersistenceAtlas::build(&points, &params).unwrap();
    assert!(atlas.coordinate_radius() > 0.0);
    let mut moved = points.clone();
    moved[0][0] += atlas.coordinate_radius() / 4.0;
    let update = atlas.update(&moved).unwrap();
    assert_eq!(update.mode, UpdateMode::Reused);
    let exact_graph = PointCloudGraph::build(&moved, PointCloudParams::new(2.0)).unwrap();
    let exact = rips_persistence_sparse(exact_graph.matrix(), &params).unwrap();
    assert!(diagram_bits_equal(&update.evaluation.diagram, &exact));
}

#[test]
fn random_order_preserving_weights_match_exact_reduction() {
    let mut state = 0xd038_72ab_54f1_c967u64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for case in 0..64 {
        let n = 5 + next() as usize % 5;
        let mut triplets = Vec::new();
        for u in 0..n {
            for v in u + 1..n {
                if next() % 5 < 3 {
                    triplets.push((u, v, (1 + next() % 9) as f64));
                }
            }
        }
        let input = SparseDistanceMatrix::from_triplets(n, &triplets).unwrap();
        let params = RipsParams::new(1).with_modulus([2, 3, 5][case % 3]);
        let atlas = PersistenceAtlas::build(&input, &params).unwrap();
        let updated_triplets: Vec<_> = input
            .edges()
            .map(|(u, v, value)| (u, v, value * 3.0 + 0.5))
            .collect();
        let updated = SparseDistanceMatrix::from_triplets(n, &updated_triplets).unwrap();
        let evaluated = atlas
            .evaluate(&updated)
            .unwrap_or_else(|error| panic!("case {case}: {error}"));
        let fast = atlas
            .evaluate_diagram(&updated)
            .unwrap_or_else(|error| panic!("case {case}: {error}"));
        let exact = rips_persistence_sparse(&updated, &params).unwrap();
        assert!(
            diagram_bits_equal(&evaluated.diagram, &exact),
            "case {case}"
        );
        assert!(diagram_bits_equal(&fast, &exact), "case {case}");
    }
}

#[test]
fn every_region_boundary_has_an_explicit_event() {
    let distinct = SparseDistanceMatrix::from_triplets(
        4,
        &[
            (0, 1, 1.0),
            (0, 2, 1.2),
            (0, 3, 1.4),
            (1, 2, 1.6),
            (1, 3, 1.8),
            (2, 3, 2.0),
        ],
    )
    .unwrap();
    let atlas =
        PersistenceAtlas::build(&distinct, &RipsParams::new(1).with_threshold(1.7)).unwrap();

    let fewer_vertices = SparseDistanceMatrix::from_triplets(3, &[]).unwrap();
    assert_eq!(
        atlas.events(&fewer_vertices)[0].kind,
        TopologyEventKind::VertexSetChanged
    );

    let missing = SparseDistanceMatrix::from_triplets(
        4,
        &distinct
            .edges()
            .filter(|&(u, v, _)| (u, v) != (2, 3))
            .collect::<Vec<_>>(),
    )
    .unwrap();
    assert_eq!(
        atlas.events(&missing)[0].kind,
        TopologyEventKind::EdgeSetChanged
    );

    let changed = |replacement: (usize, usize, f64)| {
        SparseDistanceMatrix::from_triplets(
            4,
            &distinct
                .edges()
                .map(|(u, v, value)| {
                    if (u, v) == (replacement.0, replacement.1) {
                        replacement
                    } else {
                        (u, v, value)
                    }
                })
                .collect::<Vec<_>>(),
        )
        .unwrap()
    };
    let crossing = changed((1, 2, 1.75));
    assert!(
        atlas
            .events(&crossing)
            .iter()
            .any(|event| event.kind == TopologyEventKind::ThresholdCrossing)
    );
    let merge = changed((0, 2, 1.0));
    assert!(
        atlas
            .events(&merge)
            .iter()
            .any(|event| event.kind == TopologyEventKind::EqualityMerge)
    );
    let swap = changed((0, 2, 0.9));
    assert!(
        atlas
            .events(&swap)
            .iter()
            .any(|event| event.kind == TopologyEventKind::OrderSwap)
    );

    let tied = PersistenceAtlas::build(&square(2.0), &RipsParams::new(1)).unwrap();
    let split = SparseDistanceMatrix::from_triplets(
        4,
        &[
            (0, 1, 1.0),
            (1, 2, 1.01),
            (2, 3, 1.0),
            (0, 3, 1.0),
            (0, 2, 2.0),
            (1, 3, 2.0),
        ],
    )
    .unwrap();
    assert!(
        tied.events(&split)
            .iter()
            .any(|event| event.kind == TopologyEventKind::EqualitySplit)
    );
}

#[test]
fn edge_endpoint_gradients_match_finite_differences() {
    let input = SparseDistanceMatrix::from_triplets(
        4,
        &[
            (0, 1, 1.0),
            (0, 2, 2.0),
            (0, 3, 1.1),
            (1, 2, 1.2),
            (1, 3, 2.1),
            (2, 3, 1.3),
        ],
    )
    .unwrap();
    let atlas = PersistenceAtlas::build(&input, &RipsParams::new(1)).unwrap();
    let original = atlas.evaluate(&input).unwrap();
    let sensitivity = &original.sensitivities[0];
    for (gradient, birth) in [(&sensitivity.birth, true), (&sensitivity.death, false)] {
        let EndpointGradient::Edge(source) = gradient else {
            panic!("test graph must have unique endpoint sources");
        };
        let epsilon = 1e-7;
        let changed: Vec<_> = input
            .edges()
            .map(|(u, v, value)| {
                if (u, v) == (source.u, source.v) {
                    (u, v, value + epsilon)
                } else {
                    (u, v, value)
                }
            })
            .collect();
        let changed = SparseDistanceMatrix::from_triplets(4, &changed).unwrap();
        let evaluated = atlas.evaluate(&changed).unwrap();
        let old = original.spaces[0].space.interval;
        let new = evaluated.spaces[0].space.interval;
        let difference = if birth {
            new.birth - old.birth
        } else {
            new.death - old.death
        };
        assert!((difference / epsilon - 1.0).abs() < 1e-8);
    }
}

#[test]
fn point_coordinate_gradients_match_finite_differences() {
    let points = vec![
        vec![0.0, 0.0],
        vec![1.0, 0.1],
        vec![1.2, 1.1],
        vec![0.0, 0.9],
    ];
    let params = RipsParams::new(1).with_threshold(2.0);
    let atlas = PointPersistenceAtlas::build(&points, &params).unwrap();
    let original = atlas.evaluate(&points).unwrap();
    let sensitivity = atlas.sensitivities().remove(0);
    for (gradient, birth) in [
        (sensitivity.birth.unwrap(), true),
        (sensitivity.death.unwrap(), false),
    ] {
        let term = &gradient.terms[0];
        let epsilon = atlas.coordinate_radius().min(1e-5) / 100.0;
        let mut changed = points.clone();
        changed[term.point][term.coordinate] += epsilon;
        let evaluated = atlas.evaluate(&changed).unwrap();
        let old = original.spaces[0].space.interval;
        let new = evaluated.spaces[0].space.interval;
        let difference = if birth {
            new.birth - old.birth
        } else {
            new.death - old.death
        };
        assert!((difference / epsilon - term.value).abs() < 1e-5);
    }
}

#[test]
fn random_point_trajectories_inside_the_radius_match_exact_reduction() {
    let mut state = 0x3a11_8e4d_90c7_526bu64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for case in 0..32 {
        let points: Vec<Vec<f64>> = (0..7)
            .map(|point| {
                (0..3)
                    .map(|coordinate| {
                        (next() % 10_000) as f64 / 997.0
                            + point as f64 * 1e-4
                            + coordinate as f64 * 1e-6
                    })
                    .collect()
            })
            .collect();
        let threshold = 20.0;
        let params = RipsParams::new(1)
            .with_threshold(threshold)
            .with_modulus([2, 3, 5][case % 3]);
        let atlas = PointPersistenceAtlas::build(&points, &params).unwrap();
        if atlas.coordinate_radius() == 0.0 {
            continue;
        }
        let shift = atlas.coordinate_radius() / 16.0;
        let mut changed = points.clone();
        for point in &mut changed {
            for value in point {
                *value += if next() & 1 == 0 { shift } else { -shift };
            }
        }
        let evaluated = atlas
            .evaluate(&changed)
            .unwrap_or_else(|error| panic!("case {case}: {error}"));
        let exact_graph =
            PointCloudGraph::build(&changed, PointCloudParams::new(threshold)).unwrap();
        let exact = rips_persistence_sparse(exact_graph.matrix(), &params).unwrap();
        assert!(
            diagram_bits_equal(&evaluated.diagram, &exact),
            "case {case}"
        );
    }
}

#[test]
fn point_displacement_uses_a_scaled_norm() {
    let norm = scaled_difference_norm(&[0.0, 0.0], &[1e200, -1e200]);
    assert!(norm.is_finite());
    assert!((norm / 1e200 - 2.0f64.sqrt()).abs() < 1e-15);
}
