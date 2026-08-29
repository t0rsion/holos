#![forbid(unsafe_code)]

mod explicit;
mod geometry;
mod portfolio;

use std::process::ExitCode;
use std::time::Duration;

pub(crate) struct Measurement {
    family: &'static str,
    case: &'static str,
    producer: Duration,
    checker: Duration,
    artifact_bytes: usize,
    work: String,
}

fn main() -> ExitCode {
    match parse_repetitions().and_then(run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("research-bench: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_repetitions() -> Result<usize, String> {
    let mut arguments = std::env::args().skip(1);
    let mut repetitions = 5;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--reps" => {
                repetitions = arguments
                    .next()
                    .ok_or_else(|| "--reps needs a positive integer".to_owned())?
                    .parse()
                    .map_err(|_| "--reps needs a positive integer".to_owned())?;
            }
            "--help" | "-h" => {
                println!("Usage: research-bench [--reps N]");
                return Ok(0);
            }
            _ => return Err(format!("unknown argument {argument}")),
        }
    }
    if repetitions == 0 {
        return Err("--reps must be positive".to_owned());
    }
    Ok(repetitions)
}

fn run(repetitions: usize) -> Result<(), String> {
    if repetitions == 0 {
        return Ok(());
    }
    println!(
        "kind=header format=holos-research-v1 version={} commit={} profile={} reps={repetitions}",
        holos_tda::VERSION,
        holos_tda::GIT_HASH,
        holos_tda::BUILD_PROFILE,
    );
    for measurement in [
        portfolio::measure(repetitions)?,
        explicit::measure(repetitions)?,
        geometry::measure(repetitions)?,
    ] {
        print_measurement(measurement);
    }
    Ok(())
}

fn print_measurement(measurement: Measurement) {
    println!(
        "kind=case family={} case={} producer_median_ns={} checker_median_ns={} artifact_bytes={} {}",
        measurement.family,
        measurement.case,
        measurement.producer.as_nanos(),
        measurement.checker.as_nanos(),
        measurement.artifact_bytes,
        measurement.work,
    );
}

pub(crate) fn median(mut values: Vec<Duration>) -> Duration {
    values.sort_unstable();
    values[values.len() / 2]
}
