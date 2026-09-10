use super::*;
use crate::{
    CoverageAction, CoverageFence, CoverageLimits, CoverageSpecification, CoverageState,
    CoverageSynthesisLimits, PlanarCoverageModel, SparseDistanceMatrix,
};

fn two_state_problem() -> (CoverageSpecification, Vec<CoverageAction>) {
    let graph = SparseDistanceMatrix::from_triplets(
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
    .unwrap();
    let state = CoverageState::new(0, 0, &graph, vec![0, 1, 2, 3], 1.0).unwrap();
    let specification = CoverageSpecification::new(
        6,
        PlanarCoverageModel::new(1.0, 1.0).unwrap(),
        2,
        CoverageFence::new(vec![0, 1, 2, 3]).unwrap(),
        vec![4, 5],
        0,
        vec![
            state.clone(),
            CoverageState::new(0, 1, &graph, state.base_vertices().to_vec(), 1.0).unwrap(),
        ],
        CoverageLimits::default(),
    )
    .unwrap();
    let actions = vec![
        CoverageAction::new(4, 7, vec![0]),
        CoverageAction::new(5, 3, vec![1]),
    ];
    (specification, actions)
}

#[test]
fn independent_frontiers_compose_under_one_global_limit() {
    let (specification, actions) = two_state_problem();
    let infeasible = compose_coverage_frontiers(
        &specification,
        &actions,
        1,
        CoverageSynthesisLimits::default(),
    )
    .unwrap();
    assert_eq!(infeasible.status(), CoverageCompositionStatus::Infeasible);

    let optimal = compose_coverage_frontiers(
        &specification,
        &actions,
        2,
        CoverageSynthesisLimits::default(),
    )
    .unwrap();
    assert_eq!(optimal.status(), CoverageCompositionStatus::Optimal);
    assert_eq!(optimal.cost(), Some(10));
    assert_eq!(optimal.selected(), &[0, 1]);
    assert_eq!(optimal.frontiers().len(), 2);
}

#[test]
fn a_local_work_limit_never_returns_an_optimal_claim() {
    let (specification, actions) = two_state_problem();
    let result = compose_coverage_frontiers(
        &specification,
        &actions,
        2,
        CoverageSynthesisLimits::default().with_max_oracle_calls(1),
    )
    .unwrap();
    assert_eq!(result.status(), CoverageCompositionStatus::SearchIncomplete);
    assert!(result.cost().is_none());
    assert!(result.selected().is_empty());
}

#[test]
fn proof_artifacts_use_the_composed_incumbent() {
    let (specification, actions) = two_state_problem();
    let artifact = crate::CoverageSynthesisArtifact::build(
        specification,
        actions,
        2,
        CoverageSynthesisLimits::default(),
    )
    .unwrap();
    assert_eq!(artifact.status(), crate::CoverageSynthesisStatus::Optimal);
    assert_eq!(artifact.selected(), &[0, 1]);
    assert_eq!(artifact.upper_bound_cost(), Some(10));
    assert!(artifact.proof_nodes() > 0);
}
