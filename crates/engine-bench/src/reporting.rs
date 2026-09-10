use std::fs;

use holos_tda::DenseStorage;

use super::input;
use super::model::{Args, Config, Engine, Format, Outcome, Summary};

pub(super) fn print_phase_summaries(
    args: &Args,
    configs: &[Config],
    verified: &[Outcome],
    samples: &mut [Vec<Vec<f64>>],
) {
    for ((cfg, verify), values) in configs.iter().zip(verified).zip(samples) {
        for ((name, _), phase_samples) in verify.phases.iter().zip(values) {
            let summary = summarize(phase_samples);
            println!(
                "kind=phase entry={} config={} engine={} phase={} reps={} median_s={:.6} iqr_s={:.6} q1_s={:.6} q3_s={:.6} min_s={:.6} max_s={:.6}",
                args.entry,
                cfg.name,
                engine_name(cfg.engine),
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
    configs: &[Config],
    verified: &[Outcome],
    rotation: &[Vec<usize>],
) {
    let names: Vec<&str> = configs.iter().map(|c| c.name).collect();
    let points = verified.first().map_or(0, |o| o.points);
    let graph_edges = verified
        .iter()
        .find_map(|o| o.graph_edges)
        .map_or("unavailable".to_string(), |e| e.to_string());
    println!(
        "# engine-bench {} phase-separated reduction timing",
        env!("CARGO_PKG_VERSION")
    );
    println!(
        "# one record per line, space-separated key=value; run with --help for the field rules"
    );
    println!("# vm_hwm_kb is the whole-process high-water mark and belongs to no configuration");
    println!("# engineering instrument, not a registered study; no grade reads these numbers");
    println!(
        "kind=entry entry={} input={} format={} points={} pairs={} threshold={} max_dim={} modulus={} threads={} parse_threads={} collapse=off dense_storage={} reps={} configs={} graph_edges={} order_scheme=cyclic_rotation balanced={}",
        args.entry,
        input::file_stem(&args.input),
        format_name(args.format),
        points,
        points * points.saturating_sub(1) / 2,
        args.threshold_text,
        args.max_dim,
        args.modulus,
        args.threads,
        args.parse_threads,
        storage_name(args.dense_storage),
        args.reps,
        names.join(","),
        graph_edges,
        if args.reps % configs.len().max(1) == 0 {
            "yes"
        } else {
            "no"
        }
    );
    for cfg in configs {
        println!(
            "kind=config entry={} config={} engine={} input_kind={} threads={} collapse=off routing={}",
            args.entry,
            cfg.name,
            engine_name(cfg.engine),
            args.threads,
            match (args.format, cfg.engine) {
                (Format::Sparse, Engine::Sparse) => "triplet_graph",
                (Format::Sparse, _) => "widened_matrix",
                (_, Engine::Sparse) => "thresholded_graph",
                (_, _) => "distance_matrix",
            },
            match cfg.engine {
                Engine::Auto => "auto",
                _ => "forced",
            }
        );
    }
    for (rep, order) in rotation.iter().enumerate() {
        let names: Vec<&str> = order.iter().map(|&i| configs[i].name).collect();
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

pub(super) fn engine_name(engine: Engine) -> &'static str {
    match engine {
        Engine::Auto => "auto",
        Engine::Dense => "dense",
        Engine::Sparse => "sparse",
    }
}

fn format_name(format: Format) -> &'static str {
    match format {
        Format::Points => "points",
        Format::LowerDistance => "lower-distance",
        Format::Sparse => "sparse",
    }
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

fn storage_name(storage: DenseStorage) -> &'static str {
    match storage {
        DenseStorage::Compact => "compact",
        DenseStorage::Square => "square",
        // Auto, and any form a later version of the library adds.
        _ => "auto",
    }
}
