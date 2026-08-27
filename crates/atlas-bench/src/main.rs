//! Registered trajectory benchmark for reusable persistence atlases.

#![forbid(unsafe_code)]

use std::hint::black_box;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use holos_tda::{
    Diagram, PersistenceAtlas, RipsParams, SparseDistanceMatrix, rips_persistence_sparse,
};

const USAGE: &str = "\
Usage: atlas-bench --n N --coord-dim D --seed S [options]

Generate one complete Euclidean graph with distinct distances. Compile an
H0 and H1 atlas, then compare exact atlas evaluation with a full persistence
reduction across an affine edge-weight trajectory.

Options:
  --steps K       updates per timed repetition (default 25)
  --reps K        timed repetitions per arm (default 5, minimum 5)
  --modulus P     coefficient field Z/p (default 2)
  -h, --help      print this text

Each repetition runs both arms. Their order alternates. Graph generation and
trajectory construction are outside every clock. Every updated diagram must
agree bit for bit before the record is printed.
";

#[derive(Clone, Copy)]
struct Options {
    n: usize,
    coord_dim: usize,
    seed: u64,
    steps: usize,
    reps: usize,
    modulus: u32,
}

fn parse() -> Result<Option<Options>, String> {
    let mut n = None;
    let mut coord_dim = None;
    let mut seed = None;
    let mut steps = 25;
    let mut reps = 5;
    let mut modulus = 2;
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        if argument == "-h" || argument == "--help" {
            return Ok(None);
        }
        let value = args
            .next()
            .ok_or_else(|| format!("{argument} needs a value"))?;
        match argument.as_str() {
            "--n" => n = Some(parse_value(&argument, &value)?),
            "--coord-dim" => coord_dim = Some(parse_value(&argument, &value)?),
            "--seed" => seed = Some(parse_value(&argument, &value)?),
            "--steps" => steps = parse_value(&argument, &value)?,
            "--reps" => reps = parse_value(&argument, &value)?,
            "--modulus" => modulus = parse_value(&argument, &value)?,
            _ => return Err(format!("unknown option {argument}")),
        }
    }
    let options = Options {
        n: n.ok_or_else(|| "--n is required".to_string())?,
        coord_dim: coord_dim.ok_or_else(|| "--coord-dim is required".to_string())?,
        seed: seed.ok_or_else(|| "--seed is required".to_string())?,
        steps,
        reps,
        modulus,
    };
    if options.n < 2 || options.coord_dim == 0 || options.steps == 0 || options.reps < 5 {
        return Err("n must be at least 2, dimensions and steps must be positive, and reps must be at least 5".into());
    }
    Ok(Some(options))
}

fn parse_value<T: std::str::FromStr>(label: &str, value: &str) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("invalid value for {label}: {value}"))
}

fn points(options: Options) -> Vec<Vec<f64>> {
    let mut state = options.seed;
    (0..options.n)
        .map(|point| {
            (0..options.coord_dim)
                .map(|coordinate| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    (state % 1_000_003) as f64 / 1_000_003.0
                        + point as f64 * 1e-10
                        + coordinate as f64 * 1e-12
                })
                .collect()
        })
        .collect()
}

fn complete_graph(points: &[Vec<f64>]) -> Result<SparseDistanceMatrix, String> {
    let mut triplets = Vec::new();
    for v in 1..points.len() {
        for u in 0..v {
            let distance = points[u]
                .iter()
                .zip(&points[v])
                .map(|(a, b)| (a - b) * (a - b))
                .sum::<f64>()
                .sqrt();
            triplets.push((u, v, distance));
        }
    }
    SparseDistanceMatrix::from_triplets(points.len(), &triplets).map_err(|error| error.to_string())
}

fn trajectory(
    input: &SparseDistanceMatrix,
    steps: usize,
) -> Result<Vec<SparseDistanceMatrix>, String> {
    (1..=steps)
        .map(|step| {
            let scale = 1.0 + step as f64 * 1e-6;
            let offset = step as f64 * 1e-7;
            let triplets: Vec<_> = input
                .edges()
                .map(|(u, v, value)| (u, v, value * scale + offset))
                .collect();
            SparseDistanceMatrix::from_triplets(input.len(), &triplets)
                .map_err(|error| error.to_string())
        })
        .collect()
}

