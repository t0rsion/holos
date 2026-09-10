use crate::ProofError;
use crate::cohomology::Edge;

/// Completeness status from the checker's weighted search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifiedCohomologyInterventionStatus {
    /// The selected edges have minimum total cost under the edit limit.
    Optimal,
    /// No selected edge set within the edit limit kills every target.
    Infeasible,
    /// An oracle-call or search-node limit stopped the proof.
    SearchIncomplete,
}

impl VerifiedCohomologyInterventionStatus {
    pub(super) fn from_code(code: u8) -> Result<Self, ProofError> {
        match code {
            1 => Ok(Self::Optimal),
            2 => Ok(Self::Infeasible),
            3 => Ok(Self::SearchIncomplete),
            _ => Err(ProofError::new("cohomology intervention status is invalid")),
        }
    }
}

/// Counts and bounds from one checked intervention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedCohomologyIntervention {
    /// Target cohomology dimension.
    pub dimension: usize,
    /// Prime coefficient modulus.
    pub modulus: u32,
    /// Number of graph scenarios checked together.
    pub scenarios: usize,
    /// Completeness status.
    pub status: VerifiedCohomologyInterventionStatus,
    /// Number of selected edge additions.
    pub edits: usize,
    /// Total selected cost, when a feasible edit exists.
    pub total_cost: Option<u64>,
    /// Checked lower cost bound, when finite.
    pub lower_bound_cost: Option<u64>,
    /// Checked incumbent cost, when present.
    pub upper_bound_cost: Option<u64>,
    /// Distinct topological oracle calls.
    pub oracle_calls: usize,
    /// Branch-and-bound nodes visited.
    pub search_nodes: usize,
    /// Exact subset-cache hits.
    pub cache_hits: usize,
    /// Disjoint necessary candidate sets at the root.
    pub root_blockers: usize,
    /// Additive cost lower bound from those root blockers.
    pub root_blocker_bound: u64,
    /// Cohomology ranks before editing in scenario order.
    pub before_ranks: Vec<usize>,
    /// Cohomology ranks after the selected edit in scenario order.
    pub after_ranks: Vec<usize>,
}

#[derive(Debug, Clone)]
pub(super) struct Scenario {
    pub(super) edges: Vec<Edge>,
    pub(super) target_basis: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Candidate {
    pub(super) edge: Edge,
    pub(super) cost: u64,
}

pub(super) struct Claim {
    pub(super) vertex_count: usize,
    pub(super) dimension: usize,
    pub(super) modulus: u32,
    pub(super) scenarios: Vec<Scenario>,
    pub(super) candidates: Vec<Candidate>,
    pub(super) max_edits: usize,
    pub(super) oracle_limit: usize,
    pub(super) node_limit: usize,
    pub(super) status: VerifiedCohomologyInterventionStatus,
    pub(super) edits: Vec<usize>,
    pub(super) lower_bound: Option<u64>,
    pub(super) upper_bound: Option<u64>,
    pub(super) oracle_calls: usize,
    pub(super) search_nodes: usize,
    pub(super) cache_hits: usize,
    pub(super) root_blockers: Vec<Vec<usize>>,
    pub(super) root_blocker_bound: u64,
    pub(super) before_ranks: Vec<usize>,
    pub(super) after_ranks: Vec<usize>,
}
