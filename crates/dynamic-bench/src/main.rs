//! Registered benchmark for self-adjusting persistence programs.

#![forbid(unsafe_code)]

use std::hint::black_box;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use holos_tda::{
    CertificateLimits, CorrespondenceMode, EdgeKey, PersistenceProgram, ProgramUpdate,
    ProgramUpdateMode, ProofArtifact, RipsParams, SparseDistanceMatrix,
};
use holos_tda_check::{ProofBundle, ProofLimits, VerifiedProof};

const USAGE: &str = "\
Usage: dynamic-bench --atoms A --atom-vertices K --seed S [options]

Generate complete weighted atoms that share one articulation vertex. The
repair trajectory changes one atom per step. Branch alternatives leave from
one shared checked state. The proof trajectory stores all steps in one DAG.

Options:
  --steps K       cumulative repair steps (default 24)
  --branches K    alternatives from one state (default 4)
  --reps K        timed repetitions per arm (default 5, minimum 5)
  --modulus P     coefficient field Z/p (default 2)
  -h, --help      print this text

Generation, update search, initial compilation, and warm-up are outside timed
arms. Both timed arms maintain exact current diagrams and class spaces. They
omit cross-state class correspondence. Every current result must agree.
";

#[derive(Clone, Copy)]
struct Options {
    atoms: usize,
    atom_vertices: usize,
    seed: u64,
    steps: usize,
    branches: usize,
    reps: usize,
    modulus: u32,
}

fn parse() -> Result<Option<Options>, String> {
    let mut atoms = None;
    let mut atom_vertices = None;
    let mut seed = None;
    let mut steps = 24;
    let mut branches = 4;
    let mut reps = 5;
    let mut modulus = 2;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "-h" || argument == "--help" {
            return Ok(None);
        }
        let value = arguments
            .next()
            .ok_or_else(|| format!("{argument} needs a value"))?;
        match argument.as_str() {
            "--atoms" => atoms = Some(parse_value(&argument, &value)?),
            "--atom-vertices" => atom_vertices = Some(parse_value(&argument, &value)?),
            "--seed" => seed = Some(parse_value(&argument, &value)?),
            "--steps" => steps = parse_value(&argument, &value)?,
            "--branches" => branches = parse_value(&argument, &value)?,
            "--reps" => reps = parse_value(&argument, &value)?,
            "--modulus" => modulus = parse_value(&argument, &value)?,
            _ => return Err(format!("unknown option {argument}")),
        }
    }
    let options = Options {
        atoms: atoms.ok_or_else(|| "--atoms is required".to_string())?,
        atom_vertices: atom_vertices.ok_or_else(|| "--atom-vertices is required".to_string())?,
        seed: seed.ok_or_else(|| "--seed is required".to_string())?,
        steps,
        branches,
        reps,
        modulus,
    };
    if options.atoms < 2
        || options.atom_vertices < 4
        || options.steps == 0
        || options.branches < 2
        || options.branches > options.atoms
        || options.reps < 5
    {
        return Err("atoms must be at least 2, atom vertices at least 4, steps positive, branches between 2 and atoms, and reps at least 5".into());
    }
    Ok(Some(options))
}

fn parse_value<T: std::str::FromStr>(label: &str, value: &str) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("invalid value for {label}: {value}"))
}

