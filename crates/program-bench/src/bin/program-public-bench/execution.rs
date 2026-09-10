//! Study arms, exactness checks, and measured counters.

use std::hint::black_box;
use std::time::{Duration, Instant};

use holos_tda::{
    CertificateLimits, CorrespondenceMode, Diagram, PersistenceAtlas, PersistenceProgram,
    ProgramArtifact, ProgramEventKind, ProgramTraceArtifact, ProgramUpdateMode, RipsParams,
    SparseDistanceMatrix, rips_persistence_sparse,
};

use super::args::Options;
use super::data::{Metadata, Trajectory};

pub(crate) struct StudyResult {
    pub(crate) metadata: Metadata,
    pub(crate) modulus: u32,
    pub(crate) transitions: usize,
    pub(crate) initial_atoms: usize,
    pub(crate) initial_cyclic_atoms: usize,
    pub(crate) initial_complete_guards: usize,
    pub(crate) initial_guards: usize,
    pub(crate) final_complete_guards: usize,
    pub(crate) final_guards: usize,
    pub(crate) widest_separator: usize,
    pub(crate) order_change_steps: usize,
    pub(crate) accepted_steps: usize,
    pub(crate) strict_acceptances: usize,
    pub(crate) reused_steps: usize,
    pub(crate) repaired_steps: usize,
    pub(crate) recompiled_steps: usize,
    pub(crate) guard_failure_events: usize,
    pub(crate) atoms_touched: usize,
    pub(crate) atoms_reused: usize,
    pub(crate) atoms_repaired: usize,
    pub(crate) atoms_rebuilt: usize,
    pub(crate) reduction_columns_reused: usize,
    pub(crate) reduction_columns_reduced: usize,
    pub(crate) reduction_column_additions: usize,
    pub(crate) compile_times: Vec<u128>,
    pub(crate) update_times: Vec<u128>,
    pub(crate) diagram_times: Vec<u128>,
    pub(crate) program_bytes: usize,
    pub(crate) trace_bytes: usize,
    pub(crate) program_verify_times: Vec<u128>,
    pub(crate) trace_verify_times: Vec<u128>,
}

#[derive(Default)]
struct TrajectoryCounters {
    order_change_steps: usize,
    accepted_steps: usize,
    strict_acceptances: usize,
    reused_steps: usize,
    repaired_steps: usize,
    recompiled_steps: usize,
    guard_failure_events: usize,
    atoms_touched: usize,
    atoms_reused: usize,
    atoms_repaired: usize,
    atoms_rebuilt: usize,
    reduction_columns_reused: usize,
    reduction_columns_reduced: usize,
    reduction_column_additions: usize,
    final_complete_guards: usize,
    final_guards: usize,
}

struct TimingSamples {
    update_times: Vec<u128>,
    diagram_times: Vec<u128>,
}

struct Artifacts {
    program: ProgramArtifact,
    trace: ProgramTraceArtifact,
    program_bytes: Vec<u8>,
    trace_bytes: Vec<u8>,
}

pub(crate) fn run(options: &Options) -> Result<StudyResult, String> {
    let trajectory = super::data::read(&options.trajectory)?;
    validate_schedule(&trajectory)?;
    let initial = &trajectory.graphs[0];
    let updates = &trajectory.graphs[1..];
    let params = RipsParams::new(1).with_modulus(options.modulus);
    let (program, compile_times) = compile_program(initial, &params, options.reps)?;
    let summary = program.summary();

    let expected = recompute(&params, updates)?;
    let (warm, counters) = advance_and_count(program.clone(), initial, updates, &params)?;
    require_equal("public trajectory warm-up", &warm, &expected)?;

    let timing = measure_timing(&program, updates, &params, &expected, options.reps)?;
    let artifacts = build_artifacts(
        &program,
        initial,
        updates,
        &params,
        options.artifact_prefix.as_deref(),
    )?;
    let (program_verify_times, trace_verify_times) =
        verify_artifacts(&artifacts, initial, options.reps)?;

    Ok(StudyResult {
        metadata: trajectory.metadata,
        modulus: options.modulus,
        transitions: updates.len(),
        initial_atoms: summary.atoms,
        initial_cyclic_atoms: summary.cyclic_atoms,
        initial_complete_guards: summary.complete_guards,
        initial_guards: summary.guards,
        final_complete_guards: counters.final_complete_guards,
        final_guards: counters.final_guards,
        widest_separator: summary.widest_separator,
        order_change_steps: counters.order_change_steps,
        accepted_steps: counters.accepted_steps,
        strict_acceptances: counters.strict_acceptances,
        reused_steps: counters.reused_steps,
        repaired_steps: counters.repaired_steps,
        recompiled_steps: counters.recompiled_steps,
        guard_failure_events: counters.guard_failure_events,
        atoms_touched: counters.atoms_touched,
        atoms_reused: counters.atoms_reused,
        atoms_repaired: counters.atoms_repaired,
        atoms_rebuilt: counters.atoms_rebuilt,
        reduction_columns_reused: counters.reduction_columns_reused,
        reduction_columns_reduced: counters.reduction_columns_reduced,
        reduction_column_additions: counters.reduction_column_additions,
        compile_times,
        update_times: timing.update_times,
        diagram_times: timing.diagram_times,
        program_bytes: artifacts.program_bytes.len(),
        trace_bytes: artifacts.trace_bytes.len(),
        program_verify_times,
        trace_verify_times,
    })
}

