//! Registered benchmark for local persistence-program repair.

#![forbid(unsafe_code)]

use std::hint::black_box;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use holos_tda::{
    CertificateLimits, Diagram, EdgeKey, PersistenceProgram, ProgramUpdateMode, RipsParams,
    SparseDistanceMatrix, rips_persistence_sparse, rips_persistence_with_classes_sparse,
};

const USAGE: &str = "\
Usage: program-bench --atoms A --atom-vertices K --seed S [options]

Generate complete weighted atoms that share one articulation vertex. Each
update crosses a result-sensitive guard in one atom. Compare checked local
repair with full persistence and canonical-class recomputation over the same
cumulative trajectory.

Options:
  --steps K       updates per timed repetition (default 24)
  --reps K        timed repetitions per arm (default 5, minimum 5)
  --modulus P     coefficient field Z/p (default 2)
  -h, --help      print this text

Graph generation, update search, compilation, and warm-up are outside the
update clocks. The study times accepted evaluation and one-atom repair as
separate arms. Every diagram must agree bit for bit before the record prints.
";

#[derive(Clone, Copy)]
struct Options {
    atoms: usize,
    atom_vertices: usize,
    seed: u64,
    steps: usize,
    reps: usize,
    modulus: u32,
}

fn parse() -> Result<Option<Options>, String> {
    let mut atoms = None;
    let mut atom_vertices = None;
    let mut seed = None;
    let mut steps = 24;
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
        reps,
        modulus,
    };
    if options.atoms < 2 || options.atom_vertices < 4 || options.steps == 0 || options.reps < 5 {
        return Err(
            "atoms must be at least 2, atom vertices at least 4, steps positive, and reps at least 5"
                .into(),
        );
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
    let mut rank = vec![0; endpoints.len()];
    for (position, &(_, index)) in order.iter().enumerate() {
        rank[index] = position;
    }
    let denominator = endpoints.len() as f64 + 1.0;
    let triplets: Vec<_> = endpoints
        .into_iter()
        .enumerate()
        .map(|(index, (u, v))| (u, v, 1.0 + 8.0 * (rank[index] + 1) as f64 / denominator))
        .collect();
    let vertices = 1 + options.atoms * (options.atom_vertices - 1);
    SparseDistanceMatrix::from_triplets(vertices, &triplets).map_err(|error| error.to_string())
}

fn replace_weights(
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

fn trajectory(
    input: &SparseDistanceMatrix,
    params: &RipsParams,
    steps: usize,
) -> Result<Vec<SparseDistanceMatrix>, String> {
    let mut program = PersistenceProgram::compile(input, params, CertificateLimits::default())
        .map_err(|error| error.to_string())?;
    let cyclic: Vec<_> = program
        .atoms()
        .iter()
        .filter(|atom| atom.cyclic)
        .map(|atom| atom.edges.clone())
        .collect();
    let mut current = input.clone();
    let mut updates = Vec::with_capacity(steps);
    for step in 0..steps {
        let edges = &cyclic[step % cyclic.len()];
        let mut accepted = None;
        'pairs: for left in 0..edges.len() {
            for right in left + 1..edges.len() {
                let candidate = replace_weights(&current, edges[left], edges[right])?;
                let mut trial = program.clone();
                let update = trial
                    .advance(&candidate)
                    .map_err(|error| error.to_string())?;
                if update.mode == ProgramUpdateMode::Repaired
                    && update.work.atoms_touched == 1
                    && update.work.atoms_rebuilt == 1
                {
                    accepted = Some((candidate, trial));
                    break 'pairs;
                }
            }
        }
        let (candidate, next) = accepted
            .ok_or_else(|| format!("no one-atom guard-crossing update exists at step {step}"))?;
        current = candidate.clone();
        program = next;
        updates.push(candidate);
    }
    Ok(updates)
}

