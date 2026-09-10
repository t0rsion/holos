use std::collections::BTreeSet;

use crate::proof::{Graph, ProofBar, ProofError};

use super::claim::{CriticalPairClaim, ProgramAtomClaim, ProgramClaim};
use super::graph::ProgramGraph;
use super::model::ProgramProofLimits;
use super::trace_compose::{bar_equal, compose_result, diagram_equal, same_optional_f64};
use super::trace_correspondence::{continuations, correspondences};
use super::trace_model::{
    AtomSpace, AtomState, LocalPair, ProgramState, TraceEvent, TraceMode, TraceStep, TraceWork,
};
use super::trace_reduction::reindex_reduction_for_graphs;
use super::trace_region::build_reuse_region;
use super::trace_topology::{changed_edges, preview_update, separator_edges, topology_events};
use super::trace_validation::validate_space_basis;
use super::trace_wire::{TraceClaim, decode_trace, is_trace};
use super::verify::verify_program;

pub use super::trace_model::{ProgramTraceProofLimits, VerifiedProgramTrace};

/// Return true when bytes start with a `HOLOSDLT` trace envelope.
pub fn is_program_trace(bytes: &[u8]) -> bool {
    is_trace(bytes)
}

/// Verify one self-contained `HOLOSDLT` trace.
///
/// The checker verifies every embedded program graph binding and reduction,
/// replays reuse and retained-prefix counters, and checks the declared class
/// continuation and correspondence algebra. It does not certify a scheduling
/// policy that is absent from the trace.
pub fn verify_program_trace(
    bytes: &[u8],
    limits: ProgramTraceProofLimits,
) -> Result<VerifiedProgramTrace, ProofError> {
    let claim = decode_trace(bytes, &limits.into(), limits.program)?;
    verify_claim(&claim, limits)
}

fn verify_claim(
    claim: &TraceClaim,
    limits: ProgramTraceProofLimits,
) -> Result<VerifiedProgramTrace, ProofError> {
    let mut state = verify_state(&claim.initial_program, &claim.initial_graph, limits.program)?;
    let initial_modulus = state.modulus;
    let mut reused_steps = 0usize;
    let mut repaired_steps = 0usize;
    let mut recompiled_steps = 0usize;
    for (index, step) in claim.steps.iter().enumerate() {
        verify_step(&mut state, step, index, limits)?;
        match step.mode {
            TraceMode::Reused => {
                reused_steps = reused_steps
                    .checked_add(1)
                    .ok_or_else(|| ProofError::new("reused-step count overflows usize"))?
            }
            TraceMode::Repaired => {
                repaired_steps = repaired_steps
                    .checked_add(1)
                    .ok_or_else(|| ProofError::new("repaired-step count overflows usize"))?
            }
            TraceMode::Recompiled => {
                recompiled_steps = recompiled_steps
                    .checked_add(1)
                    .ok_or_else(|| ProofError::new("recompiled-step count overflows usize"))?
            }
        }
    }
    Ok(VerifiedProgramTrace {
        modulus: initial_modulus,
        vertices: claim.initial_graph.vertex_count(),
        edges: claim.initial_graph.num_edges(),
        steps: claim.steps.len(),
        reused_steps,
        repaired_steps,
        recompiled_steps,
        bars: state.result.diagram.len(),
    })
}

fn verify_state(
    claim: &ProgramClaim,
    graph: &ProgramGraph,
    limits: ProgramProofLimits,
) -> Result<ProgramState, ProofError> {
    verify_program(claim, graph, limits)?;
    let atoms = claim
        .atoms
        .iter()
        .map(|atom| atom_state(atom, graph, limits))
        .collect::<Result<Vec<_>, _>>()?;
    let result = compose_result(
        graph,
        claim.modulus,
        claim.threshold,
        &atoms,
        limits.max_bars,
    )?;
    if !diagram_equal(&result.diagram, &claim.diagram) {
        return Err(ProofError::new(
            "program result composition differs from its diagram",
        ));
    }
    Ok(ProgramState {
        graph: graph.clone(),
        modulus: claim.modulus,
        threshold: claim.threshold,
        separator_edges: separator_edges(graph, claim.threshold)?,
        atoms,
        result,
    })
}

fn atom_state(
    atom: &ProgramAtomClaim,
    graph: &ProgramGraph,
    limits: ProgramProofLimits,
) -> Result<AtomState, ProofError> {
    let local = graph.proof_graph().local(&atom.vertices, &atom.edges)?;
    let region = build_reuse_region(&local, &atom.atlas.reduction, limits)?;
    let spaces = atom
        .atlas
        .spaces
        .iter()
        .map(|space| AtomSpace {
            interval: space.interval,
            critical_pairs: space.critical_pairs.iter().map(local_pair).collect(),
            basis: space
                .basis
                .iter()
                .map(|class| class.terms.clone())
                .collect(),
        })
        .collect();
    Ok(AtomState {
        id: atom.id,
        vertices: atom.vertices.clone(),
        edges: atom.edges.clone(),
        graph: local,
        reduction: atom.atlas.reduction.clone(),
        spaces,
        region,
    })
}