fn compile_program(
    initial: &SparseDistanceMatrix,
    params: &RipsParams,
    repetitions: usize,
) -> Result<(PersistenceProgram, Vec<u128>), String> {
    let mut compile_times = Vec::with_capacity(repetitions);
    let mut compiled = None;
    for _ in 0..repetitions {
        let (elapsed, program) =
            timed(|| PersistenceProgram::compile(initial, params, CertificateLimits::default()));
        compile_times.push(elapsed.as_nanos());
        compiled = Some(program.map_err(|error| error.to_string())?);
    }
    Ok((compiled.expect("repetitions are positive"), compile_times))
}

fn measure_timing(
    program: &PersistenceProgram,
    updates: &[SparseDistanceMatrix],
    params: &RipsParams,
    expected: &[Diagram],
    repetitions: usize,
) -> Result<TimingSamples, String> {
    let mut update_times = Vec::with_capacity(repetitions);
    let mut diagram_times = Vec::with_capacity(repetitions);
    for repetition in 0..repetitions {
        time_pair(
            repetition,
            || advance(program.clone(), updates),
            || recompute(params, updates),
            expected,
            &mut update_times,
            &mut diagram_times,
        )?;
    }
    Ok(TimingSamples {
        update_times,
        diagram_times,
    })
}

fn build_artifacts(
    program: &PersistenceProgram,
    initial: &SparseDistanceMatrix,
    updates: &[SparseDistanceMatrix],
    params: &RipsParams,
    artifact_prefix: Option<&std::path::Path>,
) -> Result<Artifacts, String> {
    let artifact = ProgramArtifact::from_program(program).map_err(|error| error.to_string())?;
    let program_bytes = artifact.encode().map_err(|error| error.to_string())?;
    let trace = ProgramTraceArtifact::build(initial, updates, params, CertificateLimits::default())
        .map_err(|error| error.to_string())?;
    let trace_bytes = trace.encode().map_err(|error| error.to_string())?;
    if let Some(prefix) = artifact_prefix {
        super::artifact_output::write(prefix, initial, &program_bytes, &trace_bytes)?;
    }
    Ok(Artifacts {
        program: artifact,
        trace,
        program_bytes,
        trace_bytes,
    })
}

fn verify_artifacts(
    artifacts: &Artifacts,
    initial: &SparseDistanceMatrix,
    repetitions: usize,
) -> Result<(Vec<u128>, Vec<u128>), String> {
    let mut program_verify_times = Vec::with_capacity(repetitions);
    let mut trace_verify_times = Vec::with_capacity(repetitions);
    for _ in 0..repetitions {
        let (elapsed, checked) = timed(|| {
            artifacts
                .program
                .verify(initial, CertificateLimits::default())
        });
        checked.map_err(|error| error.to_string())?;
        program_verify_times.push(elapsed.as_nanos());
        let (elapsed, checked) = timed(|| artifacts.trace.verify(CertificateLimits::default()));
        checked.map_err(|error| error.to_string())?;
        trace_verify_times.push(elapsed.as_nanos());
    }
    Ok((program_verify_times, trace_verify_times))
}

fn validate_schedule(trajectory: &Trajectory) -> Result<(), String> {
    if trajectory.snapshot_times.len() != trajectory.graphs.len() {
        return Err("snapshot time and graph counts differ".into());
    }
    if trajectory.metadata.snapshots != trajectory.graphs.len()
        || trajectory.metadata.edges != trajectory.graphs[0].num_edges()
        || trajectory.metadata.vertices != trajectory.graphs[0].len()
    {
        return Err("trajectory header differs from its decoded graphs".into());
    }
    let topology: Vec<_> = trajectory.graphs[0]
        .edges()
        .map(|(u, v, _)| (u, v))
        .collect();
    if !trajectory.graphs.iter().all(|graph| {
        graph.len() == trajectory.metadata.vertices
            && graph.num_edges() == trajectory.metadata.edges
            && graph
                .edges()
                .map(|(u, v, _)| (u, v))
                .eq(topology.iter().copied())
    }) {
        return Err("trajectory graph topology changes across snapshots".into());
    }
    Ok(())
}

