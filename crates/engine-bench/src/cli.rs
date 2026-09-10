use std::process::ExitCode;

use super::execution;
use super::input;
use holos_tda::DenseStorage;

use super::model::{Args, ArgsBuilder, Engine, Format};

const USAGE: &str = "\
Usage: engine-bench --input FILE --threshold T [options]

Time the input parse, the distance build, the graph build, and the
reduction as separate phases, in one process, for every engine
configuration of one input.

Required:
  --input FILE            input file: a point cloud csv, a condensed
                          lower-distance matrix with --format lower-distance,
                          or 'i j d' triplets with --format sparse
  --threshold T           filtration threshold; every configuration gets this
                          same value

Options:
  --entry ID              entry id on every output line (default: the input
                          file stem)
  --format FORMAT         points, lower-distance, or sparse (default points)
  --max-dim D             highest homology dimension (default 1)
  --modulus P             coefficient field Z/p (default 2)
  --reps K                timed repetitions per configuration (default 5)
  --threads N             worker threads for the reduction (default 1). 1
                          runs the serial engine. Above 1 the parallel
                          reducer and the parallel assembly run, and the
                          diagram must still match bar for bar
  --parse-threads N       worker threads for the input parse (default 1). 1
                          parses serially. Above 1 the parse splits the file
                          into one line chunk per thread, and a file under
                          the library's size threshold stays serial anyway
  --mode MODES            comma-separated configuration list, or all
                          (default all): auto, dense, sparse
  --dense-storage FORM    storage form a dense run reduces from: auto,
                          compact, or square (default auto). It reaches the
                          auto and dense configurations, which are the ones
                          that hold a distance matrix
  --diagram-out FILE      write the verified diagram in ripser's format, so
                          an outside tool can be compared against it
  --emit-collapsed FILE   do not time anything. Read the input, collapse its
                          dominated edges with the serial schedule, and write
                          the reduced graph to FILE as 'i j d' triplets. One
                          metadata line goes to stdout. Preprocessing for
                          corpus entries whose input is a collapsed graph
  -h, --help              print this text
  --version               print the driver version

Configurations:
  auto    the library routing rule on the whole distance matrix
  dense   the dense engine forced on the whole distance matrix
  sparse  the same input thresholded to a sparse matrix, then the sparse
          engine on that graph

--dense-storage forces the storage form so that the harness can time both
against each other. auto lets the library rule choose; compact forbids the
full row-major matrix; square forces it. A configuration routed to the
sparse engine never holds a distance matrix, so the flag does not reach it.

Every configuration runs at the same thread count with collapse off.
auto and dense set RipsParams::engine and differ in nothing else, so their
gap is the routing rule alone. sparse is an entry point instead:
rips_persistence_sparse on a graph this driver builds, which puts the
conversion on a clock of its own. The three optimization toggles stay at
their defaults.

With --format sparse the auto and dense configurations first widen the
graph back to a full matrix, one +inf per absent pair. That is the routing
regret an entry of the memory stratum measures: the widening is quick, and
the matrix it builds is not.

Phases:
  parse     read the input file and parse its numbers, under
            --parse-threads workers
  distance  build the DistanceMatrix from the parsed input. With --format
            sparse this is the widening to a full matrix, and only the
            dense configuration pays it
  graph     build the sparse graph (sparse configuration only): a threshold
            pass over the matrix, or the triplets of a sparse input
  reduce    the whole reduction. The solver exposes no per-dimension
            boundary outside the crate, so the reduction is one clock
  total     the whole configuration, from one enclosing clock

Every repetition parses the file again, so total covers the work a command
line does and compares against another tool's process time. The comparison
still favors this driver by the process start it never pays. Read the
reduce phase for the engine alone.

Counterbalancing:
  The timed repetitions interleave the configurations. Repetition r runs
  them starting at position r of the configuration order and wraps around.
  Each configuration moves one place earlier every repetition, so drift or
  a thermal ramp cannot land on one configuration alone. The rotation is
  balanced, every configuration in every position equally often, only when
  the repetition count is a multiple of the configuration count. The
  kind=entry line reports balanced=yes or balanced=no, and each repetition
  prints its exact order as a kind=order line.

