//! Failure-tolerant specifications for exact relative coverage.
//!
//! A finite state contains every communication edge that can be used in that
//! state and the sensor vertices active before a plan. An action activates one
//! additional non-fence sensor in declared states. A plan is feasible only if
//! the controlled-boundary criterion holds after every maximal allowed sensor
//! failure in every state.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use sha2::{Digest, Sha256};

use crate::coverage_frontier::{CoverageCompositionStatus, compose_coverage_frontiers};
use crate::monotone_proof::{
    BoundKind, ProofLimits, ProofNode, ProofWork, build_proof, proof_topology_checks, verify_proof,
    verify_root_blockers,
};
use crate::monotone_search::{SearchLimits, SearchStatus, minimize_antitone};
use crate::{
    CoverageFence, CoverageLimits, Error, KineticEdge, KineticEdgeKey, KineticFiltration,
    KineticLimits, PlanarCoverageModel, Result, SparseDistanceMatrix, evaluate_planar_coverage,
};

const MAGIC: &[u8; 8] = b"HOLOSCOV";
const VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;
const FORMAT_MAX_PROOF_NODES: usize = 10_000_000;
const FORMAT_MAX_PROOF_TERMS: usize = 100_000_000;

/// Resource limits for coverage optimization, proofs, and artifacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct CoverageSynthesisLimits {
    /// Largest accepted artifact byte count.
    pub max_bytes: usize,
    /// Largest producer oracle-call count.
    pub max_oracle_calls: usize,
    /// Largest producer search-node count.
    pub max_search_nodes: usize,
    /// Largest proof-tree node count.
    pub max_proof_nodes: usize,
    /// Largest proof-tree depth.
    pub max_proof_depth: usize,
    /// Largest proof blocker term count.
    pub max_proof_terms: usize,
    /// Limits for every relative coverage calculation.
    pub coverage: CoverageLimits,
    /// Limits for replaying an affine communication source.
    pub kinetic: KineticLimits,
}

impl Default for CoverageSynthesisLimits {
    fn default() -> Self {
        Self {
            max_bytes: 1 << 30,
            max_oracle_calls: 2_000_000,
            max_search_nodes: 2_000_000,
            max_proof_nodes: 2_000_000,
            max_proof_depth: 1_024,
            max_proof_terms: 10_000_000,
            coverage: CoverageLimits::default(),
            kinetic: KineticLimits::default(),
        }
    }
}

impl CoverageSynthesisLimits {
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

    fn proof(self) -> ProofLimits {
        ProofLimits {
            nodes: self.max_proof_nodes.min(FORMAT_MAX_PROOF_NODES),
            depth: self.max_proof_depth,
            terms: self.max_proof_terms.min(FORMAT_MAX_PROOF_TERMS),
            checks: self.max_oracle_calls,
        }
    }
}

/// Origin and completeness scope of a finite coverage state list.
#[derive(Debug, Clone, PartialEq)]
pub enum CoverageSource {
    /// States were supplied directly. No claim is made between them.
    Finite,
    /// States are the complete threshold schedule of affine communication edges.
    Affine {
        /// Scenario identifier assigned to every compiled state.
        scenario: u64,
        /// Canonical affine edge trajectories.
        edges: Vec<KineticEdge>,
        /// First time in the closed interval.
        start: f64,
        /// Last time in the closed interval.
        end: f64,
    },
}

/// One finite communication state and its initially active sensors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageState {
    scenario: u64,
    step: u64,
    base_vertices: Vec<usize>,
    possible_edges: Vec<KineticEdgeKey>,
}

impl CoverageState {
    /// Construct a state from a graph of every possible communication edge.
    pub fn new(
        scenario: u64,
        step: u64,
        graph: &SparseDistanceMatrix,
        mut base_vertices: Vec<usize>,
        broadcast_radius: f64,
    ) -> Result<Self> {
        base_vertices.sort_unstable();
        base_vertices.dedup();
        if !broadcast_radius.is_finite()
            || broadcast_radius <= 0.0
            || base_vertices.iter().any(|vertex| *vertex >= graph.len())
        {
            return Err(Error::InvalidInput(
                "coverage state has an invalid radius or base vertex".into(),
            ));
        }
        Ok(Self {
            scenario,
            step,
            base_vertices,
            possible_edges: graph
                .edges()
                .filter(|edge| edge.2 <= broadcast_radius)
                .map(|(u, v, _)| KineticEdgeKey::new(u, v))
                .collect(),
        })
    }

    /// Scenario identifier used to group related states.
    pub fn scenario(&self) -> u64 {
        self.scenario
    }

    /// Ordered step identifier inside the scenario.
    pub fn step(&self) -> u64 {
        self.step
    }

    /// Sensors active before selecting a plan.
    pub fn base_vertices(&self) -> &[usize] {
        &self.base_vertices
    }

    /// Communication edges available when both endpoints are active.
    pub fn possible_edges(&self) -> &[KineticEdgeKey] {
        &self.possible_edges
    }
}

/// A finite or exact affine controlled-boundary coverage specification.
#[derive(Debug, Clone, PartialEq)]
pub struct CoverageSpecification {
    vertex_count: usize,
    model: PlanarCoverageModel,
    modulus: u32,
    fence: CoverageFence,
    failable_vertices: Vec<usize>,
    failure_budget: usize,
    source: CoverageSource,
    states: Vec<CoverageState>,
}

