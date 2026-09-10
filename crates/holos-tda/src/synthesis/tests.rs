use sha2::{Digest, Sha256};

use crate::{CohomologyLimits, KineticFiltration, SparseDistanceMatrix, cohomology_space};

use super::*;
use holos_tda_check::{ProofLimits, VerifiedSynthesisSource, verify_synthesis};

fn two_cycles() -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(
        8,
        &[
            (0, 1, 1.0),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (0, 3, 1.0),
            (4, 5, 1.0),
            (5, 6, 1.0),
            (6, 7, 1.0),
            (4, 7, 1.0),
        ],
    )
    .unwrap()
}

fn state(
    graph: &SparseDistanceMatrix,
    scenario: u64,
    step: u64,
    target_basis: usize,
) -> SynthesisState {
    let space = cohomology_space(graph, 1, 1.0, 3, CohomologyLimits::default()).unwrap();
    let target = space
        .subspace_from_coordinates(&[vec![(target_basis, 1)]])
        .unwrap();
    SynthesisState::from_subspace(scenario, step, graph, 1.0, &space, &target, 0).unwrap()
}

fn problem() -> (TopologicalSpecification, Vec<SynthesisAction>) {
    let graph = two_cycles();
    let specification = TopologicalSpecification::new(
        8,
        1,
        1.0,
        3,
        vec![state(&graph, 0, 0, 0), state(&graph, 1, 0, 1)],
    );
    let mut actions = vec![
        SynthesisAction::new(0, 2, 4, vec![0]),
        SynthesisAction::new(1, 3, 7, vec![0]),
        SynthesisAction::new(4, 6, 5, vec![1]),
        SynthesisAction::new(5, 7, 9, vec![1]),
    ];
    actions.sort();
    (specification, actions)
}

#[test]
fn temporal_subspace_plan_has_a_checked_optimality_tree() {
    let (specification, actions) = problem();
    let limits = SynthesisLimits::default();
    let artifact = SynthesisArtifact::build(specification, actions, 2, limits).unwrap();
    assert_eq!(artifact.status(), SynthesisStatus::Optimal);
    assert_eq!(artifact.upper_bound_cost(), Some(9));
    assert_eq!(artifact.lower_bound_cost(), Some(9));
    assert_eq!(artifact.before_ranks(), [1, 1]);
    assert_eq!(artifact.after_ranks(), [0, 0]);
    assert!(artifact.proof_nodes() > 0);
    assert!(artifact.proof_topology_checks() > 0);
    let bytes = artifact.encode(limits).unwrap();
    let decoded = SynthesisArtifact::decode(&bytes, limits).unwrap();
    assert_eq!(decoded, artifact);
    let checked = verify_synthesis(&bytes, ProofLimits::default()).unwrap();
    assert_eq!(
        checked.status,
        holos_tda_check::VerifiedSynthesisStatus::Optimal
    );
    assert_eq!(checked.total_cost, Some(9));
    assert_eq!(
        checked.proof_topology_checks,
        artifact.proof_topology_checks()
    );
    assert!(checked.proof_topology_checks < artifact.producer_oracle_calls());
}

#[test]
fn edit_bound_has_a_complete_infeasibility_proof() {
    let (specification, actions) = problem();
    let limits = SynthesisLimits::default();
    let artifact = SynthesisArtifact::build(specification, actions, 1, limits).unwrap();
    assert_eq!(artifact.status(), SynthesisStatus::Infeasible);
    assert!(artifact.upper_bound_cost().is_none());
    artifact.verify(limits).unwrap();
    verify_synthesis(&artifact.encode(limits).unwrap(), ProofLimits::default()).unwrap();
}

#[test]
fn bounded_search_keeps_only_a_checked_gap() {
    let (specification, actions) = problem();
    let limits = SynthesisLimits::default()
        .with_max_oracle_calls(4)
        .with_max_search_nodes(2);
    let artifact = SynthesisArtifact::build(specification, actions, 2, limits).unwrap();
    assert_eq!(artifact.status(), SynthesisStatus::SearchIncomplete);
    assert_eq!(artifact.proof_nodes(), 0);
    artifact.verify(limits).unwrap();
}

#[test]
fn mutations_and_truncations_are_rejected() {
    let (specification, actions) = problem();
    let limits = SynthesisLimits::default();
    let artifact = SynthesisArtifact::build(specification, actions, 2, limits).unwrap();
    let bytes = artifact.encode(limits).unwrap();
    for end in 0..bytes.len() {
        assert!(SynthesisArtifact::decode(&bytes[..end], limits).is_err());
        assert!(verify_synthesis(&bytes[..end], ProofLimits::default()).is_err());
    }
    let mut changed = bytes;
    changed[48] ^= 1;
    assert!(SynthesisArtifact::decode(&changed, limits).is_err());
}

