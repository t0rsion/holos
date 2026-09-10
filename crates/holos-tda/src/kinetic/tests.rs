use num_rational::BigRational;

use super::arithmetic::{next_up, rational};
use super::*;
use crate::{CohomologyLimits, ZigzagDirection, ZigzagLimits};

#[test]
fn schedule_groups_exact_simultaneous_events_and_encloses_roots() {
    let trajectory = KineticFiltration::new(
        4,
        vec![
            KineticEdge {
                u: 0,
                v: 1,
                intercept: 0.0,
                velocity: 1.0,
            },
            KineticEdge {
                u: 1,
                v: 2,
                intercept: 1.0,
                velocity: -1.0,
            },
            KineticEdge {
                u: 2,
                v: 3,
                intercept: 0.5,
                velocity: 0.0,
            },
        ],
        0.0,
        1.0,
        KineticLimits::default(),
    )
    .unwrap();
    let schedule = trajectory.events(Some(0.5)).unwrap();
    assert_eq!(schedule.events.len(), 1);
    assert_eq!(schedule.events[0].time, 0.5);
    assert_eq!(schedule.events[0].kinds.len(), 5);
    assert!(schedule.events[0].lower <= 0.5 && schedule.events[0].upper >= 0.5);
    assert_eq!(schedule.persistent_ties, 0);
}

#[test]
fn fixed_scale_graphs_ignore_inactive_order_swaps() {
    let trajectory = KineticFiltration::new(
        3,
        vec![
            KineticEdge {
                u: 0,
                v: 1,
                intercept: 0.8,
                velocity: 0.4,
            },
            KineticEdge {
                u: 1,
                v: 2,
                intercept: 1.2,
                velocity: -0.4,
            },
        ],
        0.0,
        1.0,
        KineticLimits::default(),
    )
    .unwrap();
    let graphs = trajectory.critical_graphs(1.5).unwrap();
    assert_eq!(graphs.len(), 3);
    assert!(graphs.iter().all(|state| state.graph.num_edges() == 2));
    assert!(
        graphs
            .iter()
            .all(|state| !matches!(state.kind, KineticGraphStateKind::Event(_)))
    );
}

#[test]
fn nonrepresentable_event_gets_adjacent_float_bounds() {
    let trajectory = KineticFiltration::new(
        3,
        vec![
            KineticEdge {
                u: 0,
                v: 1,
                intercept: 0.0,
                velocity: 1.0,
            },
            KineticEdge {
                u: 1,
                v: 2,
                intercept: 1.0,
                velocity: -2.0,
            },
        ],
        0.0,
        0.5,
        KineticLimits::default(),
    )
    .unwrap();
    let event = &trajectory.events(None).unwrap().events[0];
    let exact = BigRational::new(1.into(), 3.into());
    assert!(rational(event.lower) < exact);
    assert!(rational(event.upper) > exact);
    assert_eq!(next_up(event.lower), event.upper);
}

#[test]
fn cohomology_event_detects_an_h2_birth_and_death_direction() {
    let mut edges = Vec::new();
    for u in 0..6 {
        for v in u + 1..6 {
            if u / 2 != v / 2 {
                edges.push(KineticEdge {
                    u,
                    v,
                    intercept: 0.0,
                    velocity: 0.0,
                });
            }
        }
    }
    edges.push(KineticEdge {
        u: 0,
        v: 1,
        intercept: 2.0,
        velocity: -2.0,
    });
    let trajectory = KineticFiltration::new(6, edges, 0.0, 1.0, KineticLimits::default()).unwrap();
    let events = trajectory
        .cohomology_events(2, 1.0, 3, CohomologyLimits::default())
        .unwrap();
    let event = events
        .iter()
        .find(|event| event.before_rank != event.after_rank)
        .unwrap();
    assert_eq!(event.before_rank, 1);
    assert_eq!(event.after_rank, 0);
    assert_eq!(event.relation.relation_rank, 0);
}

