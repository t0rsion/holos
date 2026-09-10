use sha2::{Digest, Sha256};

use crate::monotone_search::{SearchLimits, minimize_antitone};
use crate::{CohomologySpace, Error, Result};

use super::model::{
    CohomologyInterventionArtifact, CohomologyInterventionCandidate, CohomologyInterventionLimits,
    CohomologyInterventionScenario, CohomologyInterventionStatus,
};
use super::oracle::{TopologyOracle, map_status};
use super::validation::{
    validate_claim_shape, validate_decoded_artifact, validate_edits, validate_problem,
    validate_root_blocker_claim, validate_status_claim,
};
use super::wire::{
    Reader, decode_candidates, decode_header, decode_prefix, decode_proof_data, decode_scenarios,
    decode_search_data, decode_trailer, encode_candidates, encode_header, encode_prefix,
    encode_proof_data, encode_scenarios, encode_search_data, validate_artifact_size,
};

impl CohomologyInterventionArtifact {
    /// Solve one weighted intervention shared by all declared scenarios.
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        vertex_count: usize,
        dimension: usize,
        scale: f64,
        modulus: u32,
        scenarios: &[CohomologyInterventionScenario],
        candidates: &[CohomologyInterventionCandidate],
        max_edits: usize,
        limits: CohomologyInterventionLimits,
    ) -> Result<Self> {
        Self::from_parts(
            vertex_count,
            dimension,
            scale,
            modulus,
            scenarios.to_vec(),
            candidates.to_vec(),
            max_edits,
            limits.max_oracle_calls,
            limits.max_search_nodes,
            limits,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn from_parts(
        vertex_count: usize,
        dimension: usize,
        scale: f64,
        modulus: u32,
        scenarios: Vec<CohomologyInterventionScenario>,
        candidates: Vec<CohomologyInterventionCandidate>,
        max_edits: usize,
        oracle_limit: usize,
        node_limit: usize,
        limits: CohomologyInterventionLimits,
    ) -> Result<Self> {
        validate_problem(
            vertex_count,
            scale,
            &scenarios,
            &candidates,
            max_edits,
            oracle_limit,
            node_limit,
            limits,
        )?;
        let oracle = TopologyOracle::build(
            vertex_count,
            dimension,
            scale,
            modulus,
            &scenarios,
            &candidates,
            limits.cohomology,
        )?;
        let before_ranks = oracle
            .spaces
            .iter()
            .map(CohomologySpace::rank)
            .collect::<Vec<_>>();
        let costs = candidates
            .iter()
            .map(|candidate| candidate.cost)
            .collect::<Vec<_>>();
        let search = minimize_antitone(
            &costs,
            max_edits,
            SearchLimits {
                oracle_calls: oracle_limit,
                search_nodes: node_limit,
            },
            |selected| oracle.survives(selected),
        )?;
        let after_ranks = if search.selected.is_empty() {
            before_ranks.clone()
        } else {
            oracle.ranks(&search.selected)?
        };
        let edits = search
            .selected
            .iter()
            .map(|position| candidates[*position])
            .collect();
        let mut artifact = Self {
            vertex_count,
            dimension,
            scale,
            modulus,
            scenarios,
            candidates,
            max_edits,
            oracle_limit,
            node_limit,
            status: map_status(search.status),
            edits,
            lower_bound_cost: search.lower_bound,
            upper_bound_cost: search.upper_bound,
            oracle_calls: search.oracle_calls,
            search_nodes: search.search_nodes,
            cache_hits: search.cache_hits,
            root_blockers: search.root_blockers,
            root_blocker_bound: search.root_blocker_bound,
            before_ranks,
            after_ranks,
            digest: [0; 32],
        };
        artifact.validate_claim(limits)?;
        artifact.digest = artifact.compute_digest()?;
        Ok(artifact)
    }

    /// Recompute the bounded search and compare every claim.
    pub fn verify(&self, limits: CohomologyInterventionLimits) -> Result<()> {
        if self.oracle_limit > limits.max_oracle_calls || self.node_limit > limits.max_search_nodes
        {
            return Err(Error::InvalidInput(
                "cohomology intervention search limits exceed verifier limits".into(),
            ));
        }
        let rebuilt = Self::from_parts(
            self.vertex_count,
            self.dimension,
            self.scale,
            self.modulus,
            self.scenarios.clone(),
            self.candidates.clone(),
            self.max_edits,
            self.oracle_limit,
            self.node_limit,
            limits,
        )?;
        if rebuilt != *self {
            return Err(Error::InvalidInput(
                "cohomology intervention differs from weighted verification".into(),
            ));
        }
        Ok(())
    }

    /// Encode canonical `HOLOSCI` version 2 bytes.
    pub fn encode(&self, limits: CohomologyInterventionLimits) -> Result<Vec<u8>> {
        self.verify(limits)?;
        let mut output = self.encode_payload()?;
        output.extend_from_slice(&self.digest);
        if output.len() > limits.max_bytes {
            return Err(Error::InvalidInput(
                "cohomology intervention artifact exceeds its byte limit".into(),
            ));
        }
        Ok(output)
    }

    /// Decode and verify canonical `HOLOSCI` version 2 bytes.
    pub fn decode(bytes: &[u8], limits: CohomologyInterventionLimits) -> Result<Self> {
        validate_artifact_size(bytes, limits)?;
        let mut reader = Reader::new(bytes);
        decode_prefix(&mut reader)?;
        let header = decode_header(&mut reader)?;
        let scenarios = decode_scenarios(&mut reader, limits)?;
        let candidates = decode_candidates(&mut reader, limits)?;
        let search = decode_search_data(&mut reader, candidates.len())?;
        let edits = search
            .edit_indices
            .iter()
            .map(|position| candidates[*position])
            .collect();
        let proof = decode_proof_data(&mut reader, candidates.len(), scenarios.len(), limits)?;
        let digest = decode_trailer(&mut reader)?;
        let artifact = Self {
            vertex_count: header.vertex_count,
            dimension: header.dimension,
            scale: header.scale,
            modulus: header.modulus,
            scenarios,
            candidates,
            max_edits: search.max_edits,
            oracle_limit: search.oracle_limit,
            node_limit: search.node_limit,
            status: search.status,
            edits,
            lower_bound_cost: search.lower_bound_cost,
            upper_bound_cost: search.upper_bound_cost,
            oracle_calls: search.oracle_calls,
            search_nodes: search.search_nodes,
            cache_hits: search.cache_hits,
            root_blockers: proof.root_blockers,
            root_blocker_bound: proof.root_blocker_bound,
            before_ranks: proof.before_ranks,
            after_ranks: proof.after_ranks,
            digest,
        };
        validate_decoded_artifact(&artifact, bytes, limits)?;
        Ok(artifact)
    }

    /// Number of graph vertices shared by every scenario.
    pub fn vertex_count(&self) -> usize {
        self.vertex_count
    }
    /// Target cohomology dimension.
    pub fn dimension(&self) -> usize {
        self.dimension
    }
    /// Fixed filtration scale.
    pub fn scale(&self) -> f64 {
        self.scale
    }
    /// Prime coefficient modulus.
    pub fn modulus(&self) -> u32 {
        self.modulus
    }
    /// Declared graph scenarios and target basis positions.
    pub fn scenarios(&self) -> &[CohomologyInterventionScenario] {
        &self.scenarios
    }
    /// Canonical candidate list in search order.
    pub fn candidates(&self) -> &[CohomologyInterventionCandidate] {
        &self.candidates
    }
    /// Largest accepted selected edge count.
    pub fn max_edits(&self) -> usize {
        self.max_edits
    }
    /// Search completeness status.
    pub fn status(&self) -> CohomologyInterventionStatus {
        self.status
    }
    /// Selected candidates for an incumbent or optimal result.
    pub fn edits(&self) -> &[CohomologyInterventionCandidate] {
        &self.edits
    }
    /// Proved lower bound on total cost, when finite.
    pub fn lower_bound_cost(&self) -> Option<u64> {
        self.lower_bound_cost
    }
    /// Cost of the selected feasible edit, when one exists.
    pub fn upper_bound_cost(&self) -> Option<u64> {
        self.upper_bound_cost
    }
    /// Distinct topological oracle calls.
    pub fn oracle_calls(&self) -> usize {
        self.oracle_calls
    }
    /// Branch-and-bound nodes visited.
    pub fn search_nodes(&self) -> usize {
        self.search_nodes
    }
    /// Topological oracle results taken from the exact subset cache.
    pub fn cache_hits(&self) -> usize {
        self.cache_hits
    }
    /// Disjoint necessary candidate sets at the root search node.
    pub fn root_blockers(&self) -> &[Vec<usize>] {
        &self.root_blockers
    }
    /// Sum of the cheapest candidate cost in each root blocker.
    pub fn root_blocker_bound(&self) -> u64 {
        self.root_blocker_bound
    }
    /// Cohomology rank before editing in scenario order.
    pub fn before_ranks(&self) -> &[usize] {
        &self.before_ranks
    }
    /// Cohomology rank after the selected edit in scenario order.
    pub fn after_ranks(&self) -> &[usize] {
        &self.after_ranks
    }
    /// Content digest of the claim.
    pub fn digest(&self) -> &[u8; 32] {
        &self.digest
    }

    pub(super) fn validate_claim(&self, limits: CohomologyInterventionLimits) -> Result<()> {
        validate_problem(
            self.vertex_count,
            self.scale,
            &self.scenarios,
            &self.candidates,
            self.max_edits,
            self.oracle_limit,
            self.node_limit,
            limits,
        )?;
        validate_claim_shape(self)?;
        validate_edits(self)?;
        validate_root_blocker_claim(self, limits)?;
        validate_status_claim(self)
    }

    pub(super) fn compute_digest(&self) -> Result<[u8; 32]> {
        let mut hash = Sha256::new();
        hash.update(b"holos-cohomology-intervention-v2");
        hash.update(self.encode_payload()?);
        Ok(hash.finalize().into())
    }

    pub(super) fn encode_payload(&self) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        encode_prefix(&mut output);
        encode_header(&mut output, self)?;
        encode_scenarios(&mut output, &self.scenarios)?;
        encode_candidates(&mut output, &self.candidates)?;
        encode_search_data(&mut output, self)?;
        encode_proof_data(&mut output, self)?;
        Ok(output)
    }
}
