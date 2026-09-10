//! Timed study arms and the machine-readable record.

use std::hint::black_box;
use std::path::Path;
use std::time::{Duration, Instant};

use holos_tda::{
    CertificateLimits, CorrespondenceMode, Diagram, PersistenceAtlas, PersistenceProgram,
    ProgramArtifact, ProgramTraceArtifact, RipsParams, SparseDistanceMatrix,
    rips_persistence_sparse, rips_persistence_with_classes_sparse,
};

use crate::args::Options;
use crate::graph::{RepairTrajectory, accepted_trajectory, graph, repair_trajectory};

struct TimingSamples {
    evaluate_times: Vec<u128>,
    diagram_times: Vec<u128>,
    update_omit_times: Vec<u128>,
    update_exact_times: Vec<u128>,
    rich_times: Vec<u128>,
}

struct Artifacts {
    program: ProgramArtifact,
    trace: ProgramTraceArtifact,
    program_bytes: Vec<u8>,
    trace_bytes: Vec<u8>,
}

struct PreparedStudy {
    input: SparseDistanceMatrix,
    params: RipsParams,
    accepted: Vec<SparseDistanceMatrix>,
    repair: RepairTrajectory,
    atlas: PersistenceAtlas,
}

pub(crate) fn run(options: Options) -> Result<(), String> {
    let prepared = prepare(&options)?;

    let (program, compile_times) =
        compile_program(&prepared.input, &prepared.params, options.reps)?;
    let summary = program.summary();
    let strict_acceptances = strict_acceptance_count(&prepared.atlas, &prepared.accepted);
    if strict_acceptances == 0 {
        return Err("no program acceptance crossed the complete-order atlas".into());
    }

    let repair_expected = warm_up(
        &program,
        &prepared.params,
        &prepared.accepted,
        &prepared.repair.graphs,
    )?;
    let timing = measure_timing(
        &program,
        &prepared.params,
        &prepared.accepted,
        &prepared.repair.graphs,
        &repair_expected,
        options.reps,
    )?;
    let artifacts = build_artifacts(
        &program,
        &prepared.input,
        &prepared.accepted,
        &prepared.params,
        options.artifact_prefix.as_deref(),
    )?;
    let (program_verify_times, trace_verify_times) =
        verify_artifacts(&artifacts, &prepared.input, options.reps)?;

    let compile_ns = median(&compile_times);
    let evaluate_ns = median(&timing.evaluate_times);
    let diagram_ns = median(&timing.diagram_times);
    let saved = diagram_ns.saturating_sub(evaluate_ns);
    let break_even_steps = if saved == 0 {
        u128::MAX
    } else {
        (compile_ns * options.steps as u128).div_ceil(saved)
    };
    let update_omit_ns = median(&timing.update_omit_times);
    let update_exact_ns = median(&timing.update_exact_times);
    let rich_ns = median(&timing.rich_times);
    let program_verify_ns = median(&program_verify_times);
    let trace_verify_ns = median(&trace_verify_times);
    println!(
        "format=holos-program-bench-v2 atoms={} atom_vertices={} vertices={} edges={} seed={} steps={} reps={} modulus={} articulation_vertices={} zero_simplex_separators={} widest_separator={} complete_guards={} guards={} strict_acceptances={} strict_fraction={:.6} repaired_atoms={} rebuilt_atoms={} compile_ns={} evaluate_ns={} diagram_ns={} evaluate_speedup={:.6} break_even_steps={} update_omit_ns={} update_exact_ns={} rich_ns={} omit_speedup={:.6} exact_speedup={:.6} program_bytes={} trace_bytes={} program_verify_ns={} trace_verify_ns={} compile_samples_ns={} evaluate_samples_ns={} diagram_samples_ns={} update_omit_samples_ns={} update_exact_samples_ns={} rich_samples_ns={} program_verify_samples_ns={} trace_verify_samples_ns={}",
        summary.cyclic_atoms,
        options.atom_vertices,
        prepared.input.len(),
        prepared.input.num_edges(),
        options.seed,
        options.steps,
        options.reps,
        options.modulus,
        summary.articulation_vertices,
        summary.zero_simplex_separators,
        summary.widest_separator,
        summary.complete_guards,
        summary.guards,
        strict_acceptances,
        ratio(strict_acceptances as u128, options.steps as u128),
        prepared.repair.repaired_atoms,
        prepared.repair.rebuilt_atoms,
        compile_ns,
        evaluate_ns,
        diagram_ns,
        ratio(diagram_ns, evaluate_ns),
        break_even_steps,
        update_omit_ns,
        update_exact_ns,
        rich_ns,
        ratio(rich_ns, update_omit_ns),
        ratio(rich_ns, update_exact_ns),
        artifacts.program_bytes.len(),
        artifacts.trace_bytes.len(),
        program_verify_ns,
        trace_verify_ns,
        samples(&compile_times),
        samples(&timing.evaluate_times),
        samples(&timing.diagram_times),
        samples(&timing.update_omit_times),
        samples(&timing.update_exact_times),
        samples(&timing.rich_times),
        samples(&program_verify_times),
        samples(&trace_verify_times),
    );
    Ok(())
}

