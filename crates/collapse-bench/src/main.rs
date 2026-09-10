//! Phase-separated driver for the ordered and rounds collapse scaling studies.
//!
//! The driver loads one point cloud and runs every requested pipeline
//! configuration in this process. Each phase carries its own clock, so no
//! number comes from subtracting one command line from another. Every
//! configuration computes its diagram once before any timing starts. The
//! diagrams must agree bar for bar, or the run aborts. The ordered
//! configurations face a second gate: their collapsed matrix and
//! certificate must equal the serial version 1 run's.
//!
//! Timed repetitions interleave the configurations instead of running one
//! configuration to exhaustion.
//!
//! Output is one key=value line per record. benchmarks/collapse_scaling_ordered.sh
//! parses it; the fields below are its interface.

#![forbid(unsafe_code)]

mod cli;
mod execution;
mod model;
mod reporting;

fn main() -> std::process::ExitCode {
    cli::main_entry()
}
