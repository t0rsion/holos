use std::path::{Path, PathBuf};

use clap::{Parser, ValueEnum};
use holos_tda::io::{self, OutputFormat};
use holos_tda::{DistanceMatrix, RipsParams};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum InputFormat {
    PointCloud,
    LowerDistance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum DiagramFormat {
    Ripser,
    Csv,
}

fn version_string() -> &'static str {
    let profile = holos_tda::BUILD_PROFILE;
    // clap without its "string" feature wants &'static str; the one-time leak
    // lives for the whole process anyway.
    Box::leak(
        format!(
            "{} ({}, {profile})",
            holos_tda::VERSION,
            holos_tda::GIT_HASH
        )
        .into_boxed_str(),
    )
}

#[derive(Parser)]
#[command(
    name = "holos",
    version = version_string(),
    about = "Vietoris-Rips persistent homology over Z/2"
)]
struct Cli {
    /// Input file: point cloud or condensed lower-distance matrix
    input: PathBuf,

    /// Input format; inferred from the extension when omitted
    /// (.csv/.pts/.xyz: point-cloud, otherwise lower-distance)
    #[arg(long, value_enum)]
    format: Option<InputFormat>,

    /// Compute homology up to this dimension
    #[arg(long, value_name = "D", default_value_t = 1)]
    dim: usize,

    /// Filtration threshold; defaults to the enclosing radius
    #[arg(long, value_name = "T")]
    threshold: Option<f64>,

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

fn run(cli: Cli) -> holos_tda::Result<()> {
    let format = cli.format.unwrap_or_else(|| infer_format(&cli.input));
    let dist = match format {
        InputFormat::PointCloud => {
            let points = io::read_point_cloud(&cli.input)?;
            DistanceMatrix::from_points(&points)?
        }
        InputFormat::LowerDistance => io::read_lower_distance_matrix(&cli.input)?,
    };
    match cli.threshold {
        Some(t) => eprintln!("{} points, threshold {t}", dist.len()),
        None => eprintln!(
            "{} points, threshold {} (enclosing radius)",
            dist.len(),
            dist.enclosing_radius()
        ),
    }
    let params = RipsParams {
        max_dim: cli.dim,
        // None lets the library apply its own enclosing-radius default.
        threshold: cli.threshold,
        use_emergent_pairs: !cli.no_emergent_pairs,
        use_apparent_pairs: !cli.no_apparent_pairs,
        use_clearing: !cli.no_clearing,
    };
    let mut diagram = holos_tda::rips_persistence(&dist, &params)?;
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
        cli.dim.min(dist.len().saturating_sub(1)),
    )
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("holos: {e}");
        std::process::exit(1);
    }
}
