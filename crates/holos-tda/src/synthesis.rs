//! Proof-carrying synthesis for finite temporal cohomology specifications.
//!
//! A specification contains state-indexed subspaces and upper bounds on their
//! surviving restriction-image rank. One selected action set must satisfy all
//! states. The proof tree certifies the global cost result without replaying
//! the producer's branch-and-bound search.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use sha2::{Digest, Sha256};

use crate::monotone_search::{SearchLimits, SearchResult, SearchStatus, minimize_antitone};
use crate::{
    CohomologyLimits, CohomologySpace, CohomologySpaceId, CohomologySubspace, Error, KineticEdge,
    KineticEdgeKey, KineticFiltration, KineticLimits, Result, SparseDistanceMatrix,
    cohomology_restriction, cohomology_space,
};

const MAGIC: &[u8; 8] = b"HOLOSSYN";
const VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;
const FORMAT_MAX_STATES: usize = 4_096;
const FORMAT_MAX_ACTIONS: usize = 65_536;
const FORMAT_MAX_PROOF_NODES: usize = 10_000_000;
const FORMAT_MAX_PROOF_TERMS: usize = 100_000_000;

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

/// One finite state in a temporal or scenario-indexed specification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynthesisState {
    scenario: u64,
    step: u64,
    active_edges: Vec<KineticEdgeKey>,
    target_space: CohomologySpaceId,
    target: Vec<Vec<SynthesisCoordinate>>,
    max_surviving_rank: usize,
}

impl SynthesisState {
    /// Construct a state from one active graph and a subspace of its cohomology.
    #[allow(clippy::too_many_arguments)]
    pub fn from_subspace(
        scenario: u64,
        step: u64,
        graph: &SparseDistanceMatrix,
        scale: f64,
        space: &CohomologySpace,
        target: &CohomologySubspace,
        max_surviving_rank: usize,
    ) -> Result<Self> {
        if scale.to_bits() != space.scale().to_bits() || graph.len() != space.vertex_count() {
            return Err(Error::InvalidInput(
                "synthesis state graph and cohomology space disagree".into(),
            ));
        }
        let coordinates = space.subspace_coordinates(target)?;
        if target.rank() == 0 || max_surviving_rank >= target.rank() {
            return Err(Error::InvalidInput(
                "synthesis target must be nonempty and constrained by its rank bound".into(),
            ));
        }
        Ok(Self {
            scenario,
            step,
            active_edges: graph
                .edges()
                .filter(|edge| edge.2 <= scale)
                .map(|(u, v, _)| KineticEdgeKey { u, v })
                .collect(),
            target_space: target.space(),
            target: coordinates
                .into_iter()
                .map(|row| {
                    row.into_iter()
                        .map(|(basis, coefficient)| SynthesisCoordinate { basis, coefficient })
                        .collect()
                })
                .collect(),
            max_surviving_rank,
        })
    }

    /// Scenario identifier used to group related time steps.
    pub fn scenario(&self) -> u64 {
        self.scenario
    }

    /// Ordered step identifier inside the scenario.
    pub fn step(&self) -> u64 {
        self.step
    }

    /// Canonical active edge list.
    pub fn active_edges(&self) -> &[KineticEdgeKey] {
        &self.active_edges
    }

    /// Canonical target subspace generators.
    pub fn target(&self) -> &[Vec<SynthesisCoordinate>] {
        &self.target
    }

    /// Largest allowed surviving intersection rank.
    pub fn max_surviving_rank(&self) -> usize {
        self.max_surviving_rank
    }
}

/// A finite basis-independent topological specification.
#[derive(Debug, Clone, PartialEq)]
pub struct TopologicalSpecification {
    vertex_count: usize,
    dimension: usize,
    scale: f64,
    modulus: u32,
    source: SynthesisSource,
    states: Vec<SynthesisState>,
}

impl TopologicalSpecification {
    /// Construct a specification over one vertex set, scale, dimension, and field.
    pub fn new(
        vertex_count: usize,
        dimension: usize,
        scale: f64,
        modulus: u32,
        states: Vec<SynthesisState>,
    ) -> Self {
        Self {
            vertex_count,
            dimension,
            scale,
            modulus,
            source: SynthesisSource::Finite,
            states,
        }
    }

    /// Compile an affine trajectory into exact finite rank obligations.
    ///
    /// Each retained state requires the complete `H^dimension` restriction
    /// image to have rank at most `maximum_rank`. States already below the
    /// bound are omitted. Endpoints and exact event complexes are included.
    pub fn from_kinetic_rank_ceiling(
        filtration: &KineticFiltration,
        scenario: u64,
        dimension: usize,
        scale: f64,
        modulus: u32,
        maximum_rank: usize,
        limits: CohomologyLimits,
    ) -> Result<Self> {
        let states = compile_kinetic_states(
            filtration,
            scenario,
            dimension,
            scale,
            modulus,
            maximum_rank,
            limits,
            usize::MAX,
        )?;
        Ok(Self {
            vertex_count: filtration.vertex_count(),
            dimension,
            scale,
            modulus,
            source: SynthesisSource::Affine {
                scenario,
                edges: filtration.edges().to_vec(),
                start: filtration.start(),
                end: filtration.end(),
                maximum_rank,
            },
            states,
        })
    }

    /// Number of vertices shared by all states.
    pub fn vertex_count(&self) -> usize {
        self.vertex_count
    }

    /// Cohomology dimension used by every state.
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

    /// Origin and completeness scope of the finite state list.
    pub fn source(&self) -> &SynthesisSource {
        &self.source
    }

    /// State obligations in canonical scenario and step order.
    pub fn states(&self) -> &[SynthesisState] {
        &self.states
    }
}

/// One selectable edge action and the states where it applies.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SynthesisAction {
    /// Edge added by this action.
    pub edge: KineticEdgeKey,
    /// Positive additive action cost.
    pub cost: u64,
    states: Vec<usize>,
}

/// One connected component of the state-action incidence relation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynthesisComponent {
    states: Vec<usize>,
    actions: Vec<usize>,
}

impl SynthesisComponent {
    /// State indices in this component.
    pub fn states(&self) -> &[usize] {
        &self.states
    }

    /// Action indices in this component.
    pub fn actions(&self) -> &[usize] {
        &self.actions
    }
}

impl SynthesisAction {
    /// Construct an action and canonicalize its affected state indices.
    pub fn new(u: usize, v: usize, cost: u64, mut states: Vec<usize>) -> Self {
        states.sort_unstable();
        states.dedup();
        Self {
            edge: KineticEdgeKey::new(u, v),
            cost,
            states,
        }
    }

    /// State indices where this edge is added.
    pub fn states(&self) -> &[usize] {
        &self.states
    }

    /// Construct an action that applies to every retained state.
    pub fn throughout(
        u: usize,
        v: usize,
        cost: u64,
        specification: &TopologicalSpecification,
    ) -> Self {
        Self::new(u, v, cost, (0..specification.states.len()).collect())
    }
}

