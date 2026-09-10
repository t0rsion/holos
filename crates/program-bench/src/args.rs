//! Command-line arguments for the registered program study.

use std::path::PathBuf;

pub(crate) const USAGE: &str = "\
Usage: program-bench --atoms A --atom-vertices K --seed S [options]

Generate complete weighted atoms that share one articulation vertex. One
trajectory changes only unguarded cross-atom orders. A second trajectory
crosses one result-sensitive guard at each step. The benchmark compares
reduction-free evaluation and local state updates with exact recomputation.

Options:
  --steps K       updates per timed repetition (default 24)
  --reps K        timed repetitions per arm (default 5, minimum 5)
  --modulus P     coefficient field Z/p (default 2)
  --artifact-prefix PATH
                  write PATH.graph, PATH.program, and PATH.trace
  -h, --help      print this text

Graph generation, trajectory search, compilation, artifact construction, and
warm-up are outside the update clocks. Every diagram is compared bit for bit
before a timing record is printed.
";

#[derive(Clone)]
pub(crate) struct Options {
    pub(crate) atoms: usize,
    pub(crate) atom_vertices: usize,
    pub(crate) seed: u64,
    pub(crate) steps: usize,
    pub(crate) reps: usize,
    pub(crate) modulus: u32,
    pub(crate) artifact_prefix: Option<PathBuf>,
}

pub(crate) fn parse() -> Result<Option<Options>, String> {
    let mut values = ParsedArguments::default();
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        if parse_argument(&argument, &mut arguments, &mut values)? {
            return Ok(None);
        }
    }
    let options = Options {
        atoms: values
            .atoms
            .ok_or_else(|| "--atoms is required".to_string())?,
        atom_vertices: values
            .atom_vertices
            .ok_or_else(|| "--atom-vertices is required".to_string())?,
        seed: values
            .seed
            .ok_or_else(|| "--seed is required".to_string())?,
        steps: values.steps,
        reps: values.reps,
        modulus: values.modulus,
        artifact_prefix: values.artifact_prefix,
    };
    validate(&options)?;
    Ok(Some(options))
}

struct ParsedArguments {
    atoms: Option<usize>,
    atom_vertices: Option<usize>,
    seed: Option<u64>,
    steps: usize,
    reps: usize,
    modulus: u32,
    artifact_prefix: Option<PathBuf>,
}

impl Default for ParsedArguments {
    fn default() -> Self {
        Self {
            atoms: None,
            atom_vertices: None,
            seed: None,
            steps: 24,
            reps: 5,
            modulus: 2,
            artifact_prefix: None,
        }
    }
}

fn parse_argument(
    argument: &str,
    arguments: &mut impl Iterator<Item = String>,
    values: &mut ParsedArguments,
) -> Result<bool, String> {
    if argument == "-h" || argument == "--help" {
        return Ok(true);
    }
    let value = arguments
        .next()
        .ok_or_else(|| format!("{argument} needs a value"))?;
    assign_option(argument, &value, values)?;
    Ok(false)
}

fn assign_option(argument: &str, value: &str, values: &mut ParsedArguments) -> Result<(), String> {
    match argument {
        "--atoms" => assign_atoms(value, values),
        "--atom-vertices" => assign_atom_vertices(value, values),
        "--seed" => assign_seed(value, values),
        "--steps" => assign_steps(value, values),
        "--reps" => assign_reps(value, values),
        "--modulus" => assign_modulus(value, values),
        "--artifact-prefix" => {
            values.artifact_prefix = Some(PathBuf::from(value));
            Ok(())
        }
        _ => Err(format!("unknown option {argument}")),
    }
}

fn assign_atoms(value: &str, values: &mut ParsedArguments) -> Result<(), String> {
    values.atoms = Some(parse_value("--atoms", value)?);
    Ok(())
}

fn assign_atom_vertices(value: &str, values: &mut ParsedArguments) -> Result<(), String> {
    values.atom_vertices = Some(parse_value("--atom-vertices", value)?);
    Ok(())
}

fn assign_seed(value: &str, values: &mut ParsedArguments) -> Result<(), String> {
    values.seed = Some(parse_value("--seed", value)?);
    Ok(())
}

fn assign_steps(value: &str, values: &mut ParsedArguments) -> Result<(), String> {
    values.steps = parse_value("--steps", value)?;
    Ok(())
}

fn assign_reps(value: &str, values: &mut ParsedArguments) -> Result<(), String> {
    values.reps = parse_value("--reps", value)?;
    Ok(())
}

fn assign_modulus(value: &str, values: &mut ParsedArguments) -> Result<(), String> {
    values.modulus = parse_value("--modulus", value)?;
    Ok(())
}

fn validate(options: &Options) -> Result<(), String> {
    if options.atoms < 2 || options.atom_vertices < 4 || options.steps == 0 || options.reps < 5 {
        return Err(
            "atoms must be at least 2, atom vertices at least 4, steps positive, and reps at least 5"
                .into(),
        );
    }
    Ok(())
}

fn parse_value<T: std::str::FromStr>(label: &str, value: &str) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("invalid value for {label}: {value}"))
}
