use std::cmp::Ordering;

use super::super::wire::{CollapseArtifact, DecodeLimits};
use super::super::{CollapseObjective, CollapsedRips};

/// One schedule in a collapse portfolio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CollapsePortfolioCandidate {
    /// Serial version 1 collapse.
    Serial,
    /// Rounds schedule, algorithm version 2, with a worker budget.
    Rounds {
        /// Worker count for the schedule.
        threads: usize,
    },
    /// Score-ordered version 3 collapse.
    Adaptive {
        /// Downstream clique objective.
        objective: CollapseObjective,
        /// Optional removability-test limit.
        work_limit: Option<u64>,
    },
}

/// Objective used to compare collapsed graphs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CollapsePortfolioObjective {
    /// Minimize the number of surviving edges.
    Edges,
    /// Minimize explicit reduction columns in the highest dimension first.
    ///
    /// Persistence through homology dimension `q` uses simplices through
    /// dimension `q + 1`. The score counts every surviving flag simplex in
    /// dimensions 1 through `q + 1`, then compares those counts from high to
    /// low dimension.
    ReductionColumns {
        /// Highest homology dimension the reduced graph is scored for.
        max_homology_dimension: usize,
    },
}

/// Resource limits for collapse portfolio selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct CollapsePortfolioLimits {
    /// Largest accepted candidate count.
    pub max_candidates: usize,
    /// Largest homology dimension accepted by the score counter.
    pub max_homology_dimension: usize,
    /// Largest nonvertex clique count visited for one candidate.
    pub max_cliques_per_candidate: u64,
}

impl Default for CollapsePortfolioLimits {
    fn default() -> Self {
        Self {
            max_candidates: 16,
            max_homology_dimension: 8,
            max_cliques_per_candidate: 100_000_000,
        }
    }
}

impl CollapsePortfolioLimits {
    /// Set the largest homology dimension accepted by the score counter.
    pub fn with_max_homology_dimension(mut self, maximum: usize) -> Self {
        self.max_homology_dimension = maximum;
        self
    }

    /// Set the largest nonvertex clique count visited for one candidate.
    pub fn with_max_cliques_per_candidate(mut self, maximum: u64) -> Self {
        self.max_cliques_per_candidate = maximum;
        self
    }
}

/// Surviving-simplex score for one candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollapsePortfolioScore {
    pub(super) simplex_counts: Vec<u64>,
}

impl CollapsePortfolioScore {
    /// Counts in simplex dimensions 1, 2, and so on.
    pub fn simplex_counts(&self) -> &[u64] {
        &self.simplex_counts
    }

    pub(super) fn compare(&self, other: &Self) -> Ordering {
        self.simplex_counts
            .iter()
            .rev()
            .cmp(other.simplex_counts.iter().rev())
    }
}

/// One verified candidate and its score.
#[derive(Debug, Clone)]
pub struct CollapsePortfolioEntry {
    pub(super) candidate: CollapsePortfolioCandidate,
    pub(super) score: CollapsePortfolioScore,
    pub(super) result: CollapsedRips,
}

impl CollapsePortfolioEntry {
    /// Schedule that produced this entry.
    pub fn candidate(&self) -> CollapsePortfolioCandidate {
        self.candidate
    }

    /// Score recomputed from the reduced graph.
    pub fn score(&self) -> &CollapsePortfolioScore {
        &self.score
    }

    /// Verified collapsed graph and removal certificate.
    pub fn result(&self) -> &CollapsedRips {
        &self.result
    }
}

/// Result over one declared collapse portfolio.
#[derive(Debug, Clone)]
pub struct CollapsePortfolio {
    pub(super) objective: CollapsePortfolioObjective,
    pub(super) entries: Vec<CollapsePortfolioEntry>,
    pub(super) selected: usize,
}

impl CollapsePortfolio {
    /// Objective used to compare candidates.
    pub fn objective(&self) -> CollapsePortfolioObjective {
        self.objective
    }

    /// Verified candidates in caller order.
    pub fn entries(&self) -> &[CollapsePortfolioEntry] {
        &self.entries
    }

    /// Index of the lexicographic minimum.
    pub fn selected_index(&self) -> usize {
        self.selected
    }

    /// Selected candidate.
    pub fn selected(&self) -> &CollapsePortfolioEntry {
        &self.entries[self.selected]
    }
}

/// Decoder limits for a collapse portfolio artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct CollapsePortfolioDecodeLimits {
    /// Largest accepted outer envelope.
    pub max_bytes: usize,
    /// Largest accepted candidate count.
    pub max_candidates: usize,
    /// Limits for every nested collapse artifact.
    pub collapse: DecodeLimits,
}

impl Default for CollapsePortfolioDecodeLimits {
    fn default() -> Self {
        Self {
            max_bytes: 1 << 30,
            max_candidates: 16,
            collapse: DecodeLimits::default(),
        }
    }
}

/// One candidate carried by a collapse portfolio artifact.
#[derive(Debug, Clone)]
pub struct CollapsePortfolioArtifactEntry {
    pub(super) candidate: CollapsePortfolioCandidate,
    pub(super) score: CollapsePortfolioScore,
    pub(super) artifact: CollapseArtifact,
}

impl CollapsePortfolioArtifactEntry {
    /// Declared schedule.
    pub fn candidate(&self) -> CollapsePortfolioCandidate {
        self.candidate
    }

    /// Declared score.
    pub fn score(&self) -> &CollapsePortfolioScore {
        &self.score
    }

    /// Nested collapse proof.
    pub fn artifact(&self) -> &CollapseArtifact {
        &self.artifact
    }
}

/// Proof of exact selection over a finite collapse portfolio.
#[derive(Debug, Clone)]
pub struct CollapsePortfolioArtifact {
    pub(super) objective: CollapsePortfolioObjective,
    pub(super) entries: Vec<CollapsePortfolioArtifactEntry>,
    pub(super) selected: usize,
    pub(super) digest: [u8; 32],
}