fn local_pair(pair: &CriticalPairClaim) -> LocalPair {
    LocalPair {
        birth: pair.birth.vertices.clone(),
        death: pair.death.as_ref().map(|simplex| simplex.vertices.clone()),
    }
}

fn verify_step(
    state: &mut ProgramState,
    step: &TraceStep,
    index: usize,
    limits: ProgramTraceProofLimits,
) -> Result<(), ProofError> {
    if step.graph.vertex_count() > limits.max_vertices {
        return Err(ProofError::new(format!(
            "trace step {index} exceeds the vertex limit"
        )));
    }
    let topology_events = topology_events(state, &step.graph, limits.max_events)?;
    let checkpoint = checkpoint_state(state, step, index, limits)?;
    let replay = replay_step(
        state,
        step,
        index,
        limits,
        topology_events,
        checkpoint.as_ref(),
    )?;
    check_replay(state, step, index, &limits, &replay)?;
    *state = replay.next;
    Ok(())
}

struct StepReplay {
    mode: TraceMode,
    work: TraceWork,
    events: Vec<TraceEvent>,
    next: ProgramState,
}

fn checkpoint_state(
    state: &ProgramState,
    step: &TraceStep,
    index: usize,
    limits: ProgramTraceProofLimits,
) -> Result<Option<ProgramState>, ProofError> {
    let Some(claim) = step.checkpoint.as_ref() else {
        return Ok(None);
    };
    if claim.modulus != state.modulus || !same_optional_f64(claim.threshold, state.threshold) {
        return Err(ProofError::new(format!(
            "trace step {index} checkpoint parameters differ from the initial program"
        )));
    }
    verify_state(claim, &step.graph, limits.program).map(Some)
}

fn replay_step(
    state: &ProgramState,
    step: &TraceStep,
    index: usize,
    limits: ProgramTraceProofLimits,
    topology_events: Vec<TraceEvent>,
    checkpoint: Option<&ProgramState>,
) -> Result<StepReplay, ProofError> {
    if topology_events.is_empty() {
        let changed = changed_edges(&state.graph, &step.graph);
        replay_weight_update(state, step, index, limits, changed, checkpoint)
    } else {
        replay_topology_update(state, index, topology_events, checkpoint)
    }
}

fn replay_topology_update(
    state: &ProgramState,
    index: usize,
    events: Vec<TraceEvent>,
    checkpoint: Option<&ProgramState>,
) -> Result<StepReplay, ProofError> {
    let replacement = checkpoint.ok_or_else(|| {
        ProofError::new(format!(
            "trace step {index} topology change has no checkpoint"
        ))
    })?;
    let work = recompile_work(state, replacement)?;
    Ok(StepReplay {
        mode: TraceMode::Recompiled,
        work,
        events,
        next: replacement.clone(),
    })
}

fn replay_weight_update(
    state: &ProgramState,
    step: &TraceStep,
    index: usize,
    limits: ProgramTraceProofLimits,
    changed: BTreeSet<[usize; 2]>,
    checkpoint: Option<&ProgramState>,
) -> Result<StepReplay, ProofError> {
    let (work, events, can_reuse) = preview_update(
        state,
        &step.graph,
        &changed,
        limits.program,
        limits.max_events,
    )?;
    if can_reuse {
        if checkpoint.is_some() {
            return Err(ProofError::new(format!(
                "trace step {index} reused update has a checkpoint"
            )));
        }
        let next = reweight_state(state, &step.graph, &changed, limits.program)?;
        Ok(StepReplay {
            mode: TraceMode::Reused,
            work,
            events,
            next,
        })
    } else {
        let replacement = checkpoint.ok_or_else(|| {
            ProofError::new(format!("trace step {index} repair has no checkpoint"))
        })?;
        Ok(StepReplay {
            mode: TraceMode::Repaired,
            work,
            events,
            next: replacement.clone(),
        })
    }
}

fn check_replay(
    state: &ProgramState,
    step: &TraceStep,
    index: usize,
    limits: &ProgramTraceProofLimits,
    replay: &StepReplay,
) -> Result<(), ProofError> {
    check_step_accounting(step, index, replay)?;
    let continuation = continuations(
        &state.result.spaces,
        &replay.next.result.spaces,
        limits.max_continuations,
        limits.max_transports,
    )?;
    if step.continuation != continuation {
        return Err(ProofError::new(format!(
            "trace step {index} class continuation differs from independent replay"
        )));
    }
    let correspondence = correspondences(
        &state.graph,
        &state.result.spaces,
        &step.graph,
        &replay.next.result.spaces,
        state.modulus,
        limits,
    )?;
    if step.correspondence != correspondence {
        return Err(ProofError::new(format!(
            "trace step {index} class correspondence differs from independent replay"
        )));
    }
    if !diagram_equal(&step.diagram, &replay.next.result.diagram) {
        return Err(ProofError::new(format!(
            "trace step {index} diagram differs from independent replay"
        )));
    }
    Ok(())
}

