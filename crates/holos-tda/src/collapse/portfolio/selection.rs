use std::cmp::Ordering;

use super::super::verify::verify_sparse;
use super::super::{
    AdaptiveCollapseParams, CollapseCertificate, CollapsedRips, collapse_sparse,
    collapse_sparse_adaptive, collapse_sparse_rounds_parallel,
};
use super::model::{
    CollapsePortfolio, CollapsePortfolioCandidate, CollapsePortfolioEntry, CollapsePortfolioLimits,
    CollapsePortfolioObjective, CollapsePortfolioScore,
};
use crate::{Error, Result, SparseDistanceMatrix};

impl CollapsePortfolio {
    /// Recheck every certificate, score, and the final selection.
    pub fn verify(
        &self,
        input: &SparseDistanceMatrix,
        threshold: Option<f64>,
        limits: CollapsePortfolioLimits,
    ) -> Result<()> {
        validate_request(
            &self
                .entries
                .iter()
                .map(|entry| entry.candidate)
                .collect::<Vec<_>>(),
            self.objective,
            limits,
        )?;
        if self.selected >= self.entries.len() {
            return Err(portfolio_error(
                "selected candidate is outside the portfolio",
            ));
        }
        for entry in &self.entries {
            verify_entry(input, threshold, self.objective, entry, limits)?;
        }
        let selected = select_entry(&self.entries);
        if selected != self.selected {
            return Err(portfolio_error(
                "selected candidate is not the exact minimum",
            ));
        }
        Ok(())
    }
}

/// Run and verify a portfolio of sparse collapse schedules.
///
/// Ties keep the first candidate in caller order. Candidate order is part of
/// the declared finite optimization problem.
pub fn collapse_sparse_portfolio(
    input: &SparseDistanceMatrix,
    threshold: Option<f64>,
    candidates: &[CollapsePortfolioCandidate],
    objective: CollapsePortfolioObjective,
    limits: CollapsePortfolioLimits,
) -> Result<CollapsePortfolio> {
    validate_request(candidates, objective, limits)?;
    let mut entries = Vec::with_capacity(candidates.len());
    for &candidate in candidates {
        let result = run_candidate(input, threshold, candidate)?;
        verify_sparse(input, threshold, &result)
            .map_err(|error| portfolio_error(format!("candidate certificate failed: {error}")))?;
        let score = score_graph(&result.matrix, objective, limits)?;
        entries.push(CollapsePortfolioEntry {
            candidate,
            score,
            result,
        });
    }
    let selected = select_entry(&entries);
    Ok(CollapsePortfolio {
        objective,
        entries,
        selected,
    })
}

pub(super) fn validate_request(
    candidates: &[CollapsePortfolioCandidate],
    objective: CollapsePortfolioObjective,
    limits: CollapsePortfolioLimits,
) -> Result<()> {
    if candidates.is_empty() || candidates.len() > limits.max_candidates {
        return Err(portfolio_error(
            "candidate count is zero or exceeds the portfolio limit",
        ));
    }
    validate_objective(objective, limits)?;
    for (index, candidate) in candidates.iter().enumerate() {
        validate_candidate(*candidate)?;
        if candidates[..index].contains(candidate) {
            return Err(portfolio_error("portfolio contains a duplicate candidate"));
        }
    }
    Ok(())
}

fn validate_objective(
    objective: CollapsePortfolioObjective,
    limits: CollapsePortfolioLimits,
) -> Result<()> {
    let CollapsePortfolioObjective::ReductionColumns {
        max_homology_dimension,
    } = objective
    else {
        return Ok(());
    };
    if max_homology_dimension > limits.max_homology_dimension {
        return Err(portfolio_error(
            "portfolio homology dimension exceeds its limit",
        ));
    }
    Ok(())
}

fn validate_candidate(candidate: CollapsePortfolioCandidate) -> Result<()> {
    if let CollapsePortfolioCandidate::Rounds { threads: 0 } = candidate {
        return Err(portfolio_error(
            "rounds candidate needs at least one worker",
        ));
    }
    Ok(())
}

fn run_candidate(
    input: &SparseDistanceMatrix,
    threshold: Option<f64>,
    candidate: CollapsePortfolioCandidate,
) -> Result<CollapsedRips> {
    match candidate {
        CollapsePortfolioCandidate::Serial => collapse_sparse(input, threshold),
        CollapsePortfolioCandidate::Rounds { threads } => {
            collapse_sparse_rounds_parallel(input, threshold, threads)
        }
        CollapsePortfolioCandidate::Adaptive {
            objective,
            work_limit,
        } => {
            let mut params = AdaptiveCollapseParams::new(objective);
            params.work_limit = work_limit;
            collapse_sparse_adaptive(input, threshold, params)
        }
    }
}

fn verify_entry(
    input: &SparseDistanceMatrix,
    threshold: Option<f64>,
    objective: CollapsePortfolioObjective,
    entry: &CollapsePortfolioEntry,
    limits: CollapsePortfolioLimits,
) -> Result<()> {
    validate_certificate_profile(entry.candidate, &entry.result)?;
    verify_sparse(input, threshold, &entry.result)
        .map_err(|error| portfolio_error(format!("candidate certificate failed: {error}")))?;
    let expected = score_graph(&entry.result.matrix, objective, limits)?;
    if expected != entry.score {
        return Err(portfolio_error("candidate score does not match its graph"));
    }
    Ok(())
}