impl CoverageSpecification {
    /// Construct a finite coverage specification.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        vertex_count: usize,
        model: PlanarCoverageModel,
        modulus: u32,
        fence: CoverageFence,
        mut failable_vertices: Vec<usize>,
        failure_budget: usize,
        mut states: Vec<CoverageState>,
        limits: CoverageLimits,
    ) -> Result<Self> {
        failable_vertices.sort_unstable();
        failable_vertices.dedup();
        states.sort_by_key(|state| (state.scenario, state.step));
        let specification = Self {
            vertex_count,
            model,
            modulus,
            fence,
            failable_vertices,
            failure_budget,
            source: CoverageSource::Finite,
            states,
        };
        specification.validate(limits)?;
        Ok(specification)
    }

    /// Compile every graph needed for an all-time affine communication claim.
    #[allow(clippy::too_many_arguments)]
    pub fn from_kinetic(
        filtration: &KineticFiltration,
        scenario: u64,
        model: PlanarCoverageModel,
        modulus: u32,
        fence: CoverageFence,
        failable_vertices: Vec<usize>,
        failure_budget: usize,
        base_vertices: Vec<usize>,
        limits: CoverageLimits,
    ) -> Result<Self> {
        let states = filtration
            .critical_graphs(model.broadcast_radius())?
            .into_iter()
            .enumerate()
            .map(|(step, state)| {
                CoverageState::new(
                    scenario,
                    step as u64,
                    &state.graph,
                    base_vertices.clone(),
                    model.broadcast_radius(),
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let mut specification = Self::new(
            filtration.vertex_count(),
            model,
            modulus,
            fence,
            failable_vertices,
            failure_budget,
            states,
            limits,
        )?;
        specification.source = CoverageSource::Affine {
            scenario,
            edges: filtration.edges().to_vec(),
            start: filtration.start(),
            end: filtration.end(),
        };
        Ok(specification)
    }

    /// Number of sensor labels shared by every state.
    pub fn vertex_count(&self) -> usize {
        self.vertex_count
    }

    /// Controlled-boundary radius model.
    pub fn model(&self) -> PlanarCoverageModel {
        self.model
    }

    /// Prime coefficient modulus.
    pub fn modulus(&self) -> u32 {
        self.modulus
    }

    /// Canonical protected fence cycle.
    pub fn fence(&self) -> &CoverageFence {
        &self.fence
    }

    /// Sensors that the failure quantifier may remove.
    pub fn failable_vertices(&self) -> &[usize] {
        &self.failable_vertices
    }

    /// Largest simultaneous failure count.
    pub fn failure_budget(&self) -> usize {
        self.failure_budget
    }

    /// Origin and completeness scope of the state list.
    pub fn source(&self) -> &CoverageSource {
        &self.source
    }

    /// Canonical state list.
    pub fn states(&self) -> &[CoverageState] {
        &self.states
    }

    fn validate(&self, limits: CoverageLimits) -> Result<()> {
        PlanarCoverageModel::new(self.model.broadcast_radius(), self.model.sensing_radius())?;
        if self.vertex_count == 0
            || self.vertex_count > limits.max_vertices
            || self.states.is_empty()
            || self.states.len() > limits.max_states
            || self
                .fence
                .vertices()
                .iter()
                .any(|vertex| *vertex >= self.vertex_count)
            || self
                .failable_vertices
                .iter()
                .any(|vertex| *vertex >= self.vertex_count)
            || self
                .fence
                .vertices()
                .iter()
                .any(|vertex| self.failable_vertices.binary_search(vertex).is_ok())
            || self.failure_budget > self.failable_vertices.len()
        {
            return Err(Error::InvalidInput(
                "coverage specification has an invalid scope, fence, or failure model".into(),
            ));
        }
        if self
            .states
            .windows(2)
            .any(|pair| (pair[0].scenario, pair[0].step) >= (pair[1].scenario, pair[1].step))
        {
            return Err(Error::InvalidInput(
                "coverage states are not in canonical scenario and step order".into(),
            ));
        }
        for state in &self.states {
            if state
                .base_vertices
                .iter()
                .any(|vertex| *vertex >= self.vertex_count)
                || state.possible_edges.len() > limits.max_edges
                || state
                    .possible_edges
                    .iter()
                    .any(|edge| edge.u >= edge.v || edge.v >= self.vertex_count)
                || state
                    .possible_edges
                    .windows(2)
                    .any(|pair| pair[0] >= pair[1])
                || self
                    .fence
                    .vertices()
                    .iter()
                    .any(|vertex| state.base_vertices.binary_search(vertex).is_err())
            {
                return Err(Error::InvalidInput(
                    "coverage state is not canonical or omits a fence vertex".into(),
                ));
            }
        }
        Ok(())
    }

    /// Decompose the state-action incidence relation into exact components.
    pub fn components(&self, actions: &[CoverageAction]) -> Result<Vec<CoverageComponent>> {
        validate_actions(self, actions, CoverageLimits::default())?;
        let offset = self.states.len();
        let mut parent = (0..offset + actions.len()).collect::<Vec<_>>();
        for (action, candidate) in actions.iter().enumerate() {
            for &state in &candidate.states {
                union_sets(&mut parent, state, offset + action);
            }
        }
        let mut components = BTreeMap::<usize, CoverageComponent>::new();
        for state in 0..self.states.len() {
            let root = find_set(&mut parent, state);
            components.entry(root).or_default().states.push(state);
        }
        for action in 0..actions.len() {
            let root = find_set(&mut parent, offset + action);
            components.entry(root).or_default().actions.push(action);
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

/// One candidate sensor activation and the states where it is available.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CoverageAction {
    /// Sensor vertex activated by this action.
    pub vertex: usize,
    /// Positive additive action cost.
    pub cost: u64,
    states: Vec<usize>,
}

impl CoverageAction {
    /// Construct an activation and canonicalize its affected states.
    pub fn new(vertex: usize, cost: u64, mut states: Vec<usize>) -> Self {
        states.sort_unstable();
        states.dedup();
        Self {
            vertex,
            cost,
            states,
        }
    }

    /// State indices where this sensor is activated.
    pub fn states(&self) -> &[usize] {
        &self.states
    }

    /// Construct an activation that applies to every state.
    pub fn throughout(vertex: usize, cost: u64, specification: &CoverageSpecification) -> Self {
        Self::new(vertex, cost, (0..specification.states.len()).collect())
    }
}

/// One independent state-action incidence component.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoverageComponent {
    states: Vec<usize>,
    actions: Vec<usize>,
}

impl CoverageComponent {
    /// State indices in this component.
    pub fn states(&self) -> &[usize] {
        &self.states
    }

    /// Action indices in this component.
    pub fn actions(&self) -> &[usize] {
        &self.actions
    }
}

/// First state and failure set that refutes a selected plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageCounterexample {
    /// State index in the canonical specification.
    pub state: usize,
    /// Failed sensor vertices in ascending order.
    pub failed_vertices: Vec<usize>,
}

/// Exact result of evaluating one selected activation set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoveragePlanEvaluation {
    /// True when every state survives every allowed failure.
    pub criterion_holds: bool,
    /// Number of state and maximal-failure pairs checked.
    pub checks: usize,
    /// Smallest witness support among accepted checks.
    pub minimum_witness_triangles: Option<usize>,
    /// First canonical failed check, when one exists.
    pub counterexample: Option<CoverageCounterexample>,
}

/// Evaluate one plan under the complete bounded failure quantifier.
pub fn evaluate_coverage_plan(
    specification: &CoverageSpecification,
    actions: &[CoverageAction],
    selected: &[usize],
    limits: CoverageLimits,
) -> Result<CoveragePlanEvaluation> {
    evaluate_coverage_plan_states(
        specification,
        actions,
        selected,
        &(0..specification.states.len()).collect::<Vec<_>>(),
        limits,
    )
}

pub(crate) fn evaluate_coverage_plan_states(
    specification: &CoverageSpecification,
    actions: &[CoverageAction],
    selected: &[usize],
    state_indices: &[usize],
    limits: CoverageLimits,
) -> Result<CoveragePlanEvaluation> {
    specification.validate(limits)?;
    validate_actions(specification, actions, limits)?;
    evaluate_coverage_plan_states_prevalidated(
        specification,
        actions,
        selected,
        state_indices,
        limits,
    )
}

pub(crate) fn evaluate_coverage_plan_states_prevalidated(
    specification: &CoverageSpecification,
    actions: &[CoverageAction],
    selected: &[usize],
    state_indices: &[usize],
    limits: CoverageLimits,
) -> Result<CoveragePlanEvaluation> {
    if selected.windows(2).any(|pair| pair[0] >= pair[1])
        || selected.iter().any(|action| *action >= actions.len())
        || state_indices.windows(2).any(|pair| pair[0] >= pair[1])
        || state_indices
            .iter()
            .any(|state| *state >= specification.states.len())
    {
        return Err(Error::InvalidInput(
            "coverage selected action or state indices are not canonical".into(),
        ));
    }
    let mut checks = 0usize;
    let mut minimum_witness = None;
    for &state_index in state_indices {
        let state = &specification.states[state_index];
        let mut active = state.base_vertices.clone();
        for &action in selected {
            if actions[action].states.binary_search(&state_index).is_ok() {
                insert_sorted(&mut active, actions[action].vertex);
            }
        }
        let failable = active
            .iter()
            .copied()
            .filter(|vertex| {
                specification
                    .failable_vertices
                    .binary_search(vertex)
                    .is_ok()
            })
            .collect::<Vec<_>>();
        let failure_count = specification.failure_budget.min(failable.len());
        let graph = graph_from_edges(specification.vertex_count, &state.possible_edges)?;
        let mut combination = Vec::with_capacity(failure_count);
        let mut outcome = None;
        visit_combinations(
            &failable,
            failure_count,
            0,
            &mut combination,
            &mut |failures| {
                checks = checks.checked_add(1).ok_or_else(|| {
                    Error::InvalidInput("coverage failure check count overflows".into())
                })?;
                if checks > limits.max_failure_sets {
                    return Err(Error::InvalidInput(
                        "coverage failure sets exceed their limit".into(),
                    ));
                }
                let remaining = active
                    .iter()
                    .copied()
                    .filter(|vertex| failures.binary_search(vertex).is_err())
                    .collect::<Vec<_>>();
                let evaluation = evaluate_planar_coverage(
                    &graph,
                    &remaining,
                    &specification.fence,
                    specification.modulus,
                    specification.model,
                    limits,
                )?;
                if evaluation.criterion_holds {
                    minimum_witness = Some(
                        minimum_witness
                            .unwrap_or(usize::MAX)
                            .min(evaluation.witness.len()),
                    );
                    Ok(true)
                } else {
                    outcome = Some(CoverageCounterexample {
                        state: state_index,
                        failed_vertices: failures.to_vec(),
                    });
                    Ok(false)
                }
            },
        )?;
        if outcome.is_some() {
            return Ok(CoveragePlanEvaluation {
                criterion_holds: false,
                checks,
                minimum_witness_triangles: None,
                counterexample: outcome,
            });
        }
    }
    Ok(CoveragePlanEvaluation {
        criterion_holds: true,
        checks,
        minimum_witness_triangles: minimum_witness,
        counterexample: None,
    })
}

/// Completeness status of a coverage synthesis result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverageSynthesisStatus {
    /// The selected activations have minimum total cost.
    Optimal,
    /// No plan within the activation limit satisfies the specification.
    Infeasible,
    /// A producer work limit stopped search before a complete proof.
    SearchIncomplete,
}

