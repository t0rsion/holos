use std::fs;
use std::path::Path;
use std::process::ExitCode;

use super::execution;
use super::model::{Args, ArgsBuilder, InputKind, Mode};

const USAGE: &str = "\
Usage: collapse-bench --input FILE --threshold T [options]

Time the collapse and the reduction as separate phases, in one process, for
every configuration of one point cloud.

Required:
  --input FILE            point cloud csv, one point per line
  --threshold T           filtration threshold; every configuration gets this
                          same value

Options:
  --entry ID              entry id on every output line (default: the input
                          file stem)
  --max-dim D             highest homology dimension (default 1)
  --modulus P             coefficient field Z/p (default 2)
  --collapse-threads LIST comma-separated collapse worker counts for the
                          version 2 and ordered configurations; the last
                          value is P, which the shipped pipeline runs at
                          (default 1)
  --reducer-threads N     reduction workers of the phase-split
                          configurations, held fixed across them (default 1)
  --reps K                timed repetitions per configuration (default 5)
  --mode MODE             v2, v1, v1o, v1p, none, rounds, or all (default
                          all). rounds is the rounds study set: every v2-cN,
                          v1-c1, and none
  --collapse-input KIND   sparse or dense collapse entry point (default
                          sparse)
  -h, --help              print this text
  --version               print the driver version

Configurations:
  v2-cN   version 2 collapse with N collapse workers
  v1-c1   version 1 serial collapse, the pipeline baseline
  v1o-cN  ordered speculative execution of the version 1 schedule with N
          collapse workers, clocked phase by phase
  v1p-cP  the shipped collapse pipeline at P workers, clocked end to end.
          P is the last --collapse-threads value.
  none    no collapse

The agreement runs come first, in the order v2, v1, v1o, v1p, none, so the
serial version 1 result exists before the ordered gate needs it.

Shipped pipeline against the phase split:
  holos_tda::rips_persistence with RipsParams::collapse_edges set and the
  ordered collapse schedule selected is the shipped pipeline: one run-wide
  pool drives the ordered collapse and then the reduction. It takes one
  worker count for both halves and exposes no phase boundary.
  The driver measures the shipped pipeline and the phase split. v1p-cP calls
  the shipped path and reports one `pipeline` phase inside its total, and it
  is the configuration the end-to-end comparison reads. v1o-cN builds a pool
  for the collapse, drops it, and lets the reduction build its own. It gives
  the separate collapse and reduce clocks, the collapse counters, and the
  ordered gate, and it is a diagnostic that feeds no end-to-end ratio.
  Because one pool serves both halves, v1p-cP also reduces with P workers.
  That is the registered ordered arm, ordered collapse at P plus the P-worker
  reducer, exactly when --reducer-threads is P. Every kind=config line states
  the role, the pool count, the reducer width, whether the configuration
  feeds the end-to-end ratio, and whether it matches the registered arm.

Counterbalancing:
  The timed repetitions interleave the configurations. Repetition r runs
  them starting at position r of the agreement order and wraps around.
  Each configuration moves one place earlier every repetition, so drift or
  a thermal ramp cannot land on one configuration alone. The rotation is
  balanced, every configuration in every position equally often, only when
  the repetition count is a multiple of the configuration count. The
  kind=entry line reports balanced=yes or balanced=no, and a registered run
  needs yes. The rotation is deterministic, and each repetition prints its
  exact order as a kind=order line.

Phases:
  distance  the distance matrix of the cloud
  graph     the thresholded sparse graph (sparse collapse input only)
  collapse  the whole collapse call, phase-split configurations only
  reduce    the persistence reduction, phase-split configurations only
  pipeline  the whole shipped collapse-and-reduce call, v1p only
  total     the whole configuration, from one enclosing clock. It includes
            the distance and graph builds, which are the same work in every
            configuration, so a ratio of totals sits nearer 1.0 than a
            ratio of the collapse-and-reduce parts alone. On the phase-split
            configurations the collapse result is dropped after the clock
            stops; the pipeline call drops it inside its clock.

Diagram equality:
  Every configuration computes its diagram once before the timed repetitions.
  The comparison is exact and canonicalized: dimension, birth bits, and death
  bits. A mismatch prints the offending configuration and exits nonzero, and
  no timing is printed. The comparison covers the configurations this run
  selected, so --mode all is the strongest check.

