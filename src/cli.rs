//! The `holos` command-line interface, callable as a library function.

use std::path::{Path, PathBuf};

use crate::io::{self, OutputFormat};
use crate::{DistanceMatrix, RipsParams};
use clap::{Parser, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum InputFormat {
    PointCloud,
    LowerDistance,
    Sparse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum DiagramFormat {
    Ripser,
    Csv,
}

fn version_string() -> &'static str {
    let profile = crate::BUILD_PROFILE;
    // clap without its "string" feature wants &'static str; the one-time leak
    // lives for the whole process anyway.
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

    /// Input format; inferred from the extension when omitted
    /// (.csv/.pts/.xyz: point-cloud, otherwise lower-distance; sparse is
    /// never inferred)
    #[arg(long, value_enum)]
    format: Option<InputFormat>,

    /// Compute homology up to this dimension
    #[arg(long, value_name = "D", default_value_t = 1)]
    dim: usize,

    /// Filtration threshold; defaults to the enclosing radius (dense input)
    /// or to no threshold (sparse input)
    #[arg(long, value_name = "T")]
    threshold: Option<f64>,

    /// Coefficient field Z/p; must be a prime below 32768
    #[arg(long, value_name = "P", default_value_t = 2)]
    modulus: u32,

    /// Output format
    #[arg(long, value_enum, default_value_t = DiagramFormat::Ripser)]
    output: DiagramFormat,

    // Debug toggles: each disables one pure optimization; barcodes must be
    // identical either way (tested), so they are hidden from help.
    #[arg(long, hide = true)]
    no_emergent_pairs: bool,

    #[arg(long, hide = true)]
    no_apparent_pairs: bool,

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

fn run(cli: Cli) -> crate::Result<()> {
    let format = cli.format.unwrap_or_else(|| infer_format(&cli.input));
    let params = RipsParams {
        max_dim: cli.dim,
        // None lets the library apply the input's own default.
        threshold: cli.threshold,
        modulus: cli.modulus,
        use_emergent_pairs: !cli.no_emergent_pairs,
        use_apparent_pairs: !cli.no_apparent_pairs,
        use_clearing: !cli.no_clearing,
    };
    let (mut diagram, n_points) = match format {
        InputFormat::Sparse => {
            let dist = io::read_sparse_matrix(&cli.input)?;
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
            (crate::rips_persistence_sparse(&dist, &params)?, n)
        }
        _ => {
            let dist = match format {
                InputFormat::PointCloud => {
                    let points = io::read_point_cloud(&cli.input)?;
                    DistanceMatrix::from_points(&points)?
                }
                _ => io::read_lower_distance_matrix(&cli.input)?,
            };
            match cli.threshold {
                Some(t) => eprintln!("{} points, threshold {t}", dist.len()),
                None => eprintln!(
                    "{} points, threshold {} (enclosing radius)",
                    dist.len(),
                    dist.enclosing_radius()
                ),
            }
            let n = dist.len();
            (crate::rips_persistence(&dist, &params)?, n)
        }
    };
    diagram.canonicalize();
    let output = match cli.output {
        DiagramFormat::Ripser => OutputFormat::Ripser,
        DiagramFormat::Csv => OutputFormat::Csv,
    };
    let stdout = std::io::stdout();
    io::write_diagram(
        &mut stdout.lock(),
        &diagram,
        output,
        cli.dim.min(n_points.saturating_sub(1)),
    )
}

/// Run the `holos` CLI on the given arguments (`argv[0]` is the program
/// name) and return the process exit code. The binary and the Python
/// bindings both enter here, so the CLI behaves the same either way.
pub fn run_cli<I, T>(argv: I) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = match Cli::try_parse_from(argv) {
        Ok(cli) => cli,
        Err(e) => {
            // clap handles --help/--version here; both are "errors" with
            // exit code 0 and preformatted output.
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
