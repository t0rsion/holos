use std::fmt;

use crate::{CohomologyLimits, Error, KineticEdge, KineticLimits, Result};

/// Resource limits for synthesis search, proof construction, and decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SynthesisLimits {
    /// Largest accepted artifact byte count.
    pub max_bytes: usize,
    /// Largest accepted vertex count.
    pub max_vertices: usize,
    /// Largest active edge count in one state.
    pub max_edges_per_state: usize,
    /// Largest state count.
    pub max_states: usize,
    /// Largest action count.
    pub max_actions: usize,
    /// Largest total coordinate and proof term count.
    pub max_terms: usize,
    /// Largest producer oracle-call count.
    pub max_oracle_calls: usize,
    /// Largest producer search-node count.
    pub max_search_nodes: usize,
    /// Largest proof-tree node count.
    pub max_proof_nodes: usize,
    /// Largest proof-tree depth.
    pub max_proof_depth: usize,
    /// Limits for each canonical cohomology computation.
    pub cohomology: CohomologyLimits,
    /// Limits for replaying an affine trajectory.
    pub kinetic: KineticLimits,
}

impl Default for SynthesisLimits {
    fn default() -> Self {
        Self {
            max_bytes: 1 << 30,
            max_vertices: 1_000_000,
            max_edges_per_state: 20_000_000,
            max_states: 1_024,
            max_actions: 16_384,
            max_terms: 10_000_000,
            max_oracle_calls: 2_000_000,
            max_search_nodes: 2_000_000,
            max_proof_nodes: 2_000_000,
            max_proof_depth: 1_024,
            cohomology: CohomologyLimits::default(),
            kinetic: KineticLimits::default(),
        }
    }
}

/// Origin and completeness scope of the finite state list.
#[derive(Debug, Clone, PartialEq)]
pub enum SynthesisSource {
    /// States were supplied directly. No completeness claim is made outside them.
    Finite,
    /// States are the complete fixed-scale schedule of an affine trajectory.
    Affine {
        /// Scenario identifier assigned to every retained state.
        scenario: u64,
        /// Canonical affine edge trajectories.
        edges: Vec<KineticEdge>,
        /// First time in the closed interval.
        start: f64,
        /// Last time in the closed interval.
        end: f64,
        /// Largest permitted rank throughout the interval.
        maximum_rank: usize,
    },
}

impl SynthesisLimits {
    /// Set the largest producer oracle-call count.
    #[must_use]
    pub fn with_max_oracle_calls(mut self, maximum: usize) -> Self {
        self.max_oracle_calls = maximum;
        self
    }

    /// Set the largest producer search-node count.
    #[must_use]
    pub fn with_max_search_nodes(mut self, maximum: usize) -> Self {
        self.max_search_nodes = maximum;
        self
    }
}

/// One nonzero coordinate in a canonical subspace generator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SynthesisCoordinate {
    /// Position in the state's canonical cohomology basis.
    pub basis: usize,
    /// Coefficient in `1..modulus`.
    pub coefficient: u32,
}

/// Completeness status of one synthesis result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SynthesisStatus {
    /// The selected actions have minimum total cost.
    Optimal,
    /// No action set within the edit limit satisfies the specification.
    Infeasible,
    /// A producer work limit stopped the search before a complete proof.
    SearchIncomplete,
}

impl SynthesisStatus {
    pub(in crate::synthesis) fn code(self) -> u8 {
        match self {
            Self::Optimal => 1,
            Self::Infeasible => 2,
            Self::SearchIncomplete => 3,
        }
    }

    pub(in crate::synthesis) fn from_code(code: u8) -> Result<Self> {
        match code {
            1 => Ok(Self::Optimal),
            2 => Ok(Self::Infeasible),
            3 => Ok(Self::SearchIncomplete),
            _ => Err(Error::InvalidInput("synthesis status is invalid".into())),
        }
    }
}

impl fmt::Display for SynthesisStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Optimal => "optimal",
            Self::Infeasible => "infeasible",
            Self::SearchIncomplete => "search incomplete",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::synthesis) enum BoundKind {
    Cost,
    Edits,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::synthesis) enum ProofNode {
    Cost,
    SurvivingMaximum,
    SurvivingEditLimit,
    BlockerBound {
        kind: BoundKind,
        blockers: Vec<Vec<usize>>,
    },
    Branch {
        blocker: Vec<usize>,
        children: Vec<ProofNode>,
    },
}