Ordered agreement:
  An ordered configuration runs the version 1 schedule, so it must
  reproduce the v1-c1 output exactly. The diagram comparison alone cannot
  see a difference inside the collapsed graph. Before any timing, every
  v1o configuration is compared against the v1-c1 run: the collapsed
  matrix edge for edge with values by bits, the whole certificate (header,
  removal order, endpoints, values by bits, pass numbers, witness segments
  by bits), and the invariant counters (input, output and removed edges,
  passes, witness segments, and logical_tests against the serial run's
  edge_tests). A mismatch prints the first difference and exits nonzero.
  The result is one kind=ordered_gate line per v1o configuration.
  The gate needs v1-c1 in the same run. --mode v1o without v1 exits
  nonzero. The shipped path returns a diagram and no certificate, so v1p
  faces the diagram comparison alone.

Peak memory:
  vm_hwm_kb is VmHWM from /proc/self/status, read once after the timed
  repetitions. VmHWM never falls, and the repetitions interleave the
  configurations, so the mark belongs to the whole process and to no single
  configuration. Read it as an upper bound. A per-repetition delta is not
  meaningful in one process either: allocators reuse pages, and the mark
  stays at the run maximum. For a peak that belongs to one configuration
  alone, run one --mode per process.

Counters:
  The driver prints what CollapseStats, CollapseTimings, and the certificate
  expose, for the configurations that own a collapse result. The shipped
  path keeps its collapse result inside, so v1p prints no counters.

  Every counter and every clock on the kind=counters line is the median over
  the timed repetitions, the rule the phase lines use. The line carries
  stat=median_over_timed_reps and reps, so the aggregation is on the record.
  The agreement run supplies no counter: its clocks are one cold sample.
  counters_stable says whether every timed repetition reported the same
  counts, which the deterministic schedule requires; the clocks are excluded
  from that check, because they vary by design.

  The scheduler counters logical_tests, invalidated_results,
  global_invalidations, window_batches, window_slots_offered,
  window_members_formed, and window_members_reused are public fields and
  print as measured. On the v1 and v2 paths, logical_tests equals edge_tests
  and the rest are zero. edge_tests keeps meaning physical predicate
  evaluations.

  The subphase clocks predicate_median_s, retirement_median_s, and
  repair_median_s come from CollapsedRips::timings. Only the ordered path
  clocks them: on the v1 and v2 paths collapse_subphases reads
  none_on_this_path and the three clocks read `unavailable`, because a zero
  there means unclocked, not a measured zero. retirement_median_s includes
  the repairs inside it.

  A derived field is a ratio of two medians, not the median of a ratio.
  With counters_stable=yes the two are equal.

  Derived, and labeled as derived:
  work_inflation_derived   edge_tests over logical_tests
  repair_fraction_derived  invalidated_results over logical_tests, a count, not
                           a cost; every invalidated result is repaired
                           serially at its turn
  retests_derived          logical_tests minus input_edges: the tests of
                           the schedule after the first full epoch, valid
                           while the first epoch tests every edge
  window_occupancy_derived window_members_formed over window_slots_offered
  unused_window_capacity_derived   window_slots_offered minus
                           window_members_formed: the unfilled window slots
                           of the run, an upper bound on pass-tail loss. A
                           stage fills short only when its scan reached the
                           end of the edge list, which can happen mid-pass:
                           retiring a window can arm a position the form
                           scan already passed, opening another stage before
                           the pass ends. The figure is a run total and
                           names no single stage.

  cost_weighted_invalidated_fraction is the study's ceiling predictor. The
  driver computes it as the median repair wall time of the ordered run over
  the median collapse phase wall time of the serial version 1 run (v1-c1),
  both medians over the timed repetitions. The registered predictor names
  the serial version 1 predicate wall time as the denominator. The serial
  path has no separate predicate clock, so this study uses the whole serial
  v1 collapse phase, which is measurable and never smaller than that
  predicate time. The reported fraction is therefore a lower bound on the
  registered one. The substitution is stated here, in
  benchmarks/collapse_scaling_ordered.sh, and in the corpus. The line names the
  denominator it used in cost_weighted_denominator and prints its seconds,
  and the key reads `unavailable` when the run holds no v1-c1 to divide by.
  It is never approximated from counts: repair_fraction_derived counts
  repairs and does not stand in for it. Blocked successful edges, their
  retests, and conflict blocking are not public fields, so those keys print
  `unavailable`.
";

