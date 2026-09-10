use std::fmt;

use crate::{CohomologyLimits, Error, KineticEdgeKey, Result, SparseDistanceMatrix};

pub(super) const FORMAT_MAX_SCENARIOS: usize = 256;
pub(super) const FORMAT_MAX_CANDIDATES: usize = 4_096;
pub(super) const FORMAT_MAX_PROOF_TERMS: usize = 1_000_000;
pub(super) const FORMAT_MAX_ORACLE_CALLS: usize = 10_000_000;
pub(super) const FORMAT_MAX_SEARCH_NODES: usize = 10_000_000;

/// Resource limits for weighted intervention search and artifacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct CohomologyInterventionLimits {
    /// Largest accepted artifact byte count.
    pub max_bytes: usize,
    /// Largest accepted vertex count.
    pub max_vertices: usize,
    /// Largest active edge count in one scenario.
    pub max_edges_per_scenario: usize,
    /// Largest declared scenario count.
    pub max_scenarios: usize,
    /// Largest candidate edge count.
    pub max_candidates: usize,
    /// Largest total candidate-index count in lower-bound witnesses.
    pub max_proof_terms: usize,
    /// Largest distinct topological oracle call count.
    pub max_oracle_calls: usize,
    /// Largest branch-and-bound node count.
    pub max_search_nodes: usize,
    /// Limits for each canonical cohomology computation.
    pub cohomology: CohomologyLimits,
}

impl Default for CohomologyInterventionLimits {
    fn default() -> Self {
        Self {
            max_bytes: 1 << 30,
            max_vertices: 1_000_000,
            max_edges_per_scenario: 20_000_000,
            max_scenarios: 64,
            max_candidates: 4_096,
            max_proof_terms: 1_000_000,
            max_oracle_calls: 1_000_000,
            max_search_nodes: 1_000_000,
            cohomology: CohomologyLimits::default(),
        }
    }
}

impl CohomologyInterventionLimits {
    /// Set the largest distinct topological oracle call count.
    #[must_use]
    pub fn with_max_oracle_calls(mut self, max_oracle_calls: usize) -> Self {
        self.max_oracle_calls = max_oracle_calls;
        self
    }

    /// Set the largest branch-and-bound node count.
    #[must_use]
    pub fn with_max_search_nodes(mut self, max_search_nodes: usize) -> Self {
        self.max_search_nodes = max_search_nodes;
        self
    }
}

/// One active graph and a target basis position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CohomologyInterventionScenario {
    pub(super) active_edges: Vec<KineticEdgeKey>,
    pub(super) target_basis: usize,
}

impl CohomologyInterventionScenario {
    /// Construct a scenario from the edges active at `scale`.
    pub fn from_graph(
        graph: &SparseDistanceMatrix,
        scale: f64,
        target_basis: usize,
    ) -> Result<Self> {
        if !scale.is_finite() || scale < 0.0 {
            return Err(Error::InvalidInput(
                "cohomology intervention scale must be finite and non-negative".into(),
            ));
        }
        Ok(Self {
            active_edges: graph
                .edges()
                .filter(|edge| edge.2 <= scale)
                .map(|(u, v, _)| KineticEdgeKey { u, v })
                .collect(),
            target_basis,
        })
    }

    /// Canonical active edge list.
    pub fn active_edges(&self) -> &[KineticEdgeKey] {
        &self.active_edges
    }

    /// Position in this scenario's canonical cohomology basis.
    pub fn target_basis(&self) -> usize {
        self.target_basis
    }
}

/// One possible edge addition and its positive integer cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CohomologyInterventionCandidate {
    /// Canonical edge key.
    pub edge: KineticEdgeKey,
    /// Positive additive cost.
    pub cost: u64,
}

impl CohomologyInterventionCandidate {
    /// Construct a candidate and sort its edge endpoints.
    pub fn new(u: usize, v: usize, cost: u64) -> Self {
        Self {
            edge: KineticEdgeKey::new(u, v),
            cost,
        }
    }
}