impl TopologicalSpecification {
    /// Decompose the state-action incidence relation into components.
    ///
    /// Each state predicate depends only on actions in its returned component.
    /// Components share neither an action nor a state obligation.
    pub fn components(&self, actions: &[SynthesisAction]) -> Result<Vec<SynthesisComponent>> {
        if actions.iter().any(|action| {
            action.states.is_empty()
                || action
                    .states
                    .iter()
                    .any(|state| *state >= self.states.len())
        }) {
            return Err(Error::InvalidInput(
                "synthesis action names a state outside its specification".into(),
            ));
        }
        let action_offset = self.states.len();
        let mut parent = (0..self.states.len() + actions.len()).collect::<Vec<_>>();
        for (action, candidate) in actions.iter().enumerate() {
            for &state in &candidate.states {
                union_sets(&mut parent, state, action_offset + action);
            }
        }
        let mut components = BTreeMap::<usize, SynthesisComponent>::new();
        for state in 0..self.states.len() {
            let root = find_set(&mut parent, state);
            components
                .entry(root)
                .or_insert_with(|| SynthesisComponent {
                    states: Vec::new(),
                    actions: Vec::new(),
                })
                .states
                .push(state);
        }
        for action in 0..actions.len() {
            let root = find_set(&mut parent, action_offset + action);
            components
                .entry(root)
                .or_insert_with(|| SynthesisComponent {
                    states: Vec::new(),
                    actions: Vec::new(),
                })
                .actions
                .push(action);
        }
        let mut output = components.into_values().collect::<Vec<_>>();
        output.sort_by_key(|component| {
            (
                component.states.first().copied().unwrap_or(usize::MAX),
                component.actions.first().copied().unwrap_or(usize::MAX),
            )
        });
        Ok(output)
    }
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
enum BoundKind {
    Cost,
    Edits,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProofNode {
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

/// Synthesis result.
#[derive(Debug, Clone, PartialEq)]
pub struct SynthesisArtifact {
    specification: TopologicalSpecification,
    actions: Vec<SynthesisAction>,
    max_edits: usize,
    oracle_limit: usize,
    node_limit: usize,
    status: SynthesisStatus,
    selected: Vec<usize>,
    lower_bound_cost: Option<u64>,
    upper_bound_cost: Option<u64>,
    producer_oracle_calls: usize,
    producer_search_nodes: usize,
    producer_cache_hits: usize,
    root_blockers: Vec<Vec<usize>>,
    before_ranks: Vec<usize>,
    after_ranks: Vec<usize>,
    proof: Option<ProofNode>,
    proof_nodes: usize,
    proof_topology_checks: usize,
    digest: [u8; 32],
}

struct SynthesisHeader {
    vertex_count: usize,
    dimension: usize,
    scale: f64,
    modulus: u32,
    source: SynthesisSource,
}

struct WorkLimits {
    oracle: usize,
    nodes: usize,
}

struct SelectionData {
    status: SynthesisStatus,
    selected: Vec<usize>,
    lower_bound_cost: Option<u64>,
    upper_bound_cost: Option<u64>,
}

struct ProducerWork {
    oracle_calls: usize,
    search_nodes: usize,
    cache_hits: usize,
}

struct SearchData {
    max_edits: usize,
    limits: WorkLimits,
    selection: SelectionData,
    work: ProducerWork,
}

struct ProofData {
    root_blockers: Vec<Vec<usize>>,
    before_ranks: Vec<usize>,
    after_ranks: Vec<usize>,
    proof: Option<ProofNode>,
    nodes: usize,
    topology_checks: usize,
}

struct BuiltProof {
    proof: Option<ProofNode>,
    nodes: usize,
    topology_checks: usize,
}

impl SynthesisArtifact {
    /// Solve a minimum-cost action problem and build its proof tree.
    pub fn build(
        specification: TopologicalSpecification,
        actions: Vec<SynthesisAction>,
        max_edits: usize,
        limits: SynthesisLimits,
    ) -> Result<Self> {
        Self::from_problem(
            specification,
            actions,
            max_edits,
            limits.max_oracle_calls,
            limits.max_search_nodes,
            limits,
        )
    }

    fn from_problem(
        specification: TopologicalSpecification,
        actions: Vec<SynthesisAction>,
        max_edits: usize,
        oracle_limit: usize,
        node_limit: usize,
        limits: SynthesisLimits,
    ) -> Result<Self> {
        validate_problem(&specification, &actions, oracle_limit, node_limit, limits)?;
        let oracle = TopologyOracle::build(&specification, &actions, limits.cohomology)?;
        let costs = actions.iter().map(|action| action.cost).collect::<Vec<_>>();
        let search = minimize_antitone(
            &costs,
            max_edits,
            SearchLimits {
                oracle_calls: oracle_limit,
                search_nodes: node_limit,
            },
            |selected| oracle.survives(selected),
        )?;
        let status = synthesis_status(search.status);
        let before_ranks = oracle.target_ranks();
        let after_ranks = oracle.intersection_ranks(&search.selected)?;
        let built = build_synthesis_proof(
            status,
            &search,
            &costs,
            max_edits,
            actions.len(),
            &oracle,
            limits,
        )?;
        let mut artifact = Self {
            specification,
            actions,
            max_edits,
            oracle_limit,
            node_limit,
            status,
            selected: search.selected,
            lower_bound_cost: search.lower_bound,
            upper_bound_cost: search.upper_bound,
            producer_oracle_calls: search.oracle_calls,
            producer_search_nodes: search.search_nodes,
            producer_cache_hits: search.cache_hits,
            root_blockers: search.root_blockers,
            before_ranks,
            after_ranks,
            proof: built.proof,
            proof_nodes: built.nodes,
            proof_topology_checks: built.topology_checks,
            digest: [0; 32],
        };
        artifact.verify(limits)?;
        artifact.digest = artifact.compute_digest()?;
        Ok(artifact)
    }

    /// Verify the claim and proof tree.
    pub fn verify(&self, limits: SynthesisLimits) -> Result<()> {
        validate_problem(
            &self.specification,
            &self.actions,
            self.oracle_limit,
            self.node_limit,
            limits,
        )?;
        verify_result_shape(self)?;
        let oracle = TopologyOracle::build(&self.specification, &self.actions, limits.cohomology)?;
        verify_rank_claims(self, &oracle)?;
        let costs = self
            .actions
            .iter()
            .map(|action| action.cost)
            .collect::<Vec<_>>();
        let selected_cost = selected_cost(&costs, &self.selected)?;
        let selected_feasible = !oracle.survives(&self.selected)?;
        validate_root_blockers(&oracle, &self.root_blockers, &costs, self.lower_bound_cost)?;
        let mut verifier = ProofVerifier::new(
            &costs,
            self.max_edits,
            self.upper_bound_cost,
            &oracle,
            limits,
        );
        verify_status_claim(self, selected_cost, selected_feasible, &mut verifier)?;
        verify_proof_work(self, &verifier)
    }

    /// Finite specification bound to this result.
    pub fn specification(&self) -> &TopologicalSpecification {
        &self.specification
    }

    /// Canonical action list.
    pub fn actions(&self) -> &[SynthesisAction] {
        &self.actions
    }

    /// Search completeness status.
    pub fn status(&self) -> SynthesisStatus {
        self.status
    }

    /// Selected action indices.
    pub fn selected(&self) -> &[usize] {
        &self.selected
    }

    /// Proved lower cost bound, when finite.
    pub fn lower_bound_cost(&self) -> Option<u64> {
        self.lower_bound_cost
    }

    /// Feasible incumbent cost, when present.
    pub fn upper_bound_cost(&self) -> Option<u64> {
        self.upper_bound_cost
    }

    /// Producer topology calls made by branch-and-bound.
    pub fn producer_oracle_calls(&self) -> usize {
        self.producer_oracle_calls
    }

    /// Producer branch nodes visited by branch-and-bound.
    pub fn producer_search_nodes(&self) -> usize {
        self.producer_search_nodes
    }

    /// Topology checks required by the proof tree.
    pub fn proof_topology_checks(&self) -> usize {
        self.proof_topology_checks
    }

    /// Proof-tree node count.
    pub fn proof_nodes(&self) -> usize {
        self.proof_nodes
    }

    /// Target ranks before editing in state order.
    pub fn before_ranks(&self) -> &[usize] {
        &self.before_ranks
    }

    /// Surviving target ranks after editing in state order.
    pub fn after_ranks(&self) -> &[usize] {
        &self.after_ranks
    }

    /// Content digest of the claim.
    pub fn digest(&self) -> &[u8; 32] {
        &self.digest
    }

    /// Encode canonical `HOLOSSYN` version 1 bytes.
    pub fn encode(&self, limits: SynthesisLimits) -> Result<Vec<u8>> {
        self.verify(limits)?;
        let mut output = self.encode_payload()?;
        output.extend_from_slice(&self.digest);
        if output.len() > limits.max_bytes {
            return Err(Error::InvalidInput(
                "synthesis artifact exceeds its byte limit".into(),
            ));
        }
        Ok(output)
    }

    /// Decode and verify canonical `HOLOSSYN` version 1 bytes.
    pub fn decode(bytes: &[u8], limits: SynthesisLimits) -> Result<Self> {
        validate_artifact_size(bytes, limits)?;
        let mut reader = Reader::new(bytes);
        decode_prefix(&mut reader)?;
        let header = decode_header(&mut reader, limits)?;
        let states = decode_states(&mut reader, limits)?;
        let actions = decode_actions(&mut reader, states.len(), limits)?;
        let search = decode_search_data(&mut reader, actions.len())?;
        let proof = decode_proof_data(&mut reader, actions.len(), states.len(), limits)?;
        let digest = decode_trailer(&mut reader)?;
        let artifact = Self {
            specification: TopologicalSpecification {
                vertex_count: header.vertex_count,
                dimension: header.dimension,
                scale: header.scale,
                modulus: header.modulus,
                source: header.source,
                states,
            },
            actions,
            max_edits: search.max_edits,
            oracle_limit: search.limits.oracle,
            node_limit: search.limits.nodes,
            status: search.selection.status,
            selected: search.selection.selected,
            lower_bound_cost: search.selection.lower_bound_cost,
            upper_bound_cost: search.selection.upper_bound_cost,
            producer_oracle_calls: search.work.oracle_calls,
            producer_search_nodes: search.work.search_nodes,
            producer_cache_hits: search.work.cache_hits,
            root_blockers: proof.root_blockers,
            before_ranks: proof.before_ranks,
            after_ranks: proof.after_ranks,
            proof: proof.proof,
            proof_nodes: proof.nodes,
            proof_topology_checks: proof.topology_checks,
            digest,
        };
        validate_decoded_artifact(&artifact, bytes, limits)?;
        Ok(artifact)
    }

    fn compute_digest(&self) -> Result<[u8; 32]> {
        let payload = self.encode_payload()?;
        let mut hash = Sha256::new();
        hash.update(b"holos-synthesis-artifact-v1");
        hash.update(payload);
        Ok(hash.finalize().into())
    }

    fn encode_payload(&self) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        encode_prefix(&mut output);
        encode_specification(&mut output, &self.specification)?;
        encode_actions(&mut output, &self.actions)?;
        encode_search_data(&mut output, self)?;
        encode_proof_data(&mut output, self)?;
        Ok(output)
    }
}

fn verify_result_shape(artifact: &SynthesisArtifact) -> Result<()> {
    let invalid_selection = artifact.selected.len() > artifact.max_edits
        || artifact
            .selected
            .iter()
            .any(|index| *index >= artifact.actions.len())
        || artifact.selected.windows(2).any(|pair| pair[0] >= pair[1]);
    if artifact.producer_oracle_calls > artifact.oracle_limit
        || artifact.producer_search_nodes > artifact.node_limit
        || invalid_selection
    {
        Err(Error::InvalidInput(
            "synthesis result shape or producer work is invalid".into(),
        ))
    } else {
        Ok(())
    }
}

fn verify_rank_claims(artifact: &SynthesisArtifact, oracle: &TopologyOracle<'_>) -> Result<()> {
    if artifact.before_ranks != oracle.target_ranks()
        || artifact.after_ranks != oracle.intersection_ranks(&artifact.selected)?
    {
        Err(Error::InvalidInput(
            "synthesis rank claims differ from exact restriction images".into(),
        ))
    } else {
        Ok(())
    }
}

fn verify_status_claim(
    artifact: &SynthesisArtifact,
    selected_cost: u64,
    selected_feasible: bool,
    verifier: &mut ProofVerifier<'_>,
) -> Result<()> {
    match artifact.status {
        SynthesisStatus::Optimal => {
            verify_optimal_claim(artifact, selected_cost, selected_feasible, verifier)
        }
        SynthesisStatus::Infeasible => {
            verify_infeasible_claim(artifact, selected_feasible, verifier)
        }
        SynthesisStatus::SearchIncomplete => {
            verify_incomplete_claim(artifact, selected_cost, selected_feasible)
        }
    }
}

fn verify_optimal_claim(
    artifact: &SynthesisArtifact,
    selected_cost: u64,
    selected_feasible: bool,
    verifier: &mut ProofVerifier<'_>,
) -> Result<()> {
    if !selected_feasible
        || artifact.lower_bound_cost != Some(selected_cost)
        || artifact.upper_bound_cost != Some(selected_cost)
    {
        return Err(Error::InvalidInput(
            "optimal synthesis result has an invalid incumbent or bound".into(),
        ));
    }
    verifier.verify_root(required_proof(
        artifact,
        "optimal synthesis result has no proof tree",
    )?)
}

fn verify_infeasible_claim(
    artifact: &SynthesisArtifact,
    selected_feasible: bool,
    verifier: &mut ProofVerifier<'_>,
) -> Result<()> {
    if !artifact.selected.is_empty()
        || artifact.lower_bound_cost.is_some()
        || artifact.upper_bound_cost.is_some()
        || selected_feasible
    {
        return Err(Error::InvalidInput(
            "infeasible synthesis result has an incumbent or finite bound".into(),
        ));
    }
    verifier.verify_root(required_proof(
        artifact,
        "infeasible synthesis result has no proof tree",
    )?)
}

fn verify_incomplete_claim(
    artifact: &SynthesisArtifact,
    selected_cost: u64,
    selected_feasible: bool,
) -> Result<()> {
    let invalid_gap = artifact
        .lower_bound_cost
        .zip(artifact.upper_bound_cost)
        .is_some_and(|(lower, upper)| lower > upper);
    if artifact.proof.is_some()
        || artifact.upper_bound_cost.is_some() != selected_feasible
        || artifact
            .upper_bound_cost
            .is_some_and(|cost| cost != selected_cost)
        || invalid_gap
    {
        Err(Error::InvalidInput(
            "incomplete synthesis result has an invalid gap".into(),
        ))
    } else {
        Ok(())
    }
}

fn required_proof<'a>(artifact: &'a SynthesisArtifact, message: &str) -> Result<&'a ProofNode> {
    artifact
        .proof
        .as_ref()
        .ok_or_else(|| Error::InvalidInput(message.into()))
}

