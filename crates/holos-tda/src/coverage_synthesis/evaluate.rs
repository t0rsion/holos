//! Coverage-plan evaluation and input validation.

use std::collections::BTreeSet;

use crate::{
    CoverageLimits, Error, KineticEdgeKey, KineticFiltration, Result, SparseDistanceMatrix,
    evaluate_planar_coverage,
};

use super::model::{
    CoverageAction, CoverageCounterexample, CoveragePlanEvaluation, CoverageSource,
    CoverageSpecification, CoverageSynthesisLimits,
};

/// Evaluate one plan against every maximal allowed sensor failure.
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
    validate_evaluation_indices(specification, actions, selected, state_indices)?;
    evaluate_selected_states(specification, actions, selected, state_indices, limits)
}

fn validate_evaluation_indices(
    specification: &CoverageSpecification,
    actions: &[CoverageAction],
    selected: &[usize],
    state_indices: &[usize],
) -> Result<()> {
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
    Ok(())
}

fn evaluate_selected_states(
    specification: &CoverageSpecification,
    actions: &[CoverageAction],
    selected: &[usize],
    state_indices: &[usize],
    limits: CoverageLimits,
) -> Result<CoveragePlanEvaluation> {
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

pub(super) fn survives(
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

pub(super) fn selected_cost(costs: &[u64], selected: &[usize]) -> Result<u64> {
    selected.iter().try_fold(0u64, |sum, action| {
        sum.checked_add(costs[*action])
            .ok_or_else(|| Error::InvalidInput("coverage selected cost overflows".into()))
    })
}

pub(super) fn validate_source(
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
    if filtration.edges() != edges {
        return Err(Error::InvalidInput(
            "coverage affine trajectories are not canonical".into(),
        ));
    }
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

pub(super) fn validate_actions(
    specification: &CoverageSpecification,
    actions: &[CoverageAction],
    limits: CoverageLimits,
) -> Result<()> {
    let fence: BTreeSet<_> = specification.fence.vertices().iter().copied().collect();
    let mut vertices = BTreeSet::new();
    if actions.len() > limits.max_actions
        || actions
            .iter()
            .any(|action| invalid_coverage_action(specification, action, &fence, &mut vertices))
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

fn invalid_coverage_action(
    specification: &CoverageSpecification,
    action: &CoverageAction,
    fence: &BTreeSet<usize>,
    vertices: &mut BTreeSet<usize>,
) -> bool {
    let invalid_state = action
        .states
        .iter()
        .any(|state| *state >= specification.states.len());
    if action.vertex >= specification.vertex_count
        || action.cost == 0
        || action.states.is_empty()
        || invalid_state
        || fence.contains(&action.vertex)
        || !vertices.insert(action.vertex)
    {
        return true;
    }
    action.states.iter().any(|state| {
        specification.states[*state]
            .base_vertices
            .binary_search(&action.vertex)
            .is_ok()
    })
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
