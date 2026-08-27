//! The `holos` command-line interface, callable as a library function.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::io::{self, OutputFormat};
use crate::{CollapseSchedule, DenseStorage, DistanceMatrix, Engine, RipsParams};
use clap::{Parser, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum InputFormat {
    PointCloud,
    LowerDistance,
    Sparse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Schedule {
    Serial,
    Ordered,
    Rounds,
}

impl From<Schedule> for CollapseSchedule {
    fn from(s: Schedule) -> Self {
        match s {
            Schedule::Serial => CollapseSchedule::Serial,
            Schedule::Ordered => CollapseSchedule::Ordered,
            Schedule::Rounds => CollapseSchedule::Rounds,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum EngineArg {
    Auto,
    Dense,
    Sparse,
}

impl From<EngineArg> for Engine {
    fn from(e: EngineArg) -> Self {
        match e {
            EngineArg::Auto => Engine::Auto,
            EngineArg::Dense => Engine::Dense,
            EngineArg::Sparse => Engine::Sparse,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum StorageArg {
    Auto,
    Compact,
    Square,
}

impl From<StorageArg> for DenseStorage {
    fn from(s: StorageArg) -> Self {
        match s {
            StorageArg::Auto => DenseStorage::Auto,
            StorageArg::Compact => DenseStorage::Compact,
            StorageArg::Square => DenseStorage::Square,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum DiagramFormat {
    Ripser,
    Csv,
}

fn version_string() -> &'static str {
    let profile = crate::BUILD_PROFILE;
    // clap without its "string" feature wants &'static str. The one-time
    // leak lives for the whole process anyway.
    Box::leak(format!("{} ({}, {profile})", crate::VERSION, crate::GIT_HASH).into_boxed_str())
}

#[derive(Parser)]
#[command(
    name = "holos",
    version = version_string(),
    about = "Vietoris-Rips persistent homology over a prime field"
)]
struct Cli {
    /// Input file: point cloud, condensed lower-distance matrix, or sparse
    /// "i j d" triplets
    input: PathBuf,

    /// Input format. Inferred from the extension when omitted: .csv, .pts,
    /// and .xyz select point-cloud, anything else selects lower-distance.
    /// Sparse is never inferred
    #[arg(long, value_enum)]
    format: Option<InputFormat>,

    /// Compute homology up to this dimension
    #[arg(long, value_name = "D", default_value_t = 1)]
    dim: usize,

    /// Filtration threshold. Defaults to the enclosing radius for dense
    /// input, and to no threshold for sparse input
    #[arg(long, value_name = "T")]
    threshold: Option<f64>,

    /// Coefficient field Z/p; must be a prime below 32768
    #[arg(long, value_name = "P", default_value_t = 2)]
    modulus: u32,

    /// Worker budget for the input parse and the reduction, and for the
    /// collapse when --collapse-schedule is ordered or rounds (1 =
    /// serial). A file under one mebibyte parses serially at any thread
    /// count. The diagram is identical at any thread count
    #[arg(long, value_name = "N", default_value_t = 1)]
    threads: usize,

    /// Collapse dominated edges before the engine runs. The diagram is
    /// identical either way; collapse statistics go to stderr
    #[arg(long)]
    collapse_edges: bool,

    /// Collapse schedule; requires --collapse-edges. serial is the
    /// default and, in the registered studies, the fastest end to end on
    /// most inputs. ordered gives the serial result on parallel workers.
    /// rounds gives the same result at every thread count, not the serial
    /// graph, and on some inputs a smaller reduced graph. The diagram is
    /// identical under every schedule
    #[arg(long, value_enum, value_name = "SCHEDULE")]
    collapse_schedule: Option<Schedule>,

    /// Engine for a dense input: auto reduces a low-density input as a
    /// thresholded graph when the graph fits its memory budget, dense and
    /// sparse force one engine. Sparse input always uses the sparse
    /// engine. The diagram is identical under every setting
    #[arg(long, value_enum, value_name = "ENGINE", default_value_t = EngineArg::Auto)]
    engine: EngineArg,

    /// Storage form for a dense run: auto holds the condensed lower
    /// triangle and converts to a full row-major matrix when a frozen
    /// work and memory rule selects it, compact forbids the full form,
    /// square forces it. A run routed to the sparse engine builds no full
    /// matrix under any setting. The diagram is identical under every
    /// setting
    #[arg(long, value_enum, value_name = "FORM", default_value_t = StorageArg::Auto)]
    dense_storage: StorageArg,

    /// Output format
    #[arg(long, value_enum, default_value_t = DiagramFormat::Ripser)]
    output: DiagramFormat,

    // Debug toggles. Each flag disables one pure optimization. Barcodes must
    // be identical either way (tested), so the flags are hidden from help.
    #[arg(long, hide = true)]
    no_emergent_pairs: bool,

    #[arg(long, hide = true)]
    no_apparent_pairs: bool,

    #[arg(long, hide = true)]
    no_adjacency_rows: bool,

    #[arg(long, hide = true)]
    no_clearing: bool,
}

fn infer_format(path: &Path) -> InputFormat {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext)
            if ["csv", "pts", "xyz"]
                .iter()
                .any(|k| ext.eq_ignore_ascii_case(k)) =>
        {
            InputFormat::PointCloud
        }
        _ => InputFormat::LowerDistance,
    }
}

// The pipeline owns the collapse result and, for a parallel schedule,
// shares one worker pool across both phases, so the statistics come out
// of it rather than from a separate standalone call.
fn report_collapse(collapsed: &crate::collapse::CollapsedRips) {
    let s = &collapsed.stats;
    let epoch = if collapsed.certificate.algorithm_version() == 2 {
        "rounds"
    } else {
        "passes"
    };
    eprintln!(
        "collapse: kept {} of {} edges, removed {}, {} {epoch}",
        s.output_edges, s.input_edges, s.removed_edges, s.epochs
    );
    eprintln!(
        "collapse detail: {} edge tests, {} witness segments, \
         max common neighborhood {}",
        s.edge_tests, s.witness_segments, s.max_common_neighborhood
    );
}

fn rips_params(cli: &Cli) -> RipsParams {
    RipsParams {
        max_dim: cli.dim,
        threshold: cli.threshold,
        modulus: cli.modulus,
        threads: cli.threads.max(1),
        use_emergent_pairs: !cli.no_emergent_pairs,
        use_apparent_pairs: !cli.no_apparent_pairs,
        use_clearing: !cli.no_clearing,
        use_adjacency_rows: !cli.no_adjacency_rows,
        collapse_edges: false,
        collapse_schedule: cli.collapse_schedule.unwrap_or(Schedule::Serial).into(),
        engine: cli.engine.into(),
        dense_storage: cli.dense_storage.into(),
    }
}

fn solve_sparse(cli: &Cli, params: &RipsParams) -> crate::Result<(crate::Diagram, usize)> {
    let dist = io::read_sparse_matrix(&cli.input, params.threads)?;
    match cli.threshold {
        Some(t) => eprintln!(
            "{} points, {} edges, threshold {t}",
            dist.len(),
            dist.num_edges()
        ),
        None => eprintln!(
            "{} points, {} edges, no threshold (all listed edges)",
            dist.len(),
            dist.num_edges()
        ),
    }
    let n = dist.len();
    let diagram = if cli.collapse_edges {
        crate::collapse_and_solve(&dist, params, report_collapse)?
    } else {
        crate::rips_persistence_sparse(&dist, params)?
    };
    Ok((diagram, n))
}

fn read_dense(cli: &Cli, format: InputFormat, threads: usize) -> crate::Result<DistanceMatrix> {
    match format {
        InputFormat::PointCloud => {
            let points = io::read_point_cloud(&cli.input, threads)?;
            DistanceMatrix::from_points(&points)
        }
        _ => io::read_lower_distance_matrix(&cli.input, threads),
    }
}

fn solve_dense(
    cli: &Cli,
    format: InputFormat,
    params: &RipsParams,
) -> crate::Result<(crate::Diagram, usize)> {
    let dist = read_dense(cli, format, params.threads)?;
    match cli.threshold {
        Some(t) => eprintln!("{} points, threshold {t}", dist.len()),
        None => eprintln!(
            "{} points, threshold {} (enclosing radius)",
            dist.len(),
            dist.enclosing_radius()
        ),
    }
    let n = dist.len();
    let diagram = if cli.collapse_edges {
        crate::collapse_and_solve(&dist, params, report_collapse)?
    } else {
        crate::rips_persistence(&dist, params)?
    };
    Ok((diagram, n))
}

fn write_output(cli: &Cli, diagram: &crate::Diagram, n_points: usize) -> crate::Result<()> {
    let output = match cli.output {
        DiagramFormat::Ripser => OutputFormat::Ripser,
        DiagramFormat::Csv => OutputFormat::Csv,
    };
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::with_capacity(1 << 16, stdout.lock());
    io::write_diagram(
        &mut out,
        diagram,
        output,
        cli.dim.min(n_points.saturating_sub(1)),
    )?;
    out.flush().map_err(|e| crate::Error::Io(e.to_string()))
}

fn run(cli: Cli) -> crate::Result<()> {
    if cli.collapse_schedule.is_some() && !cli.collapse_edges {
        return Err(crate::Error::InvalidInput(
            "--collapse-schedule requires --collapse-edges".into(),
        ));
    }
    let format = cli.format.unwrap_or_else(|| infer_format(&cli.input));
    let params = rips_params(&cli);
    let (mut diagram, n_points) = match format {
        InputFormat::Sparse => solve_sparse(&cli, &params)?,
        _ => solve_dense(&cli, format, &params)?,
    };
    diagram.canonicalize();
    write_output(&cli, &diagram, n_points)
}

/// Run the `holos` CLI on `argv` and return the process exit code.
///
/// `argv[0]` is the program name. The binary and the Python bindings both
/// enter here, so the CLI behaves the same either way.
pub fn run_cli<I, T>(argv: I) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = match Cli::try_parse_from(argv) {
        Ok(cli) => cli,
        Err(e) => {
            // clap handles --help and --version here. Both arrive as
            // "errors" with exit code 0 and preformatted output.
            let code = e.exit_code();
            let _ = e.print();
            return code;
        }
    };
    match run(cli) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("holos: {e}");
            1
        }
    }
}
