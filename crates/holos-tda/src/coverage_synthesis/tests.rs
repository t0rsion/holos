//! Coverage synthesis tests.

use super::*;
use crate::{
    CoverageFence, CoverageLimits, KineticEdge, KineticFiltration, KineticLimits,
    PlanarCoverageModel, SparseDistanceMatrix,
};

fn model() -> PlanarCoverageModel {
    PlanarCoverageModel::new(1.0, 1.0).unwrap()
}

fn graph() -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(
        6,
        &[
            (0, 1, 1.0),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (0, 3, 1.0),
            (0, 4, 1.0),
            (1, 4, 1.0),
            (2, 4, 1.0),
            (3, 4, 1.0),
            (0, 5, 1.0),
            (1, 5, 1.0),
            (2, 5, 1.0),
            (3, 5, 1.0),
        ],
    )
    .unwrap()
}

fn specification(failures: usize) -> CoverageSpecification {
    CoverageSpecification::new(
        6,
        model(),
        2,
        CoverageFence::new(vec![0, 1, 2, 3]).unwrap(),
        vec![4, 5],
        failures,
        vec![CoverageState::new(0, 0, &graph(), vec![0, 1, 2, 3], 1.0).unwrap()],
        CoverageLimits::default(),
    )
    .unwrap()
}

#[test]
fn one_candidate_covers_without_failures() {
    let specification = specification(0);
    let actions = vec![
        CoverageAction::throughout(4, 2, &specification),
        CoverageAction::throughout(5, 3, &specification),
    ];
    assert!(
        !evaluate_coverage_plan(&specification, &actions, &[], CoverageLimits::default())
            .unwrap()
            .criterion_holds
    );
    assert!(
        evaluate_coverage_plan(&specification, &actions, &[0], CoverageLimits::default())
            .unwrap()
            .criterion_holds
    );
}

#[test]
fn one_failure_requires_both_redundant_candidates() {
    let specification = specification(1);
    let actions = vec![
        CoverageAction::throughout(4, 2, &specification),
        CoverageAction::throughout(5, 3, &specification),
    ];
    let failed =
        evaluate_coverage_plan(&specification, &actions, &[0], CoverageLimits::default()).unwrap();
    assert!(!failed.criterion_holds);
    assert_eq!(failed.counterexample.unwrap().failed_vertices, vec![4]);
    assert!(
        evaluate_coverage_plan(&specification, &actions, &[0, 1], CoverageLimits::default())
            .unwrap()
            .criterion_holds
    );
}

#[test]
fn synthesis_proves_the_minimum_failure_tolerant_plan() {
    let specification = specification(1);
    let actions = vec![
        CoverageAction::throughout(4, 2, &specification),
        CoverageAction::throughout(5, 3, &specification),
    ];
    let artifact = CoverageSynthesisArtifact::build(
        specification,
        actions,
        2,
        CoverageSynthesisLimits::default(),
    )
    .unwrap();
    assert_eq!(artifact.status(), CoverageSynthesisStatus::Optimal);
    assert_eq!(artifact.selected(), &[0, 1]);
    assert_eq!(artifact.lower_bound_cost(), Some(5));
    assert_eq!(artifact.upper_bound_cost(), Some(5));
    assert!(artifact.proof_nodes() >= 1);
    let bytes = artifact.encode(CoverageSynthesisLimits::default()).unwrap();
    let decoded =
        CoverageSynthesisArtifact::decode(&bytes, CoverageSynthesisLimits::default()).unwrap();
    assert_eq!(decoded.digest(), artifact.digest());
    let independent =
        holos_tda_check::verify_coverage(&bytes, holos_tda_check::ProofLimits::default()).unwrap();
    assert_eq!(
        independent.status,
        holos_tda_check::VerifiedCoverageStatus::Optimal
    );
    assert_eq!(independent.total_cost, Some(5));
}

