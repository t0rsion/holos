//! Phase-separated driver for the collapse scaling studies: the v0.6
//! ordered speculative collapse, with the v0.5 version 2 rounds kept as
//! historical context.
//!
//! The driver loads one point cloud and runs every requested pipeline
//! configuration in this process. Each phase carries its own clock, so no
//! number comes from subtracting one command line from another. Every
//! configuration computes its diagram once before any timing starts. The
//! diagrams must agree bar for bar, or the run aborts. The ordered
//! configurations face a second gate: their collapsed matrix and
//! certificate must equal the serial version 1 run's.
//!
//! The timed repetitions are counterbalanced: they interleave the
//! configurations instead of running one configuration to exhaustion.
//!
//! Output is one key=value line per record. benchmarks/collapse_scaling_ordered.sh
//! parses it; the fields below are its interface.

#![forbid(unsafe_code)]

use std::fs;
use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use holos_tda::collapse::{
    CollapseCertificate, CollapsedRips, collapse_dense, collapse_dense_ordered_parallel,
    collapse_dense_rounds_parallel, collapse_sparse, collapse_sparse_ordered_parallel,
    collapse_sparse_rounds_parallel,
};
use holos_tda::{
    CollapseSchedule, Diagram, DistanceMatrix, RipsParams, SparseDistanceMatrix, rips_persistence,
    rips_persistence_sparse,
};

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
serial version 1 result exists before the ordered gate needs it. The timed
repetitions then run counterbalanced.

Shipped pipeline against the phase split:
  holos_tda::rips_persistence with RipsParams::collapse_edges set and the
  ordered collapse schedule selected is the shipped pipeline: one run-wide
  pool drives the ordered collapse and then the reduction. It takes one
  worker count for both halves and exposes no phase boundary, so no outside
  clock can split it without running something else.
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
  them starting at position r of the agreement order and wraps around, so
  the configuration at position i of repetition r is configuration
  (i + r) mod m of that order. Each configuration moves one place earlier
  every repetition, and drift or a thermal ramp cannot land on one
  configuration alone. The rotation is balanced, every configuration in
  every position equally often, only when the repetition count is a
  multiple of m. The kind=entry line reports balanced=yes or balanced=no,
  and a registered run needs yes. The rotation is deterministic, and each
  repetition prints its exact order as a kind=order line.

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
  The gate needs v1-c1 in the same run: under --mode v1o it reports
  checked=no and only the diagram comparison applies.
  The shipped path returns a diagram and no certificate, so v1p faces the
  diagram comparison alone.

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

/// Which collapse a configuration runs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    V2,
    V1,
    V1Ordered,
    /// The shipped pipeline: one run-wide pool for the ordered collapse and
    /// the reduction, through `rips_persistence` with `collapse_edges` and
    /// the ordered schedule selected.
    V1Product,
    NoCollapse,
}

/// Which entry point the collapse and the no-collapse baseline read.
#[derive(Clone, Copy, PartialEq, Eq)]
enum InputKind {
    Sparse,
    Dense,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    V2,
    V1,
    V1Ordered,
    V1Product,
    NoCollapse,
    /// The rounds study set: v2-cN, v1-c1, none.
    Rounds,
    All,
}

struct Args {
    input: String,
    entry: String,
    threshold: f64,
    threshold_text: String,
    max_dim: usize,
    modulus: u32,
    collapse_threads: Vec<usize>,
    reducer_threads: usize,
    reps: usize,
    mode: Mode,
    collapse_input: InputKind,
}

struct Config {
    name: String,
    kind: Kind,
    collapse_threads: usize,
}

