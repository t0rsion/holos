//! Weighted fixed-scale class interventions across declared scenarios.
//!
//! Each scenario names one canonical cohomology basis class. Candidate edges
//! have positive integer costs. One chosen edge set must kill every named
//! class under restriction from the edited flag complexes.

use std::collections::BTreeSet;
use std::fmt;

use sha2::{Digest, Sha256};

use crate::monotone_search::{SearchLimits, SearchStatus, minimize_antitone};
use crate::{
    CohomologyClassId, CohomologyLimits, CohomologySpace, Error, KineticEdgeKey, Result,
    SparseDistanceMatrix, cohomology_restriction, cohomology_space,
};

const MAGIC: &[u8; 8] = b"HOLOSCI\0";
const VERSION: u16 = 2;
const F64_BITS_CODEC: u8 = 1;
const FORMAT_MAX_SCENARIOS: usize = 256;
const FORMAT_MAX_CANDIDATES: usize = 4_096;
const FORMAT_MAX_PROOF_TERMS: usize = 1_000_000;
const FORMAT_MAX_ORACLE_CALLS: usize = 10_000_000;
const FORMAT_MAX_SEARCH_NODES: usize = 10_000_000;

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
    active_edges: Vec<KineticEdgeKey>,
    target_basis: usize,
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
    fn code(self) -> u8 {
        match self {
            Self::Optimal => 1,
            Self::Infeasible => 2,
            Self::SearchIncomplete => 3,
        }
    }

    fn from_code(code: u8) -> Result<Self> {
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
    vertex_count: usize,
    dimension: usize,
    scale: f64,
    modulus: u32,
    scenarios: Vec<CohomologyInterventionScenario>,
    candidates: Vec<CohomologyInterventionCandidate>,
    max_edits: usize,
    oracle_limit: usize,
    node_limit: usize,
    status: CohomologyInterventionStatus,
    edits: Vec<CohomologyInterventionCandidate>,
    lower_bound_cost: Option<u64>,
    upper_bound_cost: Option<u64>,
    oracle_calls: usize,
    search_nodes: usize,
    cache_hits: usize,
    root_blockers: Vec<Vec<usize>>,
    root_blocker_bound: u64,
    before_ranks: Vec<usize>,
    after_ranks: Vec<usize>,
    digest: [u8; 32],
}

struct InterventionHeader {
    vertex_count: usize,
    dimension: usize,
    scale: f64,
    modulus: u32,
}

struct InterventionSearchData {
    max_edits: usize,
    oracle_limit: usize,
    node_limit: usize,
    status: CohomologyInterventionStatus,
    edit_indices: Vec<usize>,
    lower_bound_cost: Option<u64>,
    upper_bound_cost: Option<u64>,
    oracle_calls: usize,
    search_nodes: usize,
    cache_hits: usize,
}

struct InterventionWorkLimits {
    oracle: usize,
    nodes: usize,
}

struct InterventionSelection {
    status: CohomologyInterventionStatus,
    edit_indices: Vec<usize>,
    lower_bound_cost: Option<u64>,
    upper_bound_cost: Option<u64>,
}

struct InterventionProducerWork {
    oracle_calls: usize,
    search_nodes: usize,
    cache_hits: usize,
}