fn check_step_accounting(
    step: &TraceStep,
    index: usize,
    replay: &StepReplay,
) -> Result<(), ProofError> {
    if step.mode != replay.mode {
        return Err(ProofError::new(format!(
            "trace step {index} mode differs from independent replay"
        )));
    }
    if step.work != replay.work {
        return Err(ProofError::new(format!(
            "trace step {index} work differs from independent replay"
        )));
    }
    if step.events != replay.events {
        return Err(ProofError::new(format!(
            "trace step {index} events differ from independent replay"
        )));
    }
    Ok(())
}

fn recompile_work(
    state: &ProgramState,
    replacement: &ProgramState,
) -> Result<TraceWork, ProofError> {
    let reduced = replacement.atoms.iter().try_fold(0usize, |total, atom| {
        total
            .checked_add(atom.reduction.edge_columns.len())
            .and_then(|total| total.checked_add(atom.reduction.triangle_columns.len()))
            .ok_or_else(|| ProofError::new("reduced-column count overflows usize"))
    })?;
    Ok(TraceWork {
        edges_checked: state.graph.num_edges().max(replacement.graph.num_edges()),
        h0_edges_scanned: replacement
            .graph
            .edges()
            .iter()
            .filter(|edge| edge.value <= replacement.threshold.unwrap_or(f64::INFINITY))
            .count(),
        atoms_touched: replacement.atoms.len(),
        atoms_rebuilt: replacement.atoms.len(),
        reduction_columns_reduced: reduced,
        ..TraceWork::default()
    })
}

fn reweight_state(
    state: &ProgramState,
    graph: &ProgramGraph,
    changed: &BTreeSet<[usize; 2]>,
    limits: ProgramProofLimits,
) -> Result<ProgramState, ProofError> {
    let mut atoms = Vec::with_capacity(state.atoms.len());
    for atom in &state.atoms {
        let local = graph.proof_graph().local(&atom.vertices, &atom.edges)?;
        if atom.edges.iter().any(|edge| changed.contains(edge)) {
            atoms.push(reweight_atom(atom, &local, limits)?);
        } else {
            atoms.push(atom.clone());
        }
    }
    let result = compose_result(
        graph,
        state.modulus,
        state.threshold,
        &atoms,
        limits.max_bars,
    )?;
    Ok(ProgramState {
        graph: graph.clone(),
        modulus: state.modulus,
        threshold: state.threshold,
        separator_edges: separator_edges(graph, state.threshold)?,
        atoms,
        result,
    })
}

pub(super) fn reweight_atom(
    atom: &AtomState,
    updated: &Graph,
    limits: ProgramProofLimits,
) -> Result<AtomState, ProofError> {
    let violations = atom.region.violations(updated);
    if !violations.is_empty() {
        return Err(ProofError::new("reused atom has a failed region guard"));
    }
    let pairs = atom.region.evaluate_h1(updated);
    let expected_pairs = atom
        .spaces
        .iter()
        .try_fold(0usize, |total, space| {
            total.checked_add(space.critical_pairs.len())
        })
        .ok_or_else(|| ProofError::new("atom critical-pair count overflows usize"))?;
    if pairs.len() != expected_pairs {
        return Err(ProofError::new(
            "reused atom changed its critical-pair count",
        ));
    }
    let mut spaces = Vec::with_capacity(atom.spaces.len());
    for space in &atom.spaces {
        let mut updated_pairs = Vec::with_capacity(space.critical_pairs.len());
        let mut interval = None;
        for pair in &space.critical_pairs {
            let Some(found) = pairs.iter().find(|candidate| {
                candidate.birth.as_slice() == pair.birth.as_slice()
                    && candidate.death.map(|death| death.to_vec()).as_ref() == pair.death.as_ref()
            }) else {
                return Err(ProofError::new("reused atom lost a critical pair"));
            };
            if interval.is_some_and(|value: ProofBar| !bar_equal(value, found.interval)) {
                return Err(ProofError::new(
                    "reused atom split one class-space interval",
                ));
            }
            interval = Some(found.interval);
            updated_pairs.push(LocalPair {
                birth: found.birth.to_vec(),
                death: found.death.map(|death| death.to_vec()),
            });
        }
        let Some(interval) = interval else {
            return Err(ProofError::new("reused atom has an empty class space"));
        };
        validate_space_basis(updated, atom.reduction.modulus, interval, &space.basis)?;
        spaces.push(AtomSpace {
            interval,
            critical_pairs: updated_pairs,
            basis: space.basis.clone(),
        });
    }
    let reduction = reindex_reduction_for_graphs(&atom.graph, updated, &atom.reduction, limits)?;
    let region = build_reuse_region(updated, &reduction, limits)?;
    Ok(AtomState {
        id: atom.id,
        vertices: atom.vertices.clone(),
        edges: atom.edges.clone(),
        graph: updated.clone(),
        reduction,
        spaces,
        region,
    })
}