/// One pipeline run: the phase clocks plus what the run produced.
struct Outcome {
    phases: Vec<(&'static str, f64)>,
    diagram: Diagram,
    counters: Option<Counters>,
    graph_edges: Option<usize>,
    /// The collapse result itself, kept only by the agreement runs that
    /// the ordered gate compares. A timed repetition drops it.
    collapsed: Option<CollapsedRips>,
}

/// Collapse counters and clocks of one run, from CollapseStats,
/// CollapseTimings, and the certificate.
struct Counters {
    algorithm_version: u32,
    terminal_level: f64,
    input_edges: usize,
    output_edges: usize,
    removed_edges: usize,
    epochs: usize,
    edge_tests: usize,
    logical_tests: usize,
    invalidated_results: usize,
    global_invalidations: usize,
    window_batches: usize,
    window_slots_offered: usize,
    window_members_formed: usize,
    window_members_reused: usize,
    witness_segments: usize,
    max_common_neighborhood: usize,
    /// Nanoseconds in the parallel test phase. Zero where the path does not
    /// clock its subphases.
    predicate_ns: u64,
    /// Nanoseconds in the serial retirement walk, repairs included.
    retirement_ns: u64,
    /// Nanoseconds re-evaluating invalidated verdicts.
    repair_ns: u64,
    /// Removals per epoch, as (epoch, width), for the epochs that removed
    /// something.
    batch_widths: Vec<(usize, usize)>,
}

struct Summary {
    median: f64,
    iqr: f64,
    q1: f64,
    q3: f64,
    min: f64,
    max: f64,
}

fn main() -> ExitCode {
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
    let configs = configurations(&args);

    // Agreement first. A timed repetition never runs before every
    // configuration has produced its diagram and matched the reference.
    let mut verified: Vec<Outcome> = Vec::with_capacity(configs.len());
    let mut serial_v1: Option<CollapsedRips> = None;
    let mut gate_lines: Vec<String> = Vec::new();
    for cfg in &configs {
        let mut outcome = run_pipeline(&points, &args, cfg, true)?;
        if let Some(first) = verified.first() {
            if !diagrams_equal(&first.diagram, &outcome.diagram) {
                return Err(format!(
                    "entry {}: configuration {} disagrees with {} bar for bar ({} bars vs {}); timings void",
                    args.entry,
                    cfg.name,
                    configs[0].name,
                    outcome.diagram.bars.len(),
                    first.diagram.bars.len()
                ));
            }
        }
        let collapsed = outcome.collapsed.take();
        match cfg.kind {
            Kind::V1 => serial_v1 = collapsed,
            Kind::V1Ordered => {
                let ordered = collapsed.ok_or_else(|| {
                    format!("configuration {} produced no collapse result", cfg.name)
                })?;
                match &serial_v1 {
                    Some(reference) => {
                        ordered_equal(reference, &ordered).map_err(|diff| {
                            format!(
                                "entry {}: {} does not reproduce v1-c1: {diff}; timings void",
                                args.entry, cfg.name
                            )
                        })?;
                        gate_lines.push(format!(
                            "kind=ordered_gate entry={} config={} reference=v1-c1 checked=yes matrix=match certificate=match counters=match",
                            args.entry, cfg.name
                        ));
                    }
                    None => {
                        return Err(format!(
                            "entry {}: configuration {} has no v1-c1 reference in this run, so its ordered gate cannot run; add v1 to --mode",
                            args.entry, cfg.name
                        ));
                    }
                }
            }
            Kind::V2 | Kind::V1Product | Kind::NoCollapse => {}
        }
        verified.push(outcome);
    }
    // The gate has run, so the reference result is dead. Free it before
    // the timed runs, which must not allocate on top of it.
    drop(serial_v1);

    let hwm_start = vm_hwm_kb();
    let rotation = rotation_orders(configs.len(), args.reps);
    print_header(&args, points.len(), &configs, &verified, &rotation);
    for line in &gate_lines {
        println!("{line}");
    }

    // Counterbalanced timing: each repetition walks the configurations from
    // its own starting point, so no configuration owns the end of the run.
    let mut samples: Vec<Vec<Vec<f64>>> = verified
        .iter()
        .map(|verify| vec![Vec::with_capacity(args.reps); verify.phases.len()])
        .collect();
    // Counters and clocks come from the timed repetitions, never from the
    // agreement run: that run's schedule is the same, but its clocks are one
    // cold sample.
    let mut counter_runs: Vec<Vec<Counters>> = configs
        .iter()
        .map(|_| Vec::with_capacity(args.reps))
        .collect();
    for (rep, order) in rotation.iter().enumerate() {
        for &index in order {
            let cfg = &configs[index];
            let mut outcome = run_pipeline(&points, &args, cfg, false)?;
            // The clocks have stopped; comparing the repetition's diagram
            // against the verified reference costs no timed work and turns
            // a mid-run corruption into a hard failure instead of a timing.
            if !diagrams_equal(&outcome.diagram, &verified[index].diagram) {
                return Err(format!(
                    "config {} rep {rep}: diagram differs from the agreement run",
                    cfg.name
                ));
            }
            for (slot, (_, seconds)) in samples[index].iter_mut().zip(&outcome.phases) {
                slot.push(*seconds);
            }
            if let Some(counters) = outcome.counters.take() {
                counter_runs[index].push(counters);
            }
        }
    }

    // The ceiling predictor divides by the serial version 1 collapse clock of
    // this same run. Read it before the report loop sorts the samples.
    let serial_collapse_s = serial_collapse_median(&configs, &verified, &samples);

    for (((cfg, verify), values), runs) in configs
        .iter()
        .zip(&verified)
        .zip(&mut samples)
        .zip(&counter_runs)
    {
        for ((name, _), phase_samples) in verify.phases.iter().zip(values) {
            let summary = summarize(phase_samples);
            println!(
                "kind=phase entry={} config={} mode={} collapse_threads={} reducer_threads={} phase={} reps={} median_s={:.6} iqr_s={:.6} q1_s={:.6} q3_s={:.6} min_s={:.6} max_s={:.6}",
                args.entry,
                cfg.name,
                mode_name(cfg.kind),
                cfg.collapse_threads,
                reducer_threads(&args, cfg),
                name,
                args.reps,
                summary.median,
                summary.iqr,
                summary.q1,
                summary.q3,
                summary.min,
                summary.max
            );
        }
        print_counters(&args, cfg, runs, serial_collapse_s);
    }
    // One mark for the process. The repetitions interleave, so no
    // configuration can claim it.
    println!(
        "kind=memory entry={} config=all vm_hwm_kb={} vm_hwm_kb_at_start={} scope=process_high_water",
        args.entry,
        report_kb(vm_hwm_kb()),
        report_kb(hwm_start)
    );
    Ok(())
}

/// The counterbalancing rotation: repetition r starts at configuration r and
/// wraps around. Deterministic, and printed with the record.
fn rotation_orders(configs: usize, reps: usize) -> Vec<Vec<usize>> {
    (0..reps)
        .map(|rep| (0..configs).map(|i| (i + rep) % configs.max(1)).collect())
        .collect()
}

fn print_header(
    args: &Args,
    points: usize,
    configs: &[Config],
    verified: &[Outcome],
    rotation: &[Vec<usize>],
) {
    let names: Vec<&str> = configs.iter().map(|c| c.name.as_str()).collect();
    let product = configs
        .iter()
        .find(|c| c.kind == Kind::V1Product)
        .map_or("none", |c| c.name.as_str());
    println!(
        "# collapse-bench {} phase-separated collapse and reduction timing",
        env!("CARGO_PKG_VERSION")
    );
    println!(
        "# one record per line, space-separated key=value; run with --help for the field rules"
    );
    println!("# vm_hwm_kb is the whole-process high-water mark and belongs to no configuration");
    println!(
        "# every counter and clock on a kind=counters line is a median over the timed repetitions"
    );
    println!(
        "# the end-to-end ratio reads the shipped pipeline, the v1p configuration, not the phase split"
    );
    println!(
        "kind=entry entry={} input={} points={} threshold={} max_dim={} modulus={} collapse_input={} reducer_threads={} reps={} configs={} graph_edges={} end_to_end_config={} order_scheme=cyclic_rotation balanced={}",
        args.entry,
        file_stem(&args.input),
        points,
        args.threshold_text,
        args.max_dim,
        args.modulus,
        match args.collapse_input {
            InputKind::Sparse => "sparse",
            InputKind::Dense => "dense",
        },
        args.reducer_threads,
        args.reps,
        names.join(","),
        verified
            .first()
            .and_then(|o| o.graph_edges)
            .map_or("unavailable".to_string(), |e| e.to_string()),
        product,
        if args.reps % configs.len().max(1) == 0 {
            "yes"
        } else {
            "no"
        }
    );
    for cfg in configs {
        let registered_arm = match cfg.kind {
            Kind::V1 => "yes",
            Kind::V1Product => {
                if cfg.collapse_threads == args.reducer_threads {
                    "yes"
                } else {
                    "no"
                }
            }
            _ => "n/a",
        };
        println!(
            "kind=config entry={} config={} mode={} role={} thread_pools={} collapse_threads={} reducer_threads={} end_to_end={} registered_arm={}",
            args.entry,
            cfg.name,
            mode_name(cfg.kind),
            role_name(cfg.kind),
            thread_pools(args, cfg),
            cfg.collapse_threads,
            reducer_threads(args, cfg),
            if cfg.kind == Kind::V1Product || cfg.kind == Kind::V1 {
                "yes"
            } else {
                "no"
            },
            registered_arm
        );
    }
    for (rep, order) in rotation.iter().enumerate() {
        let names: Vec<&str> = order.iter().map(|&i| configs[i].name.as_str()).collect();
        println!(
            "kind=order entry={} rep={} scheme=cyclic_rotation order={}",
            args.entry,
            rep,
            names.join(",")
        );
    }
    for (cfg, outcome) in configs.iter().zip(verified) {
        println!(
            "kind=diagram entry={} config={} bars={} reference={} match=yes",
            args.entry,
            cfg.name,
            outcome.diagram.bars.len(),
            configs[0].name
        );
    }
}

/// One kind=counters line and one kind=batch_widths line for a
/// configuration, over its timed repetitions. `serial_collapse_s` is the
/// median collapse phase of v1-c1 in this run, the denominator of the
/// ceiling predictor.
fn print_counters(args: &Args, cfg: &Config, runs: &[Counters], serial_collapse_s: Option<f64>) {
    let Some(first) = runs.first() else {
        return;
    };
    // The batch widths come from the certificate, which the ordered gate has
    // already matched against the serial run, so one repetition carries them.
    // counters_stable reports whether the repetitions agreed on every count.
    let widths: Vec<f64> = first.batch_widths.iter().map(|&(_, w)| w as f64).collect();
    let (width_min, width_max, width_mean, width_median) = if widths.is_empty() {
        (0.0, 0.0, 0.0, 0.0)
    } else {
        let mut sorted = widths.clone();
        let summary = summarize(&mut sorted);
        (
            summary.min,
            summary.max,
            widths.iter().sum::<f64>() / widths.len() as f64,
            summary.median,
        )
    };
    let reference = count_vector(first);
    let stable = runs.iter().all(|run| count_vector(run) == reference);

    let median = |pick: fn(&Counters) -> f64| counter_median(runs, pick);
    let input_edges = median(|c| c.input_edges as f64);
    let edge_tests = median(|c| c.edge_tests as f64);
    let logical_tests = median(|c| c.logical_tests as f64);
    let invalidated_results = median(|c| c.invalidated_results as f64);
    let offered = median(|c| c.window_slots_offered as f64);
    let formed = median(|c| c.window_members_formed as f64);
    // unused_window_capacity_derived is offered minus formed: an upper
    // bound on pass-tail loss, not the loss. A stage fills short only when
    // its scan reached the end of the edge list, which can happen mid-pass
    // when retiring a window arms a position the form scan already passed.
    // The counters are run totals, so this names no single stage.
    let (occupancy, unused_capacity) = if offered > 0.0 {
        (
            format!("{:.4}", formed / offered),
            count_text(offered - formed),
        )
    } else {
        ("unavailable".to_string(), "unavailable".to_string())
    };

    // Only the ordered path clocks its subphases, and only when it really
    // speculates: at one worker it delegates to the serial collapse, which
    // has no stages and returns zero timings. Elsewhere the timings are
    // zero because nothing clocked them, and a printed zero would read as
    // a measurement.
    let clocked = cfg.kind == Kind::V1Ordered && cfg.collapse_threads > 1;
    let repair_s = median(|c| c.repair_ns as f64) / 1e9;
    let (subphases, predicate_text, retirement_text, repair_text) = if clocked {
        (
            "predicate,retirement,repair",
            format!("{:.6}", median(|c| c.predicate_ns as f64) / 1e9),
            format!("{:.6}", median(|c| c.retirement_ns as f64) / 1e9),
            format!("{repair_s:.6}"),
        )
    } else {
        (
            "none_on_this_path",
            "unavailable".to_string(),
            "unavailable".to_string(),
            "unavailable".to_string(),
        )
    };
    let (cost_weighted, denominator, denominator_s) = match (clocked, serial_collapse_s) {
        (true, Some(seconds)) if seconds > 0.0 => (
            format!("{:.6}", repair_s / seconds),
            "v1-c1_collapse_phase_median_s",
            format!("{seconds:.6}"),
        ),
        (true, _) => (
            "unavailable".to_string(),
            "unavailable_no_v1-c1_in_this_run",
            "unavailable".to_string(),
        ),
        (false, _) => (
            "unavailable".to_string(),
            "unavailable_this_path_has_no_repair_clock",
            "unavailable".to_string(),
        ),
    };

    println!(
        "kind=counters entry={} config={} mode={} collapse_threads={} stat=median_over_timed_reps reps={} counters_stable={} algorithm_version={} terminal_level={:?} input_edges={} output_edges={} removed_edges={} epochs={} removal_epochs={} edge_tests={} logical_tests={} work_inflation_derived={} retests_derived={} invalidated_results={} repair_fraction_derived={} global_invalidations={} window_batches={} window_slots_offered={} window_members_formed={} window_members_reused={} window_occupancy_derived={} unused_window_capacity_derived={} witness_segments={} max_common_neighborhood={} batch_width_min={} batch_width_median={:.2} batch_width_mean={:.2} batch_width_max={} collapse_subphases={} predicate_median_s={} retirement_median_s={} repair_median_s={} cost_weighted_invalidated_fraction={} cost_weighted_denominator={} cost_weighted_denominator_s={} blocked_successful=unavailable blocked_successful_retests=unavailable conflict_blocking=unavailable",
        args.entry,
        cfg.name,
        mode_name(cfg.kind),
        cfg.collapse_threads,
        runs.len(),
        if stable { "yes" } else { "no" },
        first.algorithm_version,
        first.terminal_level,
        count_text(input_edges),
        count_text(median(|c| c.output_edges as f64)),
        count_text(median(|c| c.removed_edges as f64)),
        count_text(median(|c| c.epochs as f64)),
        first.batch_widths.len(),
        count_text(edge_tests),
        count_text(logical_tests),
        per_logical_test(edge_tests, logical_tests),
        count_text((logical_tests - input_edges).max(0.0)),
        count_text(invalidated_results),
        per_logical_test(invalidated_results, logical_tests),
        count_text(median(|c| c.global_invalidations as f64)),
        count_text(median(|c| c.window_batches as f64)),
        count_text(offered),
        count_text(formed),
        count_text(median(|c| c.window_members_reused as f64)),
        occupancy,
        unused_capacity,
        count_text(median(|c| c.witness_segments as f64)),
        count_text(median(|c| c.max_common_neighborhood as f64)),
        width_min as usize,
        width_median,
        width_mean,
        width_max as usize,
        subphases,
        predicate_text,
        retirement_text,
        repair_text,
        cost_weighted,
        denominator,
        denominator_s
    );
    let widths: Vec<String> = first
        .batch_widths
        .iter()
        .map(|&(epoch, width)| format!("{epoch}:{width}"))
        .collect();
    println!(
        "kind=batch_widths entry={} config={} epochs={} widths={}",
        args.entry,
        cfg.name,
        first.epochs,
        if widths.is_empty() {
            "none".to_string()
        } else {
            widths.join(",")
        }
    );
}

/// The median of one counter or clock over the timed repetitions, by the
/// rule the phase medians use.
fn counter_median(runs: &[Counters], pick: fn(&Counters) -> f64) -> f64 {
    let mut values: Vec<f64> = runs.iter().map(pick).collect();
    summarize(&mut values).median
}

/// A counter median as text. Whole where the repetitions agree, which the
/// deterministic schedule makes the normal case.
fn count_text(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    }
}