#[test]
fn kinetic_compiler_covers_endpoints_events_and_open_cells() {
    let filtration = KineticFiltration::new(
        4,
        vec![
            crate::KineticEdge {
                u: 0,
                v: 1,
                intercept: 1.0,
                velocity: 0.0,
            },
            crate::KineticEdge {
                u: 1,
                v: 2,
                intercept: 1.0,
                velocity: 0.0,
            },
            crate::KineticEdge {
                u: 2,
                v: 3,
                intercept: 1.0,
                velocity: 0.0,
            },
            crate::KineticEdge {
                u: 0,
                v: 3,
                intercept: 1.0,
                velocity: 0.0,
            },
            crate::KineticEdge {
                u: 0,
                v: 2,
                intercept: 2.0,
                velocity: -1.0,
            },
        ],
        0.0,
        1.5,
        crate::KineticLimits::default(),
    )
    .unwrap();
    let critical = filtration.critical_graphs(1.0).unwrap();
    assert!(matches!(
        critical[0].kind,
        crate::KineticGraphStateKind::Start
    ));
    assert!(matches!(
        critical.last().unwrap().kind,
        crate::KineticGraphStateKind::End
    ));
    assert!(
        critical
            .iter()
            .any(|state| matches!(state.kind, crate::KineticGraphStateKind::Event(_)))
    );
    let specification = TopologicalSpecification::from_kinetic_rank_ceiling(
        &filtration,
        7,
        1,
        1.0,
        3,
        0,
        CohomologyLimits::default(),
    )
    .unwrap();
    assert!(!specification.states().is_empty());
    let action = SynthesisAction::throughout(1, 3, 2, &specification);
    let artifact =
        SynthesisArtifact::build(specification, vec![action], 1, SynthesisLimits::default())
            .unwrap();
    assert_eq!(artifact.status(), SynthesisStatus::Optimal);
    assert_eq!(artifact.upper_bound_cost(), Some(2));
    let limits = SynthesisLimits::default();
    let mut bytes = artifact.encode(limits).unwrap();
    let checked = verify_synthesis(&bytes, ProofLimits::default()).unwrap();
    assert_eq!(checked.source, VerifiedSynthesisSource::Affine);

    let velocity = (-1.0f64).to_bits().to_be_bytes();
    let position = bytes
        .windows(velocity.len())
        .position(|window| window == velocity)
        .unwrap();
    bytes[position..position + velocity.len()].copy_from_slice(&0.0f64.to_bits().to_be_bytes());
    let payload_end = bytes.len() - 32;
    let mut hash = Sha256::new();
    hash.update(b"holos-synthesis-artifact-v1");
    hash.update(&bytes[..payload_end]);
    let digest: [u8; 32] = hash.finalize().into();
    bytes[payload_end..].copy_from_slice(&digest);
    assert!(SynthesisArtifact::decode(&bytes, limits).is_err());
    assert!(verify_synthesis(&bytes, ProofLimits::default()).is_err());
}

#[test]
fn incidence_components_partition_states_and_actions() {
    let (specification, actions) = problem();
    let components = specification.components(&actions).unwrap();
    assert_eq!(components.len(), 2);
    assert_eq!(components[0].states(), [0]);
    assert_eq!(components[0].actions(), [0, 1]);
    assert_eq!(components[1].states(), [1]);
    assert_eq!(components[1].actions(), [2, 3]);

    let bridge = SynthesisAction::new(0, 6, 20, vec![0, 1]);
    let mut connected = actions;
    connected.push(bridge);
    connected.sort();
    assert_eq!(specification.components(&connected).unwrap().len(), 1);
}

#[test]
fn overlapping_obligations_require_a_recursive_optimality_proof() {
    let graph = SparseDistanceMatrix::from_triplets(
        4,
        &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)],
    )
    .unwrap();
    let specification = TopologicalSpecification::new(
        4,
        1,
        1.0,
        3,
        (0..3).map(|step| state(&graph, 0, step, 0)).collect(),
    );
    let mut actions = vec![
        SynthesisAction::new(0, 2, 1, vec![0, 1]),
        SynthesisAction::new(0, 2, 1, vec![0, 2]),
        SynthesisAction::new(0, 2, 1, vec![1, 2]),
    ];
    actions.sort();
    let limits = SynthesisLimits::default();
    let artifact = SynthesisArtifact::build(specification, actions, 2, limits).unwrap();
    assert_eq!(artifact.status(), SynthesisStatus::Optimal);
    assert_eq!(artifact.upper_bound_cost(), Some(2));
    assert_eq!(artifact.lower_bound_cost(), Some(2));
    assert_eq!(artifact.selected().len(), 2);
    assert!(artifact.proof_nodes() > 1);
    assert!(artifact.proof_topology_checks() > 1);
    verify_synthesis(&artifact.encode(limits).unwrap(), ProofLimits::default()).unwrap();
}

#[test]
fn empty_specification_has_a_zero_cost_proof() {
    let specification = TopologicalSpecification::new(4, 1, 1.0, 3, Vec::new());
    let limits = SynthesisLimits::default();
    let artifact = SynthesisArtifact::build(specification, Vec::new(), 5, limits).unwrap();
    assert_eq!(artifact.status(), SynthesisStatus::Optimal);
    assert_eq!(artifact.upper_bound_cost(), Some(0));
    assert_eq!(artifact.lower_bound_cost(), Some(0));
    assert!(artifact.selected().is_empty());
    verify_synthesis(&artifact.encode(limits).unwrap(), ProofLimits::default()).unwrap();
}