fn reuse_trajectory(
    input: &SparseDistanceMatrix,
    options: Options,
) -> Result<Vec<SparseDistanceMatrix>, String> {
    let initial_order: Vec<_> = {
        let mut weighted: Vec<_> = input
            .edges()
            .map(|(u, v, weight)| (weight, EdgeKey { u, v }))
            .collect();
        weighted.sort_by(|left, right| left.0.total_cmp(&right.0));
        weighted.into_iter().map(|(_, edge)| edge).collect()
    };
    let mut crossed_global_order = false;
    let updates = (1..=options.steps)
        .map(|step| {
            let triplets: Vec<_> = input
                .edges()
                .map(|(u, v, weight)| {
                    let labeled = if u == 0 { v } else { u };
                    let atom = (labeled - 1) / (options.atom_vertices - 1);
                    let offset = (step * (atom + 1)) as f64 * 1e-2;
                    (u, v, weight + offset)
                })
                .collect();
            let update = SparseDistanceMatrix::from_triplets(input.len(), &triplets)
                .map_err(|error| error.to_string())?;
            let mut weighted: Vec<_> = update
                .edges()
                .map(|(u, v, weight)| (weight, EdgeKey { u, v }))
                .collect();
            weighted.sort_by(|left, right| left.0.total_cmp(&right.0));
            crossed_global_order |= weighted
                .iter()
                .map(|(_, edge)| *edge)
                .ne(initial_order.iter().copied());
            Ok(update)
        })
        .collect::<Result<Vec<_>, String>>()?;
    if !crossed_global_order {
        return Err("accepted trajectory did not cross the global edge order".into());
    }
    Ok(updates)
}

fn timed<T>(operation: impl FnOnce() -> T) -> (Duration, T) {
    let start = Instant::now();
    let result = operation();
    (start.elapsed(), result)
}