/// The count fields of one run as one vector, for the repetition to
/// repetition stability check. The clocks stay out: they vary by design.
fn count_vector(c: &Counters) -> Vec<usize> {
    let mut values = vec![
        c.input_edges,
        c.output_edges,
        c.removed_edges,
        c.epochs,
        c.edge_tests,
        c.logical_tests,
        c.invalidated_results,
        c.global_invalidations,
        c.window_batches,
        c.window_slots_offered,
        c.window_members_formed,
        c.window_members_reused,
        c.witness_segments,
        c.max_common_neighborhood,
        c.algorithm_version as usize,
        c.terminal_level.to_bits() as usize,
    ];
    for &(epoch, width) in &c.batch_widths {
        values.push(epoch);
        values.push(width);
    }
    values
}

/// The median collapse phase of the serial version 1 configuration, over the
/// timed repetitions. It is the denominator of
/// cost_weighted_invalidated_fraction, and it is absent when the run holds no
/// v1-c1.
fn serial_collapse_median(
    configs: &[Config],
    verified: &[Outcome],
    samples: &[Vec<Vec<f64>>],
) -> Option<f64> {
    let index = configs.iter().position(|c| c.kind == Kind::V1)?;
    let phase = verified[index]
        .phases
        .iter()
        .position(|&(name, _)| name == "collapse")?;
    let mut values = samples[index][phase].clone();
    Some(summarize(&mut values).median)
}

