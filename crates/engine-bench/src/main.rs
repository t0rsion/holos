//! Phase-separated driver for the reduction: one input, one threshold,
//! and every engine configuration timed in one process. The default is
//! the serial engine; `--threads` puts the same phases on the parallel
//! one.
//!
//! The driver is the tuning and landing instrument of the engineering
//! benchmark, benchmarks/engine_bench.sh with
//! benchmarks/engineering_corpus.toml. It is not part of a registered
//! study, and no grade reads it.
//!
//! Each phase carries its own clock, so no number comes from subtracting
//! one command line from another. Every configuration computes its
//! diagram once before any timing starts. The diagrams must agree bar for
//! bar, or the run aborts.
//!
//! The timed repetitions are counterbalanced: they interleave the
//! configurations instead of running one configuration to exhaustion.
//!
//! Output is one key=value line per record. benchmarks/engine_bench.sh
//! parses it; the fields below are its interface.

#![forbid(unsafe_code)]

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use holos_tda::collapse::collapse_sparse;
use holos_tda::io::{self, OutputFormat, write_diagram};
use holos_tda::{
    DenseStorage, Diagram, DistanceMatrix, Engine as LibEngine, RipsParams, SparseDistanceMatrix,
    rips_persistence, rips_persistence_sparse,
};

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
                          metadata line goes to stdout. This is the
                          preprocessing step behind the corpus entries whose
                          input is a real collapsed graph
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

All run the same engine at the same thread count with the collapse off,
which is what this instrument measures. The auto and dense configurations set
RipsParams::engine and differ in nothing else, so their gap is the routing
rule alone. The sparse configuration is an entry point instead:
rips_persistence_sparse on a graph this driver builds, which puts the
conversion on a clock of its own. The three optimization toggles are for
differential testing and stay at their defaults.

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
            boundary outside the crate, so the reduction is one clock. A
            driver that split it would have to call something else.
  total     the whole configuration, from one enclosing clock

Every repetition parses the file again, so total covers the work a command
line does and compares against another tool's process time. The comparison
still favors this driver by the process start it never pays. Read a ratio
near 1.0 with that in mind, and read the reduce phase for the engine alone.

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

/// Which engine a configuration runs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Engine {
    Auto,
    Dense,
    Sparse,
}

/// How the input file spells its distances.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Points,
    LowerDistance,
    Sparse,
}

struct Args {
    input: String,
    entry: String,
    threshold: f64,
    threshold_text: String,
    format: Format,
    max_dim: usize,
    modulus: u32,
    reps: usize,
    threads: usize,
    parse_threads: usize,
    mode: Vec<Engine>,
    dense_storage: DenseStorage,
    diagram_out: Option<String>,
    emit_collapsed: Option<String>,
}

struct Config {
    name: &'static str,
    engine: Engine,
}

/// One pipeline run: the phase clocks plus what the run produced.
struct Outcome {
    phases: Vec<(&'static str, f64)>,
    diagram: Diagram,
    points: usize,
    /// Edges at or below the threshold. The sparse configuration counts
    /// them while it builds its graph; the dense one never forms them.
    graph_edges: Option<usize>,
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
        return emit_collapsed(&args, &path);
    }
    let configs = configurations(&args);

    // Agreement first. A timed repetition never runs before every
    // configuration has produced its diagram and matched the reference.
    let mut verified: Vec<Outcome> = Vec::with_capacity(configs.len());
    for cfg in &configs {
        let outcome = run_pipeline(&args, cfg)?;
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
        verified.push(outcome);
    }
    let reference = verified
        .first()
        .ok_or_else(|| "no configuration ran; --mode selected none".to_string())?;
    if let Some(path) = &args.diagram_out {
        write_reference_diagram(path, &reference.diagram, args.max_dim)?;
    }

    let hwm_start = vm_hwm_kb();
    let rotation = rotation_orders(configs.len(), args.reps);
    print_header(&args, &configs, &verified, &rotation);

    // Counterbalanced timing: each repetition walks the configurations from
    // its own starting point, so no configuration owns the end of the run.
    let mut samples: Vec<Vec<Vec<f64>>> = verified
        .iter()
        .map(|verify| vec![Vec::with_capacity(args.reps); verify.phases.len()])
        .collect();
    for (rep, order) in rotation.iter().enumerate() {
        for &index in order {
            let cfg = &configs[index];
            let outcome = run_pipeline(&args, cfg)?;
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
        }
    }

