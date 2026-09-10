use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use crate::{
    CohomologyLimits, CohomologySpace, CohomologySubspace, Error, KineticEdgeKey,
    KineticFiltration, Result, SparseDistanceMatrix, cohomology_restriction, cohomology_space,
};

use super::model::{
    SynthesisAction, SynthesisCoordinate, SynthesisLimits, SynthesisSource, SynthesisState,
    TopologicalSpecification,
};
use super::{FORMAT_MAX_ACTIONS, FORMAT_MAX_STATES};

pub(super) struct TopologyOracle<'a> {
    specification: &'a TopologicalSpecification,
    actions: &'a [SynthesisAction],
    graphs: Vec<SparseDistanceMatrix>,
    spaces: Vec<CohomologySpace>,
    targets: Vec<CohomologySubspace>,
    limits: CohomologyLimits,
    cache: RefCell<BTreeMap<(usize, Vec<usize>), usize>>,
}

impl<'a> TopologyOracle<'a> {
    pub(super) fn build(
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

    pub(super) fn target_ranks(&self) -> Vec<usize> {
        self.targets.iter().map(CohomologySubspace::rank).collect()
    }

    pub(super) fn survives(&self, selected: &[usize]) -> Result<bool> {
        Ok(self
            .intersection_ranks(selected)?
            .iter()
            .zip(&self.specification.states)
            .any(|(rank, state)| *rank > state.max_surviving_rank))
    }

    pub(super) fn intersection_ranks(&self, selected: &[usize]) -> Result<Vec<usize>> {
        (0..self.specification.states.len())
            .map(|state| self.intersection_rank(state, selected))
            .collect()
    }

    pub(super) fn intersection_rank(&self, state: usize, selected: &[usize]) -> Result<usize> {
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

pub(super) fn validate_problem(
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

pub(super) fn validate_specification_envelope(
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

pub(super) fn validate_states(
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

pub(super) fn validate_state_order(
    prior: Option<(u64, u64)>,
    state: &SynthesisState,
) -> Result<()> {
    if prior.is_some_and(|key| key >= (state.scenario, state.step)) {
        Err(Error::InvalidInput(
            "synthesis states are not in canonical scenario and step order".into(),
        ))
    } else {
        Ok(())
    }
}

pub(super) fn validate_target(
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

pub(super) fn validate_target_row(row: &[SynthesisCoordinate], modulus: u32) -> Result<()> {
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

pub(super) fn validate_actions(
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

pub(super) fn invalid_action(
    specification: &TopologicalSpecification,
    action: &SynthesisAction,
) -> bool {
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

pub(super) fn validate_work_limits(
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

pub(super) fn validate_source(
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
pub(super) fn compile_kinetic_states(
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

pub(super) fn validate_root_blockers(
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

pub(super) fn graph_from_edges(
    vertex_count: usize,
    edges: &[KineticEdgeKey],
) -> Result<SparseDistanceMatrix> {
    SparseDistanceMatrix::from_triplets(
        vertex_count,
        &edges
            .iter()
            .map(|edge| (edge.u, edge.v, 0.0))
            .collect::<Vec<_>>(),
    )
}

pub(super) fn validate_edges(
    vertex_count: usize,
    edges: &[KineticEdgeKey],
    maximum: usize,
) -> Result<()> {
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

pub(super) fn selected_cost(costs: &[u64], selected: &[usize]) -> Result<u64> {
    selected.iter().try_fold(0u64, |sum, candidate| {
        sum.checked_add(costs[*candidate])
            .ok_or_else(|| Error::InvalidInput("synthesis selected cost overflows".into()))
    })
}

pub(super) fn blocker_min_cost(costs: &[u64], blocker: &[usize]) -> u64 {
    blocker
        .iter()
        .map(|candidate| costs[*candidate])
        .min()
        .unwrap_or(0)
}

pub(super) fn blocker_bound(costs: &[u64], blockers: &[Vec<usize>]) -> Result<u64> {
    blockers.iter().try_fold(0u64, |sum, blocker| {
        sum.checked_add(blocker_min_cost(costs, blocker))
            .ok_or_else(|| Error::InvalidInput("synthesis blocker bound overflows".into()))
    })
}

pub(super) fn insert_sorted(values: &mut Vec<usize>, value: usize) {
    if let Err(position) = values.binary_search(&value) {
        values.insert(position, value);
    }
}

pub(super) fn difference(values: &[usize], removed: &[usize]) -> Vec<usize> {
    values
        .iter()
        .copied()
        .filter(|value| removed.binary_search(value).is_err())
        .collect()
}

pub(super) fn merge(left: &[usize], right: &[usize]) -> Vec<usize> {
    left.iter()
        .chain(right)
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(super) fn find_set(parent: &mut [usize], value: usize) -> usize {
    if parent[value] != value {
        parent[value] = find_set(parent, parent[value]);
    }
    parent[value]
}

pub(super) fn union_sets(parent: &mut [usize], left: usize, right: usize) {
    let left = find_set(parent, left);
    let right = find_set(parent, right);
    if left != right {
        parent[right] = left;
    }
}