/// A count per logical test, or `unavailable` when the path reports no
/// logical tests.
fn per_logical_test(count: f64, logical_tests: f64) -> String {
    if logical_tests <= 0.0 {
        return "unavailable".to_string();
    }
    format!("{:.4}", count / logical_tests)
}

/// One full pipeline run of one configuration, phase by phase. `retain`
/// keeps the collapse result for the ordered gate.
fn run_pipeline(
    points: &[Vec<f64>],
    args: &Args,
    cfg: &Config,
    retain: bool,
) -> Result<Outcome, String> {
    let mut phases: Vec<(&'static str, f64)> = Vec::with_capacity(5);
    let whole = Instant::now();

    let clock = Instant::now();
    let dist = DistanceMatrix::from_points(points).map_err(|e| e.to_string())?;
    phases.push(("distance", clock.elapsed().as_secs_f64()));

    let mut graph_edges = None;
    let sparse = if args.collapse_input == InputKind::Sparse {
        let clock = Instant::now();
        let sparse = threshold_to_sparse(&dist, args.threshold)?;
        phases.push(("graph", clock.elapsed().as_secs_f64()));
        graph_edges = Some(sparse.num_edges());
        Some(sparse)
    } else {
        None
    };

    if cfg.kind == Kind::V1Product {
        // The shipped path: one call, one run-wide pool for the collapse and
        // the reduction. It has no phase boundary an outside clock can see.
        let params = RipsParams::new(args.max_dim)
            .with_threshold(args.threshold)
            .with_modulus(args.modulus)
            .with_threads(cfg.collapse_threads)
            .with_collapse_schedule(CollapseSchedule::Ordered);
        let clock = Instant::now();
        let mut diagram = match &sparse {
            Some(sparse) => rips_persistence_sparse(sparse, &params),
            None => rips_persistence(&dist, &params),
        }
        .map_err(|e| e.to_string())?;
        phases.push(("pipeline", clock.elapsed().as_secs_f64()));
        phases.push(("total", whole.elapsed().as_secs_f64()));
        diagram.canonicalize();
        return Ok(Outcome {
            phases,
            diagram,
            counters: None,
            graph_edges,
            collapsed: None,
        });
    }

    let mut counters = None;
    let mut collapsed: Option<CollapsedRips> = None;
    if cfg.kind != Kind::NoCollapse {
        let threshold = Some(args.threshold);
        let clock = Instant::now();
        let result = match (cfg.kind, &sparse) {
            (Kind::V2, Some(sparse)) => {
                collapse_sparse_rounds_parallel(sparse, threshold, cfg.collapse_threads)
            }
            (Kind::V1, Some(sparse)) => collapse_sparse(sparse, threshold),
            (Kind::V1Ordered, Some(sparse)) => {
                collapse_sparse_ordered_parallel(sparse, threshold, cfg.collapse_threads)
            }
            (Kind::V2, None) => {
                collapse_dense_rounds_parallel(&dist, threshold, cfg.collapse_threads)
            }
            (Kind::V1, None) => collapse_dense(&dist, threshold),
            (Kind::V1Ordered, None) => {
                collapse_dense_ordered_parallel(&dist, threshold, cfg.collapse_threads)
            }
            (Kind::V1Product | Kind::NoCollapse, _) => {
                unreachable!("both take a branch above")
            }
        };
        let result = result.map_err(|e| e.to_string())?;
        phases.push(("collapse", clock.elapsed().as_secs_f64()));
        counters = Some(counters_of(&result));
        collapsed = Some(result);
    }

    let clock = Instant::now();
    let mut diagram = match (&collapsed, &sparse) {
        (Some(collapsed), _) => {
            let params = reduce_params(args, collapsed.certificate.terminal_level());
            rips_persistence_sparse(&collapsed.matrix, &params)
        }
        (None, Some(sparse)) => {
            rips_persistence_sparse(sparse, &reduce_params(args, args.threshold))
        }
        (None, None) => rips_persistence(&dist, &reduce_params(args, args.threshold)),
    }
    .map_err(|e| e.to_string())?;
    phases.push(("reduce", clock.elapsed().as_secs_f64()));
    phases.push(("total", whole.elapsed().as_secs_f64()));
    diagram.canonicalize();

    Ok(Outcome {
        phases,
        diagram,
        counters,
        graph_edges,
        collapsed: if retain { collapsed } else { None },
    })
}

fn reduce_params(args: &Args, threshold: f64) -> RipsParams {
    RipsParams::new(args.max_dim)
        .with_threshold(threshold)
        .with_modulus(args.modulus)
        .with_threads(args.reducer_threads)
}

fn counters_of(collapsed: &CollapsedRips) -> Counters {
    let stats = &collapsed.stats;
    let mut batch_widths: Vec<(usize, usize)> = Vec::new();
    for step in collapsed.certificate.steps() {
        let epoch = step.position().number();
        match batch_widths.last_mut() {
            Some(last) if last.0 == epoch => last.1 += 1,
            _ => batch_widths.push((epoch, 1)),
        }
    }
    Counters {
        algorithm_version: collapsed.certificate.algorithm_version(),
        terminal_level: collapsed.certificate.terminal_level(),
        input_edges: stats.input_edges,
        output_edges: stats.output_edges,
        removed_edges: stats.removed_edges,
        epochs: stats.epochs,
        edge_tests: stats.edge_tests,
        logical_tests: stats.logical_tests,
        invalidated_results: stats.invalidated_results,
        global_invalidations: stats.global_invalidations,
        window_batches: stats.window_batches,
        window_slots_offered: stats.window_slots_offered,
        window_members_formed: stats.window_members_formed,
        window_members_reused: stats.window_members_reused,
        witness_segments: stats.witness_segments,
        max_common_neighborhood: stats.max_common_neighborhood,
        predicate_ns: collapsed.timings.predicate_ns,
        retirement_ns: collapsed.timings.retirement_ns,
        repair_ns: collapsed.timings.repair_ns,
        batch_widths,
    }
}

/// The thresholded graph of a dense matrix, as the collapse and the
/// no-collapse baseline both see it.
fn threshold_to_sparse(
    dist: &DistanceMatrix,
    threshold: f64,
) -> Result<SparseDistanceMatrix, String> {
    let n = dist.len();
    let mut triplets: Vec<(usize, usize, f64)> = Vec::new();
    for i in 1..n {
        for j in 0..i {
            let d = dist.get(i, j);
            if d.is_finite() && d <= threshold {
                triplets.push((i, j, d));
            }
        }
    }
    SparseDistanceMatrix::from_triplets(n, &triplets).map_err(|e| e.to_string())
}

fn configurations(args: &Args) -> Vec<Config> {
    let mut configs = Vec::new();
    if matches!(args.mode, Mode::V2 | Mode::Rounds | Mode::All) {
        for &threads in &args.collapse_threads {
            configs.push(Config {
                name: format!("v2-c{threads}"),
                kind: Kind::V2,
                collapse_threads: threads,
            });
        }
    }
    if matches!(args.mode, Mode::V1 | Mode::Rounds | Mode::All) {
        configs.push(Config {
            name: "v1-c1".to_string(),
            kind: Kind::V1,
            collapse_threads: 1,
        });
    }
    // The ordered configurations follow v1-c1, so the gate always has its
    // reference when an ordered result arrives.
    if matches!(args.mode, Mode::V1Ordered | Mode::All) {
        for &threads in &args.collapse_threads {
            configs.push(Config {
                name: format!("v1o-c{threads}"),
                kind: Kind::V1Ordered,
                collapse_threads: threads,
            });
        }
    }
    // One shipped-pipeline configuration, at the last collapse thread count.
    // The end-to-end comparison is defined at P alone, and the shipped path
    // costs a whole pipeline per repetition.
    if matches!(args.mode, Mode::V1Product | Mode::All) {
        let threads = *args.collapse_threads.last().unwrap_or(&1);
        configs.push(Config {
            name: format!("v1p-c{threads}"),
            kind: Kind::V1Product,
            collapse_threads: threads,
        });
    }
    if matches!(args.mode, Mode::NoCollapse | Mode::Rounds | Mode::All) {
        configs.push(Config {
            name: "none".to_string(),
            kind: Kind::NoCollapse,
            collapse_threads: 0,
        });
    }
    configs
}

fn mode_name(kind: Kind) -> &'static str {
    match kind {
        Kind::V2 => "v2",
        Kind::V1 => "v1",
        Kind::V1Ordered => "v1o",
        Kind::V1Product => "v1p",
        Kind::NoCollapse => "none",
    }
}

