//! Certified relative coverage: the producer matches flat enumeration, a
//! maximal-failure plan covers each smaller budget, and the checker rejects
//! a mutated or truncated artifact without panicking.

use std::panic::{AssertUnwindSafe, catch_unwind};

use holos_tda::{
    CoverageAction, CoverageFence, CoverageLimits, CoverageSpecification, CoverageState,
    CoverageSynthesisArtifact, CoverageSynthesisLimits, CoverageSynthesisStatus,
    PlanarCoverageModel, SparseDistanceMatrix, compose_coverage_frontiers, evaluate_coverage_plan,
};
use holos_tda_check::{ProofLimits, VerifiedCoverageStatus, verify_coverage};

struct Fixture {
    specification: CoverageSpecification,
    actions: Vec<CoverageAction>,
    max_activations: usize,
}

fn fixture(modulus: u32, failure_budget: usize, state_count: usize) -> Fixture {
    let useful_per_state = failure_budget + 2;
    let width = useful_per_state + 1;
    let vertex_count = 4 + state_count * width;
    let fence_edges = [(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)];
    let mut states = Vec::new();
    let mut actions = Vec::new();
    let mut failable = Vec::new();
    for state_index in 0..state_count {
        let offset = 4 + state_index * width;
        let mut edges = fence_edges.to_vec();
        for center in offset..offset + useful_per_state {
            edges.extend((0..4).map(|fence| (fence, center, 1.0)));
        }
        let graph = SparseDistanceMatrix::from_triplets(vertex_count, &edges).unwrap();
        states.push(
            CoverageState::new(10, state_index as u64, &graph, vec![0, 1, 2, 3], 1.0).unwrap(),
        );
        for local in 0..width {
            let vertex = offset + local;
            let cost = if local < useful_per_state {
                3 + ((local * 5 + state_index * 2) % 11) as u64
            } else {
                1
            };
            actions.push(CoverageAction::new(vertex, cost, vec![state_index]));
            failable.push(vertex);
        }
    }
    let specification = CoverageSpecification::new(
        vertex_count,
        PlanarCoverageModel::new(1.0, 1.0).unwrap(),
        modulus,
        CoverageFence::new(vec![0, 1, 2, 3]).unwrap(),
        failable,
        failure_budget,
        states,
        CoverageLimits::default(),
    )
    .unwrap();
    Fixture {
        specification,
        actions,
        max_activations: state_count * (failure_budget + 1),
    }
}

fn flat_optimum(fixture: &Fixture) -> Option<u64> {
    let mut best = None;
    for mask in 0usize..1usize << fixture.actions.len() {
        if mask.count_ones() as usize > fixture.max_activations {
            continue;
        }
        let selected = (0..fixture.actions.len())
            .filter(|index| mask & (1 << index) != 0)
            .collect::<Vec<_>>();
        if !evaluate_coverage_plan(
            &fixture.specification,
            &fixture.actions,
            &selected,
            CoverageLimits::default(),
        )
        .unwrap()
        .criterion_holds
        {
            continue;
        }
        let cost = selected
            .iter()
            .map(|index| fixture.actions[*index].cost)
            .sum();
        if best.is_none_or(|current| cost < current) {
            best = Some(cost);
        }
    }
    best
}

#[test]
fn producer_frontiers_and_checker_match_flat_enumeration() {
    for (modulus, failure_budget, state_count) in [(2, 0, 1), (3, 1, 2), (5, 2, 1)] {
        let fixture = fixture(modulus, failure_budget, state_count);
        let expected = flat_optimum(&fixture).unwrap();
        let composition = compose_coverage_frontiers(
            &fixture.specification,
            &fixture.actions,
            fixture.max_activations,
            CoverageSynthesisLimits::default(),
        )
        .unwrap();
        assert_eq!(composition.cost(), Some(expected));

        let artifact = CoverageSynthesisArtifact::build(
            fixture.specification,
            fixture.actions,
            fixture.max_activations,
            CoverageSynthesisLimits::default(),
        )
        .unwrap_or_else(|error| {
            panic!("Z/{modulus}, failure budget {failure_budget}, {state_count} states: {error}")
        });
        assert_eq!(artifact.status(), CoverageSynthesisStatus::Optimal);
        assert_eq!(artifact.upper_bound_cost(), Some(expected));
        let checked = verify_coverage(
            &artifact.encode(CoverageSynthesisLimits::default()).unwrap(),
            ProofLimits::default(),
        )
        .unwrap();
        assert_eq!(checked.status, VerifiedCoverageStatus::Optimal);
        assert_eq!(checked.total_cost, Some(expected));
    }
}

#[test]
fn maximal_failure_acceptance_covers_each_smaller_budget() {
    let full = fixture(5, 2, 1);
    let selected = [0, 1, 2];
    assert!(
        evaluate_coverage_plan(
            &full.specification,
            &full.actions,
            &selected,
            CoverageLimits::default(),
        )
        .unwrap()
        .criterion_holds
    );
    for budget in 0..=2 {
        let smaller = fixture(5, budget, 1);
        assert!(
            evaluate_coverage_plan(
                &smaller.specification,
                &smaller.actions,
                &selected[..budget + 1],
                CoverageLimits::default(),
            )
            .unwrap()
            .criterion_holds
        );
    }

    assert!(
        !evaluate_coverage_plan(
            &full.specification,
            &full.actions,
            &selected[..2],
            CoverageLimits::default(),
        )
        .unwrap()
        .criterion_holds
    );
}

#[test]
fn checker_rejects_every_mutation_and_truncation_without_panicking() {
    let fixture = fixture(3, 1, 1);
    let bytes = CoverageSynthesisArtifact::build(
        fixture.specification,
        fixture.actions,
        fixture.max_activations,
        CoverageSynthesisLimits::default(),
    )
    .unwrap()
    .encode(CoverageSynthesisLimits::default())
    .unwrap();

    for position in 0..bytes.len() {
        let mut changed = bytes.clone();
        changed[position] ^= 1;
        let result = catch_unwind(AssertUnwindSafe(|| {
            verify_coverage(&changed, ProofLimits::default())
        }));
        assert!(
            result.is_ok(),
            "checker panicked after changing byte {position}"
        );
        assert!(
            result.unwrap().is_err(),
            "checker accepted changed byte {position}"
        );
    }
    for length in 0..bytes.len() {
        let result = catch_unwind(AssertUnwindSafe(|| {
            verify_coverage(&bytes[..length], ProofLimits::default())
        }));
        assert!(result.is_ok(), "checker panicked at length {length}");
        assert!(result.unwrap().is_err(), "checker accepted length {length}");
    }

    let mut limits = ProofLimits::default();
    limits.max_bytes = bytes.len() - 1;
    assert!(verify_coverage(&bytes, limits).is_err());
}
