//! Counterbalanced end-to-end study driver for adaptive collapse.

#![forbid(unsafe_code)]

use std::fs;
use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use holos_tda::collapse::verify::verify_sparse_artifact;
use holos_tda::collapse::wire::{CollapseArtifact, DecodeLimits};
use holos_tda::collapse::{
    AdaptiveCollapseParams, CollapseCompleteness, CollapseObjective, CollapsedRips,
    collapse_sparse, collapse_sparse_adaptive, collapse_sparse_rounds_parallel,
};
use holos_tda::{
    Diagram, DistanceMatrix, RipsParams, SparseDistanceMatrix, rips_persistence_sparse,
};

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    None,
    V1,
    V2,
    V3H1,
    V3H2,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::V1 => "v1",
            Self::V2 => "v2",
            Self::V3H1 => "v3-h1",
            Self::V3H2 => "v3-h2",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "none" => Ok(Self::None),
            "v1" => Ok(Self::V1),
            "v2" => Ok(Self::V2),
            "v3-h1" => Ok(Self::V3H1),
            "v3-h2" => Ok(Self::V3H2),
            _ => Err(format!(
                "unknown configuration {value}; use none, v1, v2, v3-h1, or v3-h2"
            )),
        }
    }
}

struct Args {
    input: String,
    entry: String,
    threshold: f64,
    threshold_text: String,
    max_dim: usize,
    modulus: u32,
    threads: usize,
    reps: usize,
    kinds: Vec<Kind>,
    work_limit: Option<u64>,
}

struct ArgsBuilder {
    input: Option<String>,
    entry: Option<String>,
    threshold_text: Option<String>,
    max_dim: usize,
    modulus: u32,
    threads: usize,
    reps: usize,
    kinds: Vec<Kind>,
    work_limit: Option<u64>,
}

