//! Artifact verification command arguments.

use std::path::PathBuf;

use clap::Parser;

use super::{InputFormat, version_string};

#[derive(Parser)]
#[command(
    name = "holos verify-collapse",
    version = version_string(),
    about = "Verify a collapse artifact against its input"
)]
pub(crate) struct VerifyCli {
    /// Original point cloud, lower-distance matrix, or sparse triplets
    pub(crate) input: PathBuf,

    /// Collapse artifact written by --collapse-certificate
    pub(crate) artifact: PathBuf,

    /// Input format. Inferred from the input extension when omitted.
    /// Sparse is never inferred
    #[arg(long, value_enum)]
    pub(crate) format: Option<InputFormat>,

    /// Filtration threshold used for the collapse
    #[arg(long, value_name = "T")]
    pub(crate) threshold: Option<f64>,

    /// Worker threads for parsing the original input
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub(crate) threads: usize,

    /// Largest artifact accepted before the file is read
    #[arg(long, value_name = "BYTES", default_value_t = 1usize << 30)]
    pub(crate) max_artifact_bytes: usize,
}

#[derive(Parser)]
#[command(
    name = "holos verify-atlas",
    version = version_string(),
    about = "Verify a proof-carrying H0 and H1 atlas against its input"
)]
pub(crate) struct VerifyAtlasCli {
    /// Original point cloud, lower-distance matrix, or sparse triplets
    pub(crate) input: PathBuf,

    /// Atlas written by --atlas
    pub(crate) artifact: PathBuf,

    /// Input format. Inferred from the input extension when omitted.
    /// Sparse is never inferred
    #[arg(long, value_enum)]
    pub(crate) format: Option<InputFormat>,

    /// Worker threads for parsing and point-graph construction
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub(crate) threads: usize,

    /// Largest artifact accepted while reading the file
    #[arg(long, value_name = "BYTES", default_value_t = 1usize << 30)]
    pub(crate) max_artifact_bytes: usize,
}

#[derive(Parser)]
#[command(
    name = "holos verify-trajectory",
    version = version_string(),
    about = "Verify a self-contained persistence trajectory"
)]
pub(crate) struct VerifyTrajectoryCli {
    /// Trajectory artifact
    pub(crate) artifact: PathBuf,

    /// Largest artifact accepted while reading the file
    #[arg(long, value_name = "BYTES", default_value_t = 1usize << 30)]
    pub(crate) max_artifact_bytes: usize,
}

#[derive(Parser)]
#[command(
    name = "holos verify-program",
    version = version_string(),
    about = "Verify a compositional H0 and H1 program against its input"
)]
pub(crate) struct VerifyProgramCli {
    /// Original point cloud, lower-distance matrix, or sparse triplets
    pub(crate) input: PathBuf,

    /// Program written by --program
    pub(crate) artifact: PathBuf,

    /// Input format. Inferred from the input extension when omitted.
    /// Sparse is never inferred
    #[arg(long, value_enum)]
    pub(crate) format: Option<InputFormat>,

    /// Worker threads for parsing and point-graph construction
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub(crate) threads: usize,

    /// Largest artifact accepted while reading the file
    #[arg(long, value_name = "BYTES", default_value_t = 1usize << 30)]
    pub(crate) max_artifact_bytes: usize,
}

#[derive(Parser)]
#[command(
    name = "holos verify-program-trace",
    version = version_string(),
    about = "Verify a self-contained compositional persistence trace"
)]
pub(crate) struct VerifyProgramTraceCli {
    /// Program trace artifact
    pub(crate) artifact: PathBuf,

    /// Largest artifact accepted while reading the file
    #[arg(long, value_name = "BYTES", default_value_t = 1usize << 30)]
    pub(crate) max_artifact_bytes: usize,
}

#[derive(Parser)]
#[command(
    name = "holos verify-intervention",
    version = version_string(),
    about = "Verify a self-contained finite H1 intervention"
)]
pub(crate) struct VerifyInterventionCli {
    /// Intervention artifact
    pub(crate) artifact: PathBuf,

    /// Largest artifact accepted while reading the file
    #[arg(long, value_name = "BYTES", default_value_t = 1usize << 30)]
    pub(crate) max_artifact_bytes: usize,
}
