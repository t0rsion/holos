//! Finite and kinetic synthesis command arguments.

use std::path::PathBuf;

use clap::Parser;

use super::version_string;

#[derive(Parser)]
#[command(
    name = "holos synthesize",
    version = version_string(),
    about = "Synthesize a minimum-cost plan for finite cohomology rank bounds",
    after_help = "Each state uses the full canonical cohomology space as a subspace. Check the result with: holos-check ARTIFACT"
)]
pub(crate) struct SynthesisCli {
    /// Output `HOLOSSYN` artifact
    pub(crate) output: PathBuf,

    /// Sparse state graph. Repeat in temporal order
    #[arg(long = "state", value_name = "GRAPH", required = true)]
    pub(crate) states: Vec<PathBuf>,

    /// Shared vertex count, including isolated vertices
    #[arg(long, value_name = "N")]
    pub(crate) vertices: usize,

    /// Fixed filtration scale
    #[arg(long = "at", value_name = "T")]
    pub(crate) scale: f64,

    /// Cohomology dimension constrained in every state
    #[arg(long = "homology-dim", value_name = "D")]
    pub(crate) dimension: usize,

    /// Largest allowed surviving rank in each full target space
    #[arg(long, value_name = "R", default_value_t = 0)]
    pub(crate) max_rank: usize,

    /// Candidate edge endpoints and positive cost. Repeat `--candidate U V COST`
    #[arg(long = "candidate", value_names = ["U", "V", "COST"], num_args = 3)]
    pub(crate) candidates: Vec<usize>,

    /// Largest selected action count
    #[arg(long, value_name = "N")]
    pub(crate) max_edits: usize,

    /// Largest producer topology call count
    #[arg(long, value_name = "N", default_value_t = 2_000_000)]
    pub(crate) oracle_limit: usize,

    /// Largest producer branch-and-bound node count
    #[arg(long, value_name = "N", default_value_t = 2_000_000)]
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
    name = "holos synthesize-kinetic",
    version = version_string(),
    about = "Synthesize a minimum-cost plan over an exact affine threshold schedule",
    after_help = "This command certifies a Rips cohomology rank condition. It does not certify physical sensor coverage. Check the result with: holos-check ARTIFACT"
)]
pub(crate) struct KineticSynthesisCli {
    /// Affine trajectory with rows `u v intercept velocity`
    pub(crate) input: PathBuf,

    /// Output `HOLOSSYN` artifact
    pub(crate) output: PathBuf,

    /// Number of vertices, including isolated vertices
    #[arg(long, value_name = "N")]
    pub(crate) vertices: usize,

    /// First trajectory time
    #[arg(long)]
    pub(crate) start: f64,

    /// Last trajectory time
    #[arg(long)]
    pub(crate) end: f64,

    /// Fixed filtration scale
    #[arg(long = "at", value_name = "T")]
    pub(crate) scale: f64,

    /// Cohomology dimension constrained throughout the trajectory
    #[arg(long = "homology-dim", value_name = "D", default_value_t = 1)]
    pub(crate) dimension: usize,

    /// Largest allowed surviving rank in each full target space
    #[arg(long, value_name = "R", default_value_t = 0)]
    pub(crate) max_rank: usize,

    /// Candidate edge endpoints and positive cost. Repeat `--candidate U V COST`
    #[arg(long = "candidate", value_names = ["U", "V", "COST"], num_args = 3)]
    pub(crate) candidates: Vec<usize>,

    /// Largest selected action count
    #[arg(long, value_name = "N")]
    pub(crate) max_edits: usize,

    /// Largest producer topology call count
    #[arg(long, value_name = "N", default_value_t = 2_000_000)]
    pub(crate) oracle_limit: usize,

    /// Largest producer branch-and-bound node count
    #[arg(long, value_name = "N", default_value_t = 2_000_000)]
    pub(crate) node_limit: usize,

    /// Coefficient field Z/p; must be a prime below 32768
    #[arg(long, value_name = "P", default_value_t = 2)]
    pub(crate) modulus: u32,

    /// Largest accepted trajectory input or output artifact
    #[arg(long, value_name = "BYTES", default_value_t = 1usize << 30)]
    pub(crate) max_artifact_bytes: usize,
}