pub(super) fn main_entry() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.iter().any(|a| a == "-h" || a == "--help") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    if argv.iter().any(|a| a == "--version") {
        println!("collapse-bench {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    match run(&argv) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("collapse-bench: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(argv: &[String]) -> Result<(), String> {
    let args = parse_args(argv)?;
    let points = read_cloud(&args.input)?;
    if points.len() < 2 {
        return Err(format!("{}: need at least two points", args.input));
    }
    execution::run(&points, &args)
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

impl ArgsBuilder {
    fn accepts(flag: &str) -> bool {
        matches!(
            flag,
            "--input"
                | "--entry"
                | "--threshold"
                | "--max-dim"
                | "--modulus"
                | "--collapse-threads"
                | "--reducer-threads"
                | "--reps"
                | "--mode"
                | "--collapse-input"
        )
    }

    fn set(&mut self, flag: &str, value: &str) -> Result<(), String> {
        if self.set_identity(flag, value) {
            return Ok(());
        }
        if self.set_dimensions(flag, value)? || self.set_threads(flag, value)? {
            return Ok(());
        }
        if self.set_choice(flag, value)? {
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

    fn set_threads(&mut self, flag: &str, value: &str) -> Result<bool, String> {
        if flag == "--collapse-threads" {
            self.collapse_threads = parse_thread_list(value)?;
            return Ok(true);
        }
        let target = match flag {
            "--reducer-threads" => &mut self.reducer_threads,
            "--reps" => &mut self.reps,
            _ => return Ok(false),
        };
        *target = parse_usize(value, flag)?;
        if flag == "--reducer-threads" {
            *target = (*target).max(1);
        }
        Ok(true)
    }

    fn set_choice(&mut self, flag: &str, value: &str) -> Result<bool, String> {
        match flag {
            "--mode" => self.mode = parse_mode(value)?,
            "--collapse-input" => self.collapse_input = parse_input_kind(value)?,
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
        if self.reps == 0 {
            return Err("--reps must be at least 1".to_string());
        }
        let entry = self.entry.unwrap_or_else(|| file_stem(&input));
        Ok(Args {
            input,
            entry,
            threshold,
            threshold_text,
            max_dim: self.max_dim,
            modulus: self.modulus,
            collapse_threads: self.collapse_threads,
            reducer_threads: self.reducer_threads,
            reps: self.reps,
            mode: self.mode,
            collapse_input: self.collapse_input,
        })
    }
}

fn parse_thread_list(text: &str) -> Result<Vec<usize>, String> {
    text.split(',')
        .map(|part| parse_usize(part, "--collapse-threads").map(|value| value.max(1)))
        .collect()
}

fn parse_mode(text: &str) -> Result<Mode, String> {
    const MODES: [(&str, Mode); 7] = [
        ("v2", Mode::V2),
        ("v1", Mode::V1),
        ("v1o", Mode::V1Ordered),
        ("v1p", Mode::V1Product),
        ("none", Mode::NoCollapse),
        ("rounds", Mode::Rounds),
        ("all", Mode::All),
    ];
    MODES
        .iter()
        .find(|(name, _)| *name == text)
        .map(|(_, mode)| *mode)
        .ok_or_else(|| format!("unknown mode {text}; use v2, v1, v1o, v1p, none, rounds, or all"))
}

fn parse_input_kind(text: &str) -> Result<InputKind, String> {
    match text {
        "sparse" => Ok(InputKind::Sparse),
        "dense" => Ok(InputKind::Dense),
        other => Err(format!(
            "unknown collapse input {other}; use sparse or dense"
        )),
    }
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

fn parse_usize(text: &str, flag: &str) -> Result<usize, String> {
    text.trim()
        .parse()
        .map_err(|_| format!("{flag} {text} is not a whole number"))
}

fn read_cloud(path: &str) -> Result<Vec<Vec<f64>>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", file_stem(path)))?;
    let mut points = Vec::new();
    for (lineno, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut point = Vec::new();
        for token in line.replace(',', " ").split_whitespace() {
            let value: f64 = token.parse().map_err(|_| {
                format!(
                    "{} line {}: bad number {token}",
                    file_stem(path),
                    lineno + 1
                )
            })?;
            point.push(value);
        }
        points.push(point);
    }
    Ok(points)
}

/// The file name without its extension. Recorded output carries no absolute
/// path, as benchmarks/_common.sh requires.
pub(super) fn file_stem(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map_or_else(|| path.to_string(), |s| s.to_string_lossy().into_owned())
}
