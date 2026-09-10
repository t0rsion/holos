use holos_tda::Diagram;

use super::args::{Args, Kind, file_stem};
use super::input::StudyInput;
use super::pipeline::{Sample, diagrams_equal, run_one};

pub(super) fn print_study_header(args: &Args, input: &StudyInput) {
    println!(
        "# collapse-adaptive-bench {} counterbalanced end-to-end study",
        env!("CARGO_PKG_VERSION")
    );
    println!(
        "kind=entry entry={} input={} points={} threshold={} max_dim={} modulus={} threads={} reps={} configs={} balanced={} work_limit={} input_edges={} input_triangles={} input_tetrahedra={}",
        args.entry,
        file_stem(&args.input),
        input.points.len(),
        args.threshold_text,
        args.max_dim,
        args.modulus,
        args.threads,
        args.reps,
        args.kinds
            .iter()
            .map(|kind| kind.name())
            .collect::<Vec<_>>()
            .join(","),
        if args.reps % args.kinds.len() == 0 {
            "yes"
        } else {
            "no"
        },
        args.work_limit
            .map_or("unlimited".to_string(), |limit| limit.to_string()),
        input.graph.num_edges(),
        input.triangles,
        input.tetrahedra
    );
    println!(
        "kind=agreement entry={} reference=none exact=bar_for_bar result=pass configs={}",
        args.entry,
        args.kinds
            .iter()
            .map(|kind| kind.name())
            .collect::<Vec<_>>()
            .join(",")
    );
}

pub(super) fn collect_samples(
    points: &[Vec<f64>],
    args: &Args,
    references: &[Diagram],
) -> Result<Vec<Vec<Sample>>, String> {
    let mut samples: Vec<Vec<Sample>> = args
        .kinds
        .iter()
        .map(|_| Vec::with_capacity(args.reps))
        .collect();
    for rep in 0..args.reps {
        let order: Vec<usize> = (0..args.kinds.len())
            .map(|offset| (rep + offset) % args.kinds.len())
            .collect();
        println!(
            "kind=order entry={} rep={} order={}",
            args.entry,
            rep,
            order
                .iter()
                .map(|&index| args.kinds[index].name())
                .collect::<Vec<_>>()
                .join(",")
        );
        for index in order {
            let kind = args.kinds[index];
            let outcome = run_one(points, args, kind)?;
            check_repetition(kind, rep, &references[index], &outcome.diagram)?;
            samples[index].push(outcome.sample);
        }
    }
    Ok(samples)
}

fn check_repetition(
    kind: Kind,
    repetition: usize,
    reference: &Diagram,
    diagram: &Diagram,
) -> Result<(), String> {
    if diagrams_equal(reference, diagram) {
        return Ok(());
    }
    Err(format!(
        "{} repetition {repetition} differs from its agreement diagram",
        kind.name()
    ))
}

pub(super) fn print_summary(args: &Args, kind: Kind, runs: &[Sample]) {
    let summary = |pick: fn(&Sample) -> f64| -> Summary {
        let mut values: Vec<_> = runs.iter().map(pick).collect();
        summarize(&mut values)
    };
    let counts = runs.first().and_then(|sample| sample.counts.as_ref());
    let stable = match counts {
        None => true,
        Some(reference) => runs
            .iter()
            .all(|sample| sample.counts.as_ref() == Some(reference)),
    };
    let (
        version,
        completeness,
        input,
        output,
        removed,
        steps,
        tests,
        scored,
        pops,
        stale,
        triangles,
        tetrahedra,
        witnesses,
        work,
        bytes,
        output_triangles,
        output_tetrahedra,
    ) = match counts {
        None => (
            "none".to_string(),
            "none",
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ),
        Some(counts) => (
            counts.algorithm_version.to_string(),
            counts.completeness,
            counts.input_edges,
            counts.output_edges,
            counts.removed_edges,
            counts.steps,
            counts.edge_tests,
            counts.scored,
            counts.queue_pops,
            counts.stale_pops,
            counts.triangles_removed,
            counts.tetrahedra_removed,
            counts.witness_segments,
            counts.work_used,
            counts.artifact_bytes,
            counts.output_triangles,
            counts.output_tetrahedra,
        ),
    };
    for (phase, values) in [
        ("distance", summary(|sample| sample.distance_s)),
        ("graph", summary(|sample| sample.graph_s)),
        ("collapse", summary(|sample| sample.collapse_s)),
        ("reduce", summary(|sample| sample.reduce_s)),
        ("compute", summary(|sample| sample.compute_s)),
        ("artifact", summary(|sample| sample.artifact_s)),
        ("verify", summary(|sample| sample.verify_s)),
        ("certified", summary(|sample| sample.certified_s)),
    ] {
        println!(
            "kind=phase entry={} config={} phase={} reps={} median_s={:.6} iqr_s={:.6} max_s={:.6}",
            args.entry,
            kind.name(),
            phase,
            runs.len(),
            values.median,
            values.iqr,
            values.max
        );
    }
    println!(
        "kind=counters entry={} config={} reps={} stable={} algorithm_version={} completeness={} input_edges={} output_edges={} removed_edges={} steps={} edge_tests={} scored_candidates={} queue_pops={} stale_pops={} triangles_removed={} tetrahedra_removed={} witness_segments={} work_used={} artifact_bytes={} output_triangles={} output_tetrahedra={}",
        args.entry,
        kind.name(),
        runs.len(),
        if stable { "yes" } else { "no" },
        version,
        completeness,
        input,
        output,
        removed,
        steps,
        tests,
        scored,
        pops,
        stale,
        triangles,
        tetrahedra,
        witnesses,
        work,
        bytes,
        output_triangles,
        output_tetrahedra
    );
}

pub(super) struct Summary {
    pub(super) median: f64,
    pub(super) iqr: f64,
    pub(super) max: f64,
}

pub(super) fn summarize(values: &mut [f64]) -> Summary {
    values.sort_by(f64::total_cmp);
    Summary {
        median: quantile(values, 0.5),
        iqr: quantile(values, 0.75) - quantile(values, 0.25),
        max: values[values.len() - 1],
    }
}

fn quantile(sorted: &[f64], probability: f64) -> f64 {
    let h = (sorted.len() as f64 - 1.0) * probability;
    let lower = h.floor() as usize;
    let fraction = h - lower as f64;
    if lower + 1 == sorted.len() {
        sorted[lower]
    } else {
        sorted[lower] + fraction * (sorted[lower + 1] - sorted[lower])
    }
}
