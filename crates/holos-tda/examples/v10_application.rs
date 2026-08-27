//! Deterministic application record for compositional persistence programs.

#![forbid(unsafe_code)]

use std::process::ExitCode;

use holos_tda::{
    CertificateLimits, ContinuationKind, InterventionBudget, ProgramArtifact, ProgramTraceArtifact,
    ProgramUpdateMode, RipsParams, SparseDistanceMatrix,
};

fn graph(second_death: f64) -> Result<SparseDistanceMatrix, String> {
    SparseDistanceMatrix::from_triplets(
        7,
        &[
            (0, 1, 1.0),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (0, 3, 1.0),
            (0, 2, 3.0),
            (1, 3, 3.0),
            (3, 4, 1.0),
            (4, 5, 1.0),
            (5, 6, 1.0),
            (3, 6, 1.0),
            (3, 5, second_death),
            (4, 6, second_death),
        ],
    )
    .map_err(|error| error.to_string())
}

fn run() -> Result<(), String> {
    let initial = graph(3.0)?;
    let split = graph(3.5)?;
    let params = RipsParams::new(1).with_modulus(3);
    let limits = CertificateLimits::default();
    let (program_artifact, mut program) =
        ProgramArtifact::compile(&initial, &params, limits).map_err(|error| error.to_string())?;
    let initial_spaces = program.result().spaces.len();
    let initial_classes = program.result().class_count();
    if initial_spaces != 1 || initial_classes != 2 {
        return Err("initial equal interval did not form one rank-two space".into());
    }
    let update = program.advance(&split).map_err(|error| error.to_string())?;
    let split_records = update
        .continuation
        .iter()
        .filter(|record| record.kind == ContinuationKind::Split)
        .count();
    let transports: usize = update
        .continuation
        .iter()
        .map(|record| record.transport.len())
        .sum();
    if update.mode != ProgramUpdateMode::Reused
        || update.result.spaces.len() != 2
        || split_records != 1
        || transports != 2
    {
        return Err("equal interval did not split through exact continuation".into());
    }

    let trace =
        ProgramTraceArtifact::build(&initial, std::slice::from_ref(&split), &params, limits)
            .map_err(|error| error.to_string())?;
    let checked_trace = trace.verify(limits).map_err(|error| error.to_string())?;
    let target = program
        .result()
        .spaces
        .iter()
        .max_by(|left, right| left.interval.death.total_cmp(&right.interval.death))
        .ok_or_else(|| "split result has no finite H1 space".to_string())?
        .id;
    let intervention = program
        .kill_h1_before(target, 2.5, InterventionBudget::default())
        .map_err(|error| error.to_string())?;
    let intervention_artifact = intervention
        .artifact
        .as_ref()
        .ok_or_else(|| "intervention returned no feasible artifact".to_string())?;
    intervention_artifact
        .verify(limits)
        .map_err(|error| error.to_string())?;

    let program_bytes = program_artifact
        .encode()
        .map_err(|error| error.to_string())?;
    let trace_bytes = trace.encode().map_err(|error| error.to_string())?;
    let intervention_bytes = intervention_artifact
        .encode()
        .map_err(|error| error.to_string())?;
    println!(
        "format=holos-program-application-v1 modulus=3 atoms={} initial_spaces={} initial_classes={} update_mode={} final_spaces={} split_records={} transports={} trace_steps={} trace_reused={} intervention_status={} intervention_edits={} program_bytes={} trace_bytes={} intervention_bytes={}",
        program.summary().cyclic_atoms,
        initial_spaces,
        initial_classes,
        match update.mode {
            ProgramUpdateMode::Reused => "reused",
            ProgramUpdateMode::Repaired => "repaired",
            ProgramUpdateMode::Recompiled => "recompiled",
        },
        update.result.spaces.len(),
        split_records,
        transports,
        checked_trace.steps.len(),
        checked_trace
            .steps
            .iter()
            .filter(|step| step.mode == ProgramUpdateMode::Reused)
            .count(),
        format!("{:?}", intervention.status).to_ascii_lowercase(),
        intervention.edits.len(),
        program_bytes.len(),
        trace_bytes.len(),
        intervention_bytes.len(),
    );
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("v10-application: {error}");
            ExitCode::FAILURE
        }
    }
}
