use std::collections::BTreeSet;

use crate::proof::{ProofError, program_blocks};

use super::graph::ProgramGraph;
use super::model::ProgramProofLimits;
use super::trace::reweight_atom;
use super::trace_model::AtomState;
use super::trace_model::{ProgramState, TraceEvent, TraceEventKind, TraceGuardKind, TraceWork};
use super::trace_reduction::repair_work_for_graphs;
use super::trace_region::{RegionGuardKind, RegionViolation, RegionViolationKind};

pub(super) fn topology_events(
    state: &ProgramState,
    graph: &ProgramGraph,
    max_events: usize,
) -> Result<Vec<TraceEvent>, ProofError> {
    if let Some(events) = topology_change_events(state, graph, max_events)? {
        return Ok(events);
    }
    let old_topology: Vec<_> = state
        .graph
        .edges()
        .iter()
        .map(|edge| [edge.u, edge.v])
        .collect();
    let threshold = state.threshold.unwrap_or(f64::INFINITY);
    let mut events = threshold_events(state, graph, &old_topology, threshold, max_events)?;
    if events.is_empty() {
        events = separator_events(state, graph, max_events)?;
    }
    Ok(events)
}

fn topology_change_events(
    state: &ProgramState,
    graph: &ProgramGraph,
    max_events: usize,
) -> Result<Option<Vec<TraceEvent>>, ProofError> {
    if graph.vertex_count() != state.graph.vertex_count() {
        let mut events = Vec::new();
        push_event(
            &mut events,
            TraceEvent {
                kind: TraceEventKind::VertexSetChanged,
                atom: None,
                edge: None,
                guard: None,
            },
            max_events,
        )?;
        return Ok(Some(events));
    }
    let old_topology: Vec<_> = state
        .graph
        .edges()
        .iter()
        .map(|edge| [edge.u, edge.v])
        .collect();
    let new_topology: Vec<_> = graph.edges().iter().map(|edge| [edge.u, edge.v]).collect();
    if old_topology != new_topology {
        let edge = old_topology
            .iter()
            .find(|edge| new_topology.binary_search(edge).is_err())
            .or_else(|| {
                new_topology
                    .iter()
                    .find(|edge| old_topology.binary_search(edge).is_err())
            })
            .copied();
        let mut events = Vec::new();
        push_event(
            &mut events,
            TraceEvent {
                kind: TraceEventKind::EdgeSetChanged,
                atom: None,
                edge,
                guard: None,
            },
            max_events,
        )?;
        return Ok(Some(events));
    }
    Ok(None)
}

fn threshold_events(
    state: &ProgramState,
    graph: &ProgramGraph,
    topology: &[[usize; 2]],
    threshold: f64,
    max_events: usize,
) -> Result<Vec<TraceEvent>, ProofError> {
    let mut events = Vec::new();
    for edge in topology.iter().filter(|edge| {
        (graph.get(edge[0], edge[1]) <= threshold)
            != (state.graph.get(edge[0], edge[1]) <= threshold)
    }) {
        push_event(
            &mut events,
            TraceEvent {
                kind: TraceEventKind::ThresholdCrossing,
                atom: None,
                edge: Some(*edge),
                guard: None,
            },
            max_events,
        )?;
    }
    Ok(events)
}

fn separator_events(
    state: &ProgramState,
    graph: &ProgramGraph,
    max_events: usize,
) -> Result<Vec<TraceEvent>, ProofError> {
    let mut events = Vec::new();
    for &edge in &state.separator_edges {
        if graph.get(edge[0], edge[1]).to_bits() != 0 {
            push_event(
                &mut events,
                TraceEvent {
                    kind: TraceEventKind::SeparatorContractChanged,
                    atom: None,
                    edge: Some(edge),
                    guard: None,
                },
                max_events,
            )?;
        }
    }
    Ok(events)
}

