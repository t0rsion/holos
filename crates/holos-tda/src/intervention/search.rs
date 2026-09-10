use std::collections::{BTreeMap, BTreeSet};

use super::Result;
use super::model::{
    EdgeWeightEdit, H1Intervention, InterventionArtifact, InterventionBudget, InterventionStatus,
};
use super::primitives::maximum_edit;
use crate::{
    EdgeKey, Error, IntervalGroupId, PersistenceProgram, ProgramTraceArtifact, ProgramUpdateMode,
    SparseDistanceMatrix, VerifiedProgramTrace,
};

impl PersistenceProgram {
    /// Find an independent-weight edit that shortens one finite H1 class
    /// space.
    ///
    /// The target must satisfy `birth < target_scale < death`. The candidate
    /// lowers every edge above `target_scale` in every declared destroyer
    /// triangle. `Optimal` is relative to the same reduction and critical-pair
    /// certificate. It is not a global inverse persistence claim across
    /// unrelated pairings.
    pub fn kill_h1_before(
        &self,
        target: IntervalGroupId,
        target_scale: f64,
        budget: InterventionBudget,
    ) -> Result<H1Intervention> {
        let space = validate_intervention_request(self, target, target_scale)?;
        if budget.max_candidates == 0 {
            return Ok(budget_limited_intervention(target, target_scale));
        }
        search_intervention(self, space, target, target_scale)
    }
}

fn validate_intervention_request(
    program: &PersistenceProgram,
    target: IntervalGroupId,
    target_scale: f64,
) -> Result<&crate::PersistentClassSpace> {
    if !target_scale.is_finite() || target_scale < 0.0 {
        return Err(Error::InvalidInput(format!(
            "intervention target scale must be non-negative and finite, got {target_scale}"
        )));
    }
    let space = program
        .result()
        .spaces
        .iter()
        .find(|space| space.id == target)
        .ok_or_else(|| Error::InvalidInput(format!("unknown class space {target}")))?;
    check_intervention_interval(space, target_scale)?;
    Ok(space)
}

fn check_intervention_interval(
    space: &crate::PersistentClassSpace,
    target_scale: f64,
) -> Result<()> {
    if space.interval.is_essential() {
        return Err(Error::InvalidInput(
            "the finite-death intervention does not support an essential class space".into(),
        ));
    }
    if target_scale <= space.interval.birth || target_scale >= space.interval.death {
        return Err(Error::InvalidInput(format!(
            "target scale must lie strictly inside ({}, {})",
            space.interval.birth, space.interval.death
        )));
    }
    Ok(())
}

fn budget_limited_intervention(target: IntervalGroupId, target_scale: f64) -> H1Intervention {
    H1Intervention {
        target,
        target_scale,
        status: InterventionStatus::BudgetLimited,
        lower_bound: 0.0,
        upper_bound: None,
        edits: Vec::new(),
        result: None,
        artifact: None,
    }
}

fn search_intervention(
    program: &PersistenceProgram,
    space: &crate::PersistentClassSpace,
    target: IntervalGroupId,
    target_scale: f64,
) -> Result<H1Intervention> {
    let edits = destroyer_edits(program.current_graph(), space, target_scale)?;
    if edits.is_empty() {
        return Err(Error::InvalidInput(
            "declared destroyer triangles need no edge edit".into(),
        ));
    }
    let updated = apply_edits(program.current_graph(), &edits)?;
    let trace = ProgramTraceArtifact::build(
        program.current_graph(),
        std::slice::from_ref(&updated),
        program.params(),
        program.limits(),
    )?;
    let verified = trace.verify(program.limits())?;
    if !continued_space_dies_by(&verified, target, target_scale) {
        return Ok(budget_limited_intervention(target, target_scale));
    }
    finish_intervention(program, space, target, target_scale, edits, trace, verified)
}