fn timed<T>(operation: impl FnOnce() -> T) -> (Duration, T) {
    let start = Instant::now();
    let result = operation();
    (start.elapsed(), result)
}

fn reuse(
    atlas: &PersistenceAtlas,
    updates: &[SparseDistanceMatrix],
) -> Result<Vec<Diagram>, String> {
    updates
        .iter()
        .map(|input| {
            atlas
                .evaluate_diagram(input)
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
        .map(|input| rips_persistence_sparse(input, params).map_err(|error| error.to_string()))
        .collect()
}

fn diagram_bits_equal(a: &Diagram, b: &Diagram) -> bool {
    a.bars.len() == b.bars.len()
        && a.bars.iter().zip(&b.bars).all(|(a, b)| {
            a.dim == b.dim
                && a.birth.to_bits() == b.birth.to_bits()
                && a.death.to_bits() == b.death.to_bits()
        })
}

fn median(mut values: Vec<u128>) -> u128 {
    values.sort_unstable();
    values[values.len() / 2]
}

fn run(options: Options) -> Result<(), String> {
    let input = complete_graph(&points(options))?;
    let updates = trajectory(&input, options.steps)?;
    let params = RipsParams::new(1).with_modulus(options.modulus);

    let mut compile_times = Vec::with_capacity(options.reps);
    let mut atlas = None;
    for _ in 0..options.reps {
        let (elapsed, compiled) = timed(|| PersistenceAtlas::build(&input, &params));
        atlas = Some(compiled.map_err(|error| error.to_string())?);
        compile_times.push(elapsed.as_nanos());
    }
    let atlas = atlas.expect("repetitions are positive");

    let expected = recompute(&params, &updates)?;
    let warm = reuse(&atlas, &updates)?;
    if !warm
        .iter()
        .zip(&expected)
        .all(|(a, b)| diagram_bits_equal(a, b))
    {
        return Err("warm-up diagrams differ".into());
    }

    let mut reuse_times = Vec::with_capacity(options.reps);
    let mut recompute_times = Vec::with_capacity(options.reps);
    for repetition in 0..options.reps {
        let (first_time, first) = if repetition % 2 == 0 {
            timed(|| reuse(&atlas, &updates))
        } else {
            timed(|| recompute(&params, &updates))
        };
        let (second_time, second) = if repetition % 2 == 0 {
            timed(|| recompute(&params, &updates))
        } else {
            timed(|| reuse(&atlas, &updates))
        };
        let (reused, exact) = if repetition % 2 == 0 {
            reuse_times.push(first_time.as_nanos());
            recompute_times.push(second_time.as_nanos());
            (first?, second?)
        } else {
            recompute_times.push(first_time.as_nanos());
            reuse_times.push(second_time.as_nanos());
            (second?, first?)
        };
        if !reused
            .iter()
            .zip(&exact)
            .all(|(a, b)| diagram_bits_equal(a, b))
        {
            return Err(format!("repetition {repetition} diagrams differ"));
        }
        black_box((reused, exact));
    }

    let compile_ns = median(compile_times);
    let reuse_ns = median(reuse_times);
    let recompute_ns = median(recompute_times);
    let saved = recompute_ns.saturating_sub(reuse_ns);
    let break_even_steps = if saved == 0 {
        u128::MAX
    } else {
        (compile_ns * options.steps as u128).div_ceil(saved)
    };
    println!(
        "format=holos-atlas-bench-v1 n={} coord_dim={} seed={} edges={} steps={} reps={} modulus={} compile_ns={} reuse_ns={} recompute_ns={} speedup={:.6} break_even_steps={}",
        options.n,
        options.coord_dim,
        options.seed,
        input.num_edges(),
        options.steps,
        options.reps,
        options.modulus,
        compile_ns,
        reuse_ns,
        recompute_ns,
        recompute_ns as f64 / reuse_ns as f64,
        break_even_steps,
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
                eprintln!("atlas-bench: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("atlas-bench: {error}\n\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}