Diagram equality:
  Every configuration computes its diagram once before the timed
  repetitions. The comparison is exact and canonicalized: dimension, birth
  bits, and death bits. A mismatch prints the offending configuration and
  exits nonzero, and no timing is printed. Every timed repetition is
  compared against its own agreement diagram as well, after its clocks
  stop.

Peak memory:
  vm_hwm_kb is VmHWM from /proc/self/status, read once after the timed
  repetitions. VmHWM never falls, and the repetitions interleave the
  configurations, so the mark belongs to the whole process and to no single
  configuration. For a peak that belongs to one configuration alone, run
  one configuration per --mode per process; benchmarks/engine_bench.sh
  does that.
";

/// Every configuration, in the order --mode all runs them.
fn all_modes() -> Vec<Engine> {
    vec![Engine::Dense, Engine::Sparse, Engine::Auto]
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut builder = ArgsBuilder::default();
    let mut it = argv.iter();
    while let Some(flag) = it.next() {
        if !ArgsBuilder::accepts(flag) {
            return Err(format!("unknown argument {flag}; run with --help"));
        }
        let value = it.next().ok_or_else(|| format!("{flag} needs a value"))?;
        builder.set(flag, value)?;
    }
    builder.finish()
}

/// A --mode value: "all", or a comma-separated list of configuration
/// names. A repeated name is an error, because the rotation and the
/// diagram table key on the name.
fn parse_modes(text: &str) -> Result<Vec<Engine>, String> {
    if text == "all" {
        return Ok(all_modes());
    }
    let mut modes = Vec::new();
    for name in text.split(',') {
        let engine = match name {
            "auto" => Engine::Auto,
            "dense" => Engine::Dense,
            "sparse" => Engine::Sparse,
            other => {
                return Err(format!(
                    "unknown mode {other}; use auto, dense, sparse, or all"
                ));
            }
        };
        if modes.contains(&engine) {
            return Err(format!("mode {name} is listed twice"));
        }
        modes.push(engine);
    }
    if modes.is_empty() {
        return Err("--mode selected no configuration".to_string());
    }
    Ok(modes)
}

fn parse_threshold(threshold_text: &str) -> Result<f64, String> {
    let threshold: f64 = threshold_text
        .parse()
        .map_err(|_| format!("--threshold {threshold_text} is not a number"))?;
    if threshold.is_nan() || threshold < 0.0 {
        return Err(format!("--threshold {threshold_text} must be non-negative"));
    }
    Ok(threshold)
}

fn require_positive(value: usize, flag: &str) -> Result<(), String> {
    if value == 0 {
        Err(format!("{flag} must be at least 1"))
    } else {
        Ok(())
    }
}

fn parse_format(text: &str) -> Result<Format, String> {
    match text {
        "points" => Ok(Format::Points),
        "lower-distance" => Ok(Format::LowerDistance),
        "sparse" => Ok(Format::Sparse),
        other => Err(format!(
            "unknown format {other}; use points, lower-distance, or sparse"
        )),
    }
}

fn parse_storage(text: &str) -> Result<DenseStorage, String> {
    match text {
        "auto" => Ok(DenseStorage::Auto),
        "compact" => Ok(DenseStorage::Compact),
        "square" => Ok(DenseStorage::Square),
        other => Err(format!(
            "unknown dense storage {other}; use auto, compact, or square"
        )),
    }
}

fn parse_usize(text: &str, flag: &str) -> Result<usize, String> {
    text.trim()
        .parse()
        .map_err(|_| format!("{flag} {text} is not a whole number"))
}

impl ArgsBuilder {
    fn accepts(flag: &str) -> bool {
        matches!(
            flag,
            "--input"
                | "--entry"
                | "--threshold"
                | "--format"
                | "--max-dim"
                | "--modulus"
                | "--reps"
                | "--threads"
                | "--parse-threads"
                | "--mode"
                | "--dense-storage"
                | "--diagram-out"
                | "--emit-collapsed"
        )
    }