fn prepare(options: &Options) -> Result<PreparedStudy, String> {
    let input = graph(options)?;
    let params = RipsParams::new(1).with_modulus(options.modulus);
    let accepted = accepted_trajectory(&input, options)?;
    let repair = repair_trajectory(&input, &params, options.steps)?;
    let atlas = PersistenceAtlas::build(&input, &params).map_err(|error| error.to_string())?;
    Ok(PreparedStudy {
        input,
        params,
        accepted,
        repair,
        atlas,
    })
}

fn compile_program(
    input: &SparseDistanceMatrix,
    params: &RipsParams,
    repetitions: usize,
) -> Result<(PersistenceProgram, Vec<u128>), String> {
    let mut compile_times = Vec::with_capacity(repetitions);
    let mut compiled = None;
    for _ in 0..repetitions {
        let (elapsed, program) =
            timed(|| PersistenceProgram::compile(input, params, CertificateLimits::default()));
        compiled = Some(program.map_err(|error| error.to_string())?);
        compile_times.push(elapsed.as_nanos());
    }
    Ok((compiled.expect("repetitions are positive"), compile_times))
}

fn strict_acceptance_count(atlas: &PersistenceAtlas, accepted: &[SparseDistanceMatrix]) -> usize {
    accepted
        .iter()
        .filter(|updated| !atlas.events(updated).is_empty())
        .count()
}

fn warm_up(
    program: &PersistenceProgram,
    params: &RipsParams,
    accepted: &[SparseDistanceMatrix],
    repair: &[SparseDistanceMatrix],
) -> Result<Vec<Diagram>, String> {
    let accepted_expected = recompute_diagrams(params, accepted)?;
    let accepted_warm = evaluate(program, accepted)?;
    require_equal(
        "accepted-evaluation warm-up",
        &accepted_warm,
        &accepted_expected,
    )?;
    let repair_expected = recompute_rich(params, repair)?;
    let omit_warm = update(program, repair, CorrespondenceMode::Omit)?;
    require_equal(
        "correspondence-omitted warm-up",
        &omit_warm,
        &repair_expected,
    )?;
    let exact_warm = update(program, repair, CorrespondenceMode::Exact)?;
    require_equal(
        "exact-correspondence warm-up",
        &exact_warm,
        &repair_expected,
    )?;
    Ok(repair_expected)
}

fn measure_timing(
    program: &PersistenceProgram,
    params: &RipsParams,
    accepted: &[SparseDistanceMatrix],
    repair: &[SparseDistanceMatrix],
    repair_expected: &[Diagram],
    repetitions: usize,
) -> Result<TimingSamples, String> {
    let mut evaluate_times = Vec::with_capacity(repetitions);
    let mut diagram_times = Vec::with_capacity(repetitions);
    let mut update_omit_times = Vec::with_capacity(repetitions);
    let mut update_exact_times = Vec::with_capacity(repetitions);
    let mut rich_times = Vec::with_capacity(repetitions);
    for repetition in 0..repetitions {
        time_pair(
            repetition,
            "accepted evaluation",
            || evaluate(program, accepted),
            || recompute_diagrams(params, accepted),
            &mut evaluate_times,
            &mut diagram_times,
        )?;
        time_pair(
            repetition,
            "correspondence-omitted update",
            || update(program, repair, CorrespondenceMode::Omit),
            || recompute_rich(params, repair),
            &mut update_omit_times,
            &mut rich_times,
        )?;
        let (elapsed, exact) = timed(|| update(program, repair, CorrespondenceMode::Exact));
        require_equal("exact-correspondence update", &exact?, repair_expected)?;
        update_exact_times.push(elapsed.as_nanos());
    }
    Ok(TimingSamples {
        evaluate_times,
        diagram_times,
        update_omit_times,
        update_exact_times,
        rich_times,
    })
}