fn validate_certificate_profile(
    candidate: CollapsePortfolioCandidate,
    result: &CollapsedRips,
) -> Result<()> {
    validate_profile(candidate, &result.certificate)
}

pub(super) fn validate_profile(
    candidate: CollapsePortfolioCandidate,
    certificate: &CollapseCertificate,
) -> Result<()> {
    let matches = match candidate {
        CollapsePortfolioCandidate::Serial => {
            certificate.algorithm_version() == 1 && certificate.objective().is_none()
        }
        CollapsePortfolioCandidate::Rounds { .. } => {
            certificate.algorithm_version() == 2 && certificate.objective().is_none()
        }
        CollapsePortfolioCandidate::Adaptive {
            objective,
            work_limit,
        } => {
            certificate.algorithm_version() == 3
                && certificate.objective() == Some(objective)
                && certificate.work_limit() == work_limit
        }
    };
    if !matches {
        return Err(portfolio_error(
            "candidate metadata does not match its certificate",
        ));
    }
    Ok(())
}

pub(super) fn score_graph(
    graph: &SparseDistanceMatrix,
    objective: CollapsePortfolioObjective,
    limits: CollapsePortfolioLimits,
) -> Result<CollapsePortfolioScore> {
    match objective {
        CollapsePortfolioObjective::Edges => Ok(CollapsePortfolioScore {
            simplex_counts: vec![graph.num_edges() as u64],
        }),
        CollapsePortfolioObjective::ReductionColumns {
            max_homology_dimension,
        } => count_flag_simplices(graph, max_homology_dimension + 1, limits),
    }
}

pub(super) fn count_flag_simplices(
    graph: &SparseDistanceMatrix,
    max_simplex_dimension: usize,
    limits: CollapsePortfolioLimits,
) -> Result<CollapsePortfolioScore> {
    let adjacency = adjacency(graph);
    let vertices = (0..graph.len()).collect::<Vec<_>>();
    let mut counter = CliqueCounter {
        adjacency: &adjacency,
        counts: vec![0; max_simplex_dimension],
        maximum_size: max_simplex_dimension + 1,
        visited: 0,
        limit: limits.max_cliques_per_candidate,
    };
    counter.extend(0, &vertices)?;
    Ok(CollapsePortfolioScore {
        simplex_counts: counter.counts,
    })
}

fn adjacency(graph: &SparseDistanceMatrix) -> Vec<Vec<usize>> {
    let mut adjacency = vec![Vec::new(); graph.len()];
    for (u, v, _) in graph.edges() {
        adjacency[u].push(v);
        adjacency[v].push(u);
    }
    adjacency
}

struct CliqueCounter<'a> {
    adjacency: &'a [Vec<usize>],
    counts: Vec<u64>,
    maximum_size: usize,
    visited: u64,
    limit: u64,
}

impl CliqueCounter<'_> {
    fn extend(&mut self, prefix_size: usize, candidates: &[usize]) -> Result<()> {
        for (position, &vertex) in candidates.iter().enumerate() {
            let size = prefix_size + 1;
            if size >= 2 {
                self.count(size)?;
            }
            if size < self.maximum_size {
                let next = intersect(&candidates[position + 1..], &self.adjacency[vertex]);
                self.extend(size, &next)?;
            }
        }
        Ok(())
    }

    fn count(&mut self, clique_size: usize) -> Result<()> {
        self.visited = self
            .visited
            .checked_add(1)
            .ok_or_else(|| portfolio_error("portfolio clique count overflows"))?;
        if self.visited > self.limit {
            return Err(portfolio_error(
                "portfolio clique count exceeds its candidate limit",
            ));
        }
        let count = &mut self.counts[clique_size - 2];
        *count = count
            .checked_add(1)
            .ok_or_else(|| portfolio_error("portfolio simplex count overflows"))?;
        Ok(())
    }
}

fn intersect(left: &[usize], right: &[usize]) -> Vec<usize> {
    let mut output = Vec::new();
    let mut left_index = 0;
    let mut right_index = 0;
    while left_index < left.len() && right_index < right.len() {
        match left[left_index].cmp(&right[right_index]) {
            Ordering::Less => left_index += 1,
            Ordering::Greater => right_index += 1,
            Ordering::Equal => {
                output.push(left[left_index]);
                left_index += 1;
                right_index += 1;
            }
        }
    }
    output
}

pub(super) fn select_entry(entries: &[CollapsePortfolioEntry]) -> usize {
    let mut selected = 0;
    for index in 1..entries.len() {
        if entries[index].score.compare(&entries[selected].score) == Ordering::Less {
            selected = index;
        }
    }
    selected
}

pub(super) fn portfolio_error(message: impl Into<String>) -> Error {
    Error::InvalidInput(format!("collapse portfolio: {}", message.into()))
}