impl CoverageSynthesisStatus {
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
                "coverage synthesis status is invalid".into(),
            )),
        }
    }
}

impl fmt::Display for CoverageSynthesisStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Optimal => "optimal",
            Self::Infeasible => "infeasible",
            Self::SearchIncomplete => "search incomplete",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EvaluationClaim {
    criterion_holds: bool,
    checks: usize,
    minimum_witness_triangles: Option<usize>,
}

impl From<&CoveragePlanEvaluation> for EvaluationClaim {
    fn from(evaluation: &CoveragePlanEvaluation) -> Self {
        Self {
            criterion_holds: evaluation.criterion_holds,
            checks: evaluation.checks,
            minimum_witness_triangles: evaluation.minimum_witness_triangles,
        }
    }
}

/// Self-contained proof-carrying coverage synthesis result.
#[derive(Debug, Clone, PartialEq)]
pub struct CoverageSynthesisArtifact {
    specification: CoverageSpecification,
    actions: Vec<CoverageAction>,
    max_activations: usize,
    oracle_limit: usize,
    node_limit: usize,
    status: CoverageSynthesisStatus,
    selected: Vec<usize>,
    lower_bound_cost: Option<u64>,
    upper_bound_cost: Option<u64>,
    producer_oracle_calls: usize,
    producer_search_nodes: usize,
    producer_cache_hits: usize,
    root_blockers: Vec<Vec<usize>>,
    before: EvaluationClaim,
    after: EvaluationClaim,
    proof: Option<ProofNode>,
    proof_work: ProofWork,
    digest: [u8; 32],
}

impl CoverageSynthesisArtifact {
    /// Solve a minimum-cost activation problem and build its proof tree.
    pub fn build(
        specification: CoverageSpecification,
        actions: Vec<CoverageAction>,
        max_activations: usize,
        limits: CoverageSynthesisLimits,
    ) -> Result<Self> {
        specification.validate(limits.coverage)?;
        validate_source(&specification, limits)?;
        validate_actions(&specification, &actions, limits.coverage)?;
        if limits.max_oracle_calls == 0 || limits.max_search_nodes == 0 {
            return Err(Error::InvalidInput(
                "coverage synthesis search limits must be positive".into(),
            ));
        }
        let costs = actions.iter().map(|action| action.cost).collect::<Vec<_>>();
        let components = specification.components(&actions)?;
        let (
            status,
            selected,
            lower_bound,
            upper_bound,
            oracle_calls,
            search_nodes,
            cache_hits,
            root_blockers,
        ) = if components.len() > 1 {
            let composition =
                compose_coverage_frontiers(&specification, &actions, max_activations, limits)?;
            match composition.status() {
                CoverageCompositionStatus::Optimal => (
                    CoverageSynthesisStatus::Optimal,
                    composition.selected().to_vec(),
                    composition.cost(),
                    composition.cost(),
                    composition.oracle_calls(),
                    composition.search_nodes(),
                    composition.cache_hits(),
                    Vec::new(),
                ),
                CoverageCompositionStatus::Infeasible => (
                    CoverageSynthesisStatus::Infeasible,
                    Vec::new(),
                    None,
                    None,
                    composition.oracle_calls(),
                    composition.search_nodes(),
                    composition.cache_hits(),
                    Vec::new(),
                ),
                CoverageCompositionStatus::SearchIncomplete => (
                    CoverageSynthesisStatus::SearchIncomplete,
                    Vec::new(),
                    Some(0),
                    None,
                    composition.oracle_calls(),
                    composition.search_nodes(),
                    composition.cache_hits(),
                    Vec::new(),
                ),
            }
        } else {
            let search = minimize_antitone(
                &costs,
                max_activations,
                SearchLimits {
                    oracle_calls: limits.max_oracle_calls,
                    search_nodes: limits.max_search_nodes,
                },
                |selected| survives(&specification, &actions, selected, limits.coverage),
            )?;
            (
                match search.status {
                    SearchStatus::Optimal => CoverageSynthesisStatus::Optimal,
                    SearchStatus::Infeasible => CoverageSynthesisStatus::Infeasible,
                    SearchStatus::Incomplete => CoverageSynthesisStatus::SearchIncomplete,
                },
                search.selected,
                search.lower_bound,
                search.upper_bound,
                search.oracle_calls,
                search.search_nodes,
                search.cache_hits,
                search.root_blockers,
            )
        };
        let before = evaluate_coverage_plan(&specification, &actions, &[], limits.coverage)?;
        let after = evaluate_coverage_plan(&specification, &actions, &selected, limits.coverage)?;
        let (proof, proof_work) = match status {
            CoverageSynthesisStatus::Optimal => {
                let cutoff = upper_bound.ok_or_else(|| {
                    Error::InvalidInput("optimal coverage result has no cost".into())
                })?;
                let mut oracle = |selected: &[usize]| {
                    survives(&specification, &actions, selected, limits.coverage)
                };
                let (proof, work) = build_proof(
                    &costs,
                    max_activations.min(actions.len()),
                    Some(cutoff),
                    limits.proof(),
                    &mut oracle,
                )?;
                (Some(proof), work)
            }
            CoverageSynthesisStatus::Infeasible => {
                let mut oracle = |selected: &[usize]| {
                    survives(&specification, &actions, selected, limits.coverage)
                };
                let (proof, work) = build_proof(
                    &costs,
                    max_activations.min(actions.len()),
                    None,
                    limits.proof(),
                    &mut oracle,
                )?;
                (Some(proof), work)
            }
            CoverageSynthesisStatus::SearchIncomplete => (None, ProofWork::default()),
        };
        let mut artifact = Self {
            specification,
            actions,
            max_activations,
            oracle_limit: limits.max_oracle_calls,
            node_limit: limits.max_search_nodes,
            status,
            selected,
            lower_bound_cost: lower_bound,
            upper_bound_cost: upper_bound,
            producer_oracle_calls: oracle_calls,
            producer_search_nodes: search_nodes,
            producer_cache_hits: cache_hits,
            root_blockers,
            before: EvaluationClaim::from(&before),
            after: EvaluationClaim::from(&after),
            proof,
            proof_work,
            digest: [0; 32],
        };
        artifact.verify(limits)?;
        artifact.digest = artifact.compute_digest()?;
        Ok(artifact)
    }