fn advance_and_count(
    mut program: PersistenceProgram,
    initial: &SparseDistanceMatrix,
    updates: &[SparseDistanceMatrix],
    params: &RipsParams,
) -> Result<(Vec<Diagram>, TrajectoryCounters), String> {
    let mut atlas = PersistenceAtlas::build(initial, params).map_err(|error| error.to_string())?;
    let mut diagrams = Vec::with_capacity(updates.len());
    let mut counters = TrajectoryCounters::default();
    for updated in updates {
        let atlas_rejects = !atlas.events(updated).is_empty();
        counters.order_change_steps += usize::from(atlas_rejects);
        let program_accepts = program.evaluate_diagram(updated).is_ok();
        counters.accepted_steps += usize::from(program_accepts);
        counters.strict_acceptances += usize::from(program_accepts && atlas_rejects);
        let update = program
            .advance_with(updated, CorrespondenceMode::Omit)
            .map_err(|error| error.to_string())?;
        match update.mode {
            ProgramUpdateMode::Reused => counters.reused_steps += 1,
            ProgramUpdateMode::Repaired => counters.repaired_steps += 1,
            ProgramUpdateMode::Recompiled => counters.recompiled_steps += 1,
        }
        counters.guard_failure_events += update
            .events
            .iter()
            .filter(|event| event.kind == ProgramEventKind::GuardFailed)
            .count();
        counters.atoms_touched += update.work.atoms_touched;
        counters.atoms_reused += update.work.atoms_reused;
        counters.atoms_repaired += update.work.atoms_repaired;
        counters.atoms_rebuilt += update.work.atoms_rebuilt;
        counters.reduction_columns_reused += update.work.reduction_columns_reused;
        counters.reduction_columns_reduced += update.work.reduction_columns_reduced;
        counters.reduction_column_additions += update.work.reduction_column_additions;
        diagrams.push(update.result.diagram);
        atlas = PersistenceAtlas::build(updated, params).map_err(|error| error.to_string())?;
    }
    counters.final_complete_guards = program.summary().complete_guards;
    counters.final_guards = program.summary().guards;
    Ok((diagrams, counters))
}

fn advance(
    mut program: PersistenceProgram,
    updates: &[SparseDistanceMatrix],
) -> Result<Vec<Diagram>, String> {
    updates
        .iter()
        .map(|updated| {
            program
                .advance_with(updated, CorrespondenceMode::Omit)
                .map(|update| update.result.diagram)
                .map_err(|error| error.to_string())
        })
        .collect()
}

fn recompute(
    params: &RipsParams,
    updates: &[SparseDistanceMatrix],
) -> Result<Vec<Diagram>, String> {
    updates
        .iter()
        .map(|updated| rips_persistence_sparse(updated, params).map_err(|error| error.to_string()))
        .collect()
}

fn time_pair(
    repetition: usize,
    mut update: impl FnMut() -> Result<Vec<Diagram>, String>,
    mut diagram: impl FnMut() -> Result<Vec<Diagram>, String>,
    expected: &[Diagram],
    update_times: &mut Vec<u128>,
    diagram_times: &mut Vec<u128>,
) -> Result<(), String> {
    let update_first = repetition & 1 == 0;
    let (first_time, first) = if update_first {
        timed(&mut update)
    } else {
        timed(&mut diagram)
    };
    let (second_time, second) = if update_first {
        timed(&mut diagram)
    } else {
        timed(&mut update)
    };
    let (updated, recomputed) = if update_first {
        update_times.push(first_time.as_nanos());
        diagram_times.push(second_time.as_nanos());
        (first?, second?)
    } else {
        diagram_times.push(first_time.as_nanos());
        update_times.push(second_time.as_nanos());
        (second?, first?)
    };
    require_equal("timed program update", &updated, expected)?;
    require_equal("timed exact diagram", &recomputed, expected)?;
    black_box((updated, recomputed));
    Ok(())
}

fn require_equal(label: &str, left: &[Diagram], right: &[Diagram]) -> Result<(), String> {
    if left.len() != right.len()
        || left
            .iter()
            .zip(right)
            .any(|(left, right)| !diagram_bits_equal(left, right))
    {
        Err(format!("{label} diagrams differ"))
    } else {
        Ok(())
    }
}

fn diagram_bits_equal(left: &Diagram, right: &Diagram) -> bool {
    left.bars.len() == right.bars.len()
        && left.bars.iter().zip(&right.bars).all(|(left, right)| {
            left.dim == right.dim
                && left.birth.to_bits() == right.birth.to_bits()
                && left.death.to_bits() == right.death.to_bits()
        })
}

fn timed<T>(operation: impl FnOnce() -> T) -> (Duration, T) {
    let start = Instant::now();
    let result = operation();
    (start.elapsed(), result)
}
