//! Cohomology, circular-coordinate, and kinetic command arguments.

use std::path::PathBuf;

use clap::Parser;

use super::{InputFormat, version_string};

#[derive(Parser)]
#[command(
    name = "holos cohomology",
    version = version_string(),
    about = "Compute canonical fixed-scale cohomology and an optional exact relation"
)]
pub(crate) struct CohomologyCli {
    /// First input graph
    pub(crate) input: PathBuf,

    /// Optional second graph related through the common active subcomplex
    pub(crate) other: Option<PathBuf>,

    /// Fixed filtration scale
    #[arg(long = "at", value_name = "T")]
    pub(crate) scale: f64,

    /// Cohomology dimension
    #[arg(long = "homology-dim", value_name = "D")]
    pub(crate) dimension: usize,

    /// Coefficient field Z/p; must be a prime below 32768
    #[arg(long, value_name = "P", default_value_t = 2)]
    pub(crate) modulus: u32,

    /// Input format. Inferred when omitted. Sparse is never inferred
    #[arg(long, value_enum)]
    pub(crate) format: Option<InputFormat>,

    /// Worker budget for input parsing
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub(crate) threads: usize,
}

#[derive(Parser)]
#[command(
    name = "holos circular",
    version = version_string(),
    about = "Build and certify a class-aware circular coordinate",
    after_help = "COCYCLE contains `u v coefficient` rows in Ripser orientation. With --class, it is the JSON written by --representatives. --integral-lift accepts signed `u v coefficient` rows. Automatic lifting needs an odd prime; a supplied lift also supports modulus 2 without --continue-to. Check OUTPUT with: holos-check OUTPUT"
)]
pub(crate) struct CircularCli {
    /// Input graph
    pub(crate) input: PathBuf,

    /// Ripser-shaped H1 cocycle rows, or class-spaces JSON with --class
    pub(crate) cocycle: PathBuf,

    /// Output `HOLOSCC` artifact
    pub(crate) output: PathBuf,

    /// Fixed filtration scale. Required for raw cocycle rows. A class record
    /// carries its own scale
    #[arg(long = "at", value_name = "T")]
    pub(crate) scale: Option<f64>,

    /// Coefficient field used to produce raw cocycle rows. Default 47. A
    /// class record carries its own field
    #[arg(long, value_name = "P")]
    pub(crate) modulus: Option<u32>,

    /// Checked integer cocycle rows for a supplied lift. Modulus 2 cannot use
    /// --continue-to.
    #[arg(long = "integral-lift", value_name = "FILE")]
    pub(crate) integral_lift: Option<PathBuf>,

    /// Zero-based class-space and basis positions in class-spaces JSON
    #[arg(long, value_names = ["SPACE", "BASIS"], num_args = 2)]
    pub(crate) class: Option<Vec<usize>>,

    /// Optional changed graph for conservative continuation
    #[arg(long = "continue-to", value_name = "GRAPH")]
    pub(crate) other: Option<PathBuf>,

    /// Write the first state's vertex phases
    #[arg(long, value_name = "FILE")]
    pub(crate) phases: Option<PathBuf>,

    /// Write continued phases when continuation is unique
    #[arg(long, value_name = "FILE", requires = "other")]
    pub(crate) continued_phases: Option<PathBuf>,

    /// Largest accepted relative normal-equation residual
    #[arg(long, value_name = "R", default_value_t = 1e-10)]
    pub(crate) tolerance: f64,

    /// Largest harmonic solver iteration count
    #[arg(long, value_name = "N", default_value_t = 10_000)]
    pub(crate) max_iterations: usize,

    /// Input format. Inferred when omitted. Sparse is never inferred
    #[arg(long, value_enum)]
    pub(crate) format: Option<InputFormat>,

    /// Worker budget for input parsing
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub(crate) threads: usize,

    /// Largest accepted cocycle, class-record, or integral-lift file
    #[arg(long, value_name = "BYTES", default_value_t = 1usize << 30)]
    pub(crate) max_record_bytes: usize,
}

#[derive(Parser)]
#[command(
    name = "holos kinetic",
    version = version_string(),
    about = "Certify events in an affine edge-weight trajectory"
)]
pub(crate) struct KineticCli {
    /// Text file with `u v intercept velocity` on each line
    pub(crate) input: PathBuf,

    /// Vertex count, including isolated vertices
    #[arg(long, value_name = "N")]
    pub(crate) vertices: usize,

    /// First trajectory time
    #[arg(long)]
    pub(crate) start: f64,

    /// Last trajectory time
    #[arg(long)]
    pub(crate) end: f64,

    /// Fixed scale whose crossings are reported
    #[arg(long = "at", value_name = "T")]
    pub(crate) scale: Option<f64>,

    /// Also compute class relations in this cohomology dimension
    #[arg(long = "homology-dim", value_name = "D", requires = "scale")]
    pub(crate) dimension: Option<usize>,

    /// Coefficient field for class relations
    #[arg(long, value_name = "P", default_value_t = 2)]
    pub(crate) modulus: u32,

    /// Write a checked `HOLOSZZ` fixed-scale zigzag artifact
    #[arg(long, value_name = "FILE", requires = "scale", requires = "dimension")]
    pub(crate) zigzag: Option<PathBuf>,

    /// Largest accepted trajectory input or output artifact
    #[arg(long, value_name = "BYTES", default_value_t = 1usize << 30)]
    pub(crate) max_artifact_bytes: usize,
}
