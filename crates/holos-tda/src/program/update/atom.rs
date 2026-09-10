use std::collections::BTreeSet;

use crate::{
    CertificateLimits, EdgeKey, Error, ExplainedDiagram, RegionViolationKind, Result,
    SparseDistanceMatrix,
};

use super::super::composition::{local_matrix, reweight_explained};
use super::super::model::{ProgramAtomState, ProgramEvent, ProgramEventKind, ProgramWork};
use super::super::topology::simplex_edge;

fn violation_event(state: &ProgramAtomState, violation: &crate::RegionViolation) -> ProgramEvent {
    let kind = match violation.kind() {
        RegionViolationKind::VertexSetChanged => ProgramEventKind::VertexSetChanged,
        RegionViolationKind::EdgeSetChanged => ProgramEventKind::EdgeSetChanged,
        RegionViolationKind::ThresholdCrossing => ProgramEventKind::ThresholdCrossing,
        RegionViolationKind::GuardFailed => ProgramEventKind::GuardFailed,
    };
    ProgramEvent {
        kind,
        atom: Some(state.info_index),
        edge: violation.first().and_then(simplex_edge),
        guard: violation
            .guard_index()
            .map(|index| state.region.guards()[index].kind()),
    }
}

fn reusable_explanation(
    state: &ProgramAtomState,
    local: &SparseDistanceMatrix,
    modulus: u32,
    events: &mut Vec<ProgramEvent>,
) -> Result<Option<ExplainedDiagram>> {
    let violations = state.region.violations(local);
    if !violations.is_empty() {
        events.extend(
            violations
                .iter()
                .map(|violation| violation_event(state, violation)),
        );
        return Ok(None);
    }
    let evaluation = state.region.evaluate(local)?;
    reweight_explained(
        local,
        &state.explained,
        &evaluation,
        modulus,
        state.artifact.threshold(),
    )
}

pub(super) fn update_atom_state(
    state: &mut ProgramAtomState,
    updated: &SparseDistanceMatrix,
    changed: &BTreeSet<EdgeKey>,
    modulus: u32,
    limits: CertificateLimits,
    work: &mut ProgramWork,
    events: &mut Vec<ProgramEvent>,
) -> Result<()> {
    work.atoms_touched += 1;
    work.guards_checked += state.region.guards().len();
    let local = local_matrix(&state.vertices, &state.edges, updated)?;
    if let Some(explained) = reusable_explanation(state, &local, modulus, events)? {
        install_reused_atom(state, local, explained, limits)?;
        work.atoms_reused += 1;
        return Ok(());
    }
    repair_atom_state(state, local, changed, limits, work, events)
}

fn install_reused_atom(
    state: &mut ProgramAtomState,
    local: SparseDistanceMatrix,
    explained: ExplainedDiagram,
    limits: CertificateLimits,
) -> Result<()> {
    state.artifact = state
        .artifact
        .rebind(&state.certified_graph, &local, explained.clone(), limits)
        .map_err(|error| Error::InvalidInput(error.to_string()))?;
    state.certified_graph = local;
    state.explained = explained;
    Ok(())
}

fn repair_atom_state(
    state: &mut ProgramAtomState,
    local: SparseDistanceMatrix,
    changed: &BTreeSet<EdgeKey>,
    limits: CertificateLimits,
    work: &mut ProgramWork,
    events: &mut Vec<ProgramEvent>,
) -> Result<()> {
    let repair = state
        .artifact
        .repair(&state.certified_graph, &local, limits)
        .map_err(|error| Error::InvalidInput(error.to_string()))?;
    let repair_mode = repair.mode();
    let repair_work = repair.work();
    let artifact = repair.into_artifact();
    let region = artifact
        .reduction_certificate()
        .compile_region(&local, limits)?;
    state.explained = artifact.explained().clone();
    state.artifact = artifact;
    state.certified_graph = local;
    state.region = region;
    work.reduction_columns_reused += repair_work.columns_reused();
    work.reduction_columns_reduced += repair_work.columns_reduced();
    work.reduction_column_additions += repair_work.column_additions;
    events.push(repair_event(state, changed, repair_mode, work));
    Ok(())
}

fn repair_event(
    state: &ProgramAtomState,
    changed: &BTreeSet<EdgeKey>,
    repair_mode: crate::ReductionRepairMode,
    work: &mut ProgramWork,
) -> ProgramEvent {
    // A class-space rebuild can retain every reduction column.
    let kind = if repair_mode == crate::ReductionRepairMode::SuffixRepaired {
        work.atoms_repaired += 1;
        ProgramEventKind::ReductionSuffixRepaired
    } else {
        work.atoms_rebuilt += 1;
        ProgramEventKind::AtomRebuilt
    };
    ProgramEvent {
        kind,
        atom: Some(state.info_index),
        edge: state
            .edges
            .iter()
            .find(|edge| changed.contains(edge))
            .copied(),
        guard: None,
    }
}

pub(super) fn preview_atom_state(
    state: &ProgramAtomState,
    updated: &SparseDistanceMatrix,
    changed: &BTreeSet<EdgeKey>,
    modulus: u32,
    limits: CertificateLimits,
    work: &mut ProgramWork,
    events: &mut Vec<ProgramEvent>,
) -> Result<()> {
    work.atoms_touched += 1;
    work.guards_checked += state.region.guards().len();
    let local = local_matrix(&state.vertices, &state.edges, updated)?;
    if reusable_explanation(state, &local, modulus, events)?.is_some() {
        work.atoms_reused += 1;
    } else {
        let repair = state
            .artifact
            .repair(&state.certified_graph, &local, limits)
            .map_err(|error| Error::InvalidInput(error.to_string()))?;
        let repair_work = repair.work();
        work.reduction_columns_reused += repair_work.columns_reused();
        work.reduction_columns_reduced += repair_work.columns_reduced();
        work.reduction_column_additions += repair_work.column_additions;
        events.push(repair_event(state, changed, repair.mode(), work));
    }
    Ok(())
}
