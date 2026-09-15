//! Arguments for source-bound persistent-class and circular-coordinate output.

use std::path::PathBuf;

use clap::Parser;

use super::{InputFormat, version_string};

#[derive(Parser)]
#[command(
    name = "holos persistent-class",
    version = version_string(),
    about = "Build a checked persistent H1 class artifact",
    after_help = "OUTPUT is a HOLOSPC artifact. Check it with: holos-check OUTPUT"
)]
pub(crate) struct PersistentClassCli {
    /// Source graph in the selected input format
    pub(crate) input: PathBuf,

    /// Output `HOLOSPC` artifact
    pub(crate) output: PathBuf,

    /// Zero-based interval-space position in the native H1 diagram
    #[arg(long, value_name = "N")]
    pub(crate) space: usize,

    /// Zero-based basis position in the selected interval space
    #[arg(long, value_name = "N")]
    pub(crate) basis: usize,

    /// Coefficient field Z/p; must be a prime below 32768
    #[arg(long, value_name = "P", default_value_t = 47)]
    pub(crate) modulus: u32,

    /// Filtration threshold. Sparse input defaults to all listed edges
    #[arg(long, value_name = "T")]
    pub(crate) threshold: Option<f64>,

    /// Input format. Inferred when omitted. Sparse is never inferred
    #[arg(long, value_enum)]
    pub(crate) format: Option<InputFormat>,

    /// Worker budget for input parsing and persistence reduction
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub(crate) threads: usize,

    /// Largest accepted artifact envelope
    #[arg(long, value_name = "BYTES", default_value_t = 1usize << 30)]
    pub(crate) max_artifact_bytes: usize,

    /// Write a concise JSON summary
    #[arg(long, value_name = "FILE")]
    pub(crate) record: Option<PathBuf>,
}

#[derive(Parser)]
#[command(
    name = "holos persistent-circular",
    version = version_string(),
    about = "Build a checked circular coordinate for one persistent H1 class",
    after_help = "OUTPUT is a HOLOSPH artifact. Check it with: holos-check OUTPUT"
)]
pub(crate) struct PersistentCircularCli {
    /// Source graph in the selected input format
    pub(crate) input: PathBuf,

    /// Output `HOLOSPH` artifact
    pub(crate) output: PathBuf,

    /// Zero-based interval-space position in the native H1 diagram
    #[arg(long, value_name = "N")]
    pub(crate) space: usize,

    /// Zero-based basis position in the selected interval space
    #[arg(long, value_name = "N")]
    pub(crate) basis: usize,

    /// Coefficient field Z/p; must be a prime below 32768
    #[arg(long, value_name = "P", default_value_t = 47)]
    pub(crate) modulus: u32,

    /// Filtration threshold. Sparse input defaults to all listed edges
    #[arg(long, value_name = "T")]
    pub(crate) threshold: Option<f64>,

    /// Checked integer cocycle rows. This also supports modulus 2
    #[arg(long = "integral-lift", value_name = "FILE")]
    pub(crate) integral_lift: Option<PathBuf>,

    /// Write the coordinate phase sidecar
    #[arg(long, value_name = "FILE")]
    pub(crate) phases: Option<PathBuf>,

    /// Largest accepted relative normal-equation residual
    #[arg(long, value_name = "R", default_value_t = 1e-10)]
    pub(crate) tolerance: f64,

    /// Largest harmonic solver iteration count
    #[arg(long, value_name = "N", default_value_t = 10_000)]
    pub(crate) max_iterations: usize,

    /// Input or output artifact byte limit
    #[arg(long, value_name = "BYTES", default_value_t = 1usize << 30)]
    pub(crate) max_artifact_bytes: usize,

    /// Input format. Inferred when omitted. Sparse is never inferred
    #[arg(long, value_enum)]
    pub(crate) format: Option<InputFormat>,

    /// Worker budget for input parsing and persistence reduction
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub(crate) threads: usize,

    /// Write a concise JSON summary
    #[arg(long, value_name = "FILE")]
    pub(crate) record: Option<PathBuf>,
}