fn verify_proof_work(artifact: &SynthesisArtifact, verifier: &ProofVerifier<'_>) -> Result<()> {
    if artifact.proof_nodes != verifier.nodes || artifact.proof_topology_checks != verifier.checks {
        Err(Error::InvalidInput(
            "synthesis proof work differs from the checked tree".into(),
        ))
    } else {
        Ok(())
    }
}

fn synthesis_status(status: SearchStatus) -> SynthesisStatus {
    match status {
        SearchStatus::Optimal => SynthesisStatus::Optimal,
        SearchStatus::Infeasible => SynthesisStatus::Infeasible,
        SearchStatus::Incomplete => SynthesisStatus::SearchIncomplete,
    }
}

fn build_synthesis_proof(
    status: SynthesisStatus,
    search: &SearchResult,
    costs: &[u64],
    max_edits: usize,
    action_count: usize,
    oracle: &TopologyOracle<'_>,
    limits: SynthesisLimits,
) -> Result<BuiltProof> {
    if status == SynthesisStatus::SearchIncomplete {
        return Ok(BuiltProof {
            proof: None,
            nodes: 0,
            topology_checks: 0,
        });
    }
    let cutoff = proof_cutoff(status, search.upper_bound)?;
    let mut builder = ProofBuilder::new(costs, max_edits, cutoff, oracle, limits);
    let proof = builder.prove(Vec::new(), (0..action_count).collect(), 0)?;
    Ok(BuiltProof {
        topology_checks: proof_topology_checks(&proof),
        proof: Some(proof),
        nodes: builder.nodes,
    })
}

fn proof_cutoff(status: SynthesisStatus, upper_bound: Option<u64>) -> Result<Option<u64>> {
    match status {
        SynthesisStatus::Optimal => upper_bound
            .map(Some)
            .ok_or_else(|| Error::InvalidInput("optimal synthesis result has no cost".into())),
        SynthesisStatus::Infeasible | SynthesisStatus::SearchIncomplete => Ok(None),
    }
}