impl Default for ArgsBuilder {
    fn default() -> Self {
        Self {
            input: None,
            entry: None,
            threshold_text: None,
            max_dim: 2,
            modulus: 2,
            threads: 1,
            reps: 5,
            kinds: vec![Kind::None, Kind::V1, Kind::V2, Kind::V3H1, Kind::V3H2],
            work_limit: None,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Counts {
    algorithm_version: u32,
    completeness: &'static str,
    input_edges: usize,
    output_edges: usize,
    removed_edges: usize,
    steps: usize,
    edge_tests: usize,
    scored: usize,
    queue_pops: usize,
    stale_pops: usize,
    triangles_removed: u64,
    tetrahedra_removed: u64,
    witness_segments: usize,
    work_used: u64,
    artifact_bytes: usize,
    output_triangles: u64,
    output_tetrahedra: u64,
}

struct Sample {
    distance_s: f64,
    graph_s: f64,
    collapse_s: f64,
    reduce_s: f64,
    compute_s: f64,
    artifact_s: f64,
    verify_s: f64,
    certified_s: f64,
    counts: Option<Counts>,
}

struct Outcome {
    sample: Sample,
    diagram: Diagram,
}

struct StudyInput {
    points: Vec<Vec<f64>>,
    graph: SparseDistanceMatrix,
    triangles: u64,
    tetrahedra: u64,
}

struct Certification {
    artifact_s: f64,
    verify_s: f64,
    certified_s: f64,
    counts: Option<Counts>,
}

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

fn prepare_study_input(args: &Args) -> Result<StudyInput, String> {
    let points = read_cloud(&args.input)?;
    if points.len() < 2 {
        return Err(format!("{}: need at least two points", args.input));
    }

    let study_graph = threshold_to_sparse(
        &DistanceMatrix::from_points(&points).map_err(|error| error.to_string())?,
        args.threshold,
    )?;
    let (triangles, tetrahedra) = graph_cliques(&study_graph);
    Ok(StudyInput {
        points,
        graph: study_graph,
        triangles,
        tetrahedra,
    })
}

fn agreement_gate(points: &[Vec<f64>], args: &Args) -> Result<Vec<Diagram>, String> {
    let reference = run_one(points, args, Kind::None)?.diagram;
    let mut references: Vec<Diagram> = Vec::with_capacity(args.kinds.len());
    for &kind in &args.kinds {
        let outcome = run_one(points, args, kind)?;
        if !diagrams_equal(&reference, &outcome.diagram) {
            return Err(format!(
                "{} disagrees with no collapse bar for bar; timings are void",
                kind.name()
            ));
        }
        references.push(outcome.diagram);
    }
    Ok(references)
}

fn print_study_header(args: &Args, input: &StudyInput) {
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

fn collect_samples(
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

fn run_one(points: &[Vec<f64>], args: &Args, kind: Kind) -> Result<Outcome, String> {
    let whole = Instant::now();
    let phase = Instant::now();
    let dense = DistanceMatrix::from_points(points).map_err(|error| error.to_string())?;
    let distance_s = phase.elapsed().as_secs_f64();

    let phase = Instant::now();
    let sparse = threshold_to_sparse(&dense, args.threshold)?;
    let graph_s = phase.elapsed().as_secs_f64();

    let (collapsed, collapse_s) = run_collapse(&sparse, args, kind)?;

    let (mut diagram, reduce_s) = run_reduction(&sparse, collapsed.as_ref(), args)?;
    let compute_s = whole.elapsed().as_secs_f64();
    let certification = certify_run(&sparse, collapsed.as_ref(), args.threshold, &whole)?;
    diagram.canonicalize();
    Ok(Outcome {
        sample: Sample {
            distance_s,
            graph_s,
            collapse_s,
            reduce_s,
            compute_s,
            artifact_s: certification.artifact_s,
            verify_s: certification.verify_s,
            certified_s: certification.certified_s,
            counts: certification.counts,
        },
        diagram,
    })
}

fn run_collapse(
    graph: &SparseDistanceMatrix,
    args: &Args,
    kind: Kind,
) -> Result<(Option<CollapsedRips>, f64), String> {
    if kind == Kind::None {
        return Ok((None, 0.0));
    }
    let started = Instant::now();
    let result = collapse_for_kind(graph, args, kind).map_err(|error| error.to_string())?;
    Ok((Some(result), started.elapsed().as_secs_f64()))
}

fn collapse_for_kind(
    graph: &SparseDistanceMatrix,
    args: &Args,
    kind: Kind,
) -> holos_tda::Result<CollapsedRips> {
    match kind {
        Kind::V1 => collapse_sparse(graph, Some(args.threshold)),
        Kind::V2 => collapse_sparse_rounds_parallel(graph, Some(args.threshold), args.threads),
        Kind::V3H1 => run_adaptive(graph, args, CollapseObjective::H1),
        Kind::V3H2 => run_adaptive(graph, args, CollapseObjective::H2),
        Kind::None => unreachable!(),
    }
}

fn run_adaptive(
    graph: &SparseDistanceMatrix,
    args: &Args,
    objective: CollapseObjective,
) -> holos_tda::Result<CollapsedRips> {
    let mut params = AdaptiveCollapseParams::new(objective);
    params.work_limit = args.work_limit;
    collapse_sparse_adaptive(graph, Some(args.threshold), params)
}

fn run_reduction(
    graph: &SparseDistanceMatrix,
    collapsed: Option<&CollapsedRips>,
    args: &Args,
) -> Result<(Diagram, f64), String> {
    let started = Instant::now();
    let reduce_input = collapsed.map_or(graph, |result| &result.matrix);
    let threshold = collapsed.map_or(args.threshold, |result| result.certificate.terminal_level());
    let params = RipsParams::new(args.max_dim)
        .with_threshold(threshold)
        .with_modulus(args.modulus)
        .with_threads(args.threads);
    let diagram =
        rips_persistence_sparse(reduce_input, &params).map_err(|error| error.to_string())?;
    Ok((diagram, started.elapsed().as_secs_f64()))
}

fn certify_run(
    graph: &SparseDistanceMatrix,
    collapsed: Option<&CollapsedRips>,
    threshold: f64,
    whole: &Instant,
) -> Result<Certification, String> {
    let Some(result) = collapsed else {
        return Ok(Certification {
            artifact_s: 0.0,
            verify_s: 0.0,
            certified_s: whole.elapsed().as_secs_f64(),
            counts: None,
        });
    };
    let artifact_started = Instant::now();
    let bytes = CollapseArtifact::from_result(result)
        .and_then(|artifact| artifact.encode())
        .map_err(|error| error.to_string())?;
    let artifact_s = artifact_started.elapsed().as_secs_f64();
    let verify_started = Instant::now();
    let artifact = CollapseArtifact::decode(&bytes, DecodeLimits::default())
        .map_err(|error| error.to_string())?;
    verify_sparse_artifact(graph, Some(threshold), &artifact).map_err(|error| error.to_string())?;
    Ok(Certification {
        artifact_s,
        verify_s: verify_started.elapsed().as_secs_f64(),
        certified_s: whole.elapsed().as_secs_f64(),
        counts: Some(counts(result, bytes.len())),
    })
}

fn counts(result: &CollapsedRips, artifact_bytes: usize) -> Counts {
    let completeness = match result.certificate.completeness() {
        CollapseCompleteness::CompleteFixedPoint => "complete",
        CollapseCompleteness::BudgetLimited => "budget_limited",
        _ => "unknown",
    };
    let (output_triangles, output_tetrahedra) = graph_cliques(&result.matrix);
    Counts {
        algorithm_version: result.certificate.algorithm_version(),
        completeness,
        input_edges: result.stats.input_edges,
        output_edges: result.stats.output_edges,
        removed_edges: result.stats.removed_edges,
        steps: result.certificate.steps().len(),
        edge_tests: result.stats.edge_tests,
        scored: result.stats.adaptive_score_evaluations,
        queue_pops: result.stats.adaptive_queue_pops,
        stale_pops: result.stats.adaptive_stale_pops,
        triangles_removed: result.stats.adaptive_triangles_removed,
        tetrahedra_removed: result.stats.adaptive_tetrahedra_removed,
        witness_segments: result.stats.witness_segments,
        work_used: result.certificate.work_used(),
        artifact_bytes,
        output_triangles,
        output_tetrahedra,
    }
}

fn print_summary(args: &Args, kind: Kind, runs: &[Sample]) {
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

fn graph_cliques(graph: &SparseDistanceMatrix) -> (u64, u64) {
    let mut adjacency = vec![Vec::new(); graph.len()];
    for (u, v, _) in graph.edges() {
        adjacency[u].push(v);
        adjacency[v].push(u);
    }
    let mut triangles = 0u64;
    let mut tetrahedra = 0u64;
    for u in 0..graph.len() {
        for &v in adjacency[u].iter().filter(|&&v| v > u) {
            let common = sorted_intersection_above(&adjacency[u], &adjacency[v], v);
            triangles = triangles.saturating_add(common.len() as u64);
            for (position, &w) in common.iter().enumerate() {
                for &x in &common[position + 1..] {
                    if adjacency[w].binary_search(&x).is_ok() {
                        tetrahedra = tetrahedra.saturating_add(1);
                    }
                }
            }
        }
    }
    (triangles, tetrahedra)
}

fn sorted_intersection_above(a: &[usize], b: &[usize], lower: usize) -> Vec<usize> {
    let mut intersection = Vec::new();
    let mut i = a.partition_point(|&value| value <= lower);
    let mut j = b.partition_point(|&value| value <= lower);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                intersection.push(a[i]);
                i += 1;
                j += 1;
            }
        }
    }
    intersection
}

fn threshold_to_sparse(
    dense: &DistanceMatrix,
    threshold: f64,
) -> Result<SparseDistanceMatrix, String> {
    let mut edges = Vec::new();
    for u in 0..dense.len() {
        for v in u + 1..dense.len() {
            let value = dense.get(u, v);
            if value.is_finite() && value <= threshold {
                edges.push((u, v, value));
            }
        }
    }
    SparseDistanceMatrix::from_triplets(dense.len(), &edges).map_err(|error| error.to_string())
}

fn diagrams_equal(a: &Diagram, b: &Diagram) -> bool {
    a.bars.len() == b.bars.len()
        && a.bars.iter().zip(&b.bars).all(|(x, y)| {
            x.dim == y.dim
                && x.birth.to_bits() == y.birth.to_bits()
                && x.death.to_bits() == y.death.to_bits()
        })
}

struct Summary {
    median: f64,
    iqr: f64,
    max: f64,
}

fn summarize(values: &mut [f64]) -> Summary {
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

fn read_cloud(path: &str) -> Result<Vec<Vec<f64>>, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", file_stem(path)))?;
    let mut points = Vec::new();
    for (line_index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut point = Vec::new();
        for token in line.replace(',', " ").split_whitespace() {
            point.push(token.parse::<f64>().map_err(|_| {
                format!(
                    "{} line {}: {token} is not a number",
                    file_stem(path),
                    line_index + 1
                )
            })?);
        }
        points.push(point);
    }
    Ok(points)
}

fn file_stem(path: &str) -> String {
    Path::new(path).file_stem().map_or_else(
        || path.to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn vm_hwm_kb() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    status.lines().find_map(|line| {
        line.strip_prefix("VmHWM:")?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    })
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut builder = ArgsBuilder::default();
    let mut arguments = argv.iter();
    while let Some(flag) = arguments.next() {
        let value = arguments
            .next()
            .ok_or_else(|| format!("{flag} needs a value"))?;
        parse_argument(&mut builder, flag, value)?;
    }
    finish_args(builder)
}

fn parse_argument(builder: &mut ArgsBuilder, flag: &str, value: &str) -> Result<(), String> {
    if matches!(flag, "--input" | "--entry" | "--threshold" | "--configs") {
        return parse_text_argument(builder, flag, value);
    }
    if matches!(
        flag,
        "--max-dim" | "--modulus" | "--threads" | "--reps" | "--work-limit"
    ) {
        return parse_numeric_argument(builder, flag, value);
    }
    Err(format!("unknown argument {flag}; run with --help"))
}

fn parse_text_argument(builder: &mut ArgsBuilder, flag: &str, value: &str) -> Result<(), String> {
    match flag {
        "--input" => builder.input = Some(value.to_string()),
        "--entry" => builder.entry = Some(value.to_string()),
        "--threshold" => builder.threshold_text = Some(value.to_string()),
        "--configs" => builder.kinds = parse_kinds(value)?,
        _ => unreachable!(),
    }
    Ok(())
}

fn parse_numeric_argument(
    builder: &mut ArgsBuilder,
    flag: &str,
    value: &str,
) -> Result<(), String> {
    let number = value
        .parse::<u64>()
        .map_err(|_| format!("{flag} {value} is not a whole number"))?;
    match flag {
        "--max-dim" => set_usize(&mut builder.max_dim, number, flag),
        "--modulus" => set_u32(&mut builder.modulus, number, flag),
        "--threads" => set_threads(&mut builder.threads, number, flag),
        "--reps" => set_usize(&mut builder.reps, number, flag),
        "--work-limit" => {
            builder.work_limit = Some(number);
            Ok(())
        }
        _ => unreachable!(),
    }
}

fn set_usize(target: &mut usize, number: u64, flag: &str) -> Result<(), String> {
    *target = usize::try_from(number).map_err(|_| format!("{flag} is out of range"))?;
    Ok(())
}

fn set_u32(target: &mut u32, number: u64, flag: &str) -> Result<(), String> {
    *target = u32::try_from(number).map_err(|_| format!("{flag} is out of range"))?;
    Ok(())
}

fn set_threads(target: &mut usize, number: u64, flag: &str) -> Result<(), String> {
    set_usize(target, number, flag)?;
    *target = (*target).max(1);
    Ok(())
}

fn parse_kinds(text: &str) -> Result<Vec<Kind>, String> {
    let kinds: Vec<_> = text.split(',').map(Kind::parse).collect::<Result<_, _>>()?;
    if kinds.is_empty() {
        return Err("--configs must name at least one configuration".to_string());
    }
    let mut deduplicated = Vec::with_capacity(kinds.len());
    for kind in kinds {
        if !deduplicated.contains(&kind) {
            deduplicated.push(kind);
        }
    }
    Ok(deduplicated)
}

fn finish_args(builder: ArgsBuilder) -> Result<Args, String> {
    let input = builder
        .input
        .ok_or_else(|| "--input is required".to_string())?;
    let threshold_text = builder
        .threshold_text
        .ok_or_else(|| "--threshold is required".to_string())?;
    let threshold: f64 = threshold_text
        .parse()
        .map_err(|_| format!("--threshold {threshold_text} is not a number"))?;
    if threshold.is_nan() || threshold < 0.0 {
        return Err(format!("--threshold {threshold_text} must be non-negative"));
    }
    if builder.reps == 0 {
        return Err("--reps must be at least 1".to_string());
    }
    let entry = builder.entry.unwrap_or_else(|| file_stem(&input));
    Ok(Args {
        input,
        entry,
        threshold,
        threshold_text,
        max_dim: builder.max_dim,
        modulus: builder.modulus,
        threads: builder.threads,
        reps: builder.reps,
        kinds: builder.kinds,
        work_limit: builder.work_limit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_parser_preserves_order_and_removes_duplicates() {
        let argv = [
            "--input",
            "cloud.csv",
            "--threshold",
            "1",
            "--configs",
            "v3-h2,none,v3-h2,v1",
        ]
        .map(String::from);
        let args = parse_args(&argv).unwrap();
        assert_eq!(
            args.kinds
                .iter()
                .map(|kind| kind.name())
                .collect::<Vec<_>>(),
            ["v3-h2", "none", "v1"]
        );
    }

    #[test]
    fn summary_uses_interpolated_quartiles() {
        let mut values = [1.0, 4.0, 2.0, 3.0];
        let summary = summarize(&mut values);
        assert_eq!(summary.median, 2.5);
        assert_eq!(summary.iqr, 1.5);
        assert_eq!(summary.max, 4.0);
    }

    #[test]
    fn clique_counter_counts_each_simplex_once() {
        let edges = [
            (0, 1, 1.0),
            (0, 2, 1.0),
            (0, 3, 1.0),
            (1, 2, 1.0),
            (1, 3, 1.0),
            (2, 3, 1.0),
        ];
        let graph = SparseDistanceMatrix::from_triplets(4, &edges).unwrap();
        assert_eq!(graph_cliques(&graph), (4, 1));
    }
}
