//! Cohomology and persistence intervention command arguments.

use std::path::PathBuf;

use clap::Parser;

use super::{InputFormat, version_string};

#[derive(Parser)]
#[command(
    name = "holos intervene-cohomology",
    version = version_string(),
    about = "Certify a minimum-cost fixed-scale cohomology intervention",
    after_help = "Check the result with: holos-check ARTIFACT"
)]
pub(crate) struct CohomologyInterventionCli {
    /// Input graph
    pub(crate) input: PathBuf,

    /// Output `HOLOSCI` artifact
    pub(crate) output: PathBuf,

    /// Fixed filtration scale
    #[arg(long = "at", value_name = "T")]
    pub(crate) scale: f64,

    /// Target cohomology dimension
    #[arg(long = "homology-dim", value_name = "D")]
    pub(crate) dimension: usize,

    /// Position in the canonical cohomology basis
    #[arg(long, value_name = "N")]
    pub(crate) target: usize,

    /// Candidate edge endpoints and positive cost. Repeat `--candidate U V COST`
    #[arg(long = "candidate", value_names = ["U", "V", "COST"], num_args = 3)]
    pub(crate) candidates: Vec<usize>,

    /// Largest selected edge count
    #[arg(long, value_name = "N")]
    pub(crate) max_edits: usize,

    /// Largest distinct topological oracle call count
    #[arg(long, value_name = "N", default_value_t = 1_000_000)]
    pub(crate) oracle_limit: usize,

    /// Largest branch-and-bound node count
    #[arg(long, value_name = "N", default_value_t = 1_000_000)]
    pub(crate) node_limit: usize,

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
    name = "holos plan-links",
    version = version_string(),
    about = "Plan one minimum-cost link set across network scenarios",
    after_help = "Check the result with: holos-check ARTIFACT"
)]
pub(crate) struct LinkPlanCli {
    /// Output `HOLOSCI` artifact
    pub(crate) output: PathBuf,

    /// Sparse scenario graph. Repeat once per target
    #[arg(long = "scenario", value_name = "GRAPH", required = true)]
    pub(crate) scenarios: Vec<PathBuf>,

    /// Canonical basis position for each scenario, in scenario order
    #[arg(long = "target", value_name = "N", required = true)]
    pub(crate) targets: Vec<usize>,

    /// Shared vertex count, including isolated vertices
    #[arg(long, value_name = "N")]
    pub(crate) vertices: usize,

    /// Fixed filtration scale
    #[arg(long = "at", value_name = "T")]
    pub(crate) scale: f64,

    /// Target cohomology dimension
    #[arg(long = "homology-dim", value_name = "D")]
    pub(crate) dimension: usize,

    /// Candidate link endpoints and positive cost. Repeat `--candidate U V COST`
    #[arg(long = "candidate", value_names = ["U", "V", "COST"], num_args = 3)]
    pub(crate) candidates: Vec<usize>,

    /// Largest selected link count
    #[arg(long, value_name = "N")]
    pub(crate) max_edits: usize,

    /// Largest distinct topological oracle call count
    #[arg(long, value_name = "N", default_value_t = 1_000_000)]
    pub(crate) oracle_limit: usize,

    /// Largest branch-and-bound node count
    #[arg(long, value_name = "N", default_value_t = 1_000_000)]
    pub(crate) node_limit: usize,

    /// Coefficient field Z/p; must be a prime below 32768
    #[arg(long, value_name = "P", default_value_t = 2)]
    pub(crate) modulus: u32,

    /// Worker budget for input parsing
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub(crate) threads: usize,
}

#[derive(Parser)]
#[command(
    name = "holos intervene",
    version = version_string(),
    about = "Certify a finite H1 lifetime intervention"
)]
pub(crate) struct InterveneCli {
    /// Original input bound to the program
    pub(crate) input: PathBuf,

    /// Checked compositional program written by --program
    pub(crate) program: PathBuf,

    /// Output `HOLOSINT` artifact
    pub(crate) output: PathBuf,

    /// Zero-based H1 class-space index in the program result
    #[arg(long, value_name = "N")]
    pub(crate) space: usize,

    /// Requested latest death scale
    #[arg(long, value_name = "T")]
    pub(crate) before: f64,

    /// Largest candidate count. The current intervention uses one candidate
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub(crate) budget: usize,

    /// Input format. Inferred from the input extension when omitted.
    /// Sparse is never inferred
    #[arg(long, value_enum)]
    pub(crate) format: Option<InputFormat>,

    /// Worker threads for parsing and point-graph construction
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub(crate) threads: usize,

    /// Largest program accepted while reading the file
    #[arg(long, value_name = "BYTES", default_value_t = 1usize << 30)]
    pub(crate) max_artifact_bytes: usize,
}