/// What a configuration is for. Only the product configuration feeds the
/// end-to-end ratio.
fn role_name(kind: Kind) -> &'static str {
    match kind {
        Kind::V2 => "context",
        Kind::V1 => "baseline",
        Kind::V1Ordered => "diagnostic",
        Kind::V1Product => "product",
        Kind::NoCollapse => "reference",
    }
}

/// Reduction workers of a configuration. The shipped path runs one pool for
/// the whole pipeline, so its reducer is as wide as its collapse.
fn reducer_threads(args: &Args, cfg: &Config) -> usize {
    match cfg.kind {
        Kind::V1Product => cfg.collapse_threads,
        _ => args.reducer_threads,
    }
}

/// Thread pools a configuration builds. The version 2 and ordered
/// configurations build one for the collapse and one for the reduction; the
/// shipped path shares a single pool; a serial collapse builds none of its
/// own.
fn thread_pools(args: &Args, cfg: &Config) -> usize {
    let reducer = usize::from(reducer_threads(args, cfg) > 1);
    match cfg.kind {
        Kind::V2 | Kind::V1Ordered => usize::from(cfg.collapse_threads > 1) + reducer,
        Kind::V1Product => usize::from(cfg.collapse_threads > 1),
        Kind::V1 | Kind::NoCollapse => reducer,
    }
}