fn validate_artifact_size(bytes: &[u8], limits: SynthesisLimits) -> Result<()> {
    if bytes.len() > limits.max_bytes || bytes.len() < 32 {
        Err(Error::InvalidInput(
            "synthesis artifact exceeds its byte limit or is truncated".into(),
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
        Err(Error::InvalidInput("unsupported synthesis artifact".into()))
    } else {
        Ok(())
    }
}

fn decode_header(reader: &mut Reader<'_>, limits: SynthesisLimits) -> Result<SynthesisHeader> {
    Ok(SynthesisHeader {
        vertex_count: reader.bounded_usize("vertex count", limits.max_vertices)?,
        dimension: reader.bounded_usize("dimension", limits.cohomology.max_dimension)?,
        scale: f64::from_bits(reader.u64()?),
        modulus: reader.u32()?,
        source: decode_source(reader, limits)?,
    })
}

fn decode_states(reader: &mut Reader<'_>, limits: SynthesisLimits) -> Result<Vec<SynthesisState>> {
    let count = reader.bounded_usize("state count", limits.max_states.min(FORMAT_MAX_STATES))?;
    let mut states = Vec::with_capacity(count);
    let mut target_terms = 0usize;
    for _ in 0..count {
        states.push(decode_state(reader, &mut target_terms, limits)?);
    }
    Ok(states)
}

fn decode_state(
    reader: &mut Reader<'_>,
    target_terms: &mut usize,
    limits: SynthesisLimits,
) -> Result<SynthesisState> {
    Ok(SynthesisState {
        scenario: reader.u64()?,
        step: reader.u64()?,
        active_edges: decode_edges(reader, limits.max_edges_per_state)?,
        target_space: CohomologySpaceId::from_bytes(reader.array32()?),
        target: decode_target(reader, target_terms, limits)?,
        max_surviving_rank: reader.usize()?,
    })
}

fn decode_target(
    reader: &mut Reader<'_>,
    target_terms: &mut usize,
    limits: SynthesisLimits,
) -> Result<Vec<Vec<SynthesisCoordinate>>> {
    let count = reader.bounded_usize("target row count", limits.max_terms)?;
    let mut target = Vec::with_capacity(count);
    for _ in 0..count {
        target.push(decode_target_row(reader, target_terms, limits)?);
    }
    Ok(target)
}

fn decode_target_row(
    reader: &mut Reader<'_>,
    target_terms: &mut usize,
    limits: SynthesisLimits,
) -> Result<Vec<SynthesisCoordinate>> {
    let count = reader.bounded_usize("target row term count", limits.max_terms)?;
    add_proof_terms(target_terms, count, limits.max_terms, "synthesis target")?;
    if count > reader.remaining() / 12 {
        return Err(Error::InvalidInput(
            "synthesis target terms exceed their limit or remaining bytes".into(),
        ));
    }
    (0..count)
        .map(|_| {
            Ok(SynthesisCoordinate {
                basis: reader.usize()?,
                coefficient: reader.u32()?,
            })
        })
        .collect()
}

fn decode_actions(
    reader: &mut Reader<'_>,
    state_count: usize,
    limits: SynthesisLimits,
) -> Result<Vec<SynthesisAction>> {
    let count = reader.bounded_usize("action count", limits.max_actions.min(FORMAT_MAX_ACTIONS))?;
    let mut actions = Vec::with_capacity(count);
    for _ in 0..count {
        actions.push(decode_action(reader, state_count)?);
    }
    Ok(actions)
}

fn decode_action(reader: &mut Reader<'_>, state_count: usize) -> Result<SynthesisAction> {
    Ok(SynthesisAction {
        edge: KineticEdgeKey {
            u: reader.usize()?,
            v: reader.usize()?,
        },
        cost: reader.u64()?,
        states: decode_indices(reader, state_count, state_count)?,
    })
}

fn decode_search_data(reader: &mut Reader<'_>, action_count: usize) -> Result<SearchData> {
    let max_edits = reader.usize()?;
    Ok(SearchData {
        max_edits,
        limits: decode_work_limits(reader)?,
        selection: decode_selection_data(reader, action_count)?,
        work: decode_producer_work(reader)?,
    })
}

fn decode_work_limits(reader: &mut Reader<'_>) -> Result<WorkLimits> {
    Ok(WorkLimits {
        oracle: reader.usize()?,
        nodes: reader.usize()?,
    })
}

fn decode_selection_data(reader: &mut Reader<'_>, action_count: usize) -> Result<SelectionData> {
    Ok(SelectionData {
        status: SynthesisStatus::from_code(reader.u8()?)?,
        selected: decode_indices(reader, action_count, action_count)?,
        lower_bound_cost: reader.optional_u64()?,
        upper_bound_cost: reader.optional_u64()?,
    })
}

fn decode_producer_work(reader: &mut Reader<'_>) -> Result<ProducerWork> {
    Ok(ProducerWork {
        oracle_calls: reader.usize()?,
        search_nodes: reader.usize()?,
        cache_hits: reader.usize()?,
    })
}

fn decode_proof_data(
    reader: &mut Reader<'_>,
    action_count: usize,
    state_count: usize,
    limits: SynthesisLimits,
) -> Result<ProofData> {
    let mut terms = 0usize;
    let root_blockers = decode_root_blockers(reader, action_count, &mut terms, limits)?;
    let before_ranks = decode_usizes(reader, state_count)?;
    let after_ranks = decode_usizes(reader, state_count)?;
    let mut nodes = 0usize;
    let proof = decode_optional_proof(reader, action_count, &mut nodes, &mut terms, limits)?;
    let claimed_nodes = reader.usize()?;
    let topology_checks = reader.usize()?;
    if claimed_nodes != nodes {
        return Err(Error::InvalidInput(
            "synthesis proof count or digest differs from its content".into(),
        ));
    }
    Ok(ProofData {
        root_blockers,
        before_ranks,
        after_ranks,
        proof,
        nodes,
        topology_checks,
    })
}

fn decode_root_blockers(
    reader: &mut Reader<'_>,
    action_count: usize,
    terms: &mut usize,
    limits: SynthesisLimits,
) -> Result<Vec<Vec<usize>>> {
    let count = reader.bounded_usize("root blocker count", limits.max_terms)?;
    let mut blockers = Vec::with_capacity(count);
    for _ in 0..count {
        let blocker = decode_indices(reader, action_count, limits.max_terms)?;
        add_proof_terms(terms, blocker.len(), limits.max_terms, "synthesis proof")?;
        blockers.push(blocker);
    }
    Ok(blockers)
}

fn decode_optional_proof(
    reader: &mut Reader<'_>,
    action_count: usize,
    nodes: &mut usize,
    terms: &mut usize,
    limits: SynthesisLimits,
) -> Result<Option<ProofNode>> {
    match reader.u8()? {
        0 => Ok(None),
        1 => decode_proof(reader, action_count, 0, nodes, terms, limits).map(Some),
        _ => Err(Error::InvalidInput(
            "synthesis proof-presence flag is invalid".into(),
        )),
    }
}

fn add_proof_terms(total: &mut usize, count: usize, maximum: usize, subject: &str) -> Result<()> {
    *total = total
        .checked_add(count)
        .ok_or_else(|| Error::InvalidInput(format!("{subject} term count overflows")))?;
    if *total > maximum {
        Err(Error::InvalidInput(format!(
            "{subject} terms exceed their limit"
        )))
    } else {
        Ok(())
    }
}

fn decode_trailer(reader: &mut Reader<'_>) -> Result<[u8; 32]> {
    let digest = reader.array32()?;
    if reader.remaining() != 0 {
        Err(Error::InvalidInput(
            "trailing bytes follow the synthesis artifact".into(),
        ))
    } else {
        Ok(digest)
    }
}

fn validate_decoded_artifact(
    artifact: &SynthesisArtifact,
    bytes: &[u8],
    limits: SynthesisLimits,
) -> Result<()> {
    if artifact.compute_digest()? != artifact.digest {
        return Err(Error::InvalidInput(
            "synthesis proof count or digest differs from its content".into(),
        ));
    }
    artifact.verify(limits)?;
    if artifact.encode(limits)? != bytes {
        return Err(Error::InvalidInput(
            "synthesis artifact encoding is not canonical".into(),
        ));
    }
    Ok(())
}

fn encode_prefix(output: &mut Vec<u8>) {
    output.extend_from_slice(MAGIC);
    output.extend_from_slice(&VERSION.to_be_bytes());
    output.push(F64_BITS_CODEC);
}

fn encode_specification(
    output: &mut Vec<u8>,
    specification: &TopologicalSpecification,
) -> Result<()> {
    put_usize(output, specification.vertex_count)?;
    put_usize(output, specification.dimension)?;
    output.extend_from_slice(&specification.scale.to_bits().to_be_bytes());
    output.extend_from_slice(&specification.modulus.to_be_bytes());
    encode_source(output, &specification.source)?;
    put_usize(output, specification.states.len())?;
    for state in &specification.states {
        encode_state(output, state)?;
    }
    Ok(())
}

fn encode_state(output: &mut Vec<u8>, state: &SynthesisState) -> Result<()> {
    output.extend_from_slice(&state.scenario.to_be_bytes());
    output.extend_from_slice(&state.step.to_be_bytes());
    encode_edges(output, &state.active_edges)?;
    output.extend_from_slice(state.target_space.as_bytes());
    encode_target(output, &state.target)?;
    put_usize(output, state.max_surviving_rank)
}

fn encode_target(output: &mut Vec<u8>, target: &[Vec<SynthesisCoordinate>]) -> Result<()> {
    put_usize(output, target.len())?;
    for row in target {
        encode_target_row(output, row)?;
    }
    Ok(())
}

fn encode_target_row(output: &mut Vec<u8>, row: &[SynthesisCoordinate]) -> Result<()> {
    put_usize(output, row.len())?;
    for term in row {
        put_usize(output, term.basis)?;
        output.extend_from_slice(&term.coefficient.to_be_bytes());
    }
    Ok(())
}

fn encode_actions(output: &mut Vec<u8>, actions: &[SynthesisAction]) -> Result<()> {
    put_usize(output, actions.len())?;
    for action in actions {
        encode_action(output, action)?;
    }
    Ok(())
}

fn encode_action(output: &mut Vec<u8>, action: &SynthesisAction) -> Result<()> {
    put_usize(output, action.edge.u)?;
    put_usize(output, action.edge.v)?;
    output.extend_from_slice(&action.cost.to_be_bytes());
    encode_usizes(output, &action.states)
}

fn encode_search_data(output: &mut Vec<u8>, artifact: &SynthesisArtifact) -> Result<()> {
    put_usize(output, artifact.max_edits)?;
    put_usize(output, artifact.oracle_limit)?;
    put_usize(output, artifact.node_limit)?;
    output.push(artifact.status.code());
    encode_usizes(output, &artifact.selected)?;
    encode_optional_u64(output, artifact.lower_bound_cost);
    encode_optional_u64(output, artifact.upper_bound_cost);
    put_usize(output, artifact.producer_oracle_calls)?;
    put_usize(output, artifact.producer_search_nodes)?;
    put_usize(output, artifact.producer_cache_hits)
}

fn encode_proof_data(output: &mut Vec<u8>, artifact: &SynthesisArtifact) -> Result<()> {
    encode_root_blockers(output, &artifact.root_blockers)?;
    encode_usizes(output, &artifact.before_ranks)?;
    encode_usizes(output, &artifact.after_ranks)?;
    encode_optional_proof(output, artifact.proof.as_ref())?;
    put_usize(output, artifact.proof_nodes)?;
    put_usize(output, artifact.proof_topology_checks)
}

fn encode_root_blockers(output: &mut Vec<u8>, blockers: &[Vec<usize>]) -> Result<()> {
    put_usize(output, blockers.len())?;
    for blocker in blockers {
        encode_usizes(output, blocker)?;
    }
    Ok(())
}

fn encode_optional_proof(output: &mut Vec<u8>, proof: Option<&ProofNode>) -> Result<()> {
    match proof {
        Some(proof) => {
            output.push(1);
            encode_proof(output, proof)
        }
        None => {
            output.push(0);
            Ok(())
        }
    }
}

struct TopologyOracle<'a> {
    specification: &'a TopologicalSpecification,
    actions: &'a [SynthesisAction],
    graphs: Vec<SparseDistanceMatrix>,
    spaces: Vec<CohomologySpace>,
    targets: Vec<CohomologySubspace>,
    limits: CohomologyLimits,
    cache: RefCell<BTreeMap<(usize, Vec<usize>), usize>>,
}

impl<'a> TopologyOracle<'a> {
    fn build(
        specification: &'a TopologicalSpecification,
        actions: &'a [SynthesisAction],
        limits: CohomologyLimits,
    ) -> Result<Self> {
        let mut graphs = Vec::with_capacity(specification.states.len());
        let mut spaces = Vec::with_capacity(specification.states.len());
        let mut targets = Vec::with_capacity(specification.states.len());
        for state in &specification.states {
            let graph = graph_from_edges(specification.vertex_count, &state.active_edges)?;
            let space = cohomology_space(
                &graph,
                specification.dimension,
                specification.scale,
                specification.modulus,
                limits,
            )?;
            if space.id() != state.target_space {
                return Err(Error::InvalidInput(
                    "synthesis state target is bound to a different active complex".into(),
                ));
            }
            let rows = state
                .target
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|term| (term.basis, term.coefficient))
                        .collect()
                })
                .collect::<Vec<_>>();
            let target = space.subspace_from_coordinates(&rows)?;
            if target.rank() != rows.len() || state.max_surviving_rank >= target.rank() {
                return Err(Error::InvalidInput(
                    "synthesis state target is not a canonical constrained subspace".into(),
                ));
            }
            graphs.push(graph);
            spaces.push(space);
            targets.push(target);
        }
        Ok(Self {
            specification,
            actions,
            graphs,
            spaces,
            targets,
            limits,
            cache: RefCell::new(BTreeMap::new()),
        })
    }

    fn target_ranks(&self) -> Vec<usize> {
        self.targets.iter().map(CohomologySubspace::rank).collect()
    }

    fn survives(&self, selected: &[usize]) -> Result<bool> {
        Ok(self
            .intersection_ranks(selected)?
            .iter()
            .zip(&self.specification.states)
            .any(|(rank, state)| *rank > state.max_surviving_rank))
    }

    fn intersection_ranks(&self, selected: &[usize]) -> Result<Vec<usize>> {
        (0..self.specification.states.len())
            .map(|state| self.intersection_rank(state, selected))
            .collect()
    }

    fn intersection_rank(&self, state: usize, selected: &[usize]) -> Result<usize> {
        let relevant = selected
            .iter()
            .copied()
            .filter(|action| self.actions[*action].states.binary_search(&state).is_ok())
            .collect::<Vec<_>>();
        let key = (state, relevant.clone());
        if let Some(rank) = self.cache.borrow().get(&key) {
            return Ok(*rank);
        }
        let mut edges = self.specification.states[state].active_edges.clone();
        for action in relevant {
            edges.push(self.actions[action].edge);
        }
        edges.sort_unstable();
        edges.dedup();
        let edited_graph = graph_from_edges(self.specification.vertex_count, &edges)?;
        let edited_space = cohomology_space(
            &edited_graph,
            self.specification.dimension,
            self.specification.scale,
            self.specification.modulus,
            self.limits,
        )?;
        let restriction = cohomology_restriction(
            &edited_graph,
            &edited_space,
            &self.graphs[state],
            &self.spaces[state],
        )?;
        let rank =
            restriction.image_intersection_rank(&self.spaces[state], &self.targets[state])?;
        self.cache.borrow_mut().insert(key, rank);
        Ok(rank)
    }
}

struct ProofBuilder<'a> {
    costs: &'a [u64],
    max_edits: usize,
    cutoff: Option<u64>,
    oracle: &'a TopologyOracle<'a>,
    limits: SynthesisLimits,
    nodes: usize,
    topology_checks: usize,
    terms: usize,
}

impl<'a> ProofBuilder<'a> {
    fn new(
        costs: &'a [u64],
        max_edits: usize,
        cutoff: Option<u64>,
        oracle: &'a TopologyOracle<'a>,
        limits: SynthesisLimits,
    ) -> Self {
        Self {
            costs,
            max_edits,
            cutoff,
            oracle,
            limits,
            nodes: 0,
            topology_checks: 0,
            terms: 0,
        }
    }

    fn prove(
        &mut self,
        included: Vec<usize>,
        available: Vec<usize>,
        depth: usize,
    ) -> Result<ProofNode> {
        self.record_node(depth)?;
        let included_cost = selected_cost(self.costs, &included)?;
        if let Some(leaf) = self.early_leaf(&included, &available, included_cost)? {
            return Ok(leaf);
        }
        let blockers = self.pack_blockers(&included, &available)?;
        if blockers.is_empty() {
            return Err(Error::InvalidInput(
                "synthesis proof found an unreported feasible action set".into(),
            ));
        }
        if let Some(leaf) = self.blocker_leaf(&included, included_cost, &blockers)? {
            return Ok(leaf);
        }
        self.branch(included, available, blockers, depth)
    }

    fn record_node(&mut self, depth: usize) -> Result<()> {
        self.nodes = self
            .nodes
            .checked_add(1)
            .ok_or_else(|| Error::InvalidInput("synthesis proof node count overflows".into()))?;
        if self.nodes > self.limits.max_proof_nodes.min(FORMAT_MAX_PROOF_NODES)
            || depth > self.limits.max_proof_depth
        {
            return Err(Error::InvalidInput(
                "synthesis proof tree exceeds its node or depth limit".into(),
            ));
        }
        Ok(())
    }