pub(super) fn separator_edges(
    graph: &ProgramGraph,
    threshold: Option<f64>,
) -> Result<Vec<[usize; 2]>, ProofError> {
    let blocks = program_blocks(&graph.proof_graph(), threshold)?;
    let mut edges = BTreeSet::new();
    for (position, left) in blocks.iter().enumerate() {
        for right in &blocks[position + 1..] {
            let intersection: Vec<_> = left
                .vertices
                .iter()
                .copied()
                .filter(|vertex| right.vertices.binary_search(vertex).is_ok())
                .collect();
            for (offset, &u) in intersection.iter().enumerate() {
                for &v in &intersection[offset + 1..] {
                    edges.insert([u, v]);
                }
            }
        }
    }
    Ok(edges.into_iter().collect())
}

pub(super) fn changed_edges(old: &ProgramGraph, new: &ProgramGraph) -> BTreeSet<[usize; 2]> {
    old.edges()
        .iter()
        .filter_map(|edge| {
            (edge.value.to_bits() != new.get(edge.u, edge.v).to_bits()).then_some([edge.u, edge.v])
        })
        .collect()
}

pub(super) fn preview_update(
    state: &ProgramState,
    graph: &ProgramGraph,
    changed: &BTreeSet<[usize; 2]>,
    limits: ProgramProofLimits,
    max_events: usize,
) -> Result<(TraceWork, Vec<TraceEvent>, bool), ProofError> {
    let context = PreviewContext {
        state,
        graph,
        changed,
        limits,
        max_events,
    };
    let mut work = TraceWork {
        edges_checked: state.graph.num_edges(),
        ..TraceWork::default()
    };
    let mut events = Vec::new();
    let mut can_reuse = true;
    for atom in &state.atoms {
        if !atom.edges.iter().any(|edge| changed.contains(edge)) {
            continue;
        }
        can_reuse &= preview_atom_update(&context, atom, &mut work, &mut events)?;
    }
    work.h0_edges_scanned = graph
        .edges()
        .iter()
        .filter(|edge| edge.value <= state.threshold.unwrap_or(f64::INFINITY))
        .count();
    Ok((work, events, can_reuse))
}

struct PreviewContext<'a> {
    state: &'a ProgramState,
    graph: &'a ProgramGraph,
    changed: &'a BTreeSet<[usize; 2]>,
    limits: ProgramProofLimits,
    max_events: usize,
}

fn preview_atom_update(
    context: &PreviewContext<'_>,
    atom: &AtomState,
    work: &mut TraceWork,
    events: &mut Vec<TraceEvent>,
) -> Result<bool, ProofError> {
    account_touched_atom(work, atom)?;
    let local = context
        .graph
        .proof_graph()
        .local(&atom.vertices, &atom.edges)?;
    let violations = atom.region.violations(&local);
    if try_reuse_atom(atom, &local, &violations, context.limits, work)? {
        return Ok(true);
    }
    repair_atom_update(context, atom, local, violations, work, events)?;
    Ok(false)
}

fn account_touched_atom(work: &mut TraceWork, atom: &AtomState) -> Result<(), ProofError> {
    work.atoms_touched = work
        .atoms_touched
        .checked_add(1)
        .ok_or_else(|| ProofError::new("touched-atom count overflows usize"))?;
    work.guards_checked = work
        .guards_checked
        .checked_add(atom.region.guards.len())
        .ok_or_else(|| ProofError::new("checked-guard count overflows usize"))?;
    Ok(())
}

fn try_reuse_atom(
    atom: &AtomState,
    local: &crate::proof::Graph,
    violations: &[RegionViolation],
    limits: ProgramProofLimits,
    work: &mut TraceWork,
) -> Result<bool, ProofError> {
    if violations.is_empty() && reweight_atom(atom, local, limits).is_ok() {
        work.atoms_reused = work
            .atoms_reused
            .checked_add(1)
            .ok_or_else(|| ProofError::new("reused-atom count overflows usize"))?;
        return Ok(true);
    }
    Ok(false)
}

fn repair_atom_update(
    context: &PreviewContext<'_>,
    atom: &AtomState,
    local: crate::proof::Graph,
    violations: Vec<RegionViolation>,
    work: &mut TraceWork,
    events: &mut Vec<TraceEvent>,
) -> Result<(), ProofError> {
    for violation in violations {
        push_event(
            events,
            violation_event(atom.id, &violation),
            context.max_events,
        )?;
    }
    let old_local = context
        .state
        .graph
        .proof_graph()
        .local(&atom.vertices, &atom.edges)?;
    let repair = repair_work_for_graphs(&old_local, &local, &atom.reduction, context.limits)?;
    let columns_reused = repair.columns_reused()?;
    let columns_reduced = repair.columns_reduced()?;
    let kind = add_repair_work(
        work,
        columns_reused,
        columns_reduced,
        repair.column_additions,
    )?;
    record_repair_event(
        events,
        work,
        atom,
        context.changed,
        kind,
        context.max_events,
    )
}

