//! Registered benchmark for result-sensitive persistence programs.

#![forbid(unsafe_code)]

mod args;
mod artifact_output;
mod graph;
mod study;

use std::process::ExitCode;

fn main() -> ExitCode {
    match args::parse() {
        Ok(None) => {
            print!("{}", args::USAGE);
            ExitCode::SUCCESS
        }
        Ok(Some(options)) => match study::run(options) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("program-bench: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("program-bench: {error}\n\n{}", args::USAGE);
            ExitCode::FAILURE
        }
    }
}
