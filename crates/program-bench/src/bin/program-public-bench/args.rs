//! Command-line arguments.

use std::path::PathBuf;

pub(crate) const USAGE: &str = "\
Usage: program-public-bench TRAJECTORY [options]

Compare persistence-program maintenance with fresh exact H0 and H1 diagrams
on a prepared HOLOSTEM1 temporal trajectory.

Options:
  --reps K        timed repetitions per arm (default 5, minimum 5)
  --modulus P     coefficient field Z/p (default 2)
  --artifact-prefix PATH
                  write PATH.graph, PATH.program, and PATH.trace
  -h, --help      print this text

Preparation, compilation, artifact construction, and warm-up are outside the
trajectory clocks. Every diagram is compared bit for bit before a timing
record is printed.
";

pub(crate) struct Options {
    pub(crate) trajectory: PathBuf,
    pub(crate) reps: usize,
    pub(crate) modulus: u32,
    pub(crate) artifact_prefix: Option<PathBuf>,
}

pub(crate) fn parse() -> Result<Option<Options>, String> {
    let mut values = ParsedArguments::default();
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "-h" || argument == "--help" {
            return Ok(None);
        }
        if argument.starts_with('-') {
            parse_option(&argument, &mut arguments, &mut values)?;
        } else if values.trajectory.is_none() {
            values.trajectory = Some(PathBuf::from(argument));
        } else {
            return Err("only one trajectory may be supplied".into());
        }
    }
    build_options(values).map(Some)
}

struct ParsedArguments {
    trajectory: Option<PathBuf>,
    reps: usize,
    modulus: u32,
    artifact_prefix: Option<PathBuf>,
}

impl Default for ParsedArguments {
    fn default() -> Self {
        Self {
            trajectory: None,
            reps: 5,
            modulus: 2,
            artifact_prefix: None,
        }
    }
}

fn parse_option(
    argument: &str,
    arguments: &mut impl Iterator<Item = String>,
    values: &mut ParsedArguments,
) -> Result<(), String> {
    match argument {
        "--reps" => values.reps = parse_next(argument, arguments.next())?,
        "--modulus" => values.modulus = parse_next(argument, arguments.next())?,
        "--artifact-prefix" => {
            values.artifact_prefix =
                Some(PathBuf::from(arguments.next().ok_or_else(|| {
                    "--artifact-prefix needs a value".to_string()
                })?));
        }
        _ => return Err(format!("unknown option {argument}")),
    }
    Ok(())
}

fn build_options(values: ParsedArguments) -> Result<Options, String> {
    if values.reps < 5 {
        return Err("reps must be at least 5".into());
    }
    Ok(Options {
        trajectory: values
            .trajectory
            .ok_or_else(|| "TRAJECTORY is required".to_string())?,
        reps: values.reps,
        modulus: values.modulus,
        artifact_prefix: values.artifact_prefix,
    })
}

fn parse_next<T: std::str::FromStr>(label: &str, value: Option<String>) -> Result<T, String> {
    let value = value.ok_or_else(|| format!("{label} needs a value"))?;
    value
        .parse()
        .map_err(|_| format!("invalid value for {label}: {value}"))
}
