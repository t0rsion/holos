use std::cmp::Ordering;

use super::selection::{count_flag_simplices, validate_request};
use super::*;
use crate::SparseDistanceMatrix;
use crate::collapse::CollapseObjective;

fn graph() -> SparseDistanceMatrix {
    SparseDistanceMatrix::from_triplets(
        6,
        &[
            (0, 1, 1.0),
            (0, 2, 1.0),
            (0, 3, 1.0),
            (1, 2, 1.0),
            (1, 3, 1.0),
            (2, 3, 1.0),
            (3, 4, 1.0),
            (3, 5, 1.0),
            (4, 5, 1.0),
        ],
    )
    .unwrap()
}

#[test]
fn clique_counter_counts_edges_triangles_and_tetrahedra() {
    let score = count_flag_simplices(&graph(), 3, CollapsePortfolioLimits::default()).unwrap();
    assert_eq!(score.simplex_counts(), &[9, 5, 1]);
}

#[test]
fn portfolio_selects_the_exact_declared_minimum() {
    let input = graph();
    let candidates = [
        CollapsePortfolioCandidate::Serial,
        CollapsePortfolioCandidate::Rounds { threads: 2 },
        CollapsePortfolioCandidate::Adaptive {
            objective: CollapseObjective::H2,
            work_limit: None,
        },
    ];
    let portfolio = collapse_sparse_portfolio(
        &input,
        None,
        &candidates,
        CollapsePortfolioObjective::ReductionColumns {
            max_homology_dimension: 2,
        },
        CollapsePortfolioLimits::default(),
    )
    .unwrap();
    portfolio
        .verify(&input, None, CollapsePortfolioLimits::default())
        .unwrap();
    let selected = portfolio.selected().score();
    assert!(
        portfolio
            .entries()
            .iter()
            .all(|entry| selected.compare(entry.score()) != Ordering::Greater)
    );
}

#[test]
fn portfolio_artifact_round_trips_and_rechecks_every_candidate() {
    let input = graph();
    let candidates = [
        CollapsePortfolioCandidate::Serial,
        CollapsePortfolioCandidate::Rounds { threads: 2 },
        CollapsePortfolioCandidate::Adaptive {
            objective: CollapseObjective::H1,
            work_limit: Some(100),
        },
    ];
    let objective = CollapsePortfolioObjective::ReductionColumns {
        max_homology_dimension: 2,
    };
    let portfolio = collapse_sparse_portfolio(
        &input,
        None,
        &candidates,
        objective,
        CollapsePortfolioLimits::default(),
    )
    .unwrap();
    let artifact =
        CollapsePortfolioArtifact::from_portfolio(&portfolio, CollapsePortfolioLimits::default())
            .unwrap();
    let bytes = artifact
        .encode(
            CollapsePortfolioLimits::default(),
            CollapsePortfolioDecodeLimits::default(),
        )
        .unwrap();
    let decoded = CollapsePortfolioArtifact::decode(
        &bytes,
        CollapsePortfolioLimits::default(),
        CollapsePortfolioDecodeLimits::default(),
    )
    .unwrap();
    decoded
        .verify_sparse(&input, None, CollapsePortfolioLimits::default())
        .unwrap();
    assert_eq!(decoded.objective(), objective);
    assert_eq!(decoded.selected_index(), portfolio.selected_index());
    assert_eq!(decoded.entries().len(), candidates.len());
}

#[test]
fn portfolio_artifact_rejects_mutation_and_a_false_selection() {
    let input = graph();
    let portfolio = collapse_sparse_portfolio(
        &input,
        None,
        &[
            CollapsePortfolioCandidate::Serial,
            CollapsePortfolioCandidate::Rounds { threads: 2 },
        ],
        CollapsePortfolioObjective::Edges,
        CollapsePortfolioLimits::default(),
    )
    .unwrap();
    let mut artifact =
        CollapsePortfolioArtifact::from_portfolio(&portfolio, CollapsePortfolioLimits::default())
            .unwrap();
    let mut bytes = artifact
        .encode(
            CollapsePortfolioLimits::default(),
            CollapsePortfolioDecodeLimits::default(),
        )
        .unwrap();
    bytes[12] ^= 1;
    assert!(
        CollapsePortfolioArtifact::decode(
            &bytes,
            CollapsePortfolioLimits::default(),
            CollapsePortfolioDecodeLimits::default(),
        )
        .is_err()
    );

    artifact.selected = (artifact.selected + 1) % artifact.entries.len();
    artifact.digest = artifact.compute_digest().unwrap();
    assert!(
        artifact
            .verify_sparse(&input, None, CollapsePortfolioLimits::default())
            .is_err()
    );
}

#[test]
fn portfolio_rejects_duplicates_and_zero_workers() {
    let limits = CollapsePortfolioLimits::default();
    assert!(
        validate_request(
            &[
                CollapsePortfolioCandidate::Serial,
                CollapsePortfolioCandidate::Serial,
            ],
            CollapsePortfolioObjective::Edges,
            limits,
        )
        .is_err()
    );
    assert!(
        validate_request(
            &[CollapsePortfolioCandidate::Rounds { threads: 0 }],
            CollapsePortfolioObjective::Edges,
            limits,
        )
        .is_err()
    );
}

#[test]
fn clique_limit_stops_before_unbounded_materialization() {
    let limits = CollapsePortfolioLimits {
        max_cliques_per_candidate: 2,
        ..CollapsePortfolioLimits::default()
    };
    assert!(count_flag_simplices(&graph(), 3, limits).is_err());
}
