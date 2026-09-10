use std::collections::BTreeMap;

use super::api::build_coordinate;
use super::harmonic::coefficient;
use super::lift::active_edges;
use super::*;
use crate::classes::{Cocycle, CocycleTerm};
use crate::cohomology::CohomologyContinuationKind;
use crate::{RipsParams, SparseDistanceMatrix, rips_persistence_with_classes_sparse};

fn cycle_graph(vertices: usize) -> SparseDistanceMatrix {
    let mut edges = (0..vertices - 1)
        .map(|u| (u, u + 1, 1.0))
        .collect::<Vec<_>>();
    edges.push((0, vertices - 1, 1.0));
    SparseDistanceMatrix::from_triplets(vertices, &edges).unwrap()
}

#[test]
fn ripser_terms_normalize_orientation_and_scale() {
    let graph =
        SparseDistanceMatrix::from_triplets(4, &[(0, 3, 1.0), (1, 2, 1.0), (0, 2, 2.0)]).unwrap();
    let cocycle =
        cocycle_from_ripser_terms(&graph, 5, 1.0, &[(3, 0, 2), (1, 2, 4), (0, 2, 3)]).unwrap();
    assert_eq!(cocycle.terms[0].coefficient, 1);
    assert_eq!((cocycle.terms[0].u, cocycle.terms[0].v), (0, 3));
    assert_eq!(cocycle.terms.len(), 2);
}

#[test]
fn persistent_cycle_class_has_a_checked_phase() {
    let graph = cycle_graph(8);
    let explained =
        rips_persistence_with_classes_sparse(&graph, &RipsParams::new(1).with_modulus(47)).unwrap();
    let coordinate = circular_coordinate_for_class(
        &graph,
        explained.classes().next().unwrap(),
        CircularCoordinateParams::default(),
    )
    .unwrap();
    assert_eq!(coordinate.phase.len(), 8);
    assert_eq!(coordinate.divisibility, 1);
    assert!(coordinate.relative_residual <= coordinate.tolerance);
    assert!(
        coordinate
            .phase
            .iter()
            .all(|phase| (0.0..1.0).contains(phase))
    );
}

#[test]
fn figure_eight_classes_produce_distinct_coordinates() {
    let graph = SparseDistanceMatrix::from_triplets(
        7,
        &[
            (0, 1, 1.0),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (0, 3, 1.0),
            (0, 4, 1.0),
            (4, 5, 1.0),
            (5, 6, 1.0),
            (0, 6, 1.0),
        ],
    )
    .unwrap();
    let first = Cocycle {
        modulus: 47,
        scale: 1.0,
        terms: vec![CocycleTerm {
            u: 0,
            v: 1,
            coefficient: 1,
        }],
    };
    let second = Cocycle {
        modulus: 47,
        scale: 1.0,
        terms: vec![CocycleTerm {
            u: 0,
            v: 4,
            coefficient: 1,
        }],
    };
    let first = circular_coordinate(&graph, &first, CircularCoordinateParams::default()).unwrap();
    let second = circular_coordinate(&graph, &second, CircularCoordinateParams::default()).unwrap();
    assert_eq!(first.space, second.space);
    assert_ne!(first.class, second.class);
    assert_eq!(&first.potential[4..], &[0.0; 3]);
    assert_eq!(&second.potential[1..4], &[0.0; 3]);
}

#[test]
fn supplied_nonclosed_lift_is_rejected() {
    let graph =
        SparseDistanceMatrix::from_triplets(3, &[(0, 1, 1.0), (1, 2, 1.0), (0, 2, 1.0)]).unwrap();
    let cocycle = Cocycle {
        modulus: 5,
        scale: 1.0,
        terms: vec![
            CocycleTerm {
                u: 0,
                v: 1,
                coefficient: 1,
            },
            CocycleTerm {
                u: 0,
                v: 2,
                coefficient: 1,
            },
        ],
    };
    let integral = vec![
        IntegralCocycleTerm {
            u: 0,
            v: 1,
            coefficient: 1,
        },
        IntegralCocycleTerm {
            u: 0,
            v: 2,
            coefficient: 1,
        },
    ];
    assert!(
        circular_coordinate_with_integral_lift(
            &graph,
            &cocycle,
            &integral,
            CircularCoordinateParams::default(),
        )
        .is_err()
    );
}