fn add_repair_work(
    work: &mut TraceWork,
    columns_reused: usize,
    columns_reduced: usize,
    additions: usize,
) -> Result<TraceEventKind, ProofError> {
    work.reduction_columns_reused = work
        .reduction_columns_reused
        .checked_add(columns_reused)
        .ok_or_else(|| ProofError::new("reused-column count overflows usize"))?;
    work.reduction_columns_reduced = work
        .reduction_columns_reduced
        .checked_add(columns_reduced)
        .ok_or_else(|| ProofError::new("reduced-column count overflows usize"))?;
    work.reduction_column_additions = work
        .reduction_column_additions
        .checked_add(additions)
        .ok_or_else(|| ProofError::new("column addition count overflows usize"))?;
    Ok(repair_event_kind(columns_reused, columns_reduced))
}

fn record_repair_event(
    events: &mut Vec<TraceEvent>,
    work: &mut TraceWork,
    atom: &AtomState,
    changed: &BTreeSet<[usize; 2]>,
    kind: TraceEventKind,
    max_events: usize,
) -> Result<(), ProofError> {
    match kind {
        TraceEventKind::AtomRebuilt => {
            work.atoms_rebuilt = work
                .atoms_rebuilt
                .checked_add(1)
                .ok_or_else(|| ProofError::new("rebuilt-atom count overflows usize"))?;
        }
        TraceEventKind::ReductionSuffixRepaired => {
            work.atoms_repaired = work
                .atoms_repaired
                .checked_add(1)
                .ok_or_else(|| ProofError::new("repaired-atom count overflows usize"))?;
        }
        _ => unreachable!("repair event kind is rebuilt or repaired"),
    }
    push_event(
        events,
        TraceEvent {
            kind,
            atom: Some(atom.id),
            edge: first_changed_edge(atom, changed),
            guard: None,
        },
        max_events,
    )
}

fn repair_event_kind(columns_reused: usize, columns_reduced: usize) -> TraceEventKind {
    // Atlas repair can rebuild class spaces while retaining every reduction column.
    if columns_reused == 0 || columns_reduced == 0 {
        TraceEventKind::AtomRebuilt
    } else {
        TraceEventKind::ReductionSuffixRepaired
    }
}

fn push_event(
    events: &mut Vec<TraceEvent>,
    event: TraceEvent,
    max_events: usize,
) -> Result<(), ProofError> {
    if events.len() == max_events {
        return Err(ProofError::new("trace events exceed the event limit"));
    }
    events.push(event);
    Ok(())
}

fn violation_event(atom: usize, violation: &RegionViolation) -> TraceEvent {
    let (kind, guard) = match violation.kind {
        RegionViolationKind::VertexSetChanged => (TraceEventKind::VertexSetChanged, None),
        RegionViolationKind::EdgeSetChanged => (TraceEventKind::EdgeSetChanged, None),
        RegionViolationKind::ThresholdCrossing => (TraceEventKind::ThresholdCrossing, None),
        RegionViolationKind::GuardFailed => (
            TraceEventKind::GuardFailed,
            violation.guard.map(|guard| match guard {
                RegionGuardKind::ChangeOfBasis => TraceGuardKind::ChangeOfBasis,
                RegionGuardKind::Pivot => TraceGuardKind::Pivot,
            }),
        ),
    };
    TraceEvent {
        kind,
        atom: Some(atom),
        edge: violation.edge,
        guard,
    }
}

fn first_changed_edge(atom: &AtomState, changed: &BTreeSet<[usize; 2]>) -> Option<[usize; 2]> {
    atom.edges
        .iter()
        .copied()
        .find(|edge| changed.contains(edge))
}

#[cfg(test)]
#[path = "trace_topology_tests.rs"]
mod tests;
