//! Proof, index, and interface command arguments.

use std::path::PathBuf;

use clap::Parser;

use super::{InputFormat, version_string};

#[derive(Parser)]
#[command(
    name = "holos prove",
    version = version_string(),
    about = "Build one checked proof DAG for a sparse persistence trajectory",
    after_help = "Check the result with: holos-check PROOF"
)]
pub(crate) struct ProveCli {
    /// Initial point cloud, lower-distance matrix, or sparse triplets
    pub(crate) input: PathBuf,

    /// Output `HOLOSPF` proof DAG
    pub(crate) output: PathBuf,

    /// Later inputs in trajectory order
    #[arg(value_name = "UPDATE")]
    pub(crate) updates: Vec<PathBuf>,

    /// Input format shared by the initial input and all updates. Inferred
    /// from each file extension when omitted. Sparse is never inferred
    #[arg(long, value_enum)]
    pub(crate) format: Option<InputFormat>,

    /// Filtration threshold. Defaults to the enclosing radius for each
    /// dense input, and to no threshold for sparse input
    #[arg(long, value_name = "T")]
    pub(crate) threshold: Option<f64>,

    /// Coefficient field Z/p; must be a prime below 32768
    #[arg(long, value_name = "P", default_value_t = 2)]
    pub(crate) modulus: u32,

    /// Worker threads for parsing and persistence reduction
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub(crate) threads: usize,
}

#[derive(Parser)]
#[command(
    name = "holos index",
    version = version_string(),
    about = "Compile a versioned exact persistence index and proof stream",
    after_help = "Check the stream with: holos-check SNAPSHOT [RECORD ...]"
)]
pub(crate) struct IndexCli {
    /// Initial point cloud, lower-distance matrix, or sparse triplets
    pub(crate) input: PathBuf,

    /// Output initial `HOLOSIP` checkpoint
    pub(crate) snapshot: PathBuf,

    /// Later input. Repeat once for each --record in the same order
    #[arg(long, value_name = "FILE")]
    pub(crate) update: Vec<PathBuf>,

    /// Output proof record. A fixed envelope writes `HOLOSDP`; an envelope
    /// change writes a new `HOLOSIP` checkpoint. Repeat once per --update
    #[arg(long, value_name = "FILE")]
    pub(crate) record: Vec<PathBuf>,

    /// Input format shared by every input. Inferred from each extension when
    /// omitted. Sparse is never inferred
    #[arg(long, value_enum)]
    pub(crate) format: Option<InputFormat>,

    /// Fixed filtration threshold
    #[arg(long, value_name = "T")]
    pub(crate) threshold: Option<f64>,

    /// Coefficient field Z/p; must be a prime below 32768
    #[arg(long, value_name = "P", default_value_t = 2)]
    pub(crate) modulus: u32,

    /// Highest homology dimension to maintain and certify
    #[arg(long, value_name = "D", default_value_t = 1)]
    pub(crate) dim: usize,

    /// Worker budget for parsing, reduction, and independent alternatives
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub(crate) threads: usize,

    /// Largest vertex separator considered by deterministic search
    #[arg(long, value_name = "W", default_value_t = 4)]
    pub(crate) separator_width: usize,

    /// Largest total separator candidate count
    #[arg(long, value_name = "N", default_value_t = 100_000)]
    pub(crate) separator_search_limit: usize,

    /// A scope at or below this vertex count remains a leaf
    #[arg(long, value_name = "N", default_value_t = 4)]
    pub(crate) leaf_vertices: usize,

    /// Retain a full reduction at every parent interface
    #[arg(long)]
    pub(crate) materialize_interfaces: bool,
}

#[derive(Parser)]
#[command(
    name = "holos interface",
    version = version_string(),
    about = "Build a checked filtered chain core relative to protected vertices",
    after_help = "Check the result with: holos-check CERTIFICATE"
)]
pub(crate) struct InterfaceCli {
    /// Input point cloud, lower-distance matrix, or sparse triplets
    pub(crate) input: PathBuf,

    /// Output `HOLOSRI` relative-interface certificate
    pub(crate) output: PathBuf,

    /// Input format. Inferred from the extension when omitted. Sparse is
    /// never inferred
    #[arg(long, value_enum)]
    pub(crate) format: Option<InputFormat>,

    /// Fixed filtration threshold
    #[arg(long, value_name = "T")]
    pub(crate) threshold: Option<f64>,

    /// Highest homology dimension to certify
    #[arg(long, value_name = "D", default_value_t = 1)]
    pub(crate) dim: usize,

    /// Coefficient field Z/p; must be a prime below 32768
    #[arg(long, value_name = "P", default_value_t = 2)]
    pub(crate) modulus: u32,

    /// Vertex retained as part of the separator subcomplex. Repeat this flag
    /// for each protected vertex
    #[arg(long = "protect", value_name = "VERTEX")]
    pub(crate) protected: Vec<usize>,

    /// Worker budget for input parsing
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub(crate) threads: usize,
}

#[derive(Parser)]
#[command(
    name = "holos merge-interfaces",
    version = version_string(),
    about = "Durably compose proof-carrying relative interface shards",
    after_help = "Check the result with: holos-check MANIFEST OBJECT [OBJECT ...]"
)]
pub(crate) struct MergeInterfacesCli {
    /// Content-addressed store directory
    pub(crate) store: PathBuf,

    /// Output `HOLOSDM` commit manifest
    pub(crate) manifest: PathBuf,

    /// Output composed `HOLOSRI` result
    pub(crate) result: PathBuf,

    /// Ordered child `HOLOSRI` artifacts
    #[arg(required = true)]
    pub(crate) shards: Vec<PathBuf>,

    /// Common separator vertex. Repeat for each separator vertex
    #[arg(long = "separator", value_name = "VERTEX")]
    pub(crate) separator: Vec<usize>,

    /// Vertex protected in the final output. Repeat for each vertex
    #[arg(long = "protect", value_name = "VERTEX")]
    pub(crate) protected: Vec<usize>,

    /// Largest artifact or manifest accepted
    #[arg(long, value_name = "BYTES", default_value_t = 1usize << 30)]
    pub(crate) max_artifact_bytes: usize,
}
