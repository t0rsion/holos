#![cfg(holos_repository_tests)]

use holos_tda::{
    Bigrade, BipersistenceArtifact, BipersistenceArtifactLimits, BipersistenceRectangle,
    CertificateLimits, CircularCoordinateArtifact, DegreeRipsBifiltration, DegreeRipsParams,
    ProgramArtifact, ProgramTraceArtifact, RipsParams, SparseDistanceMatrix,
    circular_coordinate_for_class, rips_persistence_with_classes_sparse,
};
use holos_tda_check::{
    BipersistenceProofLimits, CircularProofLimits, ProgramGraph, ProgramProofLimits,
    ProgramTraceProofLimits, verify_bipersistence, verify_circular_coordinate, verify_program,
    verify_program_trace,
};

fn cycle_with_isolated_vertex(edge: f64) -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(5, &[(0, 1, edge), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)])
        .unwrap()
}

fn weighted_square() -> SparseDistanceMatrix {
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
fn circular_bytes_cross_the_independent_checker_boundary() {
    let graph = cycle_with_isolated_vertex(1.0);
    let explained =
        rips_persistence_with_classes_sparse(&graph, &RipsParams::new(1).with_modulus(47)).unwrap();
    let coordinate = circular_coordinate_for_class(
        &graph,
        explained.classes().next().unwrap(),
        Default::default(),
    )
    .unwrap();
    let bytes = CircularCoordinateArtifact::from_coordinate(&graph, &coordinate)
        .unwrap()
        .encode()
        .unwrap();

    let checked = verify_circular_coordinate(&bytes, CircularProofLimits::default()).unwrap();
    assert_eq!(checked.coordinates, 1);
    assert_eq!(checked.states, 1);
    assert_eq!(checked.modulus, 47);
}

#[test]
fn bipersistence_bytes_cross_the_independent_checker_boundary() {
    let graph = weighted_square();
    let degree_rips =
        DegreeRipsBifiltration::from_graph(&graph, DegreeRipsParams::default()).unwrap();
    let limits = BipersistenceArtifactLimits::default();
    let (mut artifact, module) = BipersistenceArtifact::build(&degree_rips, 47, limits).unwrap();
    artifact
        .record_rectangle(
            &module,
            BipersistenceRectangle::new(Bigrade::new(1, 1), Bigrade::new(2, 3)).unwrap(),
            limits,
        )
        .unwrap();
    let bytes = artifact.encode(limits).unwrap();

    let checked = verify_bipersistence(&bytes, BipersistenceProofLimits::default()).unwrap();
    assert_eq!(checked.vertices, graph.len());
    assert_eq!(checked.nodes, module.nodes().len());
    assert_eq!(checked.rectangles, 1);
}

#[test]
fn program_bytes_require_the_full_source_vertex_set() {
    let graph = cycle_with_isolated_vertex(1.0);
    let params = RipsParams::new(1).with_modulus(47).with_threshold(2.0);
    let artifact = ProgramArtifact::build(&graph, &params, CertificateLimits::default()).unwrap();
    let bytes = artifact.encode().unwrap();

    let source = ProgramGraph::parse_text(b"5\n0 1 1\n1 2 1\n2 3 1\n0 3 1\n").unwrap();
    assert_eq!(source.vertex_count(), graph.len());
    let checked = verify_program(&bytes, &source, ProgramProofLimits::default()).unwrap();
    assert_eq!(checked.vertices, 5);

    let inferred_source = ProgramGraph::parse_text(b"0 1 1\n1 2 1\n2 3 1\n0 3 1\n").unwrap();
    assert_eq!(inferred_source.vertex_count(), 4);
    assert!(verify_program(&bytes, &inferred_source, ProgramProofLimits::default()).is_err());
}

#[test]
fn trace_bytes_are_self_contained_for_the_independent_checker() {
    let initial = cycle_with_isolated_vertex(1.0);
    let updated = cycle_with_isolated_vertex(1.1);
    let params = RipsParams::new(1).with_modulus(47).with_threshold(2.0);
    let trace =
        ProgramTraceArtifact::build(&initial, &[updated], &params, CertificateLimits::default())
            .unwrap();
    let bytes = trace.encode().unwrap();

    let checked = verify_program_trace(&bytes, ProgramTraceProofLimits::default()).unwrap();
    assert_eq!(checked.vertices, 5);
    assert_eq!(checked.steps, 1);
}