/// Completeness status of one weighted intervention search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CohomologyInterventionStatus {
    /// The selected edges have minimum total cost under the edit limit.
    Optimal,
    /// No selected edge set within the edit limit kills every target.
    Infeasible,
    /// An oracle-call or search-node limit stopped the search.
    SearchIncomplete,
}

impl CohomologyInterventionStatus {
    pub(super) fn code(self) -> u8 {
        match self {
            Self::Optimal => 1,
            Self::Infeasible => 2,
            Self::SearchIncomplete => 3,
        }
    }

    pub(super) fn from_code(code: u8) -> Result<Self> {
        match code {
            1 => Ok(Self::Optimal),
            2 => Ok(Self::Infeasible),
            3 => Ok(Self::SearchIncomplete),
            _ => Err(Error::InvalidInput(
                "cohomology intervention status is invalid".into(),
            )),
        }
    }
}

/// Weighted multi-scenario intervention certificate.
#[derive(Debug, Clone, PartialEq)]
pub struct CohomologyInterventionArtifact {
    pub(super) vertex_count: usize,
    pub(super) dimension: usize,
    pub(super) scale: f64,
    pub(super) modulus: u32,
    pub(super) scenarios: Vec<CohomologyInterventionScenario>,
    pub(super) candidates: Vec<CohomologyInterventionCandidate>,
    pub(super) max_edits: usize,
    pub(super) oracle_limit: usize,
    pub(super) node_limit: usize,
    pub(super) status: CohomologyInterventionStatus,
    pub(super) edits: Vec<CohomologyInterventionCandidate>,
    pub(super) lower_bound_cost: Option<u64>,
    pub(super) upper_bound_cost: Option<u64>,
    pub(super) oracle_calls: usize,
    pub(super) search_nodes: usize,
    pub(super) cache_hits: usize,
    pub(super) root_blockers: Vec<Vec<usize>>,
    pub(super) root_blocker_bound: u64,
    pub(super) before_ranks: Vec<usize>,
    pub(super) after_ranks: Vec<usize>,
    pub(super) digest: [u8; 32],
}

pub(super) struct InterventionHeader {
    pub(super) vertex_count: usize,
    pub(super) dimension: usize,
    pub(super) scale: f64,
    pub(super) modulus: u32,
}

pub(super) struct InterventionSearchData {
    pub(super) max_edits: usize,
    pub(super) oracle_limit: usize,
    pub(super) node_limit: usize,
    pub(super) status: CohomologyInterventionStatus,
    pub(super) edit_indices: Vec<usize>,
    pub(super) lower_bound_cost: Option<u64>,
    pub(super) upper_bound_cost: Option<u64>,
    pub(super) oracle_calls: usize,
    pub(super) search_nodes: usize,
    pub(super) cache_hits: usize,
}

pub(super) struct InterventionWorkLimits {
    pub(super) oracle: usize,
    pub(super) nodes: usize,
}

pub(super) struct InterventionSelection {
    pub(super) status: CohomologyInterventionStatus,
    pub(super) edit_indices: Vec<usize>,
    pub(super) lower_bound_cost: Option<u64>,
    pub(super) upper_bound_cost: Option<u64>,
}

pub(super) struct InterventionProducerWork {
    pub(super) oracle_calls: usize,
    pub(super) search_nodes: usize,
    pub(super) cache_hits: usize,
}

pub(super) struct InterventionProofData {
    pub(super) root_blockers: Vec<Vec<usize>>,
    pub(super) root_blocker_bound: u64,
    pub(super) before_ranks: Vec<usize>,
    pub(super) after_ranks: Vec<usize>,
}

impl fmt::Display for CohomologyInterventionStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Optimal => "optimal",
            Self::Infeasible => "infeasible",
            Self::SearchIncomplete => "search incomplete",
        })
    }
}