/// The ordered gate. An ordered run executes the version 1 schedule, so it
/// must reproduce the serial version 1 run: the same collapsed matrix, the
/// same certificate, and the counters the schedule fixes. The first
/// difference found is the error.
fn ordered_equal(reference: &CollapsedRips, ordered: &CollapsedRips) -> Result<(), String> {
    matrices_equal(&reference.matrix, &ordered.matrix)?;
    certificates_equal(&reference.certificate, &ordered.certificate)?;
    let (want, got) = (&reference.stats, &ordered.stats);
    let fields = [
        ("input_edges", want.input_edges, got.input_edges),
        ("output_edges", want.output_edges, got.output_edges),
        ("removed_edges", want.removed_edges, got.removed_edges),
        ("passes", want.epochs, got.epochs),
        (
            "witness_segments",
            want.witness_segments,
            got.witness_segments,
        ),
        ("logical_tests", want.edge_tests, got.logical_tests),
    ];
    for (name, want, got) in fields {
        if want != got {
            return Err(format!("{name} is {got}, the serial run has {want}"));
        }
    }
    Ok(())
}

fn matrices_equal(
    reference: &SparseDistanceMatrix,
    ordered: &SparseDistanceMatrix,
) -> Result<(), String> {
    if reference.len() != ordered.len() {
        return Err(format!(
            "the collapsed matrix has {} vertices, the serial run has {}",
            ordered.len(),
            reference.len()
        ));
    }
    if reference.num_edges() != ordered.num_edges() {
        return Err(format!(
            "the collapsed matrix has {} edges, the serial run has {}",
            ordered.num_edges(),
            reference.num_edges()
        ));
    }
    for (index, (want, got)) in reference.edges().zip(ordered.edges()).enumerate() {
        if want.0 != got.0 || want.1 != got.1 || want.2.to_bits() != got.2.to_bits() {
            return Err(format!(
                "collapsed edge {index} is ({}, {}, {:?}), the serial run has ({}, {}, {:?})",
                got.0, got.1, got.2, want.0, want.1, want.2
            ));
        }
    }
    Ok(())
}