    fn early_leaf(
        &mut self,
        included: &[usize],
        available: &[usize],
        included_cost: u64,
    ) -> Result<Option<ProofNode>> {
        if self.cutoff.is_some_and(|cutoff| included_cost >= cutoff) {
            return Ok(Some(ProofNode::Cost));
        }
        if included.len() == self.max_edits {
            if !self.check_survival(included)? {
                return Err(Error::InvalidInput(
                    "synthesis proof found a cheaper feasible action set".into(),
                ));
            }
            return Ok(Some(ProofNode::SurvivingEditLimit));
        }
        let maximum = merge(included, available);
        if self.check_survival(&maximum)? {
            return Ok(Some(ProofNode::SurvivingMaximum));
        }
        Ok(None)
    }

    fn blocker_leaf(
        &mut self,
        included: &[usize],
        included_cost: u64,
        blockers: &[Vec<usize>],
    ) -> Result<Option<ProofNode>> {
        let cardinality = included.len().saturating_add(blockers.len());
        if cardinality > self.max_edits {
            self.add_blocker_terms(blockers)?;
            return Ok(Some(ProofNode::BlockerBound {
                kind: BoundKind::Edits,
                blockers: blockers.to_vec(),
            }));
        }
        let bound = included_cost
            .checked_add(blocker_bound(self.costs, blockers)?)
            .ok_or_else(|| Error::InvalidInput("synthesis proof cost bound overflows".into()))?;
        if self.cutoff.is_some_and(|cutoff| bound >= cutoff) {
            self.add_blocker_terms(blockers)?;
            return Ok(Some(ProofNode::BlockerBound {
                kind: BoundKind::Cost,
                blockers: blockers.to_vec(),
            }));
        }
        Ok(None)
    }

    fn branch(
        &mut self,
        included: Vec<usize>,
        available: Vec<usize>,
        blockers: Vec<Vec<usize>>,
        depth: usize,
    ) -> Result<ProofNode> {
        let mut blocker = blockers
            .into_iter()
            .min_by_key(|blocker| (blocker.len(), blocker_min_cost(self.costs, blocker)))
            .expect("a nonempty packing has a blocker");
        blocker.sort_by_key(|candidate| (self.costs[*candidate], *candidate));
        self.add_blocker_terms(std::slice::from_ref(&blocker))?;
        let mut children = Vec::with_capacity(blocker.len());
        let mut excluded = BTreeSet::new();
        for &candidate in &blocker {
            let mut child_included = included.clone();
            insert_sorted(&mut child_included, candidate);
            let child_available = available
                .iter()
                .copied()
                .filter(|item| *item != candidate && !excluded.contains(item))
                .collect();
            children.push(self.prove(child_included, child_available, depth + 1)?);
            excluded.insert(candidate);
        }
        Ok(ProofNode::Branch { blocker, children })
    }

    fn pack_blockers(
        &mut self,
        included: &[usize],
        available: &[usize],
    ) -> Result<Vec<Vec<usize>>> {
        let mut blockers = Vec::new();
        let mut used = Vec::new();
        while self.check_survival(&merge(included, &used))? {
            let mut retained = used.clone();
            for candidate in available
                .iter()
                .copied()
                .filter(|candidate| used.binary_search(candidate).is_err())
            {
                let mut trial = merge(included, &retained);
                insert_sorted(&mut trial, candidate);
                if self.check_survival(&trial)? {
                    insert_sorted(&mut retained, candidate);
                }
            }
            let blocker = difference(available, &retained);
            if blocker.is_empty() {
                break;
            }
            for &candidate in &blocker {
                insert_sorted(&mut used, candidate);
            }
            blockers.push(blocker);
        }
        Ok(blockers)
    }

    fn check_survival(&mut self, selected: &[usize]) -> Result<bool> {
        self.topology_checks = self.topology_checks.checked_add(1).ok_or_else(|| {
            Error::InvalidInput("synthesis proof topology count overflows".into())
        })?;
        if self.topology_checks > self.limits.max_oracle_calls {
            return Err(Error::InvalidInput(
                "synthesis proof topology checks exceed their limit".into(),
            ));
        }
        self.oracle.survives(selected)
    }

    fn add_blocker_terms(&mut self, blockers: &[Vec<usize>]) -> Result<()> {
        self.terms = blockers.iter().try_fold(self.terms, |sum, blocker| {
            sum.checked_add(blocker.len())
                .ok_or_else(|| Error::InvalidInput("synthesis proof term count overflows".into()))
        })?;
        if self.terms > self.limits.max_terms.min(FORMAT_MAX_PROOF_TERMS) {
            return Err(Error::InvalidInput(
                "synthesis proof terms exceed their limit".into(),
            ));
        }
        Ok(())
    }
}

struct ProofVerifier<'a> {
    costs: &'a [u64],
    max_edits: usize,
    cutoff: Option<u64>,
    oracle: &'a TopologyOracle<'a>,
    limits: SynthesisLimits,
    nodes: usize,
    checks: usize,
    terms: usize,
}

impl<'a> ProofVerifier<'a> {
    fn new(
        costs: &'a [u64],
        max_edits: usize,
        cutoff: Option<u64>,
        oracle: &'a TopologyOracle<'a>,
        limits: SynthesisLimits,
    ) -> Self {
        Self {
            costs,
            max_edits,
            cutoff,
            oracle,
            limits,
            nodes: 0,
            checks: 0,
            terms: 0,
        }
    }

    fn verify_root(&mut self, proof: &ProofNode) -> Result<()> {
        self.verify_node(proof, Vec::new(), (0..self.costs.len()).collect(), 0)
    }

    fn verify_node(
        &mut self,
        proof: &ProofNode,
        included: Vec<usize>,
        available: Vec<usize>,
        depth: usize,
    ) -> Result<()> {
        self.record_node(depth)?;
        let included_cost = selected_cost(self.costs, &included)?;
        self.verify_node_kind(proof, included, available, included_cost, depth)
    }

    fn record_node(&mut self, depth: usize) -> Result<()> {
        self.nodes = self
            .nodes
            .checked_add(1)
            .ok_or_else(|| Error::InvalidInput("synthesis proof node count overflows".into()))?;
        if self.nodes > self.limits.max_proof_nodes.min(FORMAT_MAX_PROOF_NODES)
            || depth > self.limits.max_proof_depth
        {
            return Err(Error::InvalidInput(
                "synthesis proof tree exceeds its node or depth limit".into(),
            ));
        }
        Ok(())
    }

    fn verify_node_kind(
        &mut self,
        proof: &ProofNode,
        included: Vec<usize>,
        available: Vec<usize>,
        included_cost: u64,
        depth: usize,
    ) -> Result<()> {
        match proof {
            ProofNode::Cost => self.verify_cost_leaf(included_cost),
            ProofNode::SurvivingMaximum => self.verify_maximum_leaf(&included, &available),
            ProofNode::SurvivingEditLimit => self.verify_edit_leaf(&included),
            ProofNode::BlockerBound { kind, blockers } => {
                self.verify_bound_leaf(&included, &available, included_cost, *kind, blockers)
            }
            ProofNode::Branch { blocker, children } => {
                self.verify_branch(&included, &available, blocker, children, depth)
            }
        }
    }

    fn verify_cost_leaf(&self, included_cost: u64) -> Result<()> {
        if self.cutoff.is_none_or(|cutoff| included_cost < cutoff) {
            Err(Error::InvalidInput(
                "synthesis cost leaf does not reach the incumbent".into(),
            ))
        } else {
            Ok(())
        }
    }

    fn verify_maximum_leaf(&mut self, included: &[usize], available: &[usize]) -> Result<()> {
        if !self.check_survival(&merge(included, available))? {
            Err(Error::InvalidInput(
                "synthesis maximal-survival leaf is feasible".into(),
            ))
        } else {
            Ok(())
        }
    }

    fn verify_edit_leaf(&mut self, included: &[usize]) -> Result<()> {
        if included.len() != self.max_edits || !self.check_survival(included)? {
            Err(Error::InvalidInput(
                "synthesis edit-limit leaf is invalid".into(),
            ))
        } else {
            Ok(())
        }
    }

    fn verify_bound_leaf(
        &mut self,
        included: &[usize],
        available: &[usize],
        included_cost: u64,
        kind: BoundKind,
        blockers: &[Vec<usize>],
    ) -> Result<()> {
        self.verify_blockers(included, available, blockers)?;
        if self.bound_closes(included, included_cost, kind, blockers) {
            Ok(())
        } else {
            Err(Error::InvalidInput(
                "synthesis blocker leaf does not close its branch".into(),
            ))
        }
    }

    fn bound_closes(
        &self,
        included: &[usize],
        included_cost: u64,
        kind: BoundKind,
        blockers: &[Vec<usize>],
    ) -> bool {
        match kind {
            BoundKind::Edits => included.len().saturating_add(blockers.len()) > self.max_edits,
            BoundKind::Cost => self.cutoff.is_some_and(|cutoff| {
                included_cost
                    .checked_add(blocker_bound(self.costs, blockers).unwrap_or(u64::MAX))
                    .is_some_and(|bound| bound >= cutoff)
            }),
        }
    }

    fn verify_branch(
        &mut self,
        included: &[usize],
        available: &[usize],
        blocker: &[usize],
        children: &[ProofNode],
        depth: usize,
    ) -> Result<()> {
        self.verify_blockers(included, available, std::slice::from_ref(&blocker.to_vec()))?;
        if blocker.len() != children.len() {
            return Err(Error::InvalidInput(
                "synthesis branch child count differs from its blocker".into(),
            ));
        }
        let mut excluded = BTreeSet::new();
        for (&candidate, child) in blocker.iter().zip(children) {
            let mut child_included = included.to_vec();
            insert_sorted(&mut child_included, candidate);
            let child_available = available
                .iter()
                .copied()
                .filter(|item| *item != candidate && !excluded.contains(item))
                .collect();
            self.verify_node(child, child_included, child_available, depth + 1)?;
            excluded.insert(candidate);
        }
        Ok(())
    }

