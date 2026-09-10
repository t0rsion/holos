use super::specification::{SynthesisAction, TopologicalSpecification};
use super::types::{ProofNode, SynthesisSource, SynthesisStatus};

/// Synthesis result.
#[derive(Debug, Clone, PartialEq)]
pub struct SynthesisArtifact {
    pub(in crate::synthesis) specification: TopologicalSpecification,
    pub(in crate::synthesis) actions: Vec<SynthesisAction>,
    pub(in crate::synthesis) max_edits: usize,
    pub(in crate::synthesis) oracle_limit: usize,
    pub(in crate::synthesis) node_limit: usize,
    pub(in crate::synthesis) status: SynthesisStatus,
    pub(in crate::synthesis) selected: Vec<usize>,
    pub(in crate::synthesis) lower_bound_cost: Option<u64>,
    pub(in crate::synthesis) upper_bound_cost: Option<u64>,
    pub(in crate::synthesis) producer_oracle_calls: usize,
    pub(in crate::synthesis) producer_search_nodes: usize,
    pub(in crate::synthesis) producer_cache_hits: usize,
    pub(in crate::synthesis) root_blockers: Vec<Vec<usize>>,
    pub(in crate::synthesis) before_ranks: Vec<usize>,
    pub(in crate::synthesis) after_ranks: Vec<usize>,
    pub(in crate::synthesis) proof: Option<ProofNode>,
    pub(in crate::synthesis) proof_nodes: usize,
    pub(in crate::synthesis) proof_topology_checks: usize,
    pub(in crate::synthesis) digest: [u8; 32],
}

pub(in crate::synthesis) struct SynthesisHeader {
    pub(in crate::synthesis) vertex_count: usize,
    pub(in crate::synthesis) dimension: usize,
    pub(in crate::synthesis) scale: f64,
    pub(in crate::synthesis) modulus: u32,
    pub(in crate::synthesis) source: SynthesisSource,
}

pub(in crate::synthesis) struct WorkLimits {
    pub(in crate::synthesis) oracle: usize,
    pub(in crate::synthesis) nodes: usize,
}

pub(in crate::synthesis) struct SelectionData {
    pub(in crate::synthesis) status: SynthesisStatus,
    pub(in crate::synthesis) selected: Vec<usize>,
    pub(in crate::synthesis) lower_bound_cost: Option<u64>,
    pub(in crate::synthesis) upper_bound_cost: Option<u64>,
}

pub(in crate::synthesis) struct ProducerWork {
    pub(in crate::synthesis) oracle_calls: usize,
    pub(in crate::synthesis) search_nodes: usize,
    pub(in crate::synthesis) cache_hits: usize,
}

pub(in crate::synthesis) struct SearchData {
    pub(in crate::synthesis) max_edits: usize,
    pub(in crate::synthesis) limits: WorkLimits,
    pub(in crate::synthesis) selection: SelectionData,
    pub(in crate::synthesis) work: ProducerWork,
}

pub(in crate::synthesis) struct ProofData {
    pub(in crate::synthesis) root_blockers: Vec<Vec<usize>>,
    pub(in crate::synthesis) before_ranks: Vec<usize>,
    pub(in crate::synthesis) after_ranks: Vec<usize>,
    pub(in crate::synthesis) proof: Option<ProofNode>,
    pub(in crate::synthesis) nodes: usize,
    pub(in crate::synthesis) topology_checks: usize,
}

pub(in crate::synthesis) struct BuiltProof {
    pub(in crate::synthesis) proof: Option<ProofNode>,
    pub(in crate::synthesis) nodes: usize,
    pub(in crate::synthesis) topology_checks: usize,
}
