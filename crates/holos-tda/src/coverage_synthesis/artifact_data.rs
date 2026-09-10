//! Coverage artifact storage and wire-decoding data.

use crate::monotone_proof::{ProofNode, ProofWork};
use crate::{CoverageFence, PlanarCoverageModel};

use super::specification::{CoverageSource, CoverageSpecification};
use super::types::{CoverageAction, CoverageSynthesisStatus, EvaluationClaim};

/// Coverage synthesis result.
#[derive(Debug, Clone, PartialEq)]
pub struct CoverageSynthesisArtifact {
    pub(crate) specification: CoverageSpecification,
    pub(crate) actions: Vec<CoverageAction>,
    pub(crate) max_activations: usize,
    pub(crate) oracle_limit: usize,
    pub(crate) node_limit: usize,
    pub(crate) status: CoverageSynthesisStatus,
    pub(crate) selected: Vec<usize>,
    pub(crate) lower_bound_cost: Option<u64>,
    pub(crate) upper_bound_cost: Option<u64>,
    pub(crate) producer_oracle_calls: usize,
    pub(crate) producer_search_nodes: usize,
    pub(crate) producer_cache_hits: usize,
    pub(crate) root_blockers: Vec<Vec<usize>>,
    pub(crate) before: EvaluationClaim,
    pub(crate) after: EvaluationClaim,
    pub(crate) proof: Option<ProofNode>,
    pub(crate) proof_work: ProofWork,
    pub(crate) digest: [u8; 32],
}

pub(crate) struct CoverageSearchData {
    pub(crate) status: CoverageSynthesisStatus,
    pub(crate) selected: Vec<usize>,
    pub(crate) lower_bound: Option<u64>,
    pub(crate) upper_bound: Option<u64>,
    pub(crate) oracle_calls: usize,
    pub(crate) search_nodes: usize,
    pub(crate) cache_hits: usize,
    pub(crate) root_blockers: Vec<Vec<usize>>,
}

pub(crate) struct BuiltCoverageProof {
    pub(crate) proof: Option<ProofNode>,
    pub(crate) work: ProofWork,
}

pub(crate) struct CoverageHeader {
    pub(crate) vertex_count: usize,
    pub(crate) model: PlanarCoverageModel,
    pub(crate) modulus: u32,
    pub(crate) fence: CoverageFence,
    pub(crate) failable_vertices: Vec<usize>,
    pub(crate) failure_budget: usize,
    pub(crate) source: CoverageSource,
}

pub(crate) struct DecodedCoverageSearch {
    pub(crate) max_activations: usize,
    pub(crate) oracle_limit: usize,
    pub(crate) node_limit: usize,
    pub(crate) status: CoverageSynthesisStatus,
    pub(crate) selected: Vec<usize>,
    pub(crate) lower_bound_cost: Option<u64>,
    pub(crate) upper_bound_cost: Option<u64>,
    pub(crate) producer_oracle_calls: usize,
    pub(crate) producer_search_nodes: usize,
    pub(crate) producer_cache_hits: usize,
}

pub(crate) struct CoverageWorkLimits {
    pub(crate) oracle: usize,
    pub(crate) nodes: usize,
}

pub(crate) struct CoverageSelectionData {
    pub(crate) status: CoverageSynthesisStatus,
    pub(crate) selected: Vec<usize>,
    pub(crate) lower_bound_cost: Option<u64>,
    pub(crate) upper_bound_cost: Option<u64>,
}

pub(crate) struct CoverageProducerWork {
    pub(crate) oracle_calls: usize,
    pub(crate) search_nodes: usize,
    pub(crate) cache_hits: usize,
}

pub(crate) struct DecodedCoverageProof {
    pub(crate) root_blockers: Vec<Vec<usize>>,
    pub(crate) before: EvaluationClaim,
    pub(crate) after: EvaluationClaim,
    pub(crate) proof: Option<ProofNode>,
    pub(crate) work: ProofWork,
}
