//! Phase-separated driver for the reduction: one input, one threshold,
//! and every engine configuration timed in one process. The default is
//! the serial engine; `--threads` puts the same phases on the parallel
//! one.
//!
//! benchmarks/engine_bench.sh runs this driver on
//! benchmarks/engineering_corpus.toml. It is not a registered study.
//! No grade reads it.
//!
//! Each phase carries its own clock, so no number comes from subtracting
//! one command line from another. Every configuration computes its
//! diagram once before any timing starts. The diagrams must agree bar for
//! bar, or the run aborts.
//!
//! Timed repetitions interleave the configurations instead of running one
//! configuration to exhaustion.
//!
//! Output is one key=value line per record. benchmarks/engine_bench.sh
//! parses it; the fields below are its interface.

#![forbid(unsafe_code)]

mod cli;
mod execution;
mod input;
mod model;
mod reporting;

fn main() -> std::process::ExitCode {
    cli::main_entry()
}
