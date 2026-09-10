//! Exact component frontiers for failure-tolerant coverage synthesis.
//!
//! [`crate::CoverageSpecification::components`] splits the state-action
//! incidence graph. Each local search returns nondominated activation-cost
//! plans. [`compose_coverage_frontiers`] combines them under `max_activations`.

use std::fmt;

use crate::CoverageComponent;

mod compose;

#[cfg(test)]
mod tests;

pub use compose::compose_coverage_frontiers;

/// Completeness status of a compositional coverage calculation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverageCompositionStatus {
    /// Every component frontier is complete. The returned plan is optimal.
    Optimal,
    /// Every component frontier is complete. No plan meets the activation limit.
    Infeasible,
    /// A producer work limit stopped at least one component search.
    SearchIncomplete,
}

impl fmt::Display for CoverageCompositionStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Optimal => "optimal",
            Self::Infeasible => "infeasible",
            Self::SearchIncomplete => "search incomplete",
        })
    }
}

/// One nondominated local plan on a component frontier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageFrontierEntry {
    activations: usize,
    cost: u64,
    selected: Vec<usize>,
}

impl CoverageFrontierEntry {
    /// Number of selected actions in this local plan.
    pub fn activations(&self) -> usize {
        self.activations
    }

    /// Total action cost of this local plan.
    pub fn cost(&self) -> u64 {
        self.cost
    }

    /// Selected global action indices in canonical order.
    pub fn selected(&self) -> &[usize] {
        &self.selected
    }
}

/// Nondominated plan frontier for one incidence component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageComponentFrontier {
    component: CoverageComponent,
    entries: Vec<CoverageFrontierEntry>,
}

impl CoverageComponentFrontier {
    /// State-action incidence component represented by this frontier.
    pub fn component(&self) -> &CoverageComponent {
        &self.component
    }

    /// Nondominated local plans ordered by activation count.
    pub fn entries(&self) -> &[CoverageFrontierEntry] {
        &self.entries
    }
}

/// Result of exact frontier construction and dynamic-program composition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageComposition {
    status: CoverageCompositionStatus,
    frontiers: Vec<CoverageComponentFrontier>,
    selected: Vec<usize>,
    cost: Option<u64>,
    oracle_calls: usize,
    search_nodes: usize,
    cache_hits: usize,
}

impl CoverageComposition {
    /// Completeness status of the composed calculation.
    pub fn status(&self) -> CoverageCompositionStatus {
        self.status
    }

    /// Component frontiers completed before the reported status.
    pub fn frontiers(&self) -> &[CoverageComponentFrontier] {
        &self.frontiers
    }

    /// Selected global action indices for an optimal plan.
    pub fn selected(&self) -> &[usize] {
        &self.selected
    }

    /// Optimal cost, when a feasible plan was proved.
    pub fn cost(&self) -> Option<u64> {
        self.cost
    }

    /// Coverage predicate calls made by all local searches.
    pub fn oracle_calls(&self) -> usize {
        self.oracle_calls
    }

    /// Branch nodes visited by all local searches.
    pub fn search_nodes(&self) -> usize {
        self.search_nodes
    }

    /// Reused local predicate results across all local searches.
    pub fn cache_hits(&self) -> usize {
        self.cache_hits
    }
}
