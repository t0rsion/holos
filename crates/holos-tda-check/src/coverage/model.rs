use crate::ProofError;

/// Completeness status accepted from a coverage proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifiedCoverageStatus {
    /// The checked activation set has minimum total cost.
    Optimal,
    /// No activation set within the declared limit satisfies every state.
    Infeasible,
    /// The producer stopped with a checked bound or incumbent.
    SearchIncomplete,
}

impl VerifiedCoverageStatus {
    pub(crate) fn from_code(code: u8) -> Result<Self, ProofError> {
        match code {
            1 => Ok(Self::Optimal),
            2 => Ok(Self::Infeasible),
            3 => Ok(Self::SearchIncomplete),
            _ => Err(ProofError::new("coverage status is invalid")),
        }
    }
}

/// Completeness scope accepted from a coverage proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifiedCoverageSource {
    /// The claim covers only its listed communication states.
    Finite,
    /// The checker reconstructed the complete affine threshold schedule.
    Affine,
}

/// Summary of a checked coverage result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedCoverage {
    /// Prime coefficient modulus.
    pub modulus: u32,
    /// Completeness scope of the checked state list.
    pub source: VerifiedCoverageSource,
    /// Number of checked communication states.
    pub states: usize,
    /// Number of declared candidate activations.
    pub actions: usize,
    /// Largest number of simultaneous sensor failures.
    pub failure_budget: usize,
    /// Completeness status.
    pub status: VerifiedCoverageStatus,
    /// Number of selected actions.
    pub selected: usize,
    /// Selected action cost, when an incumbent exists.
    pub total_cost: Option<u64>,
    /// Checked lower cost bound, when finite.
    pub lower_bound_cost: Option<u64>,
    /// Checked incumbent cost, when present.
    pub upper_bound_cost: Option<u64>,
    /// Producer topology calls recorded in the artifact.
    pub producer_oracle_calls: usize,
    /// Producer branch nodes recorded in the artifact.
    pub producer_search_nodes: usize,
    /// Proof-tree node count.
    pub proof_nodes: usize,
    /// Topology checks in the recorded proof tree.
    pub proof_topology_checks: usize,
    /// State-failure pairs checked for the selected plan.
    pub selected_failure_checks: usize,
    /// Smallest selected-plan two-chain support across all failure checks.
    pub minimum_witness_triangles: Option<usize>,
}

pub(crate) struct CoverageGeometryClaim {
    pub(crate) vertex_count: usize,
    pub(crate) broadcast_radius: f64,
    pub(crate) sensing_radius: f64,
    pub(crate) fence: Vec<usize>,
    pub(crate) state_edges: Vec<Vec<(usize, usize)>>,
}

pub(crate) struct DecodedCoverage {
    pub(crate) claim: Claim,
    pub(crate) producer_oracle_calls: usize,
    pub(crate) producer_search_nodes: usize,
    pub(crate) proof_nodes: usize,
    pub(crate) proof_topology_checks: usize,
    pub(crate) proof_terms: usize,
}

pub(crate) struct PhysicalHeader {
    pub(crate) vertex_count: usize,
    pub(crate) broadcast_radius: f64,
    pub(crate) sensing_radius: f64,
    pub(crate) modulus: u32,
    pub(crate) fence: Vec<usize>,
}

pub(crate) struct CoverageHeader {
    pub(crate) physical: PhysicalHeader,
    pub(crate) failable: Vec<usize>,
    pub(crate) failure_budget: usize,
    pub(crate) source: Source,
}

pub(crate) struct WorkLimits {
    pub(crate) oracle: usize,
    pub(crate) nodes: usize,
}

pub(crate) struct Selection {
    pub(crate) status: VerifiedCoverageStatus,
    pub(crate) selected: Vec<usize>,
    pub(crate) lower_bound: Option<u64>,
    pub(crate) upper_bound: Option<u64>,
}

pub(crate) struct SearchData {
    pub(crate) max_activations: usize,
    pub(crate) selection: Selection,
    pub(crate) producer_oracle_calls: usize,
    pub(crate) producer_search_nodes: usize,
}

pub(crate) struct ProofData {
    pub(crate) root_blockers: Vec<Vec<usize>>,
    pub(crate) before: Evaluation,
    pub(crate) after: Evaluation,
    pub(crate) proof: Option<ProofNode>,
    pub(crate) nodes: usize,
    pub(crate) topology_checks: usize,
    pub(crate) terms: usize,
}

pub(crate) struct CheckedCoverage {
    pub(crate) after: Evaluation,
    pub(crate) selected_cost: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Edge {
    pub(crate) u: usize,
    pub(crate) v: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct State {
    pub(crate) scenario: u64,
    pub(crate) step: u64,
    pub(crate) base: Vec<usize>,
    pub(crate) edges: Vec<Edge>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Action {
    pub(crate) vertex: usize,
    pub(crate) cost: u64,
    pub(crate) states: Vec<usize>,
}

#[derive(Debug, Clone)]
pub(crate) struct AffineEdge {
    pub(crate) edge: Edge,
    pub(crate) intercept: f64,
    pub(crate) velocity: f64,
}

pub(crate) enum Source {
    Finite,
    Affine {
        scenario: u64,
        edges: Vec<AffineEdge>,
        start: f64,
        end: f64,
    },
}

pub(crate) struct Claim {
    pub(crate) vertex_count: usize,
    pub(crate) broadcast_radius: f64,
    pub(crate) sensing_radius: f64,
    pub(crate) modulus: u32,
    pub(crate) fence: Vec<usize>,
    pub(crate) failable: Vec<usize>,
    pub(crate) failure_budget: usize,
    pub(crate) source: Source,
    pub(crate) states: Vec<State>,
    pub(crate) actions: Vec<Action>,
    pub(crate) max_activations: usize,
    pub(crate) status: VerifiedCoverageStatus,
    pub(crate) selected: Vec<usize>,
    pub(crate) lower_bound: Option<u64>,
    pub(crate) upper_bound: Option<u64>,
    pub(crate) root_blockers: Vec<Vec<usize>>,
    pub(crate) before: Evaluation,
    pub(crate) after: Evaluation,
    pub(crate) proof: Option<ProofNode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Evaluation {
    pub(crate) criterion_holds: bool,
    pub(crate) checks: usize,
    pub(crate) minimum_witness: Option<usize>,
}

#[derive(Clone, Copy)]
pub(crate) enum BoundKind {
    Cost,
    Activations,
}

pub(crate) enum ProofNode {
    Cost,
    SurvivingMaximum,
    SurvivingActivationLimit,
    BlockerBound {
        kind: BoundKind,
        blockers: Vec<Vec<usize>>,
    },
    Branch {
        blocker: Vec<usize>,
        children: Vec<ProofNode>,
    },
}
