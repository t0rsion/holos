//! Counterbalanced end-to-end study driver for adaptive collapse.

#![forbid(unsafe_code)]

use std::process::ExitCode;

use args::parse_args;
use input::{prepare_study_input, vm_hwm_kb};
use pipeline::agreement_gate;
use reporting::{collect_samples, print_study_header, print_summary};

#[path = "adaptive_bench/args.rs"]
mod args;
#[path = "adaptive_bench/input.rs"]
mod input;
#[path = "adaptive_bench/pipeline.rs"]
mod pipeline;
#[path = "adaptive_bench/reporting.rs"]
mod reporting;

const USAGE: &str = "\
Usage: collapse-adaptive-bench --input FILE --threshold T [options]

Run exact diagram gates, then time no collapse, versions 1 and 2, and both
version 3 objectives on one point cloud. Every collapse result is encoded,
decoded, and checked by the independent verifier.

Required:
  --input FILE          point cloud csv, one point per line
  --threshold T         filtration threshold shared by every configuration

Options:
  --entry ID            record id (default: input file stem)
  --max-dim D           highest homology dimension (default 2)
  --modulus P           coefficient field Z/p (default 2)
  --threads N           reducer workers and version 2 workers (default 1)
  --reps N              timed repetitions (default 5)
  --configs LIST        comma-separated none,v1,v2,v3-h1,v3-h2 (default all)
  --work-limit N        version 3 removability-test limit (default unlimited)
  -h, --help            print this text
  --version             print the driver version

The timed order rotates by one configuration per repetition. A balanced run
uses a repetition count divisible by the number of configurations. The
record reports whether the rotation is balanced.

compute_s includes point distances, threshold graph construction, collapse,
and reduction. certified_s also includes canonical artifact encoding,
decoding, binding checks, and independent replay verification. The no-collapse
configuration has no artifact or verification phase.
";

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.iter().any(|arg| arg == "-h" || arg == "--help") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    if argv.iter().any(|arg| arg == "--version") {
        println!("collapse-adaptive-bench {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    match run(&argv) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("collapse-adaptive-bench: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(argv: &[String]) -> Result<(), String> {
    let args = parse_args(argv)?;
    let input = prepare_study_input(&args)?;
    let references = agreement_gate(&input.points, &args)?;
    print_study_header(&args, &input);
    let samples = collect_samples(&input.points, &args, &references)?;
    for (&kind, runs) in args.kinds.iter().zip(&samples) {
        print_summary(&args, kind, runs);
    }
    println!(
        "kind=memory entry={} vm_hwm_kb={} scope=whole_process",
        args.entry,
        vm_hwm_kb().map_or("unavailable".to_string(), |value| value.to_string())
    );
    Ok(())
}

#[cfg(test)]
#[path = "adaptive_bench/tests.rs"]
mod tests;