    /// Verify the semantic claims and proof without repeating producer search.
    pub fn verify(&self, limits: CoverageSynthesisLimits) -> Result<()> {
        self.specification.validate(limits.coverage)?;
        validate_source(&self.specification, limits)?;
        validate_actions(&self.specification, &self.actions, limits.coverage)?;
        if self.oracle_limit == 0
            || self.node_limit == 0
            || self.producer_oracle_calls > self.oracle_limit
            || self.producer_search_nodes > self.node_limit
            || self.selected.len() > self.max_activations.min(self.actions.len())
            || self
                .selected
                .iter()
                .any(|index| *index >= self.actions.len())
            || self.selected.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(Error::InvalidInput(
                "coverage synthesis result shape or producer work is invalid".into(),
            ));
        }
        let before =
            evaluate_coverage_plan(&self.specification, &self.actions, &[], limits.coverage)?;
        let after = evaluate_coverage_plan(
            &self.specification,
            &self.actions,
            &self.selected,
            limits.coverage,
        )?;
        if self.before != EvaluationClaim::from(&before)
            || self.after != EvaluationClaim::from(&after)
        {
            return Err(Error::InvalidInput(
                "coverage synthesis evaluation claims differ from exact checks".into(),
            ));
        }
        let costs = self
            .actions
            .iter()
            .map(|action| action.cost)
            .collect::<Vec<_>>();
        let selected_cost = selected_cost(&costs, &self.selected)?;
        let mut root_oracle = |selected: &[usize]| {
            survives(
                &self.specification,
                &self.actions,
                selected,
                limits.coverage,
            )
        };
        verify_root_blockers(
            &self.root_blockers,
            &costs,
            self.lower_bound_cost,
            &mut root_oracle,
        )?;
        let cutoff = match self.status {
            CoverageSynthesisStatus::Optimal => Some(selected_cost),
            CoverageSynthesisStatus::Infeasible => None,
            CoverageSynthesisStatus::SearchIncomplete => self.upper_bound_cost,
        };
        let mut proof_oracle = |selected: &[usize]| {
            survives(
                &self.specification,
                &self.actions,
                selected,
                limits.coverage,
            )
        };
        match self.status {
            CoverageSynthesisStatus::Optimal => {
                if !after.criterion_holds
                    || self.lower_bound_cost != Some(selected_cost)
                    || self.upper_bound_cost != Some(selected_cost)
                {
                    return Err(Error::InvalidInput(
                        "optimal coverage result has an invalid incumbent or bound".into(),
                    ));
                }
                let work = verify_proof(
                    self.proof.as_ref().ok_or_else(|| {
                        Error::InvalidInput("optimal coverage result has no proof tree".into())
                    })?,
                    &costs,
                    self.max_activations.min(self.actions.len()),
                    cutoff,
                    limits.proof(),
                    &mut proof_oracle,
                )?;
                if work != self.proof_work {
                    return Err(Error::InvalidInput(
                        "coverage proof work differs from the checked tree".into(),
                    ));
                }
            }
            CoverageSynthesisStatus::Infeasible => {
                if !self.selected.is_empty()
                    || self.lower_bound_cost.is_some()
                    || self.upper_bound_cost.is_some()
                    || after.criterion_holds
                {
                    return Err(Error::InvalidInput(
                        "infeasible coverage result has an incumbent or finite bound".into(),
                    ));
                }
                let work = verify_proof(
                    self.proof.as_ref().ok_or_else(|| {
                        Error::InvalidInput("infeasible coverage result has no proof tree".into())
                    })?,
                    &costs,
                    self.max_activations.min(self.actions.len()),
                    None,
                    limits.proof(),
                    &mut proof_oracle,
                )?;
                if work != self.proof_work {
                    return Err(Error::InvalidInput(
                        "coverage proof work differs from the checked tree".into(),
                    ));
                }
            }
            CoverageSynthesisStatus::SearchIncomplete => {
                if self.proof.is_some()
                    || self.proof_work != ProofWork::default()
                    || self.upper_bound_cost.is_some() != after.criterion_holds
                    || self
                        .upper_bound_cost
                        .is_some_and(|cost| cost != selected_cost)
                    || self
                        .lower_bound_cost
                        .zip(self.upper_bound_cost)
                        .is_some_and(|(lower, upper)| lower > upper)
                {
                    return Err(Error::InvalidInput(
                        "incomplete coverage result has an invalid gap".into(),
                    ));
                }
            }
        }
        if self
            .proof
            .as_ref()
            .is_some_and(|proof| proof_topology_checks(proof) != self.proof_work.checks)
        {
            return Err(Error::InvalidInput(
                "coverage proof check count differs from its tree".into(),
            ));
        }
        Ok(())
    }

    /// Coverage specification bound to this result.
    pub fn specification(&self) -> &CoverageSpecification {
        &self.specification
    }

    /// Canonical candidate activation list.
    pub fn actions(&self) -> &[CoverageAction] {
        &self.actions
    }

    /// Search completeness status.
    pub fn status(&self) -> CoverageSynthesisStatus {
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

    /// Producer topology calls made during optimization.
    pub fn producer_oracle_calls(&self) -> usize {
        self.producer_oracle_calls
    }

    /// Producer branch nodes visited during optimization.
    pub fn producer_search_nodes(&self) -> usize {
        self.producer_search_nodes
    }

    /// Topology checks required by the proof tree.
    pub fn proof_topology_checks(&self) -> usize {
        self.proof_work.checks
    }

    /// Proof-tree node count.
    pub fn proof_nodes(&self) -> usize {
        self.proof_work.nodes
    }

    /// Number of state-failure checks for the selected plan.
    pub fn selected_failure_checks(&self) -> usize {
        self.after.checks
    }

    /// Smallest selected-plan witness support across all checked failures.
    pub fn minimum_witness_triangles(&self) -> Option<usize> {
        self.after.minimum_witness_triangles
    }

    /// Content digest of the complete claim.
    pub fn digest(&self) -> &[u8; 32] {
        &self.digest
    }

    /// Encode canonical `HOLOSCOV` version 1 bytes.
    pub fn encode(&self, limits: CoverageSynthesisLimits) -> Result<Vec<u8>> {
        self.verify(limits)?;
        let mut output = self.encode_payload()?;
        output.extend_from_slice(&self.digest);
        if output.len() > limits.max_bytes {
            return Err(Error::InvalidInput(
                "coverage artifact exceeds its byte limit".into(),
            ));
        }
        Ok(output)
    }

    /// Decode and verify canonical `HOLOSCOV` version 1 bytes.
    pub fn decode(bytes: &[u8], limits: CoverageSynthesisLimits) -> Result<Self> {
        if bytes.len() > limits.max_bytes || bytes.len() < 32 {
            return Err(Error::InvalidInput(
                "coverage artifact exceeds its byte limit or is truncated".into(),
            ));
        }
        let mut reader = Reader::new(bytes);
        if reader.take(8)? != MAGIC || reader.u16()? != VERSION || reader.u8()? != F64_BITS_CODEC {
            return Err(Error::InvalidInput("unsupported coverage artifact".into()));
        }
        let vertex_count = reader.bounded_usize("vertex count", limits.coverage.max_vertices)?;
        let model =
            PlanarCoverageModel::new(f64::from_bits(reader.u64()?), f64::from_bits(reader.u64()?))?;
        let modulus = reader.u32()?;
        let fence = CoverageFence::new(decode_usizes(&mut reader, limits.coverage.max_vertices)?)?;
        let failable_vertices =
            decode_indices(&mut reader, vertex_count, limits.coverage.max_vertices)?;
        let failure_budget = reader.usize()?;
        let source = decode_source(&mut reader, limits)?;
        let state_count = reader.bounded_usize("state count", limits.coverage.max_states)?;
        let mut states = Vec::with_capacity(state_count);
        for _ in 0..state_count {
            states.push(CoverageState {
                scenario: reader.u64()?,
                step: reader.u64()?,
                base_vertices: decode_indices(
                    &mut reader,
                    vertex_count,
                    limits.coverage.max_vertices,
                )?,
                possible_edges: decode_edges(&mut reader, vertex_count, limits.coverage.max_edges)?,
            });
        }
        let specification = CoverageSpecification {
            vertex_count,
            model,
            modulus,
            fence,
            failable_vertices,
            failure_budget,
            source,
            states,
        };
        let action_count = reader.bounded_usize("action count", limits.coverage.max_actions)?;
        let mut actions = Vec::with_capacity(action_count);
        for _ in 0..action_count {
            actions.push(CoverageAction {
                vertex: reader.usize()?,
                cost: reader.u64()?,
                states: decode_indices(&mut reader, state_count, state_count)?,
            });
        }
        let max_activations = reader.usize()?;
        let oracle_limit = reader.usize()?;
        let node_limit = reader.usize()?;
        let status = CoverageSynthesisStatus::from_code(reader.u8()?)?;
        let selected = decode_indices(&mut reader, action_count, action_count)?;
        let lower_bound_cost = reader.optional_u64()?;
        let upper_bound_cost = reader.optional_u64()?;
        let producer_oracle_calls = reader.usize()?;
        let producer_search_nodes = reader.usize()?;
        let producer_cache_hits = reader.usize()?;
        let blocker_count = reader.bounded_usize("root blocker count", limits.max_proof_terms)?;
        let mut root_blockers = Vec::with_capacity(blocker_count);
        let mut terms = 0usize;
        for _ in 0..blocker_count {
            let blocker = decode_indices(&mut reader, action_count, limits.max_proof_terms)?;
            terms = terms
                .checked_add(blocker.len())
                .ok_or_else(|| Error::InvalidInput("coverage proof term count overflows".into()))?;
            if terms > limits.max_proof_terms.min(FORMAT_MAX_PROOF_TERMS) {
                return Err(Error::InvalidInput(
                    "coverage proof terms exceed their limit".into(),
                ));
            }
            root_blockers.push(blocker);
        }
        let before = decode_evaluation(&mut reader)?;
        let after = decode_evaluation(&mut reader)?;
        let mut decoded_work = ProofWork::default();
        let proof = match reader.u8()? {
            0 => None,
            1 => Some(decode_proof(
                &mut reader,
                action_count,
                0,
                &mut decoded_work,
                limits,
            )?),
            _ => {
                return Err(Error::InvalidInput(
                    "coverage proof-presence flag is invalid".into(),
                ));
            }
        };
        let proof_work = ProofWork {
            nodes: reader.usize()?,
            checks: reader.usize()?,
            terms: reader.usize()?,
        };
        if proof.is_some() && decoded_work.nodes != proof_work.nodes {
            return Err(Error::InvalidInput(
                "coverage decoded proof node count differs from its claim".into(),
            ));
        }
        let digest = reader.array32()?;
        if reader.remaining() != 0 {
            return Err(Error::InvalidInput(
                "trailing bytes follow the coverage artifact".into(),
            ));
        }
        let artifact = Self {
            specification,
            actions,
            max_activations,
            oracle_limit,
            node_limit,
            status,
            selected,
            lower_bound_cost,
            upper_bound_cost,
            producer_oracle_calls,
            producer_search_nodes,
            producer_cache_hits,
            root_blockers,
            before,
            after,
            proof,
            proof_work,
            digest,
        };
        artifact.verify(limits)?;
        if artifact.compute_digest()? != artifact.digest {
            return Err(Error::InvalidInput(
                "coverage artifact digest differs from its content".into(),
            ));
        }
        Ok(artifact)
    }

    fn encode_payload(&self) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        output.extend_from_slice(MAGIC);
        output.extend_from_slice(&VERSION.to_be_bytes());
        output.push(F64_BITS_CODEC);
        put_usize(&mut output, self.specification.vertex_count)?;
        output.extend_from_slice(
            &self
                .specification
                .model
                .broadcast_radius()
                .to_bits()
                .to_be_bytes(),
        );
        output.extend_from_slice(
            &self
                .specification
                .model
                .sensing_radius()
                .to_bits()
                .to_be_bytes(),
        );
        output.extend_from_slice(&self.specification.modulus.to_be_bytes());
        encode_usizes(&mut output, self.specification.fence.vertices())?;
        encode_usizes(&mut output, &self.specification.failable_vertices)?;
        put_usize(&mut output, self.specification.failure_budget)?;
        encode_source(&mut output, &self.specification.source)?;
        put_usize(&mut output, self.specification.states.len())?;
        for state in &self.specification.states {
            output.extend_from_slice(&state.scenario.to_be_bytes());
            output.extend_from_slice(&state.step.to_be_bytes());
            encode_usizes(&mut output, &state.base_vertices)?;
            encode_edges(&mut output, &state.possible_edges)?;
        }
        put_usize(&mut output, self.actions.len())?;
        for action in &self.actions {
            put_usize(&mut output, action.vertex)?;
            output.extend_from_slice(&action.cost.to_be_bytes());
            encode_usizes(&mut output, &action.states)?;
        }
        put_usize(&mut output, self.max_activations)?;
        put_usize(&mut output, self.oracle_limit)?;
        put_usize(&mut output, self.node_limit)?;
        output.push(self.status.code());
        encode_usizes(&mut output, &self.selected)?;
        encode_optional_u64(&mut output, self.lower_bound_cost);
        encode_optional_u64(&mut output, self.upper_bound_cost);
        put_usize(&mut output, self.producer_oracle_calls)?;
        put_usize(&mut output, self.producer_search_nodes)?;
        put_usize(&mut output, self.producer_cache_hits)?;
        put_usize(&mut output, self.root_blockers.len())?;
        for blocker in &self.root_blockers {
            encode_usizes(&mut output, blocker)?;
        }
        encode_evaluation(&mut output, self.before)?;
        encode_evaluation(&mut output, self.after)?;
        match &self.proof {
            Some(proof) => {
                output.push(1);
                encode_proof(&mut output, proof)?;
            }
            None => output.push(0),
        }
        put_usize(&mut output, self.proof_work.nodes)?;
        put_usize(&mut output, self.proof_work.checks)?;
        put_usize(&mut output, self.proof_work.terms)?;
        Ok(output)
    }

    fn compute_digest(&self) -> Result<[u8; 32]> {
        let payload = self.encode_payload()?;
        Ok(Sha256::digest(payload).into())
    }
}