#[test]
fn synthesis_artifact_mutations_fail_closed() {
    let specification = specification(0);
    let actions = vec![
        CoverageAction::throughout(4, 2, &specification),
        CoverageAction::throughout(5, 3, &specification),
    ];
    let artifact = CoverageSynthesisArtifact::build(
        specification,
        actions,
        1,
        CoverageSynthesisLimits::default(),
    )
    .unwrap();
    let bytes = artifact.encode(CoverageSynthesisLimits::default()).unwrap();
    for position in [0, bytes.len() / 2, bytes.len() - 1] {
        let mut changed = bytes.clone();
        changed[position] ^= 1;
        assert!(
            CoverageSynthesisArtifact::decode(&changed, CoverageSynthesisLimits::default())
                .is_err()
        );
        assert!(
            holos_tda_check::verify_coverage(&changed, holos_tda_check::ProofLimits::default())
                .is_err()
        );
    }
    assert!(
        CoverageSynthesisArtifact::decode(
            &bytes[..bytes.len() - 1],
            CoverageSynthesisLimits::default()
        )
        .is_err()
    );
    assert!(
        holos_tda_check::verify_coverage(
            &bytes[..bytes.len() - 1],
            holos_tda_check::ProofLimits::default()
        )
        .is_err()
    );
}

#[test]
fn limited_search_returns_only_a_checked_gap() {
    let specification = specification(0);
    let actions = vec![
        CoverageAction::throughout(4, 2, &specification),
        CoverageAction::throughout(5, 3, &specification),
    ];
    let artifact = CoverageSynthesisArtifact::build(
        specification,
        actions,
        1,
        CoverageSynthesisLimits::default().with_max_oracle_calls(1),
    )
    .unwrap();
    assert_eq!(artifact.status(), CoverageSynthesisStatus::SearchIncomplete);
    assert!(artifact.upper_bound_cost().is_none());
    assert_eq!(artifact.lower_bound_cost(), Some(0));
}

#[test]
fn maximal_failure_sets_cover_every_smaller_failure() {
    let specification = specification(1);
    let actions = vec![
        CoverageAction::throughout(4, 2, &specification),
        CoverageAction::throughout(5, 3, &specification),
    ];
    let evaluation =
        evaluate_coverage_plan(&specification, &actions, &[0, 1], CoverageLimits::default())
            .unwrap();
    assert_eq!(evaluation.checks, 2);
    assert!(evaluation.criterion_holds);
}

#[test]
fn affine_compilation_includes_every_threshold_cell() {
    let edges = graph()
        .edges()
        .map(|(u, v, _)| KineticEdge {
            u,
            v,
            intercept: if (u, v) == (0, 4) { 0.5 } else { 1.0 },
            velocity: if (u, v) == (0, 4) { 1.0 } else { 0.0 },
        })
        .collect();
    let kinetic = KineticFiltration::new(6, edges, 0.0, 1.0, KineticLimits::default()).unwrap();
    let specification = CoverageSpecification::from_kinetic(
        &kinetic,
        7,
        model(),
        2,
        CoverageFence::new(vec![0, 1, 2, 3]).unwrap(),
        vec![4, 5],
        0,
        vec![0, 1, 2, 3],
        CoverageLimits::default(),
    )
    .unwrap();
    assert_eq!(specification.states.len(), 5);
    assert!(matches!(
        specification.source,
        CoverageSource::Affine { .. }
    ));
    let action = CoverageAction::throughout(5, 1, &specification);
    let artifact = CoverageSynthesisArtifact::build(
        specification,
        vec![action],
        1,
        CoverageSynthesisLimits::default(),
    )
    .unwrap();
    let bytes = artifact.encode(CoverageSynthesisLimits::default()).unwrap();
    let independent =
        holos_tda_check::verify_coverage(&bytes, holos_tda_check::ProofLimits::default()).unwrap();
    assert_eq!(
        independent.source,
        holos_tda_check::VerifiedCoverageSource::Affine
    );
}

#[test]
fn incidence_components_separate_state_local_candidates() {
    let state = CoverageState::new(0, 0, &graph(), vec![0, 1, 2, 3], 1.0).unwrap();
    let specification = CoverageSpecification::new(
        6,
        model(),
        2,
        CoverageFence::new(vec![0, 1, 2, 3]).unwrap(),
        vec![4, 5],
        0,
        vec![state.clone(), CoverageState { step: 1, ..state }],
        CoverageLimits::default(),
    )
    .unwrap();
    let components = specification
        .components(&[
            CoverageAction::new(4, 1, vec![0]),
            CoverageAction::new(5, 1, vec![1]),
        ])
        .unwrap();
    assert_eq!(components.len(), 2);
}