fn next_random(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn graph(options: Options) -> Result<SparseDistanceMatrix, String> {
    let mut endpoints = Vec::new();
    for atom in 0..options.atoms {
        let first = 1 + atom * (options.atom_vertices - 1);
        let vertices: Vec<_> = std::iter::once(0)
            .chain(first..first + options.atom_vertices - 1)
            .collect();
        for v in 1..vertices.len() {
            for u in 0..v {
                endpoints.push((vertices[u], vertices[v]));
            }
        }
    }
    let mut state = options.seed;
    let mut order: Vec<_> = (0..endpoints.len())
        .map(|index| (next_random(&mut state), index))
        .collect();
    order.sort_unstable();
    let mut rank = vec![0usize; endpoints.len()];
    for (position, &(_, index)) in order.iter().enumerate() {
        rank[index] = position;
    }
    let denominator = endpoints.len() as f64 + 1.0;
    let triplets: Vec<_> = endpoints
        .into_iter()
        .enumerate()
        .map(|(index, (u, v))| (u, v, 1.0 + 8.0 * (rank[index] + 1) as f64 / denominator))
        .collect();
    SparseDistanceMatrix::from_triplets(1 + options.atoms * (options.atom_vertices - 1), &triplets)
        .map_err(|error| error.to_string())
}

fn swap_weights(
    input: &SparseDistanceMatrix,
    first: EdgeKey,
    second: EdgeKey,
) -> Result<SparseDistanceMatrix, String> {
    let first_weight = input.get(first.u, first.v);
    let second_weight = input.get(second.u, second.v);
    let triplets: Vec<_> = input
        .edges()
        .map(|(u, v, value)| {
            let edge = EdgeKey { u, v };
            let value = if edge == first {
                second_weight
            } else if edge == second {
                first_weight
            } else {
                value
            };
            (u, v, value)
        })
        .collect();
    SparseDistanceMatrix::from_triplets(input.len(), &triplets).map_err(|error| error.to_string())
}

fn repair_candidate(
    program: &PersistenceProgram,
    current: &SparseDistanceMatrix,
    atom: usize,
) -> Result<(SparseDistanceMatrix, PersistenceProgram, ProgramUpdate), String> {
    let edges = &program
        .atoms()
        .iter()
        .filter(|atom| atom.cyclic)
        .nth(atom)
        .ok_or_else(|| format!("cyclic atom {atom} does not exist"))?
        .edges;
    let mut weighted: Vec<_> = edges
        .iter()
        .copied()
        .map(|edge| (current.get(edge.u, edge.v), edge))
        .collect();
    weighted.sort_by(|left, right| left.0.total_cmp(&right.0));
    for right in (1..weighted.len()).rev() {
        let candidate = swap_weights(current, weighted[right - 1].1, weighted[right].1)?;
        let mut next = program.clone();
        let update = next
            .advance(&candidate)
            .map_err(|error| error.to_string())?;
        if update.mode == ProgramUpdateMode::Repaired
            && update.work.atoms_touched == 1
            && update.work.atoms_repaired == 1
            && update.work.reduction_columns_reused > 0
            && update.work.reduction_columns_reduced > 0
        {
            return Ok((candidate, next, update));
        }
    }
    Err(format!(
        "no dependency-frontier repair exists for cyclic atom {atom}"
    ))
}

fn repair_trajectory(
    initial: &SparseDistanceMatrix,
    program: &PersistenceProgram,
    steps: usize,
) -> Result<(Vec<SparseDistanceMatrix>, usize, usize), String> {
    let atom_count = program.summary().cyclic_atoms;
    let mut current = initial.clone();
    let mut state = program.clone();
    let mut updates = Vec::with_capacity(steps);
    let mut reused = 0usize;
    let mut reduced = 0usize;
    for step in 0..steps {
        let (candidate, next, update) = repair_candidate(&state, &current, step % atom_count)?;
        reused += update.work.reduction_columns_reused;
        reduced += update.work.reduction_columns_reduced;
        updates.push(candidate.clone());
        current = candidate;
        state = next;
    }
    Ok((updates, reused, reduced))
}

fn branch_alternatives(
    initial: &SparseDistanceMatrix,
    program: &PersistenceProgram,
    count: usize,
) -> Result<Vec<SparseDistanceMatrix>, String> {
    (0..count)
        .map(|atom| repair_candidate(program, initial, atom).map(|value| value.0))
        .collect()
}

fn timed<T>(operation: impl FnOnce() -> T) -> (Duration, T) {
    let start = Instant::now();
    let value = operation();
    (start.elapsed(), value)
}

fn advance_all(
    initial: &PersistenceProgram,
    updates: &[SparseDistanceMatrix],
) -> Result<Vec<PersistenceProgram>, String> {
    let mut program = initial.clone();
    let mut programs = Vec::with_capacity(updates.len());
    for graph in updates {
        let update = program
            .advance_with(graph, CorrespondenceMode::Omit)
            .map_err(|error| error.to_string())?;
        if update.work.atoms_repaired != 1 {
            return Err("timed update did not repair one dependency frontier".into());
        }
        programs.push(program.clone());
    }
    Ok(programs)
}

fn compile_all(
    params: &RipsParams,
    updates: &[SparseDistanceMatrix],
) -> Result<Vec<PersistenceProgram>, String> {
    updates
        .iter()
        .map(|graph| {
            PersistenceProgram::compile(graph, params, CertificateLimits::default())
                .map_err(|error| error.to_string())
        })
        .collect()
}

fn same_result(left: &PersistenceProgram, right: &PersistenceProgram) -> bool {
    let left = left.result();
    let right = right.result();
    left.diagram.bars.len() == right.diagram.bars.len()
        && left
            .diagram
            .bars
            .iter()
            .zip(&right.diagram.bars)
            .all(|(left, right)| {
                left.dim == right.dim
                    && left.birth.to_bits() == right.birth.to_bits()
                    && left.death.to_bits() == right.death.to_bits()
            })
        && left.spaces == right.spaces
}

fn same_programs(left: &[PersistenceProgram], right: &[PersistenceProgram]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| same_result(left, right))
}

