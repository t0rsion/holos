use super::{
    CohomologyInterventionArtifact, CohomologyInterventionCandidate, CohomologyInterventionLimits,
    CohomologyInterventionScenario, CohomologyInterventionStatus,
};
use crate::{KineticEdgeKey, SparseDistanceMatrix};
use holos_tda_check::{ProofLimits, verify_cohomology_intervention};

fn two_spheres() -> SparseDistanceMatrix {
    let edges = [0, 6]
        .into_iter()
        .flat_map(|offset| {
            (0..6).flat_map(move |u| {
                ((u + 1)..6)
                    .filter(move |v| u / 2 != v / 2)
                    .map(move |v| (offset + u, offset + v, 1.0))
            })
        })
        .collect::<Vec<_>>();
    SparseDistanceMatrix::from_triplets(12, &edges).unwrap()
}

fn scenario(graph: &SparseDistanceMatrix, target: usize) -> CohomologyInterventionScenario {
    CohomologyInterventionScenario::from_graph(graph, 1.0, target).unwrap()
}

#[test]
fn weighted_single_scenario_chooses_the_cheapest_killer() {
    let graph = two_spheres();
    let artifact = CohomologyInterventionArtifact::build(
        12,
        2,
        1.0,
        5,
        &[scenario(&graph, 0)],
        &[
            CohomologyInterventionCandidate::new(0, 1, 9),
            CohomologyInterventionCandidate::new(2, 3, 2),
            CohomologyInterventionCandidate::new(4, 5, 5),
        ],
        2,
        CohomologyInterventionLimits::default(),
    )
    .unwrap();
    assert_eq!(artifact.status(), CohomologyInterventionStatus::Optimal);
    assert_eq!(artifact.edits()[0].edge, KineticEdgeKey::new(2, 3));
    assert_eq!(artifact.lower_bound_cost(), Some(2));
    assert_eq!(artifact.upper_bound_cost(), Some(2));
}

#[test]
fn one_plan_kills_targets_in_two_scenarios_and_checks_independently() {
    let graph = two_spheres();
    let limits = CohomologyInterventionLimits::default();
    let artifact = CohomologyInterventionArtifact::build(
        12,
        2,
        1.0,
        3,
        &[scenario(&graph, 0), scenario(&graph, 1)],
        &[
            CohomologyInterventionCandidate::new(0, 1, 4),
            CohomologyInterventionCandidate::new(6, 7, 7),
        ],
        2,
        limits,
    )
    .unwrap();
    assert_eq!(artifact.status(), CohomologyInterventionStatus::Optimal);
    assert_eq!(artifact.edits().len(), 2);
    assert_eq!(artifact.upper_bound_cost(), Some(11));
    assert_eq!(artifact.root_blocker_bound(), 11);
    let bytes = artifact.encode(limits).unwrap();
    let decoded = CohomologyInterventionArtifact::decode(&bytes, limits).unwrap();
    assert_eq!(decoded, artifact);
    let checked = verify_cohomology_intervention(&bytes, ProofLimits::default()).unwrap();
    assert_eq!(checked.scenarios, 2);
    assert_eq!(checked.upper_bound_cost, Some(11));
}

#[test]
fn one_plan_handles_distinct_scenario_graphs() {
    let first = SparseDistanceMatrix::from_triplets(
        6,
        &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)],
    )
    .unwrap();
    let second = SparseDistanceMatrix::from_triplets(
        6,
        &[(0, 1, 1.0), (1, 4, 1.0), (4, 5, 1.0), (0, 5, 1.0)],
    )
    .unwrap();
    let artifact = CohomologyInterventionArtifact::build(
        6,
        1,
        1.0,
        3,
        &[scenario(&first, 0), scenario(&second, 0)],
        &[
            CohomologyInterventionCandidate::new(0, 2, 4),
            CohomologyInterventionCandidate::new(0, 4, 7),
        ],
        2,
        CohomologyInterventionLimits::default(),
    )
    .unwrap();
    assert_eq!(artifact.status(), CohomologyInterventionStatus::Optimal);
    assert_eq!(artifact.upper_bound_cost(), Some(11));
    assert_eq!(artifact.before_ranks(), [1, 1]);
    assert_eq!(artifact.after_ranks(), [0, 0]);
}

#[test]
fn edit_limit_proves_infeasibility() {
    let graph = two_spheres();
    let limits = CohomologyInterventionLimits::default();
    let artifact = CohomologyInterventionArtifact::build(
        12,
        2,
        1.0,
        5,
        &[scenario(&graph, 0), scenario(&graph, 1)],
        &[
            CohomologyInterventionCandidate::new(0, 1, 4),
            CohomologyInterventionCandidate::new(6, 7, 7),
        ],
        1,
        limits,
    )
    .unwrap();
    assert_eq!(artifact.status(), CohomologyInterventionStatus::Infeasible);
    assert!(artifact.edits().is_empty());
    assert_eq!(artifact.root_blockers().len(), 2);
    let checked =
        verify_cohomology_intervention(&artifact.encode(limits).unwrap(), ProofLimits::default())
            .unwrap();
    assert_eq!(
        checked.status,
        holos_tda_check::VerifiedCohomologyInterventionStatus::Infeasible
    );
}