fn update(
    initial: &PersistenceProgram,
    updates: &[SparseDistanceMatrix],
) -> Result<Vec<Diagram>, String> {
    let mut program = initial.clone();
    updates
        .iter()
        .map(|input| {
            let result = program.advance(input).map_err(|error| error.to_string())?;
            if result.mode != ProgramUpdateMode::Repaired
                || result.work.atoms_touched != 1
                || result.work.atoms_rebuilt != 1
            {
                return Err("timed update did not repair exactly one atom".into());
            }
            Ok(result.result.diagram)
        })
        .collect()
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

fn recompute_diagram(
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

fn diagram_bits_equal(left: &Diagram, right: &Diagram) -> bool {
    left.bars.len() == right.bars.len()
        && left.bars.iter().zip(&right.bars).all(|(left, right)| {
            left.dim == right.dim
                && left.birth.to_bits() == right.birth.to_bits()
                && left.death.to_bits() == right.death.to_bits()
        })
}

fn median(mut values: Vec<u128>) -> u128 {
    values.sort_unstable();
    values[values.len() / 2]
}

fn run(options: Options) -> Result<(), String> {
    let input = graph(options)?;
    let params = RipsParams::new(1).with_modulus(options.modulus);
    let reuse_updates = reuse_trajectory(&input, options)?;
    let repair_updates = trajectory(&input, &params, options.steps)?;

    let mut compile_times = Vec::with_capacity(options.reps);
    let mut compiled = None;
    for _ in 0..options.reps {
        let (elapsed, program) =
            timed(|| PersistenceProgram::compile(&input, &params, CertificateLimits::default()));
        compiled = Some(program.map_err(|error| error.to_string())?);
        compile_times.push(elapsed.as_nanos());
    }
    let program = compiled.expect("repetitions are positive");
    let summary = program.summary();
    let reuse_expected = recompute_diagram(&params, &reuse_updates)?;
    let reuse_warm = evaluate(&program, &reuse_updates)?;
    if !reuse_warm
        .iter()
        .zip(&reuse_expected)
        .all(|(left, right)| diagram_bits_equal(left, right))
    {
        return Err("accepted-evaluation warm-up diagrams differ".into());
    }
    let repair_expected = recompute_rich(&params, &repair_updates)?;
    let repair_warm = update(&program, &repair_updates)?;
    if !repair_warm
        .iter()
        .zip(&repair_expected)
        .all(|(left, right)| diagram_bits_equal(left, right))
    {
        return Err("local-repair warm-up diagrams differ".into());
    }

    let mut evaluate_times = Vec::with_capacity(options.reps);
    let mut diagram_times = Vec::with_capacity(options.reps);
    let mut repair_times = Vec::with_capacity(options.reps);
    let mut rich_times = Vec::with_capacity(options.reps);
    for repetition in 0..options.reps {
        let program_first = repetition % 2 == 0;
        let (first_time, first) = if program_first {
            timed(|| evaluate(&program, &reuse_updates))
        } else {
            timed(|| recompute_diagram(&params, &reuse_updates))
        };
        let (second_time, second) = if program_first {
            timed(|| recompute_diagram(&params, &reuse_updates))
        } else {
            timed(|| evaluate(&program, &reuse_updates))
        };
        let (evaluated, exact) = if program_first {
            evaluate_times.push(first_time.as_nanos());
            diagram_times.push(second_time.as_nanos());
            (first?, second?)
        } else {
            diagram_times.push(first_time.as_nanos());
            evaluate_times.push(second_time.as_nanos());
            (second?, first?)
        };
        if !evaluated
            .iter()
            .zip(&exact)
            .all(|(left, right)| diagram_bits_equal(left, right))
        {
            return Err(format!(
                "accepted-evaluation repetition {repetition} diagrams differ"
            ));
        }
        black_box((evaluated, exact));

        let (first_time, first) = if program_first {
            timed(|| update(&program, &repair_updates))
        } else {
            timed(|| recompute_rich(&params, &repair_updates))
        };
        let (second_time, second) = if program_first {
            timed(|| recompute_rich(&params, &repair_updates))
        } else {
            timed(|| update(&program, &repair_updates))
        };
        let (repaired, exact) = if program_first {
            repair_times.push(first_time.as_nanos());
            rich_times.push(second_time.as_nanos());
            (first?, second?)
        } else {
            rich_times.push(first_time.as_nanos());
            repair_times.push(second_time.as_nanos());
            (second?, first?)
        };
        if !repaired
            .iter()
            .zip(&exact)
            .all(|(left, right)| diagram_bits_equal(left, right))
        {
            return Err(format!(
                "local-repair repetition {repetition} diagrams differ"
            ));
        }
        black_box((repaired, exact));
    }

    let compile_ns = median(compile_times);
    let evaluate_ns = median(evaluate_times);
    let diagram_ns = median(diagram_times);
    let repair_ns = median(repair_times);
    let rich_ns = median(rich_times);
    let saved = diagram_ns.saturating_sub(evaluate_ns);
    let break_even_steps = if saved == 0 {
        u128::MAX
    } else {
        (compile_ns * options.steps as u128).div_ceil(saved)
    };
    println!(
        "format=holos-program-bench-v1 atoms={} atom_vertices={} vertices={} edges={} seed={} steps={} reps={} modulus={} guards={} compile_ns={} evaluate_ns={} diagram_ns={} evaluate_speedup={:.6} break_even_steps={} repair_ns={} rich_ns={} repair_speedup={:.6}",
        summary.cyclic_atoms,
        options.atom_vertices,
        input.len(),
        input.num_edges(),
        options.seed,
        options.steps,
        options.reps,
        options.modulus,
        summary.guards,
        compile_ns,
        evaluate_ns,
        diagram_ns,
        diagram_ns as f64 / evaluate_ns as f64,
        break_even_steps,
        repair_ns,
        rich_ns,
        rich_ns as f64 / repair_ns as f64,
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
                eprintln!("program-bench: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("program-bench: {error}\n\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}