fn survives(
    specification: &CoverageSpecification,
    actions: &[CoverageAction],
    selected: &[usize],
    limits: CoverageLimits,
) -> Result<bool> {
    Ok(!evaluate_coverage_plan_states_prevalidated(
        specification,
        actions,
        selected,
        &(0..specification.states.len()).collect::<Vec<_>>(),
        limits,
    )?
    .criterion_holds)
}

fn selected_cost(costs: &[u64], selected: &[usize]) -> Result<u64> {
    selected.iter().try_fold(0u64, |sum, action| {
        sum.checked_add(costs[*action])
            .ok_or_else(|| Error::InvalidInput("coverage selected cost overflows".into()))
    })
}

fn validate_source(
    specification: &CoverageSpecification,
    limits: CoverageSynthesisLimits,
) -> Result<()> {
    let CoverageSource::Affine {
        scenario,
        edges,
        start,
        end,
    } = &specification.source
    else {
        return Ok(());
    };
    let base = specification
        .states
        .first()
        .map(|state| state.base_vertices.clone())
        .ok_or_else(|| Error::InvalidInput("coverage affine source has no states".into()))?;
    if specification
        .states
        .iter()
        .any(|state| state.base_vertices != base)
    {
        return Err(Error::InvalidInput(
            "coverage affine source changes its base sensor set".into(),
        ));
    }
    let filtration = KineticFiltration::new(
        specification.vertex_count,
        edges.clone(),
        *start,
        *end,
        limits.kinetic,
    )?;
    let rebuilt = CoverageSpecification::from_kinetic(
        &filtration,
        *scenario,
        specification.model,
        specification.modulus,
        specification.fence.clone(),
        specification.failable_vertices.clone(),
        specification.failure_budget,
        base,
        limits.coverage,
    )?;
    if rebuilt.states != specification.states {
        return Err(Error::InvalidInput(
            "coverage affine states differ from the complete threshold schedule".into(),
        ));
    }
    Ok(())
}