fn build_artifacts(
    program: &PersistenceProgram,
    input: &SparseDistanceMatrix,
    accepted: &[SparseDistanceMatrix],
    params: &RipsParams,
    artifact_prefix: Option<&Path>,
) -> Result<Artifacts, String> {
    let artifact = ProgramArtifact::from_program(program).map_err(|error| error.to_string())?;
    let program_bytes = artifact.encode().map_err(|error| error.to_string())?;
    let trace = ProgramTraceArtifact::build(input, accepted, params, CertificateLimits::default())
        .map_err(|error| error.to_string())?;
    let trace_bytes = trace.encode().map_err(|error| error.to_string())?;
    if let Some(prefix) = artifact_prefix {
        crate::artifact_output::write(prefix, input, &program_bytes, &trace_bytes)?;
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
    input: &SparseDistanceMatrix,
    repetitions: usize,
) -> Result<(Vec<u128>, Vec<u128>), String> {
    let mut program_verify_times = Vec::with_capacity(repetitions);
    let mut trace_verify_times = Vec::with_capacity(repetitions);
    for _ in 0..repetitions {
        let (elapsed, checked) = timed(|| {
            artifacts
                .program
                .verify(input, CertificateLimits::default())
        });
        checked.map_err(|error| error.to_string())?;
        program_verify_times.push(elapsed.as_nanos());
        let (elapsed, checked) = timed(|| artifacts.trace.verify(CertificateLimits::default()));
        checked.map_err(|error| error.to_string())?;
        trace_verify_times.push(elapsed.as_nanos());
    }
    Ok((program_verify_times, trace_verify_times))
}

fn evaluate(
    program: &PersistenceProgram,
    updates: &[SparseDistanceMatrix],
) -> Result<Vec<Diagram>, String> {
    updates
        .iter()
        .map(|input| {
            program
                .evaluate_diagram(input)
                .map(|evaluation| evaluation.diagram)
                .map_err(|error| error.to_string())
        })
        .collect()
}

fn update(
    initial: &PersistenceProgram,
    updates: &[SparseDistanceMatrix],
    correspondence: CorrespondenceMode,
) -> Result<Vec<Diagram>, String> {
    let mut program = initial.clone();
    updates
        .iter()
        .map(|input| {
            program
                .advance_with(input, correspondence)
                .map(|result| result.result.diagram)
                .map_err(|error| error.to_string())
        })
        .collect()
}

fn recompute_diagrams(
    params: &RipsParams,
    updates: &[SparseDistanceMatrix],
) -> Result<Vec<Diagram>, String> {
    updates
        .iter()
        .map(|input| rips_persistence_sparse(input, params).map_err(|error| error.to_string()))
        .collect()
}

fn recompute_rich(
    params: &RipsParams,
    updates: &[SparseDistanceMatrix],
) -> Result<Vec<Diagram>, String> {
    updates
        .iter()
        .map(|input| {
            rips_persistence_with_classes_sparse(input, params)
                .map(|result| result.diagram)
                .map_err(|error| error.to_string())
        })
        .collect()
}

fn time_pair(
    repetition: usize,
    label: &str,
    mut left: impl FnMut() -> Result<Vec<Diagram>, String>,
    mut right: impl FnMut() -> Result<Vec<Diagram>, String>,
    left_times: &mut Vec<u128>,
    right_times: &mut Vec<u128>,
) -> Result<(), String> {
    let left_first = repetition & 1 == 0;
    let (first_time, first) = if left_first {
        timed(&mut left)
    } else {
        timed(&mut right)
    };
    let (second_time, second) = if left_first {
        timed(&mut right)
    } else {
        timed(&mut left)
    };
    let (left_result, right_result) = if left_first {
        left_times.push(first_time.as_nanos());
        right_times.push(second_time.as_nanos());
        (first?, second?)
    } else {
        right_times.push(first_time.as_nanos());
        left_times.push(second_time.as_nanos());
        (second?, first?)
    };
    require_equal(label, &left_result, &right_result)?;
    black_box((left_result, right_result));
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

fn median(values: &[u128]) -> u128 {
    let mut ordered = values.to_vec();
    ordered.sort_unstable();
    ordered[ordered.len() / 2]
}

fn ratio(numerator: u128, denominator: u128) -> f64 {
    numerator as f64 / denominator as f64
}

fn samples(values: &[u128]) -> String {
    values
        .iter()
        .map(u128::to_string)
        .collect::<Vec<_>>()
        .join(",")
}
