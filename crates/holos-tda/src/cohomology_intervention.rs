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

/// One active graph and target basis position in a shared intervention.
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
    /// Positive additive installation cost.
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
    /// An oracle-call or search-node limit stopped the proof.
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

/// Self-contained weighted multi-scenario intervention certificate.
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

    /// Recompute the complete bounded search and compare every claim.
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
        if bytes.len() > limits.max_bytes || bytes.len() < 32 {
            return Err(Error::InvalidInput(
                "cohomology intervention exceeds its byte limit or is truncated".into(),
            ));
        }
        let mut reader = Reader::new(bytes);
        if reader.take(8)? != MAGIC || reader.u16()? != VERSION || reader.u8()? != F64_BITS_CODEC {
            return Err(Error::InvalidInput(
                "unsupported cohomology intervention artifact".into(),
            ));
        }
        let vertex_count = reader.usize()?;
        let dimension = reader.usize()?;
        let scale = f64::from_bits(reader.u64()?);
        let modulus = reader.u32()?;
        let scenario_count = reader.bounded_usize(
            "scenario count",
            limits.max_scenarios.min(FORMAT_MAX_SCENARIOS),
        )?;
        let mut scenarios = Vec::with_capacity(scenario_count);
        for _ in 0..scenario_count {
            scenarios.push(CohomologyInterventionScenario {
                active_edges: decode_edges(&mut reader, limits.max_edges_per_scenario)?,
                target_basis: reader.usize()?,
            });
        }
        let candidate_count = reader.bounded_usize(
            "candidate count",
            limits.max_candidates.min(FORMAT_MAX_CANDIDATES),
        )?;
        if candidate_count > reader.remaining() / 24 {
            return Err(Error::InvalidInput(
                "cohomology intervention candidate count exceeds the remaining bytes".into(),
            ));
        }
        let mut candidates = Vec::with_capacity(candidate_count);
        for _ in 0..candidate_count {
            candidates.push(CohomologyInterventionCandidate {
                edge: KineticEdgeKey {
                    u: reader.usize()?,
                    v: reader.usize()?,
                },
                cost: reader.u64()?,
            });
        }
        let max_edits = reader.usize()?;
        let oracle_limit = reader.usize()?;
        let node_limit = reader.usize()?;
        let status = CohomologyInterventionStatus::from_code(reader.u8()?)?;
        let edit_indices = decode_indices(&mut reader, candidate_count, candidate_count)?;
        let edits = edit_indices
            .iter()
            .map(|position| candidates[*position])
            .collect();
        let lower_bound_cost = reader.optional_u64()?;
        let upper_bound_cost = reader.optional_u64()?;
        let oracle_calls = reader.usize()?;
        let search_nodes = reader.usize()?;
        let cache_hits = reader.usize()?;
        let blocker_count = reader.bounded_usize(
            "root blocker count",
            limits.max_proof_terms.min(FORMAT_MAX_PROOF_TERMS),
        )?;
        let mut root_blockers = Vec::with_capacity(blocker_count);
        let mut proof_terms = 0usize;
        for _ in 0..blocker_count {
            let blocker = decode_indices(
                &mut reader,
                candidate_count,
                limits.max_proof_terms.min(FORMAT_MAX_PROOF_TERMS),
            )?;
            proof_terms = proof_terms.checked_add(blocker.len()).ok_or_else(|| {
                Error::InvalidInput("intervention proof term count overflows".into())
            })?;
            if proof_terms > limits.max_proof_terms.min(FORMAT_MAX_PROOF_TERMS) {
                return Err(Error::InvalidInput(
                    "cohomology intervention proof terms exceed their limit".into(),
                ));
            }
            root_blockers.push(blocker);
        }
        let root_blocker_bound = reader.u64()?;
        let before_ranks = decode_usizes(&mut reader, scenario_count)?;
        let after_ranks = decode_usizes(&mut reader, scenario_count)?;
        let digest = reader.array32()?;
        if reader.remaining() != 0 {
            return Err(Error::InvalidInput(
                "trailing bytes follow the cohomology intervention artifact".into(),
            ));
        }
        let artifact = Self {
            vertex_count,
            dimension,
            scale,
            modulus,
            scenarios,
            candidates,
            max_edits,
            oracle_limit,
            node_limit,
            status,
            edits,
            lower_bound_cost,
            upper_bound_cost,
            oracle_calls,
            search_nodes,
            cache_hits,
            root_blockers,
            root_blocker_bound,
            before_ranks,
            after_ranks,
            digest,
        };
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
    /// Topological oracle results served from the exact subset cache.
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
    /// Content digest of the complete claim.
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
        if self.before_ranks.len() != self.scenarios.len()
            || self.after_ranks.len() != self.scenarios.len()
            || self.oracle_calls > self.oracle_limit
            || self.search_nodes > self.node_limit
        {
            return Err(Error::InvalidInput(
                "cohomology intervention claim shape is invalid".into(),
            ));
        }
        let candidate_positions = self
            .edits
            .iter()
            .map(|edit| {
                self.candidates.binary_search(edit).map_err(|_| {
                    Error::InvalidInput("cohomology intervention edit is not a candidate".into())
                })
            })
            .collect::<Result<Vec<_>>>()?;
        if candidate_positions
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
            || self.edits.len() > self.max_edits
        {
            return Err(Error::InvalidInput(
                "cohomology intervention edit list is not canonical".into(),
            ));
        }
        let mut seen = BTreeSet::new();
        let mut proof_terms = 0usize;
        let mut bound = 0u64;
        for blocker in &self.root_blockers {
            if blocker.is_empty()
                || blocker.windows(2).any(|pair| pair[0] >= pair[1])
                || blocker
                    .iter()
                    .any(|position| *position >= self.candidates.len())
                || blocker.iter().any(|position| !seen.insert(*position))
            {
                return Err(Error::InvalidInput(
                    "cohomology intervention root blockers are not canonical and disjoint".into(),
                ));
            }
            proof_terms = proof_terms.checked_add(blocker.len()).ok_or_else(|| {
                Error::InvalidInput("cohomology intervention proof term count overflows".into())
            })?;
            let minimum = blocker
                .iter()
                .map(|position| self.candidates[*position].cost)
                .min()
                .expect("a checked blocker is nonempty");
            bound = bound.checked_add(minimum).ok_or_else(|| {
                Error::InvalidInput("cohomology intervention blocker bound overflows".into())
            })?;
        }
        if proof_terms > limits.max_proof_terms.min(FORMAT_MAX_PROOF_TERMS)
            || bound != self.root_blocker_bound
        {
            return Err(Error::InvalidInput(
                "cohomology intervention blocker claim is invalid".into(),
            ));
        }
        match self.status {
            CohomologyInterventionStatus::Optimal => {
                if self.lower_bound_cost.is_none()
                    || self.lower_bound_cost != self.upper_bound_cost
                    || self.edits.is_empty() && self.upper_bound_cost != Some(0)
                {
                    return Err(Error::InvalidInput(
                        "optimal intervention bounds are invalid".into(),
                    ));
                }
            }
            CohomologyInterventionStatus::Infeasible => {
                if !self.edits.is_empty()
                    || self.lower_bound_cost.is_some()
                    || self.upper_bound_cost.is_some()
                {
                    return Err(Error::InvalidInput(
                        "infeasible intervention carries a finite bound".into(),
                    ));
                }
            }
            CohomologyInterventionStatus::SearchIncomplete => {
                if self.lower_bound_cost.is_none()
                    || self
                        .lower_bound_cost
                        .zip(self.upper_bound_cost)
                        .is_some_and(|(lower, upper)| lower > upper)
                {
                    return Err(Error::InvalidInput(
                        "incomplete intervention bounds are invalid".into(),
                    ));
                }
            }
        }
        Ok(())
    }

    fn compute_digest(&self) -> Result<[u8; 32]> {
        let mut hash = Sha256::new();
        hash.update(b"holos-cohomology-intervention-v2");
        hash.update(self.encode_payload()?);
        Ok(hash.finalize().into())
    }

    fn encode_payload(&self) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        output.extend_from_slice(MAGIC);
        output.extend_from_slice(&VERSION.to_be_bytes());
        output.push(F64_BITS_CODEC);
        put_usize(&mut output, self.vertex_count)?;
        put_usize(&mut output, self.dimension)?;
        output.extend_from_slice(&self.scale.to_bits().to_be_bytes());
        output.extend_from_slice(&self.modulus.to_be_bytes());
        put_usize(&mut output, self.scenarios.len())?;
        for scenario in &self.scenarios {
            encode_edges(&mut output, &scenario.active_edges)?;
            put_usize(&mut output, scenario.target_basis)?;
        }
        put_usize(&mut output, self.candidates.len())?;
        for candidate in &self.candidates {
            put_usize(&mut output, candidate.edge.u)?;
            put_usize(&mut output, candidate.edge.v)?;
            output.extend_from_slice(&candidate.cost.to_be_bytes());
        }
        put_usize(&mut output, self.max_edits)?;
        put_usize(&mut output, self.oracle_limit)?;
        put_usize(&mut output, self.node_limit)?;
        output.push(self.status.code());
        encode_indices(
            &mut output,
            &self
                .edits
                .iter()
                .map(|edit| self.candidates.binary_search(edit).expect("validated edit"))
                .collect::<Vec<_>>(),
        )?;
        encode_optional_u64(&mut output, self.lower_bound_cost);
        encode_optional_u64(&mut output, self.upper_bound_cost);
        put_usize(&mut output, self.oracle_calls)?;
        put_usize(&mut output, self.search_nodes)?;
        put_usize(&mut output, self.cache_hits)?;
        put_usize(&mut output, self.root_blockers.len())?;
        for blocker in &self.root_blockers {
            encode_indices(&mut output, blocker)?;
        }
        output.extend_from_slice(&self.root_blocker_bound.to_be_bytes());
        encode_usizes(&mut output, &self.before_ranks)?;
        encode_usizes(&mut output, &self.after_ranks)?;
        Ok(output)
    }
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
    for scenario in scenarios {
        validate_edges(
            vertex_count,
            &scenario.active_edges,
            limits.max_edges_per_scenario,
            "active edge",
        )?;
    }
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
    if max_edits > candidates.len() {
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