#[test]
fn kinetic_zigzag_records_an_exact_h2_death() {
    let mut edges = Vec::new();
    for u in 0..6 {
        for v in u + 1..6 {
            if u / 2 != v / 2 {
                edges.push(KineticEdge {
                    u,
                    v,
                    intercept: 0.0,
                    velocity: 0.0,
                });
            }
        }
    }
    edges.push(KineticEdge {
        u: 0,
        v: 1,
        intercept: 2.0,
        velocity: -2.0,
    });
    let trajectory = KineticFiltration::new(6, edges, 0.0, 1.0, KineticLimits::default()).unwrap();
    let zigzag = trajectory
        .cohomology_zigzag(
            2,
            1.0,
            5,
            CohomologyLimits::default(),
            ZigzagLimits::default(),
        )
        .unwrap();
    assert_eq!(
        zigzag
            .nodes
            .iter()
            .map(|node| node.rank)
            .collect::<Vec<_>>(),
        vec![1, 0, 0]
    );
    assert_eq!(zigzag.arrows.len(), 2);
    assert_eq!(zigzag.arrows[0].direction, ZigzagDirection::Backward);
    assert_eq!(zigzag.arrows[1].direction, ZigzagDirection::Forward);
    assert_eq!(zigzag.barcode.intervals.len(), 1);
    assert_eq!(zigzag.barcode.intervals[0].start, 0);
    assert_eq!(zigzag.barcode.intervals[0].end, 0);
}

#[test]
fn inactive_order_swap_carries_one_class_through_the_event() {
    let trajectory = KineticFiltration::new(
        4,
        vec![
            KineticEdge {
                u: 0,
                v: 1,
                intercept: 0.8,
                velocity: 0.4,
            },
            KineticEdge {
                u: 1,
                v: 2,
                intercept: 1.2,
                velocity: -0.4,
            },
            KineticEdge {
                u: 2,
                v: 3,
                intercept: 0.9,
                velocity: 0.0,
            },
            KineticEdge {
                u: 0,
                v: 3,
                intercept: 0.9,
                velocity: 0.0,
            },
        ],
        0.0,
        1.0,
        KineticLimits::default(),
    )
    .unwrap();
    let zigzag = trajectory
        .cohomology_zigzag(
            1,
            1.5,
            3,
            CohomologyLimits::default(),
            ZigzagLimits::default(),
        )
        .unwrap();
    assert!(zigzag.nodes.len() >= 3);
    assert!(zigzag.nodes.iter().all(|node| node.rank == 1));
    assert_eq!(zigzag.barcode.intervals.len(), 1);
    assert_eq!(zigzag.barcode.intervals[0].start, 0);
    assert_eq!(zigzag.barcode.intervals[0].end, zigzag.nodes.len() - 1);
}

#[test]
fn simultaneous_death_and_birth_do_not_create_a_false_identity() {
    let mut edges = Vec::new();
    for offset in [0, 4] {
        for (u, v) in [(0, 1), (1, 2), (2, 3), (0, 3)] {
            edges.push(KineticEdge {
                u: offset + u,
                v: offset + v,
                intercept: 0.5,
                velocity: 0.0,
            });
        }
    }
    edges.push(KineticEdge {
        u: 0,
        v: 2,
        intercept: 2.0,
        velocity: -2.0,
    });
    edges.push(KineticEdge {
        u: 4,
        v: 6,
        intercept: 0.0,
        velocity: 2.0,
    });
    let trajectory = KineticFiltration::new(8, edges, 0.0, 1.0, KineticLimits::default()).unwrap();
    let zigzag = trajectory
        .cohomology_zigzag(
            1,
            1.0,
            3,
            CohomologyLimits::default(),
            ZigzagLimits::default(),
        )
        .unwrap();
    let dimensions = zigzag
        .nodes
        .iter()
        .map(|node| node.rank)
        .collect::<Vec<_>>();
    let split = dimensions
        .iter()
        .position(|rank| *rank == 0)
        .expect("the simultaneous event separates both classes");
    assert!(dimensions[..split].iter().all(|rank| *rank == 1));
    assert!(dimensions[split + 1..].iter().all(|rank| *rank == 1));
    assert_eq!(
        zigzag
            .barcode
            .intervals
            .iter()
            .map(|interval| (interval.start, interval.end, interval.multiplicity))
            .collect::<Vec<_>>(),
        vec![(0, split - 1, 1), (split + 1, dimensions.len() - 1, 1)]
    );
}

#[test]
fn limits_and_invalid_trajectories_are_rejected() {
    assert!(
        KineticFiltration::new(
            2,
            vec![KineticEdge {
                u: 0,
                v: 1,
                intercept: -1.0,
                velocity: 0.0,
            }],
            0.0,
            1.0,
            KineticLimits::default(),
        )
        .is_err()
    );
    let trajectory = KineticFiltration::new(
        3,
        vec![
            KineticEdge {
                u: 0,
                v: 1,
                intercept: 0.0,
                velocity: 1.0,
            },
            KineticEdge {
                u: 1,
                v: 2,
                intercept: 1.0,
                velocity: -1.0,
            },
        ],
        0.0,
        1.0,
        KineticLimits {
            max_pair_tests: 0,
            ..KineticLimits::default()
        },
    )
    .unwrap();
    assert!(trajectory.events(None).is_err());
}