fn certificates_equal(
    reference: &CollapseCertificate,
    ordered: &CollapseCertificate,
) -> Result<(), String> {
    if reference.algorithm_version() != ordered.algorithm_version() {
        return Err(format!(
            "the certificate is algorithm version {}, the serial run is version {}",
            ordered.algorithm_version(),
            reference.algorithm_version()
        ));
    }
    let header = [
        (
            "vertex_count",
            reference.vertex_count(),
            ordered.vertex_count(),
        ),
        (
            "input_edge_count",
            reference.input_edge_count(),
            ordered.input_edge_count(),
        ),
        (
            "output_edge_count",
            reference.output_edge_count(),
            ordered.output_edge_count(),
        ),
        ("steps", reference.steps().len(), ordered.steps().len()),
    ];
    for (name, want, got) in header {
        if want != got {
            return Err(format!(
                "the certificate {name} is {got}, the serial run has {want}"
            ));
        }
    }
    if optional_bits(reference.requested_threshold())
        != optional_bits(ordered.requested_threshold())
    {
        return Err(format!(
            "the certificate requested threshold is {:?}, the serial run has {:?}",
            ordered.requested_threshold(),
            reference.requested_threshold()
        ));
    }
    if reference.terminal_level().to_bits() != ordered.terminal_level().to_bits() {
        return Err(format!(
            "the certificate terminal level is {:?}, the serial run has {:?}",
            ordered.terminal_level(),
            reference.terminal_level()
        ));
    }
    for (index, (want, got)) in reference.steps().iter().zip(ordered.steps()).enumerate() {
        if want.edge() != got.edge() {
            return Err(format!(
                "removal {index} is edge {:?}, the serial run removes {:?}",
                got.edge(),
                want.edge()
            ));
        }
        if want.value().to_bits() != got.value().to_bits() {
            return Err(format!(
                "removal {index} has value {:?}, the serial run has {:?}",
                got.value(),
                want.value()
            ));
        }
        if want.position().number() != got.position().number() {
            return Err(format!(
                "removal {index} is in pass {}, the serial run puts it in pass {}",
                got.position().number(),
                want.position().number()
            ));
        }
        if want.witnesses().len() != got.witnesses().len() {
            return Err(format!(
                "removal {index} has {} witness segments, the serial run has {}",
                got.witnesses().len(),
                want.witnesses().len()
            ));
        }
        for (segment, (want, got)) in want.witnesses().iter().zip(got.witnesses()).enumerate() {
            if want.0.to_bits() != got.0.to_bits() || want.1 != got.1 {
                return Err(format!(
                    "removal {index} witness segment {segment} is {:?}, the serial run has {:?}",
                    got, want
                ));
            }
        }
    }
    Ok(())
}