fn median(mut values: Vec<u128>) -> u128 {
    values.sort_unstable();
    values[values.len() / 2]
}

fn verify_proof(bytes: &[u8]) -> Result<VerifiedProof, String> {
    ProofBundle::decode(bytes, ProofLimits::default())
        .and_then(|proof| proof.verify())
        .map_err(|error| error.to_string())
}

fn run(options: Options) -> Result<(), String> {
    let input = graph(options)?;
    let serial_params = RipsParams::new(1).with_modulus(options.modulus);
    let mut parallel_params = serial_params.clone();
    parallel_params.threads = options.branches;
    let initial = PersistenceProgram::compile(&input, &serial_params, CertificateLimits::default())
        .map_err(|error| error.to_string())?;
    let parallel =
        PersistenceProgram::compile(&input, &parallel_params, CertificateLimits::default())
            .map_err(|error| error.to_string())?;
    let (updates, columns_reused, columns_reduced) =
        repair_trajectory(&input, &initial, options.steps)?;
    let alternatives = branch_alternatives(&input, &initial, options.branches)?;

    let warm_repair = advance_all(&initial, &updates)?;
    let warm_compile = compile_all(&serial_params, &updates)?;
    if !same_programs(&warm_repair, &warm_compile) {
        return Err("repair and clean compilation differ".into());
    }
    let warm_serial = initial
        .branch(&alternatives)
        .map_err(|error| error.to_string())?;
    let warm_parallel = parallel
        .branch(&alternatives)
        .map_err(|error| error.to_string())?;
    if !warm_serial
        .iter()
        .zip(&warm_parallel)
        .all(|(left, right)| same_result(left.program(), right.program()))
    {
        return Err("serial and parallel branches differ".into());
    }

    let proof = ProofArtifact::build(
        &input,
        &updates,
        &serial_params,
        CertificateLimits::default(),
    )
    .map_err(|error| error.to_string())?;
    let proof_summary = proof.summary();
    let proof_bytes = proof.encode().map_err(|error| error.to_string())?;
    let proof_checked = verify_proof(&proof_bytes)?;

    let mut repair_times = Vec::with_capacity(options.reps);
    let mut compile_times = Vec::with_capacity(options.reps);
    let mut serial_branch_times = Vec::with_capacity(options.reps);
    let mut parallel_branch_times = Vec::with_capacity(options.reps);
    let mut proof_times = Vec::with_capacity(options.reps);
    for repetition in 0..options.reps {
        let dynamic_first = repetition % 2 == 0;
        let (first_time, first) = if dynamic_first {
            timed(|| advance_all(&initial, &updates))
        } else {
            timed(|| compile_all(&serial_params, &updates))
        };
        let (second_time, second) = if dynamic_first {
            timed(|| compile_all(&serial_params, &updates))
        } else {
            timed(|| advance_all(&initial, &updates))
        };
        let (repaired, compiled) = if dynamic_first {
            repair_times.push(first_time.as_nanos());
            compile_times.push(second_time.as_nanos());
            (first?, second?)
        } else {
            compile_times.push(first_time.as_nanos());
            repair_times.push(second_time.as_nanos());
            (second?, first?)
        };
        if !same_programs(&repaired, &compiled) {
            return Err(format!("repetition {repetition} repair results differ"));
        }
        black_box((repaired, compiled));

        let (serial_time, serial) = timed(|| initial.branch(&alternatives));
        let (parallel_time, parallel_results) = timed(|| parallel.branch(&alternatives));
        let serial = serial.map_err(|error| error.to_string())?;
        let parallel_results = parallel_results.map_err(|error| error.to_string())?;
        if !serial
            .iter()
            .zip(&parallel_results)
            .all(|(left, right)| same_result(left.program(), right.program()))
        {
            return Err(format!("repetition {repetition} branch results differ"));
        }
        serial_branch_times.push(serial_time.as_nanos());
        parallel_branch_times.push(parallel_time.as_nanos());
        black_box((serial, parallel_results));

        let (proof_time, checked) = timed(|| verify_proof(&proof_bytes));
        let checked = checked?;
        if checked != proof_checked {
            return Err(format!("repetition {repetition} proof counts differ"));
        }
        proof_times.push(proof_time.as_nanos());
        black_box(checked);
    }

    let repair_ns = median(repair_times);
    let compile_ns = median(compile_times);
    let serial_branch_ns = median(serial_branch_times);
    let parallel_branch_ns = median(parallel_branch_times);
    let proof_verify_ns = median(proof_times);
    println!(
        "format=holos-dynamic-bench-v1 atoms={} atom_vertices={} vertices={} edges={} seed={} steps={} branches={} reps={} modulus={} columns_reused={} columns_reduced={} repair_ns={} clean_compile_ns={} repair_speedup={:.6} branch_serial_ns={} branch_parallel_ns={} branch_speedup={:.6} proof_bytes={} proof_snapshots={} proof_nodes={} proof_references={} proof_reused_references={} proof_cached_references={} proof_edge_columns_checked={} proof_triangle_columns_checked={} proof_verify_ns={}",
        initial.summary().cyclic_atoms,
        options.atom_vertices,
        input.len(),
        input.num_edges(),
        options.seed,
        options.steps,
        options.branches,
        options.reps,
        options.modulus,
        columns_reused,
        columns_reduced,
        repair_ns,
        compile_ns,
        compile_ns as f64 / repair_ns as f64,
        serial_branch_ns,
        parallel_branch_ns,
        serial_branch_ns as f64 / parallel_branch_ns as f64,
        proof_bytes.len(),
        proof_summary.snapshots,
        proof_summary.unique_nodes,
        proof_summary.node_references,
        proof_summary.reused_references,
        proof_checked.cached_references,
        proof_checked.edge_columns_checked,
        proof_checked.triangle_columns_checked,
        proof_verify_ns,
    );
    Ok(())
}

fn main() -> ExitCode {
    match parse() {
        Ok(None) => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Some(options)) => match run(options) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("dynamic-bench: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("dynamic-bench: {error}\n\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}