#[test]
fn equal_cost_killers_have_a_deterministic_incumbent() {
    let graph = SparseDistanceMatrix::from_triplets(
        4,
        &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)],
    )
    .unwrap();
    let artifact = CohomologyInterventionArtifact::build(
        4,
        1,
        1.0,
        2,
        &[scenario(&graph, 0)],
        &[
            CohomologyInterventionCandidate::new(0, 2, 3),
            CohomologyInterventionCandidate::new(1, 3, 3),
        ],
        1,
        CohomologyInterventionLimits::default(),
    )
    .unwrap();
    assert_eq!(artifact.status(), CohomologyInterventionStatus::Optimal);
    assert_eq!(
        artifact.edits(),
        [CohomologyInterventionCandidate::new(0, 2, 3)]
    );
}

#[test]
fn harmless_candidates_prove_global_infeasibility() {
    let graph = SparseDistanceMatrix::from_triplets(
        6,
        &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)],
    )
    .unwrap();
    let artifact = CohomologyInterventionArtifact::build(
        6,
        1,
        1.0,
        2,
        &[scenario(&graph, 0)],
        &[CohomologyInterventionCandidate::new(4, 5, 1)],
        1,
        CohomologyInterventionLimits::default(),
    )
    .unwrap();
    assert_eq!(artifact.status(), CohomologyInterventionStatus::Infeasible);
    assert_eq!(artifact.oracle_calls(), 2);
    assert!(artifact.root_blockers().is_empty());
}

#[test]
fn bounded_search_returns_a_valid_gap() {
    let graph = two_spheres();
    let limits = CohomologyInterventionLimits::default()
        .with_max_oracle_calls(4)
        .with_max_search_nodes(2);
    let artifact = CohomologyInterventionArtifact::build(
        12,
        2,
        1.0,
        2,
        &[scenario(&graph, 0), scenario(&graph, 1)],
        &[
            CohomologyInterventionCandidate::new(0, 1, 4),
            CohomologyInterventionCandidate::new(2, 3, 5),
            CohomologyInterventionCandidate::new(6, 7, 7),
            CohomologyInterventionCandidate::new(8, 9, 8),
        ],
        2,
        limits,
    )
    .unwrap();
    assert_eq!(
        artifact.status(),
        CohomologyInterventionStatus::SearchIncomplete
    );
    assert!(artifact.lower_bound_cost().is_some());
    assert!(artifact.oracle_calls() <= 4);
}

#[test]
fn mutations_and_truncations_are_rejected() {
    let graph = two_spheres();
    let limits = CohomologyInterventionLimits::default();
    let artifact = CohomologyInterventionArtifact::build(
        12,
        2,
        1.0,
        5,
        &[scenario(&graph, 0)],
        &[CohomologyInterventionCandidate::new(0, 1, 1)],
        1,
        limits,
    )
    .unwrap();
    let bytes = artifact.encode(limits).unwrap();
    for end in 0..bytes.len() {
        assert!(CohomologyInterventionArtifact::decode(&bytes[..end], limits).is_err());
        assert!(verify_cohomology_intervention(&bytes[..end], ProofLimits::default()).is_err());
    }
    let mut changed = bytes;
    changed[40] ^= 1;
    assert!(CohomologyInterventionArtifact::decode(&changed, limits).is_err());
    assert!(verify_cohomology_intervention(&changed, ProofLimits::default()).is_err());
}

#[test]
fn necessary_sets_scale_across_many_network_scenarios() {
    let square_start = 64;
    let mut edges = Vec::new();
    for component in 0..4 {
        let offset = square_start + 4 * component;
        edges.extend([
            (offset, offset + 1, 1.0),
            (offset + 1, offset + 2, 1.0),
            (offset + 2, offset + 3, 1.0),
            (offset, offset + 3, 1.0),
        ]);
    }
    let graph = SparseDistanceMatrix::from_triplets(80, &edges).unwrap();
    let scenarios = (0..4)
        .map(|target| scenario(&graph, target))
        .collect::<Vec<_>>();
    let mut candidates = (1..64)
        .map(|vertex| CohomologyInterventionCandidate::new(0, vertex, 100 + vertex as u64))
        .collect::<Vec<_>>();
    for (component, cost) in [3, 5, 7, 11].into_iter().enumerate() {
        let offset = square_start + 4 * component;
        candidates.push(CohomologyInterventionCandidate::new(
            offset,
            offset + 2,
            cost,
        ));
    }
    candidates.sort_by_key(|candidate| candidate.edge);
    let artifact = CohomologyInterventionArtifact::build(
        80,
        1,
        1.0,
        5,
        &scenarios,
        &candidates,
        4,
        CohomologyInterventionLimits::default(),
    )
    .unwrap();
    assert_eq!(artifact.status(), CohomologyInterventionStatus::Optimal);
    assert_eq!(artifact.edits().len(), 4);
    assert_eq!(artifact.upper_bound_cost(), Some(26));
    assert_eq!(artifact.root_blocker_bound(), 26);
    assert_eq!(artifact.root_blockers().len(), 4);
    assert!(artifact.oracle_calls() < 2_000);
    let exhaustive_subsets = (1..=4)
        .map(|chosen| binomial(candidates.len(), chosen))
        .sum::<usize>();
    assert!(exhaustive_subsets > 800_000);
}

fn binomial(count: usize, chosen: usize) -> usize {
    (0..chosen).fold(1usize, |value, position| {
        value * (count - position) / (position + 1)
    })
}