    for ((cfg, verify), values) in configs.iter().zip(&verified).zip(&mut samples) {
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

fn print_header(args: &Args, configs: &[Config], verified: &[Outcome], rotation: &[Vec<usize>]) {
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
        file_stem(&args.input),
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

/// One full pipeline run of one configuration, phase by phase.
fn run_pipeline(args: &Args, cfg: &Config) -> Result<Outcome, String> {
    let mut phases: Vec<(&'static str, f64)> = Vec::with_capacity(5);
    let whole = Instant::now();

    let clock = Instant::now();
    let values = read_input(&args.input, args.format, args.parse_threads)?;
    phases.push(("parse", clock.elapsed().as_secs_f64()));

    // The dense configuration forces its engine, or the routing rule
    // would answer for it. The sparse configuration reduces a graph, which
    // no rule routes.
    let params = RipsParams::new(args.max_dim)
        .with_threshold(args.threshold)
        .with_modulus(args.modulus)
        .with_threads(args.threads)
        .with_engine(match cfg.engine {
            Engine::Auto | Engine::Sparse => LibEngine::Auto,
            Engine::Dense => LibEngine::Dense,
        })
        .with_dense_storage(args.dense_storage);

    let mut graph_edges = None;
    let (points, mut diagram) = match (values, cfg.engine) {
        (Parsed::Triplets(n, triplets), Engine::Sparse) => {
            let clock = Instant::now();
            let sparse =
                SparseDistanceMatrix::from_triplets(n, &triplets).map_err(|e| e.to_string())?;
            phases.push(("graph", clock.elapsed().as_secs_f64()));
            graph_edges = Some(sparse.num_edges());
            let clock = Instant::now();
            let diagram = rips_persistence_sparse(&sparse, &params).map_err(|e| e.to_string())?;
            phases.push(("reduce", clock.elapsed().as_secs_f64()));
            (n, diagram)
        }
        (Parsed::Triplets(n, triplets), Engine::Dense | Engine::Auto) => {
            let clock = Instant::now();
            let dist = widen_to_dense(n, &triplets)?;
            phases.push(("distance", clock.elapsed().as_secs_f64()));
            let clock = Instant::now();
            let diagram = rips_persistence(&dist, &params).map_err(|e| e.to_string())?;
            phases.push(("reduce", clock.elapsed().as_secs_f64()));
            (n, diagram)
        }
        (values, engine) => {
            let clock = Instant::now();
            let dist = match values {
                Parsed::Points(points) => DistanceMatrix::from_points(&points),
                Parsed::Condensed(data) => DistanceMatrix::from_condensed(data),
                Parsed::Triplets(..) => unreachable!("triplets take their own arms"),
            }
            .map_err(|e| e.to_string())?;
            phases.push(("distance", clock.elapsed().as_secs_f64()));
            if dist.len() < 2 {
                return Err(format!(
                    "{}: need at least two points",
                    file_stem(&args.input)
                ));
            }
            let n = dist.len();
            let diagram = match engine {
                Engine::Dense | Engine::Auto => {
                    let clock = Instant::now();
                    let diagram = rips_persistence(&dist, &params).map_err(|e| e.to_string())?;
                    phases.push(("reduce", clock.elapsed().as_secs_f64()));
                    diagram
                }
                Engine::Sparse => {
                    let clock = Instant::now();
                    let sparse = threshold_to_sparse(&dist, args.threshold)?;
                    phases.push(("graph", clock.elapsed().as_secs_f64()));
                    graph_edges = Some(sparse.num_edges());
                    let clock = Instant::now();
                    let diagram =
                        rips_persistence_sparse(&sparse, &params).map_err(|e| e.to_string())?;
                    phases.push(("reduce", clock.elapsed().as_secs_f64()));
                    diagram
                }
            };
            (n, diagram)
        }
    };
    phases.push(("total", whole.elapsed().as_secs_f64()));
    if points < 2 {
        return Err(format!(
            "{}: need at least two points",
            file_stem(&args.input)
        ));
    }
    diagram.canonicalize();

    Ok(Outcome {
        phases,
        diagram,
        points,
        graph_edges,
    })
}

/// The full matrix of a sparse graph: every absent pair is +inf, which the
/// engine reads as an edge that never enters the filtration. The cost of
/// this call, in time and in memory, is what an entry of the memory
/// stratum exists to show.
fn widen_to_dense(n: usize, triplets: &[(usize, usize, f64)]) -> Result<DistanceMatrix, String> {
    let mut data = vec![f64::INFINITY; n * n.saturating_sub(1) / 2];
    for &(i, j, d) in triplets {
        if i >= n || j >= n || i == j {
            return Err(format!("triplet ({i}, {j}) is out of range for n = {n}"));
        }
        let (hi, lo) = if i > j { (i, j) } else { (j, i) };
        data[hi * (hi - 1) / 2 + lo] = d;
    }
    DistanceMatrix::from_condensed(data).map_err(|e| e.to_string())
}

/// Collapse the input and write the reduced graph, then stop. No clock
/// runs: this is a preprocessing step, and the collapse itself is measured
/// by the collapse benchmarks.
fn emit_collapsed(args: &Args, path: &str) -> Result<(), String> {
    let sparse = match read_input(&args.input, args.format, args.parse_threads)? {
        Parsed::Triplets(n, triplets) => {
            SparseDistanceMatrix::from_triplets(n, &triplets).map_err(|e| e.to_string())?
        }
        Parsed::Points(points) => {
            let dist = DistanceMatrix::from_points(&points).map_err(|e| e.to_string())?;
            threshold_to_sparse(&dist, args.threshold)?
        }
        Parsed::Condensed(data) => {
            let dist = DistanceMatrix::from_condensed(data).map_err(|e| e.to_string())?;
            threshold_to_sparse(&dist, args.threshold)?
        }
    };
    let n = sparse.len();
    let before = sparse.num_edges();
    let collapsed =
        collapse_sparse(&sparse, Some(args.threshold)).map_err(|e| format!("collapse: {e}"))?;
    let after = collapsed.matrix.num_edges();

    // The sparse reader takes the vertex count from the largest index it
    // sees. A collapse can strip every edge off the last vertices, so the
    // reduced graph is relabelled the way benchmarks/gen_graph.py
    // relabels: isolated vertices first, the rest in order. The barcode
    // does not depend on the labelling.
    let mut degree = vec![0usize; n];
    for (u, v, _) in collapsed.matrix.edges() {
        degree[u] += 1;
        degree[v] += 1;
    }
    let mut label = vec![0usize; n];
    let mut next = 0;
    for pass in [0usize, 1] {
        for (v, &deg) in degree.iter().enumerate() {
            if (deg == 0) == (pass == 0) {
                label[v] = next;
                next += 1;
            }
        }
    }
    let mut rows: Vec<(usize, usize, f64)> = collapsed
        .matrix
        .edges()
        .map(|(u, v, d)| (label[u].max(label[v]), label[u].min(label[v]), d))
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.total_cmp(&b.2)));

    let file = File::create(path).map_err(|e| format!("{}: {e}", file_stem(path)))?;
    let mut out = BufWriter::new(file);
    for (u, v, d) in &rows {
        writeln!(out, "{u} {v} {d:?}").map_err(|e| e.to_string())?;
    }
    out.flush().map_err(|e| e.to_string())?;

    let isolated = degree.iter().filter(|&&d| d == 0).count();
    println!(
        "collapsed entry={} n={} edges_in={} edges_out={} isolated={} threshold={} kept={:.6}",
        args.entry,
        n,
        before,
        after,
        isolated,
        args.threshold_text,
        if before == 0 {
            0.0
        } else {
            after as f64 / before as f64
        }
    );
    Ok(())
}

/// The thresholded graph of a dense matrix, as the sparse engine sees it.
/// The vertex count comes from the matrix, so a vertex with no edge keeps
/// its place and its essential H0 bar.
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

/// Every configuration, in the order --mode all runs them.
fn all_modes() -> Vec<Engine> {
    vec![Engine::Dense, Engine::Sparse, Engine::Auto]
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

fn configurations(args: &Args) -> Vec<Config> {
    args.mode
        .iter()
        .map(|&engine| Config {
            name: engine_name(engine),
            engine,
        })
        .collect()
}

fn engine_name(engine: Engine) -> &'static str {
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

/// The verified diagram in ripser's format, for the comparison the runner
/// makes against ripser's own output.
fn write_reference_diagram(path: &str, diagram: &Diagram, max_dim: usize) -> Result<(), String> {
    let file = File::create(path).map_err(|e| format!("{}: {e}", file_stem(path)))?;
    let mut out = BufWriter::new(file);
    write_diagram(&mut out, diagram, OutputFormat::Ripser, max_dim).map_err(|e| e.to_string())
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

/// The parsed input, before any matrix exists.
enum Parsed {
    Points(Vec<Vec<f64>>),
    Condensed(Vec<f64>),
    /// Vertex count and `(i, j, d)` triplets of a sparse input. The vertex
    /// count is one more than the largest index in the file, the rule
    /// holos and ripser both follow.
    Triplets(usize, Vec<(usize, usize, f64)>),
}

/// Read and parse the input file with the library's own parsers. The parse
/// is a phase of its own, so this stops short of the matrix, which
/// holos_tda::io::read_lower_distance_matrix would build in the same call.
fn read_input(path: &str, format: Format, threads: usize) -> Result<Parsed, String> {
    let name = file_stem(path);
    let text = fs::read_to_string(path).map_err(|e| format!("{name}: {e}"))?;
    let parsed = match format {
        Format::Points => {
            Parsed::Points(io::parse_point_cloud(&name, &text, threads).map_err(|e| e.to_string())?)
        }
        Format::LowerDistance => Parsed::Condensed(
            io::parse_condensed(&name, &text, threads).map_err(|e| e.to_string())?,
        ),
        Format::Sparse => {
            let (n, triplets) =
                io::parse_triplets(&name, &text, threads).map_err(|e| e.to_string())?;
            Parsed::Triplets(n, triplets)
        }
    };
    Ok(parsed)
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
    let mut format = Format::Points;
    let mut max_dim = 1usize;
    let mut modulus = 2u32;
    let mut reps = 5usize;
    let mut threads = 1usize;
    let mut parse_threads = 1usize;
    let mut mode = all_modes();
    let mut dense_storage = DenseStorage::Auto;
    let mut diagram_out = None;
    let mut emit_collapsed = None;

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
            "--format" => {
                let name = value()?;
                format = match name.as_str() {
                    "points" => Format::Points,
                    "lower-distance" => Format::LowerDistance,
                    "sparse" => Format::Sparse,
                    other => {
                        return Err(format!(
                            "unknown format {other}; use points, lower-distance, or sparse"
                        ));
                    }
                }
            }
            "--max-dim" => max_dim = parse_usize(&value()?, "--max-dim")?,
            "--modulus" => {
                modulus = u32::try_from(parse_usize(&value()?, "--modulus")?)
                    .map_err(|_| "--modulus is out of range".to_string())?
            }
            "--reps" => reps = parse_usize(&value()?, "--reps")?,
            "--threads" => threads = parse_usize(&value()?, "--threads")?,
            "--parse-threads" => parse_threads = parse_usize(&value()?, "--parse-threads")?,
            "--mode" => mode = parse_modes(&value()?)?,
            "--dense-storage" => dense_storage = parse_storage(&value()?)?,
            "--diagram-out" => diagram_out = Some(value()?),
            "--emit-collapsed" => emit_collapsed = Some(value()?),
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
    if threads == 0 {
        return Err("--threads must be at least 1".to_string());
    }
    if parse_threads == 0 {
        return Err("--parse-threads must be at least 1".to_string());
    }
    let entry = entry.unwrap_or_else(|| file_stem(&input));

    Ok(Args {
        input,
        entry,
        threshold,
        threshold_text,
        format,
        max_dim,
        modulus,
        reps,
        threads,
        parse_threads,
        mode,
        dense_storage,
        diagram_out,
        emit_collapsed,
    })
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

fn storage_name(storage: DenseStorage) -> &'static str {
    match storage {
        DenseStorage::Compact => "compact",
        DenseStorage::Square => "square",
        // Auto, and any form a later version of the library adds.
        _ => "auto",
    }
}

fn parse_usize(text: &str, flag: &str) -> Result<usize, String> {
    text.trim()
        .parse()
        .map_err(|_| format!("{flag} {text} is not a whole number"))
}