fn validate_actions(
    specification: &CoverageSpecification,
    actions: &[CoverageAction],
    limits: CoverageLimits,
) -> Result<()> {
    let fence: BTreeSet<_> = specification.fence.vertices().iter().copied().collect();
    let mut vertices = BTreeSet::new();
    if actions.len() > limits.max_actions
        || actions.iter().any(|action| {
            action.vertex >= specification.vertex_count
                || action.cost == 0
                || action.states.is_empty()
                || action
                    .states
                    .iter()
                    .any(|state| *state >= specification.states.len())
                || fence.contains(&action.vertex)
                || !vertices.insert(action.vertex)
                || action.states.iter().any(|state| {
                    specification.states[*state]
                        .base_vertices
                        .binary_search(&action.vertex)
                        .is_ok()
                })
        })
    {
        return Err(Error::InvalidInput(
            "coverage actions exceed their limit or are not canonical activations".into(),
        ));
    }
    actions.iter().try_fold(0u64, |sum, action| {
        sum.checked_add(action.cost)
            .ok_or_else(|| Error::InvalidInput("coverage action cost sum overflows".into()))
    })?;
    Ok(())
}

fn visit_combinations<F>(
    values: &[usize],
    count: usize,
    start: usize,
    current: &mut Vec<usize>,
    callback: &mut F,
) -> Result<bool>
where
    F: FnMut(&[usize]) -> Result<bool>,
{
    if current.len() == count {
        return callback(current);
    }
    let needed = count - current.len();
    for position in start..=values.len() - needed {
        current.push(values[position]);
        if !visit_combinations(values, count, position + 1, current, callback)? {
            current.pop();
            return Ok(false);
        }
        current.pop();
    }
    Ok(true)
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

fn insert_sorted(values: &mut Vec<usize>, value: usize) {
    if let Err(position) = values.binary_search(&value) {
        values.insert(position, value);
    }
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

fn encode_source(output: &mut Vec<u8>, source: &CoverageSource) -> Result<()> {
    match source {
        CoverageSource::Finite => output.push(0),
        CoverageSource::Affine {
            scenario,
            edges,
            start,
            end,
        } => {
            output.push(1);
            output.extend_from_slice(&scenario.to_be_bytes());
            output.extend_from_slice(&start.to_bits().to_be_bytes());
            output.extend_from_slice(&end.to_bits().to_be_bytes());
            put_usize(output, edges.len())?;
            for edge in edges {
                put_usize(output, edge.u)?;
                put_usize(output, edge.v)?;
                output.extend_from_slice(&edge.intercept.to_bits().to_be_bytes());
                output.extend_from_slice(&edge.velocity.to_bits().to_be_bytes());
            }
        }
    }
    Ok(())
}

fn decode_source(
    reader: &mut Reader<'_>,
    limits: CoverageSynthesisLimits,
) -> Result<CoverageSource> {
    match reader.u8()? {
        0 => Ok(CoverageSource::Finite),
        1 => {
            let scenario = reader.u64()?;
            let start = f64::from_bits(reader.u64()?);
            let end = f64::from_bits(reader.u64()?);
            let count = reader.bounded_usize("affine edge count", limits.kinetic.max_edges)?;
            if count > reader.remaining() / 32 {
                return Err(Error::InvalidInput(
                    "coverage affine edges exceed the remaining bytes".into(),
                ));
            }
            let mut edges = Vec::with_capacity(count);
            for _ in 0..count {
                edges.push(KineticEdge {
                    u: reader.usize()?,
                    v: reader.usize()?,
                    intercept: f64::from_bits(reader.u64()?),
                    velocity: f64::from_bits(reader.u64()?),
                });
            }
            Ok(CoverageSource::Affine {
                scenario,
                edges,
                start,
                end,
            })
        }
        _ => Err(Error::InvalidInput(
            "coverage source kind is invalid".into(),
        )),
    }
}

fn encode_evaluation(output: &mut Vec<u8>, claim: EvaluationClaim) -> Result<()> {
    output.push(u8::from(claim.criterion_holds));
    put_usize(output, claim.checks)?;
    encode_optional_usize(output, claim.minimum_witness_triangles)?;
    Ok(())
}

fn decode_evaluation(reader: &mut Reader<'_>) -> Result<EvaluationClaim> {
    let criterion_holds = match reader.u8()? {
        0 => false,
        1 => true,
        _ => {
            return Err(Error::InvalidInput(
                "coverage evaluation Boolean is invalid".into(),
            ));
        }
    };
    Ok(EvaluationClaim {
        criterion_holds,
        checks: reader.usize()?,
        minimum_witness_triangles: reader.optional_usize()?,
    })
}

fn encode_proof(output: &mut Vec<u8>, proof: &ProofNode) -> Result<()> {
    match proof {
        ProofNode::Cost => output.push(1),
        ProofNode::SurvivingMaximum => output.push(2),
        ProofNode::SurvivingSelectionLimit => output.push(3),
        ProofNode::BlockerBound { kind, blockers } => {
            output.push(4);
            output.push(match kind {
                BoundKind::Cost => 1,
                BoundKind::Selections => 2,
            });
            put_usize(output, blockers.len())?;
            for blocker in blockers {
                encode_usizes(output, blocker)?;
            }
        }
        ProofNode::Branch { blocker, children } => {
            output.push(5);
            encode_usizes(output, blocker)?;
            put_usize(output, children.len())?;
            for child in children {
                encode_proof(output, child)?;
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn decode_proof(
    reader: &mut Reader<'_>,
    action_count: usize,
    depth: usize,
    work: &mut ProofWork,
    limits: CoverageSynthesisLimits,
) -> Result<ProofNode> {
    work.nodes = work
        .nodes
        .checked_add(1)
        .ok_or_else(|| Error::InvalidInput("coverage proof node count overflows".into()))?;
    if work.nodes > limits.max_proof_nodes.min(FORMAT_MAX_PROOF_NODES)
        || depth > limits.max_proof_depth
    {
        return Err(Error::InvalidInput(
            "coverage proof exceeds its node or depth limit".into(),
        ));
    }
    match reader.u8()? {
        1 => Ok(ProofNode::Cost),
        2 => Ok(ProofNode::SurvivingMaximum),
        3 => Ok(ProofNode::SurvivingSelectionLimit),
        4 => {
            let kind = match reader.u8()? {
                1 => BoundKind::Cost,
                2 => BoundKind::Selections,
                _ => {
                    return Err(Error::InvalidInput(
                        "coverage proof bound kind is invalid".into(),
                    ));
                }
            };
            let count = reader.bounded_usize("proof blocker count", limits.max_proof_terms)?;
            let mut blockers = Vec::with_capacity(count);
            for _ in 0..count {
                let blocker = decode_indices(reader, action_count, limits.max_proof_terms)?;
                work.terms = work.terms.checked_add(blocker.len()).ok_or_else(|| {
                    Error::InvalidInput("coverage proof term count overflows".into())
                })?;
                if work.terms > limits.max_proof_terms.min(FORMAT_MAX_PROOF_TERMS) {
                    return Err(Error::InvalidInput(
                        "coverage proof terms exceed their limit".into(),
                    ));
                }
                blockers.push(blocker);
            }
            Ok(ProofNode::BlockerBound { kind, blockers })
        }
        5 => {
            let blocker = decode_indices(reader, action_count, limits.max_proof_terms)?;
            work.terms = work
                .terms
                .checked_add(blocker.len())
                .ok_or_else(|| Error::InvalidInput("coverage proof term count overflows".into()))?;
            if work.terms > limits.max_proof_terms.min(FORMAT_MAX_PROOF_TERMS) {
                return Err(Error::InvalidInput(
                    "coverage proof terms exceed their limit".into(),
                ));
            }
            let child_count = reader.bounded_usize("proof child count", action_count)?;
            if child_count != blocker.len() {
                return Err(Error::InvalidInput(
                    "coverage branch child count differs from its blocker".into(),
                ));
            }
            let mut children = Vec::with_capacity(child_count);
            for _ in 0..child_count {
                children.push(decode_proof(reader, action_count, depth + 1, work, limits)?);
            }
            Ok(ProofNode::Branch { blocker, children })
        }
        _ => Err(Error::InvalidInput(
            "coverage proof node kind is invalid".into(),
        )),
    }
}

fn encode_edges(output: &mut Vec<u8>, edges: &[KineticEdgeKey]) -> Result<()> {
    put_usize(output, edges.len())?;
    for edge in edges {
        put_usize(output, edge.u)?;
        put_usize(output, edge.v)?;
    }
    Ok(())
}

fn decode_edges(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    maximum: usize,
) -> Result<Vec<KineticEdgeKey>> {
    let count = reader.bounded_usize("edge count", maximum)?;
    if count > reader.remaining() / 16 {
        return Err(Error::InvalidInput(
            "coverage edge count exceeds the remaining bytes".into(),
        ));
    }
    let edges = (0..count)
        .map(|_| {
            Ok(KineticEdgeKey {
                u: reader.usize()?,
                v: reader.usize()?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    if edges
        .iter()
        .any(|edge| edge.u >= edge.v || edge.v >= vertex_count)
        || edges.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(Error::InvalidInput(
            "coverage edges are not canonical".into(),
        ));
    }
    Ok(edges)
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
            "coverage integer list exceeds the remaining bytes".into(),
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
            "coverage index list is not canonical".into(),
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

fn encode_optional_usize(output: &mut Vec<u8>, value: Option<usize>) -> Result<()> {
    match value {
        Some(value) => {
            output.push(1);
            put_usize(output, value)?;
        }
        None => output.push(0),
    }
    Ok(())
}

fn put_usize(output: &mut Vec<u8>, value: usize) -> Result<()> {
    let value = u64::try_from(value)
        .map_err(|_| Error::InvalidInput("coverage integer does not fit u64".into()))?;
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
            .ok_or_else(|| Error::InvalidInput("coverage artifact position overflows".into()))?;
        if end > self.bytes.len() {
            return Err(Error::InvalidInput("coverage artifact is truncated".into()));
        }
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }

    fn bounded_usize(&mut self, name: &str, maximum: usize) -> Result<usize> {
        let value = self.usize()?;
        if value > maximum {
            return Err(Error::InvalidInput(format!(
                "coverage {name} exceeds its limit"
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
            .map_err(|_| Error::InvalidInput("coverage integer does not fit usize".into()))
    }

    fn optional_u64(&mut self) -> Result<Option<u64>> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.u64()?)),
            _ => Err(Error::InvalidInput(
                "coverage optional integer flag is invalid".into(),
            )),
        }
    }

    fn optional_usize(&mut self) -> Result<Option<usize>> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.usize()?)),
            _ => Err(Error::InvalidInput(
                "coverage optional integer flag is invalid".into(),
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
    use crate::KineticLimits;

    fn model() -> PlanarCoverageModel {
        PlanarCoverageModel::new(1.0, 1.0).unwrap()
    }

    fn graph() -> SparseDistanceMatrix {
        SparseDistanceMatrix::from_triplets(
            6,
            &[
                (0, 1, 1.0),
                (1, 2, 1.0),
                (2, 3, 1.0),
                (0, 3, 1.0),
                (0, 4, 1.0),
                (1, 4, 1.0),
                (2, 4, 1.0),
                (3, 4, 1.0),
                (0, 5, 1.0),
                (1, 5, 1.0),
                (2, 5, 1.0),
                (3, 5, 1.0),
            ],
        )
        .unwrap()
    }

    fn specification(failures: usize) -> CoverageSpecification {
        CoverageSpecification::new(
            6,
            model(),
            2,
            CoverageFence::new(vec![0, 1, 2, 3]).unwrap(),
            vec![4, 5],
            failures,
            vec![CoverageState::new(0, 0, &graph(), vec![0, 1, 2, 3], 1.0).unwrap()],
            CoverageLimits::default(),
        )
        .unwrap()
    }

    #[test]
    fn one_candidate_covers_without_failures() {
        let specification = specification(0);
        let actions = vec![
            CoverageAction::throughout(4, 2, &specification),
            CoverageAction::throughout(5, 3, &specification),
        ];
        assert!(
            !evaluate_coverage_plan(&specification, &actions, &[], CoverageLimits::default())
                .unwrap()
                .criterion_holds
        );
        assert!(
            evaluate_coverage_plan(&specification, &actions, &[0], CoverageLimits::default())
                .unwrap()
                .criterion_holds
        );
    }

    #[test]
    fn one_failure_requires_both_redundant_candidates() {
        let specification = specification(1);
        let actions = vec![
            CoverageAction::throughout(4, 2, &specification),
            CoverageAction::throughout(5, 3, &specification),
        ];
        let failed =
            evaluate_coverage_plan(&specification, &actions, &[0], CoverageLimits::default())
                .unwrap();
        assert!(!failed.criterion_holds);
        assert_eq!(failed.counterexample.unwrap().failed_vertices, vec![4]);
        assert!(
            evaluate_coverage_plan(&specification, &actions, &[0, 1], CoverageLimits::default())
                .unwrap()
                .criterion_holds
        );
    }

    #[test]
    fn synthesis_proves_the_minimum_failure_tolerant_plan() {
        let specification = specification(1);
        let actions = vec![
            CoverageAction::throughout(4, 2, &specification),
            CoverageAction::throughout(5, 3, &specification),
        ];
        let artifact = CoverageSynthesisArtifact::build(
            specification,
            actions,
            2,
            CoverageSynthesisLimits::default(),
        )
        .unwrap();
        assert_eq!(artifact.status(), CoverageSynthesisStatus::Optimal);
        assert_eq!(artifact.selected(), &[0, 1]);
        assert_eq!(artifact.lower_bound_cost(), Some(5));
        assert_eq!(artifact.upper_bound_cost(), Some(5));
        assert!(artifact.proof_nodes() >= 1);
        let bytes = artifact.encode(CoverageSynthesisLimits::default()).unwrap();
        let decoded =
            CoverageSynthesisArtifact::decode(&bytes, CoverageSynthesisLimits::default()).unwrap();
        assert_eq!(decoded.digest(), artifact.digest());
        let independent =
            holos_tda_check::verify_coverage(&bytes, holos_tda_check::ProofLimits::default())
                .unwrap();
        assert_eq!(
            independent.status,
            holos_tda_check::VerifiedCoverageStatus::Optimal
        );
        assert_eq!(independent.total_cost, Some(5));
    }

    #[test]
    fn synthesis_artifact_mutations_fail_closed() {
        let specification = specification(0);
        let actions = vec![
            CoverageAction::throughout(4, 2, &specification),
            CoverageAction::throughout(5, 3, &specification),
        ];
        let artifact = CoverageSynthesisArtifact::build(
            specification,
            actions,
            1,
            CoverageSynthesisLimits::default(),
        )
        .unwrap();
        let bytes = artifact.encode(CoverageSynthesisLimits::default()).unwrap();
        for position in [0, bytes.len() / 2, bytes.len() - 1] {
            let mut changed = bytes.clone();
            changed[position] ^= 1;
            assert!(
                CoverageSynthesisArtifact::decode(&changed, CoverageSynthesisLimits::default())
                    .is_err()
            );
            assert!(
                holos_tda_check::verify_coverage(&changed, holos_tda_check::ProofLimits::default())
                    .is_err()
            );
        }
        assert!(
            CoverageSynthesisArtifact::decode(
                &bytes[..bytes.len() - 1],
                CoverageSynthesisLimits::default()
            )
            .is_err()
        );
        assert!(
            holos_tda_check::verify_coverage(
                &bytes[..bytes.len() - 1],
                holos_tda_check::ProofLimits::default()
            )
            .is_err()
        );
    }

    #[test]
    fn limited_search_returns_only_a_checked_gap() {
        let specification = specification(0);
        let actions = vec![
            CoverageAction::throughout(4, 2, &specification),
            CoverageAction::throughout(5, 3, &specification),
        ];
        let artifact = CoverageSynthesisArtifact::build(
            specification,
            actions,
            1,
            CoverageSynthesisLimits::default().with_max_oracle_calls(1),
        )
        .unwrap();
        assert_eq!(artifact.status(), CoverageSynthesisStatus::SearchIncomplete);
        assert!(artifact.upper_bound_cost().is_none());
        assert_eq!(artifact.lower_bound_cost(), Some(0));
    }

    #[test]
    fn maximal_failure_sets_cover_every_smaller_failure() {
        let specification = specification(1);
        let actions = vec![
            CoverageAction::throughout(4, 2, &specification),
            CoverageAction::throughout(5, 3, &specification),
        ];
        let evaluation =
            evaluate_coverage_plan(&specification, &actions, &[0, 1], CoverageLimits::default())
                .unwrap();
        assert_eq!(evaluation.checks, 2);
        assert!(evaluation.criterion_holds);
    }

    #[test]
    fn affine_compilation_includes_every_threshold_cell() {
        let edges = graph()
            .edges()
            .map(|(u, v, _)| KineticEdge {
                u,
                v,
                intercept: if (u, v) == (0, 4) { 0.5 } else { 1.0 },
                velocity: if (u, v) == (0, 4) { 1.0 } else { 0.0 },
            })
            .collect();
        let kinetic = KineticFiltration::new(6, edges, 0.0, 1.0, KineticLimits::default()).unwrap();
        let specification = CoverageSpecification::from_kinetic(
            &kinetic,
            7,
            model(),
            2,
            CoverageFence::new(vec![0, 1, 2, 3]).unwrap(),
            vec![4, 5],
            0,
            vec![0, 1, 2, 3],
            CoverageLimits::default(),
        )
        .unwrap();
        assert_eq!(specification.states.len(), 5);
        assert!(matches!(
            specification.source,
            CoverageSource::Affine { .. }
        ));
        let action = CoverageAction::throughout(5, 1, &specification);
        let artifact = CoverageSynthesisArtifact::build(
            specification,
            vec![action],
            1,
            CoverageSynthesisLimits::default(),
        )
        .unwrap();
        let bytes = artifact.encode(CoverageSynthesisLimits::default()).unwrap();
        let independent =
            holos_tda_check::verify_coverage(&bytes, holos_tda_check::ProofLimits::default())
                .unwrap();
        assert_eq!(
            independent.source,
            holos_tda_check::VerifiedCoverageSource::Affine
        );
    }

    #[test]
    fn incidence_components_separate_state_local_candidates() {
        let state = CoverageState::new(0, 0, &graph(), vec![0, 1, 2, 3], 1.0).unwrap();
        let specification = CoverageSpecification::new(
            6,
            model(),
            2,
            CoverageFence::new(vec![0, 1, 2, 3]).unwrap(),
            vec![4, 5],
            0,
            vec![state.clone(), CoverageState { step: 1, ..state }],
            CoverageLimits::default(),
        )
        .unwrap();
        let components = specification
            .components(&[
                CoverageAction::new(4, 1, vec![0]),
                CoverageAction::new(5, 1, vec![1]),
            ])
            .unwrap();
        assert_eq!(components.len(), 2);
    }
}
