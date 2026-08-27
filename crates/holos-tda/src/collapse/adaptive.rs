//! Deterministic score-ordered collapse schedule.

use super::*;
use std::cmp::Ordering;

/// Score of one removable edge in the current graph.
///
/// The heap compares `primary`, then `secondary`. The raw clique counts
/// travel with the entry so the run records what each selected removal
/// destroyed without recomputing the score.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Score {
    primary: u64,
    secondary: u64,
    triangles: u64,
    tetrahedra: u64,
}

/// A removable edge planned for one pass. Smaller indices win score ties.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Candidate {
    score: Score,
    index: usize,
}

impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score
            .primary
            .cmp(&other.score.primary)
            .then(self.score.secondary.cmp(&other.score.secondary))
            .then_with(|| other.index.cmp(&self.index))
    }
}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Count the edges in the graph induced by `cands`.
///
/// A triangle containing the target edge corresponds to one candidate. An
/// edge between two candidates extends it to a tetrahedron. Candidate
/// vertices and adjacency rows are sorted, so each induced edge is counted
/// once by a forward merge.
fn tetrahedra(adj: &[Vec<AdjEntry>], cands: &[(usize, f64)]) -> u64 {
    let mut total = 0u64;
    for (p, &(x, _)) in cands.iter().enumerate() {
        let list = &adj[x];
        let mut i = 0usize;
        let mut q = p + 1;
        while i < list.len() && q < cands.len() {
            let (y, d, _) = list[i];
            match y.cmp(&cands[q].0) {
                Ordering::Less => i += 1,
                Ordering::Greater => q += 1,
                Ordering::Equal => {
                    if d.is_finite() {
                        total = total.saturating_add(1);
                    }
                    i += 1;
                    q += 1;
                }
            }
        }
    }
    total
}

fn score(adj: &[Vec<AdjEntry>], cands: &[(usize, f64)], objective: CollapseObjective) -> Score {
    let triangles = cands.len() as u64;
    let tetrahedra = match objective {
        CollapseObjective::H1 => 0,
        CollapseObjective::H2 => tetrahedra(adj, cands),
    };
    let (primary, secondary) = match objective {
        CollapseObjective::H1 => (triangles, 0),
        CollapseObjective::H2 => (tetrahedra, triangles),
    };
    Score {
        primary,
        secondary,
        triangles,
        tetrahedra,
    }
}

/// Collapse a dense distance matrix with the adaptive version 3 schedule.
///
/// Each pass scores the live removable edges against one graph, then tests
/// them again in score order as the graph changes. With a work limit, the
/// output can be a safe partial collapse. Read
/// [`CollapseCertificate::completeness`] before claiming a fixed point.
pub fn collapse_dense_adaptive(
    dist: &DistanceMatrix,
    threshold: Option<f64>,
    params: AdaptiveCollapseParams,
) -> Result<CollapsedRips> {
    collapse_adaptive_in(dist, threshold, params)
}

/// Collapse a sparse distance matrix with the adaptive version 3 schedule.
///
/// See [`collapse_dense_adaptive`] for the schedule and stopping contract.
pub fn collapse_sparse_adaptive(
    dist: &SparseDistanceMatrix,
    threshold: Option<f64>,
    params: AdaptiveCollapseParams,
) -> Result<CollapsedRips> {
    collapse_adaptive_in(dist, threshold, params)
}