struct InterventionProofData {
    root_blockers: Vec<Vec<usize>>,
    root_blocker_bound: u64,
    before_ranks: Vec<usize>,
    after_ranks: Vec<usize>,
}

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
    fn from_parts(
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

    fn validate_claim(&self, limits: CohomologyInterventionLimits) -> Result<()> {
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

    fn compute_digest(&self) -> Result<[u8; 32]> {
        let mut hash = Sha256::new();
        hash.update(b"holos-cohomology-intervention-v2");
        hash.update(self.encode_payload()?);
        Ok(hash.finalize().into())
    }

    fn encode_payload(&self) -> Result<Vec<u8>> {
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

fn validate_claim_shape(artifact: &CohomologyInterventionArtifact) -> Result<()> {
    if artifact.before_ranks.len() != artifact.scenarios.len()
        || artifact.after_ranks.len() != artifact.scenarios.len()
        || artifact.oracle_calls > artifact.oracle_limit
        || artifact.search_nodes > artifact.node_limit
    {
        Err(Error::InvalidInput(
            "cohomology intervention claim shape is invalid".into(),
        ))
    } else {
        Ok(())
    }
}

fn validate_edits(artifact: &CohomologyInterventionArtifact) -> Result<()> {
    let positions = artifact
        .edits
        .iter()
        .map(|edit| {
            artifact.candidates.binary_search(edit).map_err(|_| {
                Error::InvalidInput("cohomology intervention edit is not a candidate".into())
            })
        })
        .collect::<Result<Vec<_>>>()?;
    if positions.windows(2).any(|pair| pair[0] >= pair[1])
        || artifact.edits.len() > artifact.max_edits
    {
        Err(Error::InvalidInput(
            "cohomology intervention edit list is not canonical".into(),
        ))
    } else {
        Ok(())
    }
}

fn validate_root_blocker_claim(
    artifact: &CohomologyInterventionArtifact,
    limits: CohomologyInterventionLimits,
) -> Result<()> {
    let mut seen = BTreeSet::new();
    let mut proof_terms = 0usize;
    let mut bound = 0u64;
    for blocker in &artifact.root_blockers {
        validate_root_blocker(blocker, artifact.candidates.len(), &mut seen)?;
        proof_terms = proof_terms.checked_add(blocker.len()).ok_or_else(|| {
            Error::InvalidInput("cohomology intervention proof term count overflows".into())
        })?;
        let minimum = blocker
            .iter()
            .map(|position| artifact.candidates[*position].cost)
            .min()
            .expect("a checked blocker is nonempty");
        bound = bound.checked_add(minimum).ok_or_else(|| {
            Error::InvalidInput("cohomology intervention blocker bound overflows".into())
        })?;
    }
    if proof_terms > limits.max_proof_terms.min(FORMAT_MAX_PROOF_TERMS)
        || bound != artifact.root_blocker_bound
    {
        Err(Error::InvalidInput(
            "cohomology intervention blocker claim is invalid".into(),
        ))
    } else {
        Ok(())
    }
}

fn validate_root_blocker(
    blocker: &[usize],
    candidate_count: usize,
    seen: &mut BTreeSet<usize>,
) -> Result<()> {
    if blocker.is_empty()
        || blocker.windows(2).any(|pair| pair[0] >= pair[1])
        || blocker.iter().any(|position| *position >= candidate_count)
        || blocker.iter().any(|position| !seen.insert(*position))
    {
        Err(Error::InvalidInput(
            "cohomology intervention root blockers are not canonical and disjoint".into(),
        ))
    } else {
        Ok(())
    }
}

fn validate_status_claim(artifact: &CohomologyInterventionArtifact) -> Result<()> {
    match artifact.status {
        CohomologyInterventionStatus::Optimal => validate_optimal_claim(artifact),
        CohomologyInterventionStatus::Infeasible => validate_infeasible_claim(artifact),
        CohomologyInterventionStatus::SearchIncomplete => validate_incomplete_claim(artifact),
    }
}

fn validate_optimal_claim(artifact: &CohomologyInterventionArtifact) -> Result<()> {
    if artifact.lower_bound_cost.is_none()
        || artifact.lower_bound_cost != artifact.upper_bound_cost
        || artifact.edits.is_empty() && artifact.upper_bound_cost != Some(0)
    {
        Err(Error::InvalidInput(
            "optimal intervention bounds are invalid".into(),
        ))
    } else {
        Ok(())
    }
}

fn validate_infeasible_claim(artifact: &CohomologyInterventionArtifact) -> Result<()> {
    if !artifact.edits.is_empty()
        || artifact.lower_bound_cost.is_some()
        || artifact.upper_bound_cost.is_some()
    {
        Err(Error::InvalidInput(
            "infeasible intervention carries a finite bound".into(),
        ))
    } else {
        Ok(())
    }
}

fn validate_incomplete_claim(artifact: &CohomologyInterventionArtifact) -> Result<()> {
    if artifact.lower_bound_cost.is_none()
        || artifact
            .lower_bound_cost
            .zip(artifact.upper_bound_cost)
            .is_some_and(|(lower, upper)| lower > upper)
    {
        Err(Error::InvalidInput(
            "incomplete intervention bounds are invalid".into(),
        ))
    } else {
        Ok(())
    }
}

fn validate_artifact_size(bytes: &[u8], limits: CohomologyInterventionLimits) -> Result<()> {
    if bytes.len() > limits.max_bytes || bytes.len() < 32 {
        Err(Error::InvalidInput(
            "cohomology intervention exceeds its byte limit or is truncated".into(),
        ))
    } else {
        Ok(())
    }
}

fn decode_prefix(reader: &mut Reader<'_>) -> Result<()> {
    let magic = reader.take(8)?;
    let version = reader.u16()?;
    let codec = reader.u8()?;
    if magic != MAGIC || version != VERSION || codec != F64_BITS_CODEC {
        Err(Error::InvalidInput(
            "unsupported cohomology intervention artifact".into(),
        ))
    } else {
        Ok(())
    }
}

fn decode_header(reader: &mut Reader<'_>) -> Result<InterventionHeader> {
    Ok(InterventionHeader {
        vertex_count: reader.usize()?,
        dimension: reader.usize()?,
        scale: f64::from_bits(reader.u64()?),
        modulus: reader.u32()?,
    })
}

fn decode_scenarios(
    reader: &mut Reader<'_>,
    limits: CohomologyInterventionLimits,
) -> Result<Vec<CohomologyInterventionScenario>> {
    let count = reader.bounded_usize(
        "scenario count",
        limits.max_scenarios.min(FORMAT_MAX_SCENARIOS),
    )?;
    let mut scenarios = Vec::with_capacity(count);
    for _ in 0..count {
        scenarios.push(decode_scenario(reader, limits)?);
    }
    Ok(scenarios)
}

fn decode_scenario(
    reader: &mut Reader<'_>,
    limits: CohomologyInterventionLimits,
) -> Result<CohomologyInterventionScenario> {
    Ok(CohomologyInterventionScenario {
        active_edges: decode_edges(reader, limits.max_edges_per_scenario)?,
        target_basis: reader.usize()?,
    })
}

fn decode_candidates(
    reader: &mut Reader<'_>,
    limits: CohomologyInterventionLimits,
) -> Result<Vec<CohomologyInterventionCandidate>> {
    let count = reader.bounded_usize(
        "candidate count",
        limits.max_candidates.min(FORMAT_MAX_CANDIDATES),
    )?;
    if count > reader.remaining() / 24 {
        return Err(Error::InvalidInput(
            "cohomology intervention candidate count exceeds the remaining bytes".into(),
        ));
    }
    (0..count).map(|_| decode_candidate(reader)).collect()
}

fn decode_candidate(reader: &mut Reader<'_>) -> Result<CohomologyInterventionCandidate> {
    Ok(CohomologyInterventionCandidate {
        edge: KineticEdgeKey {
            u: reader.usize()?,
            v: reader.usize()?,
        },
        cost: reader.u64()?,
    })
}

fn decode_search_data(
    reader: &mut Reader<'_>,
    candidate_count: usize,
) -> Result<InterventionSearchData> {
    let max_edits = reader.usize()?;
    let limits = decode_work_limits(reader)?;
    let selection = decode_selection(reader, candidate_count)?;
    let work = decode_producer_work(reader)?;
    Ok(InterventionSearchData {
        max_edits,
        oracle_limit: limits.oracle,
        node_limit: limits.nodes,
        status: selection.status,
        edit_indices: selection.edit_indices,
        lower_bound_cost: selection.lower_bound_cost,
        upper_bound_cost: selection.upper_bound_cost,
        oracle_calls: work.oracle_calls,
        search_nodes: work.search_nodes,
        cache_hits: work.cache_hits,
    })
}

fn decode_work_limits(reader: &mut Reader<'_>) -> Result<InterventionWorkLimits> {
    Ok(InterventionWorkLimits {
        oracle: reader.usize()?,
        nodes: reader.usize()?,
    })
}

fn decode_selection(
    reader: &mut Reader<'_>,
    candidate_count: usize,
) -> Result<InterventionSelection> {
    Ok(InterventionSelection {
        status: CohomologyInterventionStatus::from_code(reader.u8()?)?,
        edit_indices: decode_indices(reader, candidate_count, candidate_count)?,
        lower_bound_cost: reader.optional_u64()?,
        upper_bound_cost: reader.optional_u64()?,
    })
}

fn decode_producer_work(reader: &mut Reader<'_>) -> Result<InterventionProducerWork> {
    Ok(InterventionProducerWork {
        oracle_calls: reader.usize()?,
        search_nodes: reader.usize()?,
        cache_hits: reader.usize()?,
    })
}

fn decode_proof_data(
    reader: &mut Reader<'_>,
    candidate_count: usize,
    scenario_count: usize,
    limits: CohomologyInterventionLimits,
) -> Result<InterventionProofData> {
    Ok(InterventionProofData {
        root_blockers: decode_root_blockers(reader, candidate_count, limits)?,
        root_blocker_bound: reader.u64()?,
        before_ranks: decode_usizes(reader, scenario_count)?,
        after_ranks: decode_usizes(reader, scenario_count)?,
    })
}

fn decode_root_blockers(
    reader: &mut Reader<'_>,
    candidate_count: usize,
    limits: CohomologyInterventionLimits,
) -> Result<Vec<Vec<usize>>> {
    let maximum = limits.max_proof_terms.min(FORMAT_MAX_PROOF_TERMS);
    let count = reader.bounded_usize("root blocker count", maximum)?;
    let mut blockers = Vec::with_capacity(count);
    let mut terms = 0usize;
    for _ in 0..count {
        let blocker = decode_indices(reader, candidate_count, maximum)?;
        terms = add_proof_terms(terms, blocker.len(), maximum)?;
        blockers.push(blocker);
    }
    Ok(blockers)
}

fn add_proof_terms(total: usize, add: usize, maximum: usize) -> Result<usize> {
    let total = total
        .checked_add(add)
        .ok_or_else(|| Error::InvalidInput("intervention proof term count overflows".into()))?;
    if total > maximum {
        Err(Error::InvalidInput(
            "cohomology intervention proof terms exceed their limit".into(),
        ))
    } else {
        Ok(total)
    }
}

fn decode_trailer(reader: &mut Reader<'_>) -> Result<[u8; 32]> {
    let digest = reader.array32()?;
    if reader.remaining() != 0 {
        Err(Error::InvalidInput(
            "trailing bytes follow the cohomology intervention artifact".into(),
        ))
    } else {
        Ok(digest)
    }
}

fn validate_decoded_artifact(
    artifact: &CohomologyInterventionArtifact,
    bytes: &[u8],
    limits: CohomologyInterventionLimits,
) -> Result<()> {
    artifact.validate_claim(limits)?;
    if artifact.compute_digest()? != artifact.digest {
        return Err(Error::InvalidInput(
            "cohomology intervention digest differs from its content".into(),
        ));
    }
    artifact.verify(limits)?;
    if artifact.encode(limits)? != bytes {
        return Err(Error::InvalidInput(
            "cohomology intervention encoding is not canonical".into(),
        ));
    }
    Ok(())
}

fn encode_prefix(output: &mut Vec<u8>) {
    output.extend_from_slice(MAGIC);
    output.extend_from_slice(&VERSION.to_be_bytes());
    output.push(F64_BITS_CODEC);
}

fn encode_header(output: &mut Vec<u8>, artifact: &CohomologyInterventionArtifact) -> Result<()> {
    put_usize(output, artifact.vertex_count)?;
    put_usize(output, artifact.dimension)?;
    output.extend_from_slice(&artifact.scale.to_bits().to_be_bytes());
    output.extend_from_slice(&artifact.modulus.to_be_bytes());
    Ok(())
}

fn encode_scenarios(
    output: &mut Vec<u8>,
    scenarios: &[CohomologyInterventionScenario],
) -> Result<()> {
    put_usize(output, scenarios.len())?;
    for scenario in scenarios {
        encode_edges(output, &scenario.active_edges)?;
        put_usize(output, scenario.target_basis)?;
    }
    Ok(())
}

fn encode_candidates(
    output: &mut Vec<u8>,
    candidates: &[CohomologyInterventionCandidate],
) -> Result<()> {
    put_usize(output, candidates.len())?;
    for candidate in candidates {
        put_usize(output, candidate.edge.u)?;
        put_usize(output, candidate.edge.v)?;
        output.extend_from_slice(&candidate.cost.to_be_bytes());
    }
    Ok(())
}

fn encode_search_data(
    output: &mut Vec<u8>,
    artifact: &CohomologyInterventionArtifact,
) -> Result<()> {
    put_usize(output, artifact.max_edits)?;
    put_usize(output, artifact.oracle_limit)?;
    put_usize(output, artifact.node_limit)?;
    output.push(artifact.status.code());
    encode_indices(output, &edit_indices(artifact))?;
    encode_optional_u64(output, artifact.lower_bound_cost);
    encode_optional_u64(output, artifact.upper_bound_cost);
    put_usize(output, artifact.oracle_calls)?;
    put_usize(output, artifact.search_nodes)?;
    put_usize(output, artifact.cache_hits)
}

fn edit_indices(artifact: &CohomologyInterventionArtifact) -> Vec<usize> {
    artifact
        .edits
        .iter()
        .map(|edit| {
            artifact
                .candidates
                .binary_search(edit)
                .expect("validated edit")
        })
        .collect()
}

fn encode_proof_data(
    output: &mut Vec<u8>,
    artifact: &CohomologyInterventionArtifact,
) -> Result<()> {
    put_usize(output, artifact.root_blockers.len())?;
    for blocker in &artifact.root_blockers {
        encode_indices(output, blocker)?;
    }
    output.extend_from_slice(&artifact.root_blocker_bound.to_be_bytes());
    encode_usizes(output, &artifact.before_ranks)?;
    encode_usizes(output, &artifact.after_ranks)
}

struct TopologyOracle<'a> {
    vertex_count: usize,
    dimension: usize,
    scale: f64,
    modulus: u32,
    scenarios: &'a [CohomologyInterventionScenario],
    candidates: &'a [CohomologyInterventionCandidate],
    graphs: Vec<SparseDistanceMatrix>,
    spaces: Vec<CohomologySpace>,
    targets: Vec<CohomologyClassId>,
    limits: CohomologyLimits,
}

impl<'a> TopologyOracle<'a> {
    #[allow(clippy::too_many_arguments)]
    fn build(
        vertex_count: usize,
        dimension: usize,
        scale: f64,
        modulus: u32,
        scenarios: &'a [CohomologyInterventionScenario],
        candidates: &'a [CohomologyInterventionCandidate],
        limits: CohomologyLimits,
    ) -> Result<Self> {
        let mut graphs = Vec::with_capacity(scenarios.len());
        let mut spaces = Vec::with_capacity(scenarios.len());
        let mut targets = Vec::with_capacity(scenarios.len());
        for scenario in scenarios {
            let graph = graph_from_edges(vertex_count, &scenario.active_edges)?;
            let space = cohomology_space(&graph, dimension, scale, modulus, limits)?;
            let target = space
                .basis()
                .get(scenario.target_basis)
                .ok_or_else(|| {
                    Error::InvalidInput(
                        "cohomology intervention target basis is out of range".into(),
                    )
                })?
                .id;
            graphs.push(graph);
            spaces.push(space);
            targets.push(target);
        }
        Ok(Self {
            vertex_count,
            dimension,
            scale,
            modulus,
            scenarios,
            candidates,
            graphs,
            spaces,
            targets,
            limits,
        })
    }

    fn survives(&self, selected: &[usize]) -> Result<bool> {
        for scenario in 0..self.scenarios.len() {
            let (graph, space) = self.edited_space(scenario, selected)?;
            let restriction = cohomology_restriction(
                &graph,
                &space,
                &self.graphs[scenario],
                &self.spaces[scenario],
            )?;
            if restriction.image_contains(self.targets[scenario]) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn ranks(&self, selected: &[usize]) -> Result<Vec<usize>> {
        (0..self.scenarios.len())
            .map(|scenario| {
                self.edited_space(scenario, selected)
                    .map(|(_, space)| space.rank())
            })
            .collect()
    }

    fn edited_space(
        &self,
        scenario: usize,
        selected: &[usize],
    ) -> Result<(SparseDistanceMatrix, CohomologySpace)> {
        let mut edges = self.scenarios[scenario].active_edges.clone();
        edges.extend(
            selected
                .iter()
                .map(|position| self.candidates[*position].edge),
        );
        edges.sort();
        let graph = graph_from_edges(self.vertex_count, &edges)?;
        let space = cohomology_space(
            &graph,
            self.dimension,
            self.scale,
            self.modulus,
            self.limits,
        )?;
        Ok((graph, space))
    }
}

fn map_status(status: SearchStatus) -> CohomologyInterventionStatus {
    match status {
        SearchStatus::Optimal => CohomologyInterventionStatus::Optimal,
        SearchStatus::Infeasible => CohomologyInterventionStatus::Infeasible,
        SearchStatus::Incomplete => CohomologyInterventionStatus::SearchIncomplete,
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_problem(
    vertex_count: usize,
    scale: f64,
    scenarios: &[CohomologyInterventionScenario],
    candidates: &[CohomologyInterventionCandidate],
    max_edits: usize,
    oracle_limit: usize,
    node_limit: usize,
    limits: CohomologyInterventionLimits,
) -> Result<()> {
    validate_problem_scope(vertex_count, scale, scenarios, limits)?;
    for scenario in scenarios {
        validate_edges(
            vertex_count,
            &scenario.active_edges,
            limits.max_edges_per_scenario,
            "active edge",
        )?;
    }
    validate_candidates(vertex_count, candidates, limits)?;
    validate_inactive_candidates(scenarios, candidates)?;
    validate_search_limits(
        max_edits,
        candidates.len(),
        oracle_limit,
        node_limit,
        limits,
    )
}

fn validate_problem_scope(
    vertex_count: usize,
    scale: f64,
    scenarios: &[CohomologyInterventionScenario],
    limits: CohomologyInterventionLimits,
) -> Result<()> {
    if vertex_count > limits.max_vertices {
        return Err(Error::InvalidInput(
            "cohomology intervention vertex count exceeds its limit".into(),
        ));
    }
    if !scale.is_finite() || scale < 0.0 {
        return Err(Error::InvalidInput(
            "cohomology intervention scale must be finite and non-negative".into(),
        ));
    }
    if scenarios.is_empty() || scenarios.len() > limits.max_scenarios.min(FORMAT_MAX_SCENARIOS) {
        return Err(Error::InvalidInput(
            "cohomology intervention scenario count is invalid".into(),
        ));
    }
    Ok(())
}

fn validate_candidates(
    vertex_count: usize,
    candidates: &[CohomologyInterventionCandidate],
    limits: CohomologyInterventionLimits,
) -> Result<()> {
    if candidates.len() > limits.max_candidates.min(FORMAT_MAX_CANDIDATES)
        || candidates.iter().any(|candidate| candidate.cost == 0)
        || candidates.iter().any(|candidate| {
            candidate.edge.u >= candidate.edge.v || candidate.edge.v >= vertex_count
        })
        || candidates
            .windows(2)
            .any(|pair| pair[0].edge >= pair[1].edge)
    {
        return Err(Error::InvalidInput(
            "cohomology intervention candidate list is not canonical".into(),
        ));
    }
    Ok(())
}

fn validate_inactive_candidates(
    scenarios: &[CohomologyInterventionScenario],
    candidates: &[CohomologyInterventionCandidate],
) -> Result<()> {
    for scenario in scenarios {
        let active = scenario
            .active_edges
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        if candidates
            .iter()
            .any(|candidate| active.contains(&candidate.edge))
        {
            return Err(Error::InvalidInput(
                "cohomology intervention candidate is active in a scenario".into(),
            ));
        }
    }
    Ok(())
}

fn validate_search_limits(
    max_edits: usize,
    candidate_count: usize,
    oracle_limit: usize,
    node_limit: usize,
    limits: CohomologyInterventionLimits,
) -> Result<()> {
    if max_edits > candidate_count {
        return Err(Error::InvalidInput(
            "cohomology intervention edit limit exceeds the candidate count".into(),
        ));
    }
    if oracle_limit == 0
        || oracle_limit > limits.max_oracle_calls.min(FORMAT_MAX_ORACLE_CALLS)
        || node_limit == 0
        || node_limit > limits.max_search_nodes.min(FORMAT_MAX_SEARCH_NODES)
    {
        return Err(Error::InvalidInput(
            "cohomology intervention search limits are invalid".into(),
        ));
    }
    Ok(())
}

fn graph_from_edges(vertex_count: usize, edges: &[KineticEdgeKey]) -> Result<SparseDistanceMatrix> {
    let triplets = edges
        .iter()
        .map(|edge| (edge.u, edge.v, 0.0))
        .collect::<Vec<_>>();
    SparseDistanceMatrix::from_triplets(vertex_count, &triplets)
}

fn validate_edges(
    vertex_count: usize,
    edges: &[KineticEdgeKey],
    maximum: usize,
    name: &str,
) -> Result<()> {
    if edges.len() > maximum
        || edges
            .iter()
            .any(|edge| edge.u >= edge.v || edge.v >= vertex_count)
        || edges.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(Error::InvalidInput(format!(
            "cohomology intervention {name} list is not canonical or exceeds its limit"
        )));
    }
    Ok(())
}

fn encode_edges(output: &mut Vec<u8>, edges: &[KineticEdgeKey]) -> Result<()> {
    put_usize(output, edges.len())?;
    for edge in edges {
        put_usize(output, edge.u)?;
        put_usize(output, edge.v)?;
    }
    Ok(())
}

fn decode_edges(reader: &mut Reader<'_>, maximum: usize) -> Result<Vec<KineticEdgeKey>> {
    let count = reader.bounded_usize("edge count", maximum)?;
    if count > reader.remaining() / 16 {
        return Err(Error::InvalidInput(
            "cohomology intervention edge count exceeds the remaining bytes".into(),
        ));
    }
    (0..count)
        .map(|_| {
            Ok(KineticEdgeKey {
                u: reader.usize()?,
                v: reader.usize()?,
            })
        })
        .collect()
}

fn encode_indices(output: &mut Vec<u8>, indices: &[usize]) -> Result<()> {
    encode_usizes(output, indices)
}

fn decode_indices(
    reader: &mut Reader<'_>,
    candidates: usize,
    maximum: usize,
) -> Result<Vec<usize>> {
    let indices = decode_usizes(reader, maximum)?;
    if indices.iter().any(|position| *position >= candidates)
        || indices.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(Error::InvalidInput(
            "cohomology intervention candidate indices are not canonical".into(),
        ));
    }
    Ok(indices)
}

fn encode_usizes(output: &mut Vec<u8>, values: &[usize]) -> Result<()> {
    put_usize(output, values.len())?;
    for value in values {
        put_usize(output, *value)?;
    }
    Ok(())
}

fn decode_usizes(reader: &mut Reader<'_>, maximum: usize) -> Result<Vec<usize>> {
    let count = reader.bounded_usize("integer list count", maximum)?;
    if count > reader.remaining() / 8 {
        return Err(Error::InvalidInput(
            "cohomology intervention integer list exceeds the remaining bytes".into(),
        ));
    }
    (0..count).map(|_| reader.usize()).collect()
}

fn encode_optional_u64(output: &mut Vec<u8>, value: Option<u64>) {
    match value {
        Some(value) => {
            output.push(1);
            output.extend_from_slice(&value.to_be_bytes());
        }
        None => output.push(0),
    }
}

fn put_usize(output: &mut Vec<u8>, value: usize) -> Result<()> {
    let value = u64::try_from(value)
        .map_err(|_| Error::InvalidInput("artifact integer does not fit u64".into()))?;
    output.extend_from_slice(&value.to_be_bytes());
    Ok(())
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }
    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| Error::InvalidInput("artifact position overflows".into()))?;
        if end > self.bytes.len() {
            return Err(Error::InvalidInput(
                "cohomology intervention artifact is truncated".into(),
            ));
        }
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }
    fn bounded_usize(&mut self, name: &str, maximum: usize) -> Result<usize> {
        let value = self.usize()?;
        if value > maximum {
            return Err(Error::InvalidInput(format!(
                "cohomology intervention {name} exceeds its limit"
            )));
        }
        Ok(value)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn usize(&mut self) -> Result<usize> {
        usize::try_from(self.u64()?)
            .map_err(|_| Error::InvalidInput("artifact integer does not fit usize".into()))
    }
    fn optional_u64(&mut self) -> Result<Option<u64>> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.u64()?)),
            _ => Err(Error::InvalidInput(
                "cohomology intervention optional integer flag is invalid".into(),
            )),
        }
    }
    fn array32(&mut self) -> Result<[u8; 32]> {
        Ok(self.take(32)?.try_into().unwrap())
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use holos_tda_check::{ProofLimits, verify_cohomology_intervention};

    fn two_spheres() -> SparseDistanceMatrix {
        let edges = [0, 6]
            .into_iter()
            .flat_map(|offset| {
                (0..6).flat_map(move |u| {
                    ((u + 1)..6)
                        .filter(move |v| u / 2 != v / 2)
                        .map(move |v| (offset + u, offset + v, 1.0))
                })
            })
            .collect::<Vec<_>>();
        SparseDistanceMatrix::from_triplets(12, &edges).unwrap()
    }

    fn scenario(graph: &SparseDistanceMatrix, target: usize) -> CohomologyInterventionScenario {
        CohomologyInterventionScenario::from_graph(graph, 1.0, target).unwrap()
    }

    #[test]
    fn weighted_single_scenario_chooses_the_cheapest_killer() {
        let graph = two_spheres();
        let artifact = CohomologyInterventionArtifact::build(
            12,
            2,
            1.0,
            5,
            &[scenario(&graph, 0)],
            &[
                CohomologyInterventionCandidate::new(0, 1, 9),
                CohomologyInterventionCandidate::new(2, 3, 2),
                CohomologyInterventionCandidate::new(4, 5, 5),
            ],
            2,
            CohomologyInterventionLimits::default(),
        )
        .unwrap();
        assert_eq!(artifact.status(), CohomologyInterventionStatus::Optimal);
        assert_eq!(artifact.edits()[0].edge, KineticEdgeKey::new(2, 3));
        assert_eq!(artifact.lower_bound_cost(), Some(2));
        assert_eq!(artifact.upper_bound_cost(), Some(2));
    }

    #[test]
    fn one_plan_kills_targets_in_two_scenarios_and_checks_independently() {
        let graph = two_spheres();
        let limits = CohomologyInterventionLimits::default();
        let artifact = CohomologyInterventionArtifact::build(
            12,
            2,
            1.0,
            3,
            &[scenario(&graph, 0), scenario(&graph, 1)],
            &[
                CohomologyInterventionCandidate::new(0, 1, 4),
                CohomologyInterventionCandidate::new(6, 7, 7),
            ],
            2,
            limits,
        )
        .unwrap();
        assert_eq!(artifact.status(), CohomologyInterventionStatus::Optimal);
        assert_eq!(artifact.edits().len(), 2);
        assert_eq!(artifact.upper_bound_cost(), Some(11));
        assert_eq!(artifact.root_blocker_bound(), 11);
        let bytes = artifact.encode(limits).unwrap();
        let decoded = CohomologyInterventionArtifact::decode(&bytes, limits).unwrap();
        assert_eq!(decoded, artifact);
        let checked = verify_cohomology_intervention(&bytes, ProofLimits::default()).unwrap();
        assert_eq!(checked.scenarios, 2);
        assert_eq!(checked.upper_bound_cost, Some(11));
    }

    #[test]
    fn one_plan_handles_distinct_scenario_graphs() {
        let first = SparseDistanceMatrix::from_triplets(
            6,
            &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)],
        )
        .unwrap();
        let second = SparseDistanceMatrix::from_triplets(
            6,
            &[(0, 1, 1.0), (1, 4, 1.0), (4, 5, 1.0), (0, 5, 1.0)],
        )
        .unwrap();
        let artifact = CohomologyInterventionArtifact::build(
            6,
            1,
            1.0,
            3,
            &[scenario(&first, 0), scenario(&second, 0)],
            &[
                CohomologyInterventionCandidate::new(0, 2, 4),
                CohomologyInterventionCandidate::new(0, 4, 7),
            ],
            2,
            CohomologyInterventionLimits::default(),
        )
        .unwrap();
        assert_eq!(artifact.status(), CohomologyInterventionStatus::Optimal);
        assert_eq!(artifact.upper_bound_cost(), Some(11));
        assert_eq!(artifact.before_ranks(), [1, 1]);
        assert_eq!(artifact.after_ranks(), [0, 0]);
    }

    #[test]
    fn edit_limit_proves_infeasibility() {
        let graph = two_spheres();
        let limits = CohomologyInterventionLimits::default();
        let artifact = CohomologyInterventionArtifact::build(
            12,
            2,
            1.0,
            5,
            &[scenario(&graph, 0), scenario(&graph, 1)],
            &[
                CohomologyInterventionCandidate::new(0, 1, 4),
                CohomologyInterventionCandidate::new(6, 7, 7),
            ],
            1,
            limits,
        )
        .unwrap();
        assert_eq!(artifact.status(), CohomologyInterventionStatus::Infeasible);
        assert!(artifact.edits().is_empty());
        assert_eq!(artifact.root_blockers().len(), 2);
        let checked = verify_cohomology_intervention(
            &artifact.encode(limits).unwrap(),
            ProofLimits::default(),
        )
        .unwrap();
        assert_eq!(
            checked.status,
            holos_tda_check::VerifiedCohomologyInterventionStatus::Infeasible
        );
    }

    #[test]
    fn equal_cost_killers_have_a_deterministic_incumbent() {
        let graph = SparseDistanceMatrix::from_triplets(
            4,
            &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)],
        )
        .unwrap();
        let artifact = CohomologyInterventionArtifact::build(
            4,
            1,
            1.0,
            2,
            &[scenario(&graph, 0)],
            &[
                CohomologyInterventionCandidate::new(0, 2, 3),
                CohomologyInterventionCandidate::new(1, 3, 3),
            ],
            1,
            CohomologyInterventionLimits::default(),
        )
        .unwrap();
        assert_eq!(artifact.status(), CohomologyInterventionStatus::Optimal);
        assert_eq!(
            artifact.edits(),
            [CohomologyInterventionCandidate::new(0, 2, 3)]
        );
    }

    #[test]
    fn harmless_candidates_prove_global_infeasibility() {
        let graph = SparseDistanceMatrix::from_triplets(
            6,
            &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)],
        )
        .unwrap();
        let artifact = CohomologyInterventionArtifact::build(
            6,
            1,
            1.0,
            2,
            &[scenario(&graph, 0)],
            &[CohomologyInterventionCandidate::new(4, 5, 1)],
            1,
            CohomologyInterventionLimits::default(),
        )
        .unwrap();
        assert_eq!(artifact.status(), CohomologyInterventionStatus::Infeasible);
        assert_eq!(artifact.oracle_calls(), 2);
        assert!(artifact.root_blockers().is_empty());
    }

    #[test]
    fn bounded_search_returns_a_valid_gap() {
        let graph = two_spheres();
        let limits = CohomologyInterventionLimits::default()
            .with_max_oracle_calls(4)
            .with_max_search_nodes(2);
        let artifact = CohomologyInterventionArtifact::build(
            12,
            2,
            1.0,
            2,
            &[scenario(&graph, 0), scenario(&graph, 1)],
            &[
                CohomologyInterventionCandidate::new(0, 1, 4),
                CohomologyInterventionCandidate::new(2, 3, 5),
                CohomologyInterventionCandidate::new(6, 7, 7),
                CohomologyInterventionCandidate::new(8, 9, 8),
            ],
            2,
            limits,
        )
        .unwrap();
        assert_eq!(
            artifact.status(),
            CohomologyInterventionStatus::SearchIncomplete
        );
        assert!(artifact.lower_bound_cost().is_some());
        assert!(artifact.oracle_calls() <= 4);
    }

    #[test]
    fn mutations_and_truncations_are_rejected() {
        let graph = two_spheres();
        let limits = CohomologyInterventionLimits::default();
        let artifact = CohomologyInterventionArtifact::build(
            12,
            2,
            1.0,
            5,
            &[scenario(&graph, 0)],
            &[CohomologyInterventionCandidate::new(0, 1, 1)],
            1,
            limits,
        )
        .unwrap();
        let bytes = artifact.encode(limits).unwrap();
        for end in 0..bytes.len() {
            assert!(CohomologyInterventionArtifact::decode(&bytes[..end], limits).is_err());
            assert!(verify_cohomology_intervention(&bytes[..end], ProofLimits::default()).is_err());
        }
        let mut changed = bytes;
        changed[40] ^= 1;
        assert!(CohomologyInterventionArtifact::decode(&changed, limits).is_err());
        assert!(verify_cohomology_intervention(&changed, ProofLimits::default()).is_err());
    }

    #[test]
    fn necessary_sets_scale_across_many_network_scenarios() {
        let square_start = 64;
        let mut edges = Vec::new();
        for component in 0..4 {
            let offset = square_start + 4 * component;
            edges.extend([
                (offset, offset + 1, 1.0),
                (offset + 1, offset + 2, 1.0),
                (offset + 2, offset + 3, 1.0),
                (offset, offset + 3, 1.0),
            ]);
        }
        let graph = SparseDistanceMatrix::from_triplets(80, &edges).unwrap();
        let scenarios = (0..4)
            .map(|target| scenario(&graph, target))
            .collect::<Vec<_>>();
        let mut candidates = (1..64)
            .map(|vertex| CohomologyInterventionCandidate::new(0, vertex, 100 + vertex as u64))
            .collect::<Vec<_>>();
        for (component, cost) in [3, 5, 7, 11].into_iter().enumerate() {
            let offset = square_start + 4 * component;
            candidates.push(CohomologyInterventionCandidate::new(
                offset,
                offset + 2,
                cost,
            ));
        }
        candidates.sort_by_key(|candidate| candidate.edge);
        let artifact = CohomologyInterventionArtifact::build(
            80,
            1,
            1.0,
            5,
            &scenarios,
            &candidates,
            4,
            CohomologyInterventionLimits::default(),
        )
        .unwrap();
        assert_eq!(artifact.status(), CohomologyInterventionStatus::Optimal);
        assert_eq!(artifact.edits().len(), 4);
        assert_eq!(artifact.upper_bound_cost(), Some(26));
        assert_eq!(artifact.root_blocker_bound(), 26);
        assert_eq!(artifact.root_blockers().len(), 4);
        assert!(artifact.oracle_calls() < 2_000);
        let exhaustive_subsets = (1..=4)
            .map(|chosen| binomial(candidates.len(), chosen))
            .sum::<usize>();
        assert!(exhaustive_subsets > 800_000);
    }

    fn binomial(count: usize, chosen: usize) -> usize {
        (0..chosen).fold(1usize, |value, position| {
            value * (count - position) / (position + 1)
        })
    }
}
