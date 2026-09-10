use std::fs;

use super::cli;
use super::model::{Args, Config, Counters, InputKind, Kind, Outcome, Summary};

pub(super) fn print_samples(
    args: &Args,
    configs: &[Config],
    verified: &[Outcome],
    samples: &mut [Vec<Vec<f64>>],
    counter_runs: &[Vec<Counters>],
) {
    let serial_collapse_s = serial_collapse_median(configs, verified, samples);
    for (((cfg, verify), values), runs) in
        configs.iter().zip(verified).zip(samples).zip(counter_runs)
    {
        for ((name, _), phase_samples) in verify.phases.iter().zip(values) {
            let summary = summarize(phase_samples);
            println!(
                "kind=phase entry={} config={} mode={} collapse_threads={} reducer_threads={} phase={} reps={} median_s={:.6} iqr_s={:.6} q1_s={:.6} q3_s={:.6} min_s={:.6} max_s={:.6}",
                args.entry,
                cfg.name,
                mode_name(cfg.kind),
                cfg.collapse_threads,
                reducer_threads(args, cfg),
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
        print_counters(args, cfg, runs, serial_collapse_s);
    }
}

/// The counterbalancing rotation: repetition r starts at configuration r and
/// wraps around. Deterministic, and printed with the record.
pub(super) fn rotation_orders(configs: usize, reps: usize) -> Vec<Vec<usize>> {
    (0..reps)
        .map(|rep| (0..configs).map(|i| (i + rep) % configs.max(1)).collect())
        .collect()
}

pub(super) fn print_header(
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
        cli::file_stem(&args.input),
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
fn width_summary(first: &Counters) -> (f64, f64, f64, f64) {
    let widths: Vec<f64> = first
        .batch_widths
        .iter()
        .map(|&(_, width)| width as f64)
        .collect();
    if widths.is_empty() {
        return (0.0, 0.0, 0.0, 0.0);
    }
    let mut sorted = widths.clone();
    let summary = summarize(&mut sorted);
    (
        summary.min,
        summary.max,
        widths.iter().sum::<f64>() / widths.len() as f64,
        summary.median,
    )
}

fn occupancy_text(offered: f64, formed: f64) -> (String, String) {
    if offered > 0.0 {
        (
            format!("{:.4}", formed / offered),
            count_text(offered - formed),
        )
    } else {
        ("unavailable".to_string(), "unavailable".to_string())
    }
}

fn subphase_text(runs: &[Counters], clocked: bool) -> (&'static str, String, String, String, f64) {
    let median = |pick: fn(&Counters) -> f64| counter_median(runs, pick);
    let repair_s = median(|counter| counter.repair_ns as f64) / 1e9;
    if clocked {
        (
            "predicate,retirement,repair",
            format!("{:.6}", median(|counter| counter.predicate_ns as f64) / 1e9),
            format!(
                "{:.6}",
                median(|counter| counter.retirement_ns as f64) / 1e9
            ),
            format!("{repair_s:.6}"),
            repair_s,
        )
    } else {
        (
            "none_on_this_path",
            "unavailable".to_string(),
            "unavailable".to_string(),
            "unavailable".to_string(),
            repair_s,
        )
    }
}

fn weighted_repair_text(
    clocked: bool,
    repair_s: f64,
    serial_collapse_s: Option<f64>,
) -> (String, &'static str, String) {
    match (clocked, serial_collapse_s) {
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
    }
}

fn batch_width_text(first: &Counters) -> String {
    let widths: Vec<String> = first
        .batch_widths
        .iter()
        .map(|&(epoch, width)| format!("{epoch}:{width}"))
        .collect();
    if widths.is_empty() {
        "none".to_string()
    } else {
        widths.join(",")
    }
}

fn print_counters(args: &Args, cfg: &Config, runs: &[Counters], serial_collapse_s: Option<f64>) {
    let Some(first) = runs.first() else {
        return;
    };
    let (width_min, width_max, width_mean, width_median) = width_summary(first);
    let reference = count_vector(first);
    let stable = runs.iter().all(|run| count_vector(run) == reference);

    let median = |pick: fn(&Counters) -> f64| counter_median(runs, pick);
    let input_edges = median(|c| c.input_edges as f64);
    let edge_tests = median(|c| c.edge_tests as f64);
    let logical_tests = median(|c| c.logical_tests as f64);
    let invalidated_results = median(|c| c.invalidated_results as f64);
    let offered = median(|c| c.window_slots_offered as f64);
    let formed = median(|c| c.window_members_formed as f64);
    let (occupancy, unused_capacity) = occupancy_text(offered, formed);
    let clocked = cfg.kind == Kind::V1Ordered && cfg.collapse_threads > 1;
    let (subphases, predicate_text, retirement_text, repair_text, repair_s) =
        subphase_text(runs, clocked);
    let (cost_weighted, denominator, denominator_s) =
        weighted_repair_text(clocked, repair_s, serial_collapse_s);

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
    println!(
        "kind=batch_widths entry={} config={} epochs={} widths={}",
        args.entry,
        cfg.name,
        first.epochs,
        batch_width_text(first)
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

/// Quantiles interpolate linearly between the two neighboring order
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

pub(super) fn vm_hwm_kb() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    status.lines().find_map(|line| {
        let rest = line.strip_prefix("VmHWM:")?;
        rest.split_whitespace().next()?.parse().ok()
    })
}

pub(super) fn report_kb(kb: Option<u64>) -> String {
    kb.map_or("unavailable".to_string(), |kb| kb.to_string())
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