#[test]
fn automatic_mod_two_lift_is_explicitly_rejected() {
    let graph = cycle_graph(8);
    let explained =
        rips_persistence_with_classes_sparse(&graph, &RipsParams::new(1).with_modulus(2)).unwrap();
    let error = circular_coordinate_for_class(
        &graph,
        explained.classes().next().unwrap(),
        CircularCoordinateParams::default(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("odd prime"));
}

#[test]
fn persistent_class_coordinate_rejects_a_different_active_graph() {
    let graph = cycle_graph(8);
    let explained =
        rips_persistence_with_classes_sparse(&graph, &RipsParams::new(1).with_modulus(47)).unwrap();
    let class = explained.classes().next().unwrap();
    let changed = SparseDistanceMatrix::from_triplets(
        8,
        &[
            (0, 1, 0.9),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (3, 4, 1.0),
            (4, 5, 1.0),
            (5, 6, 1.0),
            (6, 7, 1.0),
            (0, 7, 1.0),
        ],
    )
    .unwrap();
    let error = circular_coordinate_for_class(&changed, class, CircularCoordinateParams::default())
        .unwrap_err();
    assert!(error.to_string().contains("different active graph"));
}

#[test]
fn identity_continuation_recomputes_the_coordinate() {
    let graph = cycle_graph(8);
    let explained =
        rips_persistence_with_classes_sparse(&graph, &RipsParams::new(1).with_modulus(47)).unwrap();
    let coordinate = circular_coordinate_for_class(
        &graph,
        explained.classes().next().unwrap(),
        CircularCoordinateParams::default(),
    )
    .unwrap();
    let continued = continue_circular_coordinate(
        &graph,
        &coordinate,
        &graph,
        CircularCoordinateParams::default(),
    )
    .unwrap();
    assert_eq!(continued.topology.kind, CohomologyContinuationKind::Unique);
    for (actual, expected) in continued
        .coordinate
        .unwrap()
        .phase
        .iter()
        .zip(&coordinate.phase)
    {
        let difference = (actual - expected).abs();
        assert!(difference.min(1.0 - difference) < 1e-12);
    }
}

#[test]
fn continuation_preserves_a_nonunit_class_vector() {
    let graph = cycle_graph(8);
    let cocycle = Cocycle {
        modulus: 47,
        scale: 1.0,
        terms: vec![CocycleTerm {
            u: 0,
            v: 1,
            coefficient: 2,
        }],
    };
    let coordinate = build_coordinate(
        &graph,
        &cocycle,
        1,
        vec![IntegralCocycleTerm {
            u: 0,
            v: 1,
            coefficient: 2,
        }],
        CircularCoordinateParams::default(),
    )
    .unwrap();
    assert_eq!(coordinate.class[0].coefficient, 2);
    let continued = continue_circular_coordinate(
        &graph,
        &coordinate,
        &graph,
        CircularCoordinateParams::default(),
    )
    .unwrap();
    assert_eq!(continued.topology.kind, CohomologyContinuationKind::Unique);
    assert_eq!(continued.coordinate.unwrap().class, coordinate.class);
}

#[test]
fn supplied_lift_covers_a_class_without_a_centered_scalar_lift() {
    let graph = SparseDistanceMatrix::from_triplets(
        5,
        &[
            (0, 1, 1.0),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (0, 3, 1.0),
            (0, 4, 1.0),
            (1, 4, 1.0),
        ],
    )
    .unwrap();
    let cocycle = Cocycle {
        modulus: 3,
        scale: 1.0,
        terms: vec![
            CocycleTerm {
                u: 0,
                v: 1,
                coefficient: 1,
            },
            CocycleTerm {
                u: 0,
                v: 4,
                coefficient: 2,
            },
            CocycleTerm {
                u: 1,
                v: 4,
                coefficient: 1,
            },
        ],
    };
    let integral = vec![
        IntegralCocycleTerm {
            u: 0,
            v: 1,
            coefficient: 1,
        },
        IntegralCocycleTerm {
            u: 0,
            v: 4,
            coefficient: 2,
        },
        IntegralCocycleTerm {
            u: 1,
            v: 4,
            coefficient: 1,
        },
    ];

    let automatic =
        circular_coordinate(&graph, &cocycle, CircularCoordinateParams::default()).unwrap_err();
    assert!(automatic.to_string().contains("centered scalar"));
    let supplied = circular_coordinate_with_integral_lift(
        &graph,
        &cocycle,
        &integral,
        CircularCoordinateParams::default(),
    )
    .unwrap();
    assert_eq!(supplied.field_multiplier, 1);
    assert_eq!(supplied.divisibility, 1);
}

#[test]
fn supplied_lift_records_nonprimitive_divisibility() {
    let graph = cycle_graph(8);
    let cocycle = Cocycle {
        modulus: 47,
        scale: 1.0,
        terms: vec![CocycleTerm {
            u: 0,
            v: 1,
            coefficient: 1,
        }],
    };
    let integral = [IntegralCocycleTerm {
        u: 0,
        v: 1,
        coefficient: 2,
    }];
    let coordinate = circular_coordinate_with_integral_lift(
        &graph,
        &cocycle,
        &integral,
        CircularCoordinateParams::default(),
    )
    .unwrap();
    assert_eq!(coordinate.field_multiplier, 2);
    assert_eq!(coordinate.divisibility, 2);
}

#[test]
fn harmonic_solve_matches_a_dense_reference() {
    let graph = cycle_graph(11);
    let cocycle = Cocycle {
        modulus: 47,
        scale: 1.0,
        terms: vec![CocycleTerm {
            u: 0,
            v: 1,
            coefficient: 1,
        }],
    };
    let coordinate =
        circular_coordinate(&graph, &cocycle, CircularCoordinateParams::default()).unwrap();
    let edges = active_edges(&graph, 1.0);
    let coefficients = coordinate
        .integral
        .iter()
        .map(|term| ((term.u, term.v), term.coefficient))
        .collect::<BTreeMap<_, _>>();
    let reference = dense_reference_potential(graph.len(), &edges, &coefficients);
    for (actual, expected) in coordinate.potential.iter().zip(reference) {
        assert!((actual - expected).abs() < 1e-12);
    }
}

#[test]
fn disconnected_vertices_use_a_zero_component_gauge() {
    let mut edges = vec![(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)];
    edges.extend([(4, 5, 1.0), (5, 6, 1.0), (6, 7, 1.0), (4, 7, 1.0)]);
    let graph = SparseDistanceMatrix::from_triplets(9, &edges).unwrap();
    let cocycle = Cocycle {
        modulus: 47,
        scale: 1.0,
        terms: vec![CocycleTerm {
            u: 0,
            v: 1,
            coefficient: 1,
        }],
    };
    let coordinate =
        circular_coordinate(&graph, &cocycle, CircularCoordinateParams::default()).unwrap();
    assert_eq!(coordinate.potential[0], 0.0);
    assert_eq!(&coordinate.potential[4..], &[0.0; 5]);
}

#[test]
fn triplet_order_does_not_change_the_coordinate() {
    let ordered = [
        (0, 1, 1.0),
        (1, 2, 1.0),
        (2, 3, 1.0),
        (3, 4, 1.0),
        (4, 5, 1.0),
        (0, 5, 1.0),
    ];
    let mut reversed = ordered;
    reversed.reverse();
    let a = SparseDistanceMatrix::from_triplets(6, &ordered).unwrap();
    let b = SparseDistanceMatrix::from_triplets(6, &reversed).unwrap();
    let cocycle = Cocycle {
        modulus: 47,
        scale: 1.0,
        terms: vec![CocycleTerm {
            u: 0,
            v: 1,
            coefficient: 1,
        }],
    };
    let params = CircularCoordinateParams::default();
    assert_eq!(
        circular_coordinate(&a, &cocycle, params).unwrap(),
        circular_coordinate(&b, &cocycle, params).unwrap()
    );
}

#[test]
fn a_filled_triangle_coboundary_is_rejected() {
    let graph =
        SparseDistanceMatrix::from_triplets(3, &[(0, 1, 1.0), (1, 2, 1.0), (0, 2, 1.0)]).unwrap();
    let cocycle = Cocycle {
        modulus: 47,
        scale: 1.0,
        terms: vec![
            CocycleTerm {
                u: 0,
                v: 1,
                coefficient: 1,
            },
            CocycleTerm {
                u: 0,
                v: 2,
                coefficient: 1,
            },
        ],
    };
    let error =
        circular_coordinate(&graph, &cocycle, CircularCoordinateParams::default()).unwrap_err();
    assert!(error.to_string().contains("vertex coboundary"));
}

fn dense_reference_potential(
    vertex_count: usize,
    edges: &[(usize, usize)],
    coefficients: &BTreeMap<(usize, usize), i64>,
) -> Vec<f64> {
    let dimension = vertex_count - 1;
    let mut matrix = vec![vec![0.0; dimension + 1]; dimension];
    for &(u, v) in edges {
        let value = coefficient(coefficients, u, v) as f64;
        add_reference_edge(&mut matrix, dimension, u, v, value);
    }
    for pivot in 0..dimension {
        eliminate_reference_pivot(&mut matrix, dimension, pivot);
    }
    let mut potential = vec![0.0];
    potential.extend(matrix.into_iter().map(|row| row[dimension]));
    potential
}

fn add_reference_edge(matrix: &mut [Vec<f64>], dimension: usize, u: usize, v: usize, value: f64) {
    if u != 0 {
        matrix[u - 1][u - 1] += 1.0;
        matrix[u - 1][dimension] += value;
    }
    if v != 0 {
        matrix[v - 1][v - 1] += 1.0;
        matrix[v - 1][dimension] -= value;
    }
    if u != 0 && v != 0 {
        matrix[u - 1][v - 1] -= 1.0;
        matrix[v - 1][u - 1] -= 1.0;
    }
}

fn eliminate_reference_pivot(matrix: &mut [Vec<f64>], dimension: usize, pivot: usize) {
    let selected = (pivot..dimension)
        .max_by(|&a, &b| matrix[a][pivot].abs().total_cmp(&matrix[b][pivot].abs()))
        .unwrap();
    matrix.swap(pivot, selected);
    let divisor = matrix[pivot][pivot];
    for value in &mut matrix[pivot][pivot..=dimension] {
        *value /= divisor;
    }
    let pivot_row = matrix[pivot].clone();
    for (row, row_values) in matrix.iter_mut().enumerate().take(dimension) {
        if row == pivot {
            continue;
        }
        let factor = row_values[pivot];
        for (value, &pivot_value) in row_values[pivot..=dimension]
            .iter_mut()
            .zip(&pivot_row[pivot..=dimension])
        {
            *value -= factor * pivot_value;
        }
    }
}