    fn verify_blockers(
        &mut self,
        included: &[usize],
        available: &[usize],
        blockers: &[Vec<usize>],
    ) -> Result<()> {
        let mut used = BTreeSet::new();
        for blocker in blockers {
            if blocker.is_empty()
                || blocker.iter().any(|candidate| {
                    available.binary_search(candidate).is_err() || !used.insert(*candidate)
                })
                || blocker.windows(2).any(|pair| pair[0] >= pair[1])
            {
                return Err(Error::InvalidInput(
                    "synthesis blocker family is not canonical and disjoint".into(),
                ));
            }
            let witness = merge(included, &difference(available, blocker));
            if !self.check_survival(&witness)? {
                return Err(Error::InvalidInput(
                    "synthesis blocker complement does not survive".into(),
                ));
            }
        }
        self.terms = blockers.iter().try_fold(self.terms, |sum, blocker| {
            sum.checked_add(blocker.len())
                .ok_or_else(|| Error::InvalidInput("synthesis proof term count overflows".into()))
        })?;
        if self.terms > self.limits.max_terms.min(FORMAT_MAX_PROOF_TERMS) {
            return Err(Error::InvalidInput(
                "synthesis proof terms exceed their limit".into(),
            ));
        }
        Ok(())
    }

    fn check_survival(&mut self, selected: &[usize]) -> Result<bool> {
        self.checks = self.checks.checked_add(1).ok_or_else(|| {
            Error::InvalidInput("synthesis proof topology count overflows".into())
        })?;
        if self.checks > self.limits.max_oracle_calls {
            return Err(Error::InvalidInput(
                "synthesis proof topology checks exceed their limit".into(),
            ));
        }
        self.oracle.survives(selected)
    }
}

fn validate_problem(
    specification: &TopologicalSpecification,
    actions: &[SynthesisAction],
    oracle_limit: usize,
    node_limit: usize,
    limits: SynthesisLimits,
) -> Result<()> {
    validate_specification_envelope(specification, limits)?;
    validate_source(specification, limits)?;
    validate_states(specification, limits)?;
    validate_actions(specification, actions, limits)?;
    validate_work_limits(oracle_limit, node_limit, limits)
}

fn validate_specification_envelope(
    specification: &TopologicalSpecification,
    limits: SynthesisLimits,
) -> Result<()> {
    if specification.vertex_count > limits.max_vertices
        || specification.states.len() > limits.max_states.min(FORMAT_MAX_STATES)
        || !specification.scale.is_finite()
        || specification.scale < 0.0
    {
        return Err(Error::InvalidInput(
            "synthesis specification size or scale is invalid".into(),
        ));
    }
    Ok(())
}

fn validate_states(
    specification: &TopologicalSpecification,
    limits: SynthesisLimits,
) -> Result<()> {
    let mut prior = None;
    let mut terms = 0usize;
    for state in &specification.states {
        validate_state_order(prior, state)?;
        prior = Some((state.scenario, state.step));
        validate_edges(
            specification.vertex_count,
            &state.active_edges,
            limits.max_edges_per_state,
        )?;
        validate_target(&state.target, specification.modulus, &mut terms)?;
    }
    if terms > limits.max_terms {
        return Err(Error::InvalidInput(
            "synthesis target terms exceed their limit".into(),
        ));
    }
    Ok(())
}

fn validate_state_order(prior: Option<(u64, u64)>, state: &SynthesisState) -> Result<()> {
    if prior.is_some_and(|key| key >= (state.scenario, state.step)) {
        Err(Error::InvalidInput(
            "synthesis states are not in canonical scenario and step order".into(),
        ))
    } else {
        Ok(())
    }
}

fn validate_target(
    target: &[Vec<SynthesisCoordinate>],
    modulus: u32,
    terms: &mut usize,
) -> Result<()> {
    for row in target {
        validate_target_row(row, modulus)?;
        *terms = terms
            .checked_add(row.len())
            .ok_or_else(|| Error::InvalidInput("synthesis target term count overflows".into()))?;
    }
    Ok(())
}

fn validate_target_row(row: &[SynthesisCoordinate], modulus: u32) -> Result<()> {
    let invalid_coefficient = row
        .iter()
        .any(|term| term.coefficient == 0 || term.coefficient >= modulus);
    if row.is_empty()
        || row.windows(2).any(|pair| pair[0].basis >= pair[1].basis)
        || invalid_coefficient
    {
        Err(Error::InvalidInput(
            "synthesis target coordinates are not canonical".into(),
        ))
    } else {
        Ok(())
    }
}

fn validate_actions(
    specification: &TopologicalSpecification,
    actions: &[SynthesisAction],
    limits: SynthesisLimits,
) -> Result<()> {
    if actions.len() > limits.max_actions.min(FORMAT_MAX_ACTIONS)
        || actions.windows(2).any(|pair| pair[0] >= pair[1])
        || actions
            .iter()
            .any(|action| invalid_action(specification, action))
    {
        Err(Error::InvalidInput(
            "synthesis actions, edit bound, or work limits are invalid".into(),
        ))
    } else {
        Ok(())
    }
}

fn invalid_action(specification: &TopologicalSpecification, action: &SynthesisAction) -> bool {
    action.cost == 0
        || action.edge.u >= action.edge.v
        || action.edge.v >= specification.vertex_count
        || action.states.is_empty()
        || action
            .states
            .iter()
            .any(|state| *state >= specification.states.len())
        || action.states.windows(2).any(|pair| pair[0] >= pair[1])
}

fn validate_work_limits(
    oracle_limit: usize,
    node_limit: usize,
    limits: SynthesisLimits,
) -> Result<()> {
    if oracle_limit == 0
        || oracle_limit > limits.max_oracle_calls
        || node_limit == 0
        || node_limit > limits.max_search_nodes
    {
        Err(Error::InvalidInput(
            "synthesis actions, edit bound, or work limits are invalid".into(),
        ))
    } else {
        Ok(())
    }
}

