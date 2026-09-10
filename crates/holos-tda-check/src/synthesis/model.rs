use crate::ProofError;
use crate::cohomology::{Edge, MapTerm};

pub(super) const MAGIC: &[u8; 8] = b"HOLOSSYN";
pub(super) const VERSION: u16 = 1;
pub(super) const F64_BITS_CODEC: u8 = 1;
pub(super) const MODULUS_LIMIT: u64 = 32_768;
pub(super) const FORMAT_MAX_STATES: usize = 4_096;
pub(super) const FORMAT_MAX_ACTIONS: usize = 65_536;
pub(super) const FORMAT_MAX_ORACLE_CALLS: usize = 10_000_000;
pub(super) const FORMAT_MAX_SEARCH_NODES: usize = 10_000_000;
pub(super) const FORMAT_MAX_PROOF_NODES: usize = 10_000_000;
pub(super) const FORMAT_MAX_PROOF_TERMS: usize = 100_000_000;
pub(super) const FORMAT_MAX_PROOF_DEPTH: usize = 4_096;

/// Completeness status accepted from a synthesis proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifiedSynthesisStatus {
    /// The checked action set has minimum total cost.
    Optimal,
    /// No checked action set satisfies every state under the edit limit.
    Infeasible,
    /// The producer stopped with a checked bound or incumbent.
    SearchIncomplete,
}

/// Completeness scope accepted from a synthesis proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifiedSynthesisSource {
    /// The claim covers only its listed states.
    Finite,
    /// The checker reconstructed all fixed-scale states of an affine trajectory.
    Affine,
}

impl VerifiedSynthesisStatus {
    pub(super) fn from_code(code: u8) -> Result<Self, ProofError> {
        match code {
            1 => Ok(Self::Optimal),
            2 => Ok(Self::Infeasible),
            3 => Ok(Self::SearchIncomplete),
            _ => Err(ProofError::new("synthesis status is invalid")),
        }
    }
}

/// Summary of a checked synthesis result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedSynthesis {
    /// Cohomology dimension.
    pub dimension: usize,
    /// Prime coefficient modulus.
    pub modulus: u32,
    /// Completeness scope of the checked state list.
    pub source: VerifiedSynthesisSource,
    /// Number of checked temporal states.
    pub states: usize,
    /// Number of declared actions.
    pub actions: usize,
    /// Completeness status.
    pub status: VerifiedSynthesisStatus,
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
    /// Target subspace ranks before editing.
    pub before_ranks: Vec<usize>,
    /// Surviving target ranks after editing.
    pub after_ranks: Vec<usize>,
}

pub(super) struct DecodedSynthesis {
    pub(super) claim: Claim,
    pub(super) producer_oracle_calls: usize,
    pub(super) producer_search_nodes: usize,
    pub(super) proof_nodes: usize,
    pub(super) proof_topology_checks: usize,
}

pub(super) struct SynthesisHeader {
    pub(super) vertex_count: usize,
    pub(super) dimension: usize,
    pub(super) scale: f64,
    pub(super) modulus: u32,
    pub(super) source: Source,
}

pub(super) struct WorkLimits {
    pub(super) oracle: usize,
    pub(super) nodes: usize,
}

pub(super) struct Selection {
    pub(super) status: VerifiedSynthesisStatus,
    pub(super) selected: Vec<usize>,
    pub(super) lower_bound: Option<u64>,
    pub(super) upper_bound: Option<u64>,
}

pub(super) struct SearchData {
    pub(super) max_edits: usize,
    pub(super) selection: Selection,
    pub(super) producer_oracle_calls: usize,
    pub(super) producer_search_nodes: usize,
}

pub(super) struct ProofData {
    pub(super) root_blockers: Vec<Vec<usize>>,
    pub(super) before_ranks: Vec<usize>,
    pub(super) after_ranks: Vec<usize>,
    pub(super) proof: Option<ProofNode>,
    pub(super) nodes: usize,
    pub(super) topology_checks: usize,
}

pub(super) struct CheckedSynthesis {
    pub(super) selected_cost: u64,
    pub(super) selected_feasible: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct State {
    pub(super) scenario: u64,
    pub(super) step: u64,
    pub(super) edges: Vec<Edge>,
    pub(super) target_space: [u8; 32],
    pub(super) target: Vec<Vec<MapTerm>>,
    pub(super) max_surviving_rank: usize,
}

pub(super) enum Source {
    Finite,
    Affine {
        scenario: u64,
        edges: Vec<AffineEdge>,
        start: f64,
        end: f64,
        maximum_rank: usize,
    },
}

#[derive(Clone)]
pub(super) struct AffineEdge {
    pub(super) edge: Edge,
    pub(super) intercept: f64,
    pub(super) velocity: f64,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Action {
    pub(super) edge: Edge,
    pub(super) cost: u64,
    pub(super) states: Vec<usize>,
}

pub(super) struct Claim {
    pub(super) vertex_count: usize,
    pub(super) dimension: usize,
    pub(super) scale: f64,
    pub(super) modulus: u32,
    pub(super) source: Source,
    pub(super) states: Vec<State>,
    pub(super) actions: Vec<Action>,
    pub(super) max_edits: usize,
    pub(super) status: VerifiedSynthesisStatus,
    pub(super) selected: Vec<usize>,
    pub(super) lower_bound: Option<u64>,
    pub(super) upper_bound: Option<u64>,
    pub(super) root_blockers: Vec<Vec<usize>>,
    pub(super) before_ranks: Vec<usize>,
    pub(super) after_ranks: Vec<usize>,
    pub(super) proof: Option<ProofNode>,
}

#[derive(Clone, Copy)]
pub(super) enum BoundKind {
    Cost,
    Edits,
}

pub(super) enum ProofNode {
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