    fn set(&mut self, flag: &str, value: &str) -> Result<(), String> {
        if self.set_identity(flag, value) || self.set_output(flag, value) {
            return Ok(());
        }
        if self.set_dimensions(flag, value)? || self.set_execution(flag, value)? {
            return Ok(());
        }
        if self.set_mode(flag, value)? {
            return Ok(());
        }
        Err(format!("unknown argument {flag}; run with --help"))
    }

    fn set_identity(&mut self, flag: &str, value: &str) -> bool {
        match flag {
            "--input" => self.input = Some(value.to_string()),
            "--entry" => self.entry = Some(value.to_string()),
            "--threshold" => self.threshold_text = Some(value.to_string()),
            _ => return false,
        }
        true
    }

    fn set_output(&mut self, flag: &str, value: &str) -> bool {
        match flag {
            "--diagram-out" => self.diagram_out = Some(value.to_string()),
            "--emit-collapsed" => self.emit_collapsed = Some(value.to_string()),
            _ => return false,
        }
        true
    }

    fn set_dimensions(&mut self, flag: &str, value: &str) -> Result<bool, String> {
        match flag {
            "--max-dim" => self.max_dim = parse_usize(value, flag)?,
            "--modulus" => {
                self.modulus = u32::try_from(parse_usize(value, flag)?)
                    .map_err(|_| "--modulus is out of range".to_string())?;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn set_execution(&mut self, flag: &str, value: &str) -> Result<bool, String> {
        match flag {
            "--reps" => self.reps = parse_usize(value, flag)?,
            "--threads" => self.threads = parse_usize(value, flag)?,
            "--parse-threads" => self.parse_threads = parse_usize(value, flag)?,
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn set_mode(&mut self, flag: &str, value: &str) -> Result<bool, String> {
        match flag {
            "--format" => self.format = parse_format(value)?,
            "--mode" => self.mode = parse_modes(value)?,
            "--dense-storage" => self.dense_storage = parse_storage(value)?,
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn finish(self) -> Result<Args, String> {
        let input = self
            .input
            .ok_or_else(|| "--input is required".to_string())?;
        let threshold_text = self
            .threshold_text
            .ok_or_else(|| "--threshold is required".to_string())?;
        let threshold = parse_threshold(&threshold_text)?;
        require_positive(self.reps, "--reps")?;
        require_positive(self.threads, "--threads")?;
        require_positive(self.parse_threads, "--parse-threads")?;
        let entry = self.entry.unwrap_or_else(|| input::file_stem(&input));
        Ok(Args {
            input,
            entry,
            threshold,
            threshold_text,
            format: self.format,
            max_dim: self.max_dim,
            modulus: self.modulus,
            reps: self.reps,
            threads: self.threads,
            parse_threads: self.parse_threads,
            mode: self.mode,
            dense_storage: self.dense_storage,
            diagram_out: self.diagram_out,
            emit_collapsed: self.emit_collapsed,
        })
    }
}

impl Default for ArgsBuilder {
    fn default() -> Self {
        Self {
            input: None,
            entry: None,
            threshold_text: None,
            format: Format::Points,
            max_dim: 1,
            modulus: 2,
            reps: 5,
            threads: 1,
            parse_threads: 1,
            mode: all_modes(),
            dense_storage: DenseStorage::Auto,
            diagram_out: None,
            emit_collapsed: None,
        }
    }
}

pub(super) fn main_entry() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.iter().any(|a| a == "-h" || a == "--help") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    if argv.iter().any(|a| a == "--version") {
        println!("engine-bench {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    match run(&argv) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("engine-bench: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(argv: &[String]) -> Result<(), String> {
    let args = parse_args(argv)?;
    if let Some(path) = args.emit_collapsed.clone() {
        return input::emit_collapsed(&args, &path);
    }
    execution::run(&args)
}
