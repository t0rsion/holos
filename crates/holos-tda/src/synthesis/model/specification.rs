use std::collections::BTreeMap;

use crate::{
    CohomologyLimits, CohomologySpace, CohomologySpaceId, CohomologySubspace, Error,
    KineticEdgeKey, KineticFiltration, Result, SparseDistanceMatrix,
};

use super::super::oracle::{compile_kinetic_states, find_set, union_sets};
use super::types::{SynthesisCoordinate, SynthesisSource};

/// One finite state in a temporal or scenario-indexed specification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynthesisState {
    pub(in crate::synthesis) scenario: u64,
    pub(in crate::synthesis) step: u64,
    pub(in crate::synthesis) active_edges: Vec<KineticEdgeKey>,
    pub(in crate::synthesis) target_space: CohomologySpaceId,
    pub(in crate::synthesis) target: Vec<Vec<SynthesisCoordinate>>,
    pub(in crate::synthesis) max_surviving_rank: usize,
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
    pub(in crate::synthesis) vertex_count: usize,
    pub(in crate::synthesis) dimension: usize,
    pub(in crate::synthesis) scale: f64,
    pub(in crate::synthesis) modulus: u32,
    pub(in crate::synthesis) source: SynthesisSource,
    pub(in crate::synthesis) states: Vec<SynthesisState>,
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
    pub(in crate::synthesis) states: Vec<usize>,
}

/// One connected component of the state-action incidence relation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynthesisComponent {
    pub(in crate::synthesis) states: Vec<usize>,
    pub(in crate::synthesis) actions: Vec<usize>,
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
