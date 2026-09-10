//! Coverage specifications and their finite state sources.

use std::collections::BTreeMap;

use crate::monotone_proof::ProofLimits;
use crate::{
    CoverageFence, CoverageLimits, Error, KineticEdge, KineticEdgeKey, KineticFiltration,
    KineticLimits, PlanarCoverageModel, Result, SparseDistanceMatrix,
};

use super::super::evaluate::{find_set, union_sets, validate_actions};
use super::types::{CoverageAction, CoverageComponent};

pub(crate) const FORMAT_MAX_PROOF_NODES: usize = 10_000_000;
pub(crate) const FORMAT_MAX_PROOF_TERMS: usize = 100_000_000;

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

    pub(crate) fn proof(self) -> ProofLimits {
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
    /// Affine edge weights need not have a Euclidean realization.
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
    pub(crate) scenario: u64,
    pub(crate) step: u64,
    pub(crate) base_vertices: Vec<usize>,
    pub(crate) possible_edges: Vec<KineticEdgeKey>,
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

/// Finite or affine coverage specification over [`PlanarCoverageModel`].
#[derive(Debug, Clone, PartialEq)]
pub struct CoverageSpecification {
    pub(crate) vertex_count: usize,
    pub(crate) model: PlanarCoverageModel,
    pub(crate) modulus: u32,
    pub(crate) fence: CoverageFence,
    pub(crate) failable_vertices: Vec<usize>,
    pub(crate) failure_budget: usize,
    pub(crate) source: CoverageSource,
    pub(crate) states: Vec<CoverageState>,
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

    /// Compile the complete affine communication threshold schedule.
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

    /// Declared planar coverage model.
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

    pub(crate) fn validate(&self, limits: CoverageLimits) -> Result<()> {
        PlanarCoverageModel::new(self.model.broadcast_radius(), self.model.sensing_radius())?;
        self.validate_scope(limits)?;
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
            self.validate_state(state, limits)?;
        }
        Ok(())
    }

    pub(super) fn validate_scope(&self, limits: CoverageLimits) -> Result<()> {
        let invalid_failable = self
            .failable_vertices
            .iter()
            .any(|vertex| *vertex >= self.vertex_count);
        let failable_fence = self
            .fence
            .vertices()
            .iter()
            .any(|vertex| self.failable_vertices.binary_search(vertex).is_ok());
        if self.vertex_count == 0
            || self.vertex_count > limits.max_vertices
            || self.states.is_empty()
            || self.states.len() > limits.max_states
            || self
                .fence
                .vertices()
                .iter()
                .any(|vertex| *vertex >= self.vertex_count)
            || invalid_failable
            || failable_fence
            || self.failure_budget > self.failable_vertices.len()
        {
            Err(Error::InvalidInput(
                "coverage specification has an invalid scope, fence, or failure model".into(),
            ))
        } else {
            Ok(())
        }
    }

    pub(super) fn validate_state(
        &self,
        state: &CoverageState,
        limits: CoverageLimits,
    ) -> Result<()> {
        let invalid_edge = state
            .possible_edges
            .iter()
            .any(|edge| edge.u >= edge.v || edge.v >= self.vertex_count);
        let missing_fence = self
            .fence
            .vertices()
            .iter()
            .any(|vertex| state.base_vertices.binary_search(vertex).is_err());
        if state
            .base_vertices
            .iter()
            .any(|vertex| *vertex >= self.vertex_count)
            || state.possible_edges.len() > limits.max_edges
            || invalid_edge
            || state
                .possible_edges
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || missing_fence
        {
            Err(Error::InvalidInput(
                "coverage state is not canonical or omits a fence vertex".into(),
            ))
        } else {
            Ok(())
        }
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