/// Adaptive collapse shared by the standalone entry points and pipeline.
pub(crate) fn collapse_adaptive_in<D: Distances>(
    dist: &D,
    threshold: Option<f64>,
    params: AdaptiveCollapseParams,
) -> Result<CollapsedRips> {
    let Prepared {
        mut edges,
        mut adj,
        run,
    } = prepare(dist, threshold)?;

    let mut stats = CollapseStats::new(edges.len());
    let mut steps = Vec::new();
    let mut scratch = Scratch::default();
    let mut work_used = 0u64;
    let mut budget_limited = false;
    let mut passes = 0usize;

    'schedule: loop {
        passes += 1;
        let mut candidates = Vec::new();
        for (index, edge) in edges.iter().enumerate() {
            if !edge.alive {
                continue;
            }
            if params.work_limit.is_some_and(|limit| work_used >= limit) {
                budget_limited = true;
                break 'schedule;
            }

            work_used += 1;
            stats.edge_tests += 1;
            let witnesses = test_edge(&adj, edge.u, edge.v, edge.value, run.terminal, &mut scratch);
            stats.max_common_neighborhood = stats.max_common_neighborhood.max(scratch.cands.len());
            if witnesses.is_some() {
                stats.adaptive_score_evaluations += 1;
                candidates.push(Candidate {
                    score: score(&adj, &scratch.cands, params.objective),
                    index,
                });
            }
        }

        if candidates.is_empty() {
            break;
        }
        candidates.sort_unstable_by(|a, b| b.cmp(a));
        for candidate in candidates {
            stats.adaptive_queue_pops += 1;
            if !edges[candidate.index].alive {
                stats.adaptive_stale_pops += 1;
                continue;
            }
            if params.work_limit.is_some_and(|limit| work_used >= limit) {
                budget_limited = true;
                break 'schedule;
            }

            work_used += 1;
            stats.edge_tests += 1;
            let index = candidate.index;
            let (u, v, value) = (edges[index].u, edges[index].v, edges[index].value);
            let Some(witnesses) = test_edge(&adj, u, v, value, run.terminal, &mut scratch) else {
                stats.adaptive_stale_pops += 1;
                continue;
            };
            stats.max_common_neighborhood = stats.max_common_neighborhood.max(scratch.cands.len());
            stats.adaptive_score_evaluations += 1;
            let selected_score = score(&adj, &scratch.cands, params.objective);
            edges[index].alive = false;
            tombstone(&mut adj, u, v);
            stats.witness_segments += witnesses.len();
            stats.adaptive_triangles_removed = stats
                .adaptive_triangles_removed
                .saturating_add(selected_score.triangles);
            stats.adaptive_tetrahedra_removed = stats
                .adaptive_tetrahedra_removed
                .saturating_add(selected_score.tetrahedra);
            steps.push(RemovalStep {
                u,
                v,
                value,
                position: SchedulePosition::Sequence(steps.len() + 1),
                witnesses,
            });
        }
    }

    let completeness = if budget_limited {
        CollapseCompleteness::BudgetLimited
    } else {
        CollapseCompleteness::CompleteFixedPoint
    };
    stats.epochs = passes;
    stats.logical_tests = stats.edge_tests;
    finish(
        run,
        Execution::Adaptive {
            objective: params.objective,
            completeness,
            work_limit: params.work_limit,
            work_used,
        },
        &edges,
        steps,
        stats,
        CollapseTimings::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collapse::verify::{verify_dense, verify_sparse};

    fn k4() -> DistanceMatrix {
        DistanceMatrix::from_condensed(vec![1.0; 6]).unwrap()
    }

    #[test]
    fn complete_k4_is_version_three_and_verifies() {
        for objective in [CollapseObjective::H1, CollapseObjective::H2] {
            let result = collapse_dense_adaptive(
                &k4(),
                None,
                AdaptiveCollapseParams {
                    objective,
                    work_limit: None,
                },
            )
            .unwrap();
            assert_eq!(result.certificate.algorithm_version(), 3);
            assert_eq!(result.certificate.objective(), Some(objective));
            assert_eq!(
                result.certificate.completeness(),
                CollapseCompleteness::CompleteFixedPoint
            );
            assert_eq!(result.certificate.work_limit(), None);
            assert_eq!(
                result.certificate.work_used(),
                result.stats.edge_tests as u64
            );
            assert_eq!(result.matrix.num_edges(), 3);
            verify_dense(&k4(), None, &result).unwrap();
        }
    }

    #[test]
    fn zero_budget_returns_the_input_as_a_safe_partial_collapse() {
        let result = collapse_dense_adaptive(
            &k4(),
            None,
            AdaptiveCollapseParams {
                objective: CollapseObjective::H2,
                work_limit: Some(0),
            },
        )
        .unwrap();
        assert_eq!(
            result.certificate.completeness(),
            CollapseCompleteness::BudgetLimited
        );
        assert_eq!(result.certificate.work_used(), 0);
        assert!(result.certificate.steps().is_empty());
        assert_eq!(result.matrix.num_edges(), 6);
        verify_dense(&k4(), None, &result).unwrap();
    }

    #[test]
    fn dense_and_sparse_runs_are_identical() {
        let dense = k4();
        let sparse = SparseDistanceMatrix::from_triplets(
            4,
            &[
                (0, 1, 1.0),
                (0, 2, 1.0),
                (0, 3, 1.0),
                (1, 2, 1.0),
                (1, 3, 1.0),
                (2, 3, 1.0),
            ],
        )
        .unwrap();
        let params = AdaptiveCollapseParams::default();
        let a = collapse_dense_adaptive(&dense, None, params).unwrap();
        let b = collapse_sparse_adaptive(&sparse, None, params).unwrap();
        assert_eq!(
            a.matrix.edges().collect::<Vec<_>>(),
            b.matrix.edges().collect::<Vec<_>>()
        );
        assert_eq!(a.certificate, b.certificate);
        assert_eq!(a.stats, b.stats);
        verify_sparse(&sparse, None, &b).unwrap();
    }
}