fn optional_bits(value: Option<f64>) -> Option<u64> {
    value.map(f64::to_bits)
}

fn diagrams_equal(a: &Diagram, b: &Diagram) -> bool {
    a.bars.len() == b.bars.len()
        && a.bars.iter().zip(&b.bars).all(|(x, y)| {
            x.dim == y.dim
                && x.birth.to_bits() == y.birth.to_bits()
                && x.death.to_bits() == y.death.to_bits()
        })
}

/// Quantiles interpolate linearly between the two neighbouring order
/// statistics, the rule benchmarks/_common.sh uses.
fn summarize(samples: &mut [f64]) -> Summary {
    if samples.is_empty() {
        return Summary {
            median: 0.0,
            iqr: 0.0,
            q1: 0.0,
            q3: 0.0,
            min: 0.0,
            max: 0.0,
        };
    }
    samples.sort_by(|a, b| a.total_cmp(b));
    let q1 = quantile(samples, 0.25);
    let q3 = quantile(samples, 0.75);
    Summary {
        median: quantile(samples, 0.5),
        iqr: q3 - q1,
        q1,
        q3,
        min: samples[0],
        max: samples[samples.len() - 1],
    }
}

fn quantile(sorted: &[f64], p: f64) -> f64 {
    let n = sorted.len();
    let h = (n as f64 - 1.0) * p;
    let lo = h.floor() as usize;
    let frac = h - lo as f64;
    if lo + 2 > n {
        return sorted[n - 1];
    }
    sorted[lo] + frac * (sorted[lo + 1] - sorted[lo])
}

fn vm_hwm_kb() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    status.lines().find_map(|line| {
        let rest = line.strip_prefix("VmHWM:")?;
        rest.split_whitespace().next()?.parse().ok()
    })
}

fn report_kb(kb: Option<u64>) -> String {
    kb.map_or("unavailable".to_string(), |kb| kb.to_string())
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
fn file_stem(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map_or_else(|| path.to_string(), |s| s.to_string_lossy().into_owned())
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut input = None;
    let mut entry = None;
    let mut threshold_text = None;
    let mut max_dim = 1usize;
    let mut modulus = 2u32;
    let mut collapse_threads = vec![1usize];
    let mut reducer_threads = 1usize;
    let mut reps = 5usize;
    let mut mode = Mode::All;
    let mut collapse_input = InputKind::Sparse;

    let mut it = argv.iter();
    while let Some(flag) = it.next() {
        let mut value = || {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--input" => input = Some(value()?),
            "--entry" => entry = Some(value()?),
            "--threshold" => threshold_text = Some(value()?),
            "--max-dim" => max_dim = parse_usize(&value()?, "--max-dim")?,
            "--modulus" => {
                modulus = u32::try_from(parse_usize(&value()?, "--modulus")?)
                    .map_err(|_| "--modulus is out of range".to_string())?
            }
            "--collapse-threads" => {
                let list = value()?;
                let mut threads = Vec::new();
                for part in list.split(',') {
                    threads.push(parse_usize(part, "--collapse-threads")?.max(1));
                }
                collapse_threads = threads;
            }
            "--reducer-threads" => {
                reducer_threads = parse_usize(&value()?, "--reducer-threads")?.max(1)
            }
            "--reps" => reps = parse_usize(&value()?, "--reps")?,
            "--mode" => {
                let name = value()?;
                mode = match name.as_str() {
                    "v2" => Mode::V2,
                    "v1" => Mode::V1,
                    "v1o" => Mode::V1Ordered,
                    "v1p" => Mode::V1Product,
                    "none" => Mode::NoCollapse,
                    "rounds" => Mode::Rounds,
                    "all" => Mode::All,
                    other => {
                        return Err(format!(
                            "unknown mode {other}; use v2, v1, v1o, v1p, none, rounds, or all"
                        ));
                    }
                }
            }
            "--collapse-input" => {
                let name = value()?;
                collapse_input = match name.as_str() {
                    "sparse" => InputKind::Sparse,
                    "dense" => InputKind::Dense,
                    other => {
                        return Err(format!(
                            "unknown collapse input {other}; use sparse or dense"
                        ));
                    }
                }
            }
            other => return Err(format!("unknown argument {other}; run with --help")),
        }
    }

    let input = input.ok_or_else(|| "--input is required".to_string())?;
    let threshold_text = threshold_text.ok_or_else(|| "--threshold is required".to_string())?;
    let threshold: f64 = threshold_text
        .parse()
        .map_err(|_| format!("--threshold {threshold_text} is not a number"))?;
    if threshold.is_nan() || threshold < 0.0 {
        return Err(format!("--threshold {threshold_text} must be non-negative"));
    }
    if reps == 0 {
        return Err("--reps must be at least 1".to_string());
    }
    let entry = entry.unwrap_or_else(|| file_stem(&input));

    Ok(Args {
        input,
        entry,
        threshold,
        threshold_text,
        max_dim,
        modulus,
        collapse_threads,
        reducer_threads,
        reps,
        mode,
        collapse_input,
    })
}

fn parse_usize(text: &str, flag: &str) -> Result<usize, String> {
    text.trim()
        .parse()
        .map_err(|_| format!("{flag} {text} is not a whole number"))
}
