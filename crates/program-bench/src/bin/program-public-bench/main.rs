//! Public temporal-graph control for persistence programs.

#![forbid(unsafe_code)]

mod args;
#[path = "../../artifact_output.rs"]
mod artifact_output;
mod data;
mod execution;
mod record;

use std::process::ExitCode;

fn main() -> ExitCode {
    match args::parse() {
        Ok(None) => {
            print!("{}", args::USAGE);
            ExitCode::SUCCESS
        }
        Ok(Some(options)) => match execution::run(&options) {
            Ok(result) => {
                println!("{}", record::line(&result));
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("program-public-bench: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("program-public-bench: {error}\n\n{}", args::USAGE);
            ExitCode::FAILURE
        }
    }
}