fn validate_source(
    specification: &TopologicalSpecification,
    limits: SynthesisLimits,
) -> Result<()> {
    let SynthesisSource::Affine {
        scenario,
        edges,
        start,
        end,
        maximum_rank,
    } = &specification.source
    else {
        return Ok(());
    };
    let trajectory = KineticFiltration::new(
        specification.vertex_count,
        edges.clone(),
        *start,
        *end,
        limits.kinetic,
    )?;
    if trajectory.edges() != edges {
        return Err(Error::InvalidInput(
            "synthesis affine trajectories are not canonical".into(),
        ));
    }
    let states = compile_kinetic_states(
        &trajectory,
        *scenario,
        specification.dimension,
        specification.scale,
        specification.modulus,
        *maximum_rank,
        limits.cohomology,
        limits.max_states.min(FORMAT_MAX_STATES),
    )?;
    if states != specification.states {
        return Err(Error::InvalidInput(
            "synthesis states are not the complete affine threshold schedule".into(),
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn compile_kinetic_states(
    filtration: &KineticFiltration,
    scenario: u64,
    dimension: usize,
    scale: f64,
    modulus: u32,
    maximum_rank: usize,
    limits: CohomologyLimits,
    maximum_schedule_states: usize,
) -> Result<Vec<SynthesisState>> {
    let graphs = filtration.critical_graphs(scale)?;
    if graphs.len() > maximum_schedule_states {
        return Err(Error::InvalidInput(
            "synthesis affine schedule exceeds its state limit".into(),
        ));
    }
    let mut states = Vec::new();
    for (step, kinetic) in graphs.into_iter().enumerate() {
        let space = cohomology_space(&kinetic.graph, dimension, scale, modulus, limits)?;
        if space.rank() <= maximum_rank {
            continue;
        }
        let target = space.full_subspace();
        states.push(SynthesisState::from_subspace(
            scenario,
            step as u64,
            &kinetic.graph,
            scale,
            &space,
            &target,
            maximum_rank,
        )?);
    }
    Ok(states)
}

fn validate_root_blockers(
    oracle: &TopologyOracle<'_>,
    blockers: &[Vec<usize>],
    costs: &[u64],
    lower_bound: Option<u64>,
) -> Result<()> {
    let available = (0..costs.len()).collect::<Vec<_>>();
    let mut used = BTreeSet::new();
    for blocker in blockers {
        if blocker.is_empty()
            || blocker
                .iter()
                .any(|candidate| *candidate >= costs.len() || !used.insert(*candidate))
            || blocker.windows(2).any(|pair| pair[0] >= pair[1])
            || !oracle.survives(&difference(&available, blocker))?
        {
            return Err(Error::InvalidInput(
                "synthesis root blocker does not certify a necessary action set".into(),
            ));
        }
    }
    let bound = blocker_bound(costs, blockers)?;
    if lower_bound.is_some_and(|lower| lower < bound) {
        return Err(Error::InvalidInput(
            "synthesis lower bound is below its root blocker certificate".into(),
        ));
    }
    Ok(())
}

fn graph_from_edges(vertex_count: usize, edges: &[KineticEdgeKey]) -> Result<SparseDistanceMatrix> {
    SparseDistanceMatrix::from_triplets(
        vertex_count,
        &edges
            .iter()
            .map(|edge| (edge.u, edge.v, 0.0))
            .collect::<Vec<_>>(),
    )
}

fn validate_edges(vertex_count: usize, edges: &[KineticEdgeKey], maximum: usize) -> Result<()> {
    if edges.len() > maximum
        || edges
            .iter()
            .any(|edge| edge.u >= edge.v || edge.v >= vertex_count)
        || edges.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(Error::InvalidInput(
            "synthesis active edges are not canonical or exceed their limit".into(),
        ));
    }
    Ok(())
}

fn selected_cost(costs: &[u64], selected: &[usize]) -> Result<u64> {
    selected.iter().try_fold(0u64, |sum, candidate| {
        sum.checked_add(costs[*candidate])
            .ok_or_else(|| Error::InvalidInput("synthesis selected cost overflows".into()))
    })
}

fn blocker_min_cost(costs: &[u64], blocker: &[usize]) -> u64 {
    blocker
        .iter()
        .map(|candidate| costs[*candidate])
        .min()
        .unwrap_or(0)
}

fn blocker_bound(costs: &[u64], blockers: &[Vec<usize>]) -> Result<u64> {
    blockers.iter().try_fold(0u64, |sum, blocker| {
        sum.checked_add(blocker_min_cost(costs, blocker))
            .ok_or_else(|| Error::InvalidInput("synthesis blocker bound overflows".into()))
    })
}

fn insert_sorted(values: &mut Vec<usize>, value: usize) {
    if let Err(position) = values.binary_search(&value) {
        values.insert(position, value);
    }
}

fn difference(values: &[usize], removed: &[usize]) -> Vec<usize> {
    values
        .iter()
        .copied()
        .filter(|value| removed.binary_search(value).is_err())
        .collect()
}

fn merge(left: &[usize], right: &[usize]) -> Vec<usize> {
    left.iter()
        .chain(right)
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn find_set(parent: &mut [usize], value: usize) -> usize {
    if parent[value] != value {
        parent[value] = find_set(parent, parent[value]);
    }
    parent[value]
}

fn union_sets(parent: &mut [usize], left: usize, right: usize) {
    let left = find_set(parent, left);
    let right = find_set(parent, right);
    if left != right {
        parent[right] = left;
    }
}

fn proof_topology_checks(proof: &ProofNode) -> usize {
    match proof {
        ProofNode::Cost => 0,
        ProofNode::SurvivingMaximum | ProofNode::SurvivingEditLimit => 1,
        ProofNode::BlockerBound { blockers, .. } => blockers.len(),
        ProofNode::Branch { children, .. } => {
            1 + children.iter().map(proof_topology_checks).sum::<usize>()
        }
    }
}

fn encode_proof(output: &mut Vec<u8>, proof: &ProofNode) -> Result<()> {
    match proof {
        ProofNode::Cost => output.push(1),
        ProofNode::SurvivingMaximum => output.push(2),
        ProofNode::SurvivingEditLimit => output.push(3),
        ProofNode::BlockerBound { kind, blockers } => encode_bound_node(output, *kind, blockers)?,
        ProofNode::Branch { blocker, children } => encode_branch_node(output, blocker, children)?,
    }
    Ok(())
}

fn encode_bound_node(output: &mut Vec<u8>, kind: BoundKind, blockers: &[Vec<usize>]) -> Result<()> {
    output.push(4);
    output.push(match kind {
        BoundKind::Cost => 1,
        BoundKind::Edits => 2,
    });
    encode_root_blockers(output, blockers)
}

fn encode_branch_node(
    output: &mut Vec<u8>,
    blocker: &[usize],
    children: &[ProofNode],
) -> Result<()> {
    output.push(5);
    encode_usizes(output, blocker)?;
    put_usize(output, children.len())?;
    for child in children {
        encode_proof(output, child)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn decode_proof(
    reader: &mut Reader<'_>,
    action_count: usize,
    depth: usize,
    nodes: &mut usize,
    terms: &mut usize,
    limits: SynthesisLimits,
) -> Result<ProofNode> {
    record_decoded_node(nodes, depth, limits)?;
    match reader.u8()? {
        1 => Ok(ProofNode::Cost),
        2 => Ok(ProofNode::SurvivingMaximum),
        3 => Ok(ProofNode::SurvivingEditLimit),
        4 => decode_bound_node(reader, action_count, terms, limits),
        5 => decode_branch_node(reader, action_count, depth, nodes, terms, limits),
        _ => Err(Error::InvalidInput(
            "synthesis proof node kind is invalid".into(),
        )),
    }
}

fn record_decoded_node(nodes: &mut usize, depth: usize, limits: SynthesisLimits) -> Result<()> {
    *nodes = nodes
        .checked_add(1)
        .ok_or_else(|| Error::InvalidInput("synthesis proof node count overflows".into()))?;
    if *nodes > limits.max_proof_nodes.min(FORMAT_MAX_PROOF_NODES) || depth > limits.max_proof_depth
    {
        Err(Error::InvalidInput(
            "synthesis proof tree exceeds its node or depth limit".into(),
        ))
    } else {
        Ok(())
    }
}

fn decode_bound_node(
    reader: &mut Reader<'_>,
    action_count: usize,
    terms: &mut usize,
    limits: SynthesisLimits,
) -> Result<ProofNode> {
    Ok(ProofNode::BlockerBound {
        kind: decode_bound_kind(reader)?,
        blockers: decode_proof_blockers(reader, action_count, terms, limits)?,
    })
}

fn decode_bound_kind(reader: &mut Reader<'_>) -> Result<BoundKind> {
    match reader.u8()? {
        1 => Ok(BoundKind::Cost),
        2 => Ok(BoundKind::Edits),
        _ => Err(Error::InvalidInput(
            "synthesis proof bound kind is invalid".into(),
        )),
    }
}

fn decode_proof_blockers(
    reader: &mut Reader<'_>,
    action_count: usize,
    terms: &mut usize,
    limits: SynthesisLimits,
) -> Result<Vec<Vec<usize>>> {
    let count = reader.bounded_usize("proof blocker count", limits.max_terms)?;
    let mut blockers = Vec::with_capacity(count);
    for _ in 0..count {
        let blocker = decode_indices(reader, action_count, limits.max_terms)?;
        add_proof_terms(
            terms,
            blocker.len(),
            limits.max_terms.min(FORMAT_MAX_PROOF_TERMS),
            "synthesis proof",
        )?;
        blockers.push(blocker);
    }
    Ok(blockers)
}

#[allow(clippy::too_many_arguments)]
fn decode_branch_node(
    reader: &mut Reader<'_>,
    action_count: usize,
    depth: usize,
    nodes: &mut usize,
    terms: &mut usize,
    limits: SynthesisLimits,
) -> Result<ProofNode> {
    let blocker = decode_indices(reader, action_count, limits.max_terms)?;
    add_proof_terms(
        terms,
        blocker.len(),
        limits.max_terms.min(FORMAT_MAX_PROOF_TERMS),
        "synthesis proof",
    )?;
    let count = reader.bounded_usize("proof child count", action_count)?;
    if count != blocker.len() {
        return Err(Error::InvalidInput(
            "synthesis branch child count differs from its blocker".into(),
        ));
    }
    let children =
        decode_proof_children(reader, action_count, count, depth + 1, nodes, terms, limits)?;
    Ok(ProofNode::Branch { blocker, children })
}

#[allow(clippy::too_many_arguments)]
fn decode_proof_children(
    reader: &mut Reader<'_>,
    action_count: usize,
    count: usize,
    depth: usize,
    nodes: &mut usize,
    terms: &mut usize,
    limits: SynthesisLimits,
) -> Result<Vec<ProofNode>> {
    let mut children = Vec::with_capacity(count);
    for _ in 0..count {
        children.push(decode_proof(
            reader,
            action_count,
            depth,
            nodes,
            terms,
            limits,
        )?);
    }
    Ok(children)
}

fn encode_source(output: &mut Vec<u8>, source: &SynthesisSource) -> Result<()> {
    match source {
        SynthesisSource::Finite => output.push(0),
        SynthesisSource::Affine { .. } => encode_affine_source(output, source)?,
    }
    Ok(())
}

fn encode_affine_source(output: &mut Vec<u8>, source: &SynthesisSource) -> Result<()> {
    let SynthesisSource::Affine {
        scenario,
        edges,
        start,
        end,
        maximum_rank,
    } = source
    else {
        return Ok(());
    };
    output.push(1);
    output.extend_from_slice(&scenario.to_be_bytes());
    output.extend_from_slice(&start.to_bits().to_be_bytes());
    output.extend_from_slice(&end.to_bits().to_be_bytes());
    put_usize(output, *maximum_rank)?;
    put_usize(output, edges.len())?;
    for edge in edges {
        encode_affine_edge(output, edge)?;
    }
    Ok(())
}

fn encode_affine_edge(output: &mut Vec<u8>, edge: &KineticEdge) -> Result<()> {
    put_usize(output, edge.u)?;
    put_usize(output, edge.v)?;
    output.extend_from_slice(&edge.intercept.to_bits().to_be_bytes());
    output.extend_from_slice(&edge.velocity.to_bits().to_be_bytes());
    Ok(())
}

fn decode_source(reader: &mut Reader<'_>, limits: SynthesisLimits) -> Result<SynthesisSource> {
    match reader.u8()? {
        0 => Ok(SynthesisSource::Finite),
        1 => decode_affine_source(reader, limits),
        _ => Err(Error::InvalidInput(
            "synthesis source kind is invalid".into(),
        )),
    }
}

fn decode_affine_source(
    reader: &mut Reader<'_>,
    limits: SynthesisLimits,
) -> Result<SynthesisSource> {
    let scenario = reader.u64()?;
    let start = f64::from_bits(reader.u64()?);
    let end = f64::from_bits(reader.u64()?);
    let maximum_rank = reader.usize()?;
    let edges = decode_affine_edges(reader, limits)?;
    Ok(SynthesisSource::Affine {
        scenario,
        edges,
        start,
        end,
        maximum_rank,
    })
}

fn decode_affine_edges(
    reader: &mut Reader<'_>,
    limits: SynthesisLimits,
) -> Result<Vec<KineticEdge>> {
    let count = reader.bounded_usize("affine edge count", limits.kinetic.max_edges)?;
    if count > reader.remaining() / 32 {
        return Err(Error::InvalidInput(
            "synthesis affine edge count exceeds the remaining bytes".into(),
        ));
    }
    (0..count).map(|_| decode_affine_edge(reader)).collect()
}

fn decode_affine_edge(reader: &mut Reader<'_>) -> Result<KineticEdge> {
    Ok(KineticEdge {
        u: reader.usize()?,
        v: reader.usize()?,
        intercept: f64::from_bits(reader.u64()?),
        velocity: f64::from_bits(reader.u64()?),
    })
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
            "synthesis edge count exceeds the remaining bytes".into(),
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
            "synthesis integer list exceeds the remaining bytes".into(),
        ));
    }
    (0..count).map(|_| reader.usize()).collect()
}

fn decode_indices(
    reader: &mut Reader<'_>,
    exclusive_maximum: usize,
    maximum_count: usize,
) -> Result<Vec<usize>> {
    let values = decode_usizes(reader, maximum_count)?;
    if values.iter().any(|value| *value >= exclusive_maximum)
        || values.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(Error::InvalidInput(
            "synthesis index list is not canonical".into(),
        ));
    }
    Ok(values)
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
        .map_err(|_| Error::InvalidInput("synthesis integer does not fit u64".into()))?;
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
            .ok_or_else(|| Error::InvalidInput("synthesis artifact position overflows".into()))?;
        if end > self.bytes.len() {
            return Err(Error::InvalidInput(
                "synthesis artifact is truncated".into(),
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
                "synthesis {name} exceeds its limit"
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
            .map_err(|_| Error::InvalidInput("synthesis integer does not fit usize".into()))
    }

    fn optional_u64(&mut self) -> Result<Option<u64>> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.u64()?)),
            _ => Err(Error::InvalidInput(
                "synthesis optional integer flag is invalid".into(),
            )),
        }
    }

    fn array32(&mut self) -> Result<[u8; 32]> {
        Ok(self.take(32)?.try_into().unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use holos_tda_check::{ProofLimits, VerifiedSynthesisSource, verify_synthesis};

    fn two_cycles() -> SparseDistanceMatrix {
        SparseDistanceMatrix::from_triplets(
            8,
            &[
                (0, 1, 1.0),
                (1, 2, 1.0),
                (2, 3, 1.0),
                (0, 3, 1.0),
                (4, 5, 1.0),
                (5, 6, 1.0),
                (6, 7, 1.0),
                (4, 7, 1.0),
            ],
        )
        .unwrap()
    }

    fn state(
        graph: &SparseDistanceMatrix,
        scenario: u64,
        step: u64,
        target_basis: usize,
    ) -> SynthesisState {
        let space = cohomology_space(graph, 1, 1.0, 3, CohomologyLimits::default()).unwrap();
        let target = space
            .subspace_from_coordinates(&[vec![(target_basis, 1)]])
            .unwrap();
        SynthesisState::from_subspace(scenario, step, graph, 1.0, &space, &target, 0).unwrap()
    }

    fn problem() -> (TopologicalSpecification, Vec<SynthesisAction>) {
        let graph = two_cycles();
        let specification = TopologicalSpecification::new(
            8,
            1,
            1.0,
            3,
            vec![state(&graph, 0, 0, 0), state(&graph, 1, 0, 1)],
        );
        let mut actions = vec![
            SynthesisAction::new(0, 2, 4, vec![0]),
            SynthesisAction::new(1, 3, 7, vec![0]),
            SynthesisAction::new(4, 6, 5, vec![1]),
            SynthesisAction::new(5, 7, 9, vec![1]),
        ];
        actions.sort();
        (specification, actions)
    }

    #[test]
    fn temporal_subspace_plan_has_a_checked_optimality_tree() {
        let (specification, actions) = problem();
        let limits = SynthesisLimits::default();
        let artifact = SynthesisArtifact::build(specification, actions, 2, limits).unwrap();
        assert_eq!(artifact.status(), SynthesisStatus::Optimal);
        assert_eq!(artifact.upper_bound_cost(), Some(9));
        assert_eq!(artifact.lower_bound_cost(), Some(9));
        assert_eq!(artifact.before_ranks(), [1, 1]);
        assert_eq!(artifact.after_ranks(), [0, 0]);
        assert!(artifact.proof_nodes() > 0);
        assert!(artifact.proof_topology_checks() > 0);
        let bytes = artifact.encode(limits).unwrap();
        let decoded = SynthesisArtifact::decode(&bytes, limits).unwrap();
        assert_eq!(decoded, artifact);
        let checked = verify_synthesis(&bytes, ProofLimits::default()).unwrap();
        assert_eq!(
            checked.status,
            holos_tda_check::VerifiedSynthesisStatus::Optimal
        );
        assert_eq!(checked.total_cost, Some(9));
        assert_eq!(
            checked.proof_topology_checks,
            artifact.proof_topology_checks()
        );
        assert!(checked.proof_topology_checks < artifact.producer_oracle_calls());
    }

    #[test]
    fn edit_bound_has_a_complete_infeasibility_proof() {
        let (specification, actions) = problem();
        let limits = SynthesisLimits::default();
        let artifact = SynthesisArtifact::build(specification, actions, 1, limits).unwrap();
        assert_eq!(artifact.status(), SynthesisStatus::Infeasible);
        assert!(artifact.upper_bound_cost().is_none());
        artifact.verify(limits).unwrap();
        verify_synthesis(&artifact.encode(limits).unwrap(), ProofLimits::default()).unwrap();
    }

    #[test]
    fn bounded_search_keeps_only_a_checked_gap() {
        let (specification, actions) = problem();
        let limits = SynthesisLimits::default()
            .with_max_oracle_calls(4)
            .with_max_search_nodes(2);
        let artifact = SynthesisArtifact::build(specification, actions, 2, limits).unwrap();
        assert_eq!(artifact.status(), SynthesisStatus::SearchIncomplete);
        assert_eq!(artifact.proof_nodes(), 0);
        artifact.verify(limits).unwrap();
    }

    #[test]
    fn mutations_and_truncations_are_rejected() {
        let (specification, actions) = problem();
        let limits = SynthesisLimits::default();
        let artifact = SynthesisArtifact::build(specification, actions, 2, limits).unwrap();
        let bytes = artifact.encode(limits).unwrap();
        for end in 0..bytes.len() {
            assert!(SynthesisArtifact::decode(&bytes[..end], limits).is_err());
            assert!(verify_synthesis(&bytes[..end], ProofLimits::default()).is_err());
        }
        let mut changed = bytes;
        changed[48] ^= 1;
        assert!(SynthesisArtifact::decode(&changed, limits).is_err());
    }

    #[test]
    fn kinetic_compiler_covers_endpoints_events_and_open_cells() {
        let filtration = KineticFiltration::new(
            4,
            vec![
                crate::KineticEdge {
                    u: 0,
                    v: 1,
                    intercept: 1.0,
                    velocity: 0.0,
                },
                crate::KineticEdge {
                    u: 1,
                    v: 2,
                    intercept: 1.0,
                    velocity: 0.0,
                },
                crate::KineticEdge {
                    u: 2,
                    v: 3,
                    intercept: 1.0,
                    velocity: 0.0,
                },
                crate::KineticEdge {
                    u: 0,
                    v: 3,
                    intercept: 1.0,
                    velocity: 0.0,
                },
                crate::KineticEdge {
                    u: 0,
                    v: 2,
                    intercept: 2.0,
                    velocity: -1.0,
                },
            ],
            0.0,
            1.5,
            crate::KineticLimits::default(),
        )
        .unwrap();
        let critical = filtration.critical_graphs(1.0).unwrap();
        assert!(matches!(
            critical[0].kind,
            crate::KineticGraphStateKind::Start
        ));
        assert!(matches!(
            critical.last().unwrap().kind,
            crate::KineticGraphStateKind::End
        ));
        assert!(
            critical
                .iter()
                .any(|state| matches!(state.kind, crate::KineticGraphStateKind::Event(_)))
        );
        let specification = TopologicalSpecification::from_kinetic_rank_ceiling(
            &filtration,
            7,
            1,
            1.0,
            3,
            0,
            CohomologyLimits::default(),
        )
        .unwrap();
        assert!(!specification.states().is_empty());
        let action = SynthesisAction::throughout(1, 3, 2, &specification);
        let artifact =
            SynthesisArtifact::build(specification, vec![action], 1, SynthesisLimits::default())
                .unwrap();
        assert_eq!(artifact.status(), SynthesisStatus::Optimal);
        assert_eq!(artifact.upper_bound_cost(), Some(2));
        let limits = SynthesisLimits::default();
        let mut bytes = artifact.encode(limits).unwrap();
        let checked = verify_synthesis(&bytes, ProofLimits::default()).unwrap();
        assert_eq!(checked.source, VerifiedSynthesisSource::Affine);

        let velocity = (-1.0f64).to_bits().to_be_bytes();
        let position = bytes
            .windows(velocity.len())
            .position(|window| window == velocity)
            .unwrap();
        bytes[position..position + velocity.len()].copy_from_slice(&0.0f64.to_bits().to_be_bytes());
        let payload_end = bytes.len() - 32;
        let mut hash = Sha256::new();
        hash.update(b"holos-synthesis-artifact-v1");
        hash.update(&bytes[..payload_end]);
        let digest: [u8; 32] = hash.finalize().into();
        bytes[payload_end..].copy_from_slice(&digest);
        assert!(SynthesisArtifact::decode(&bytes, limits).is_err());
        assert!(verify_synthesis(&bytes, ProofLimits::default()).is_err());
    }

    #[test]
    fn incidence_components_partition_states_and_actions() {
        let (specification, actions) = problem();
        let components = specification.components(&actions).unwrap();
        assert_eq!(components.len(), 2);
        assert_eq!(components[0].states(), [0]);
        assert_eq!(components[0].actions(), [0, 1]);
        assert_eq!(components[1].states(), [1]);
        assert_eq!(components[1].actions(), [2, 3]);

        let bridge = SynthesisAction::new(0, 6, 20, vec![0, 1]);
        let mut connected = actions;
        connected.push(bridge);
        connected.sort();
        assert_eq!(specification.components(&connected).unwrap().len(), 1);
    }

    #[test]
    fn overlapping_obligations_require_a_recursive_optimality_proof() {
        let graph = SparseDistanceMatrix::from_triplets(
            4,
            &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)],
        )
        .unwrap();
        let specification = TopologicalSpecification::new(
            4,
            1,
            1.0,
            3,
            (0..3).map(|step| state(&graph, 0, step, 0)).collect(),
        );
        let mut actions = vec![
            SynthesisAction::new(0, 2, 1, vec![0, 1]),
            SynthesisAction::new(0, 2, 1, vec![0, 2]),
            SynthesisAction::new(0, 2, 1, vec![1, 2]),
        ];
        actions.sort();
        let limits = SynthesisLimits::default();
        let artifact = SynthesisArtifact::build(specification, actions, 2, limits).unwrap();
        assert_eq!(artifact.status(), SynthesisStatus::Optimal);
        assert_eq!(artifact.upper_bound_cost(), Some(2));
        assert_eq!(artifact.lower_bound_cost(), Some(2));
        assert_eq!(artifact.selected().len(), 2);
        assert!(artifact.proof_nodes() > 1);
        assert!(artifact.proof_topology_checks() > 1);
        verify_synthesis(&artifact.encode(limits).unwrap(), ProofLimits::default()).unwrap();
    }

    #[test]
    fn empty_specification_has_a_zero_cost_proof() {
        let specification = TopologicalSpecification::new(4, 1, 1.0, 3, Vec::new());
        let limits = SynthesisLimits::default();
        let artifact = SynthesisArtifact::build(specification, Vec::new(), 5, limits).unwrap();
        assert_eq!(artifact.status(), SynthesisStatus::Optimal);
        assert_eq!(artifact.upper_bound_cost(), Some(0));
        assert_eq!(artifact.lower_bound_cost(), Some(0));
        assert!(artifact.selected().is_empty());
        verify_synthesis(&artifact.encode(limits).unwrap(), ProofLimits::default()).unwrap();
    }
}