fn finish_intervention(
    program: &PersistenceProgram,
    space: &crate::PersistentClassSpace,
    target: IntervalGroupId,
    target_scale: f64,
    edits: Vec<EdgeWeightEdit>,
    trace: ProgramTraceArtifact,
    verified: VerifiedProgramTrace,
) -> Result<H1Intervention> {
    let upper_bound = maximum_edit(&edits);
    let reused = verified.steps[0].mode == ProgramUpdateMode::Reused;
    let lower_bound = if reused {
        space.interval.death - target_scale
    } else {
        0.0
    };
    let status = if reused && lower_bound.to_bits() == upper_bound.to_bits() {
        InterventionStatus::Optimal
    } else {
        InterventionStatus::BoundedGap
    };
    let artifact = InterventionArtifact {
        target,
        target_scale,
        status,
        lower_bound,
        upper_bound,
        edits: edits.clone(),
        trace,
    };
    artifact.verify(program.limits())?;
    Ok(H1Intervention {
        target,
        target_scale,
        status,
        lower_bound,
        upper_bound: Some(upper_bound),
        edits,
        result: Some(verified.final_program.result().clone()),
        artifact: Some(artifact),
    })
}
pub(super) fn destroyer_edits(
    graph: &SparseDistanceMatrix,
    space: &crate::PersistentClassSpace,
    target_scale: f64,
) -> Result<Vec<EdgeWeightEdit>> {
    let mut edges = BTreeSet::new();
    for pair in &space.critical_pairs {
        let death = pair.death.as_ref().ok_or_else(|| {
            Error::InvalidInput("finite class space has no destroyer triangle".into())
        })?;
        let [u, v, w]: [usize; 3] = death
            .vertices
            .as_slice()
            .try_into()
            .map_err(|_| Error::InvalidInput("destroyer is not a triangle".into()))?;
        for edge in [EdgeKey::new(u, v), EdgeKey::new(u, w), EdgeKey::new(v, w)] {
            if graph.get(edge.u, edge.v) > target_scale {
                edges.insert(edge);
            }
        }
    }
    Ok(edges
        .into_iter()
        .map(|edge| EdgeWeightEdit {
            edge,
            before: graph.get(edge.u, edge.v),
            after: target_scale,
        })
        .collect())
}

pub(super) fn apply_edits(
    graph: &SparseDistanceMatrix,
    edits: &[EdgeWeightEdit],
) -> Result<SparseDistanceMatrix> {
    let by_edge: BTreeMap<_, _> = edits.iter().map(|edit| (edit.edge, edit)).collect();
    let triplets: Vec<_> = graph
        .edges()
        .map(|(u, v, value)| {
            let edge = EdgeKey::new(u, v);
            let value = by_edge.get(&edge).map_or(value, |edit| edit.after);
            (u, v, value)
        })
        .collect();
    if by_edge
        .keys()
        .any(|edge| graph.get(edge.u, edge.v).is_infinite())
    {
        return Err(Error::InvalidInput(
            "intervention edit names an absent edge".into(),
        ));
    }
    SparseDistanceMatrix::from_triplets(graph.len(), &triplets)
}

pub(super) fn continued_space_dies_by(
    verified: &VerifiedProgramTrace,
    target: IntervalGroupId,
    target_scale: f64,
) -> bool {
    let Some(initial) = verified
        .initial_result
        .spaces
        .iter()
        .find(|space| space.id == target)
    else {
        return false;
    };
    let Some(step) = verified.steps.first() else {
        return false;
    };
    let final_by_basis: BTreeMap<_, _> = verified
        .final_program
        .result()
        .spaces
        .iter()
        .flat_map(|space| space.basis.iter().map(move |class| (class.id, space)))
        .collect();
    let mapped: BTreeMap<_, _> = step
        .continuation
        .iter()
        .filter(|record| record.old_spaces.contains(&target))
        .flat_map(|record| record.transport.iter().map(|term| (term.old, term.new)))
        .collect();
    initial.basis.iter().all(|class| {
        mapped
            .get(&class.id)
            .and_then(|new| final_by_basis.get(new))
            .is_some_and(|space| space.interval.death <= target_scale)
    })
}
