//! Static and affine coverage command arguments.

use std::path::PathBuf;

use clap::Parser;

use super::version_string;

#[derive(Parser)]
#[command(
    name = "holos cover",
    version = version_string(),
    about = "Certify a minimum-cost failure-tolerant relative coverage plan",
    after_help = "Physical coverage is conditional on the controlled-boundary domain and sensor-placement assumptions. STATES is `all` or a comma-separated state-index list. Check the result with: holos-check ARTIFACT"
)]
pub(crate) struct CoverageCli {
    /// Output `HOLOSCOV` artifact
    pub(crate) output: PathBuf,

    /// Sparse graph of possible communication edges. Repeat in state order
    #[arg(long = "state", value_name = "GRAPH", required = true)]
    pub(crate) states: Vec<PathBuf>,

    /// Planar point file for each state. When present, the artifact checks
    /// the fence polygon and the complete Euclidean radius graph.
    #[arg(long = "coordinates", value_name = "POINTS")]
    pub(crate) coordinates: Vec<PathBuf>,

    /// Shared sensor count, including inactive sensors
    #[arg(long, value_name = "N")]
    pub(crate) vertices: usize,

    /// Broadcast radius used to retain communication edges
    #[arg(long, value_name = "R")]
    pub(crate) broadcast_radius: f64,

    /// Sensing-disc radius used by the controlled-boundary theorem
    #[arg(long, value_name = "R")]
    pub(crate) sensing_radius: f64,

    /// Ordered fence cycle as comma-separated sensor labels
    #[arg(long, value_name = "V0,V1,...", value_delimiter = ',', required = true)]
    pub(crate) fence: Vec<usize>,

    /// Initially active non-fence sensors, as comma-separated labels
    #[arg(long, value_name = "V0,V1,...", value_delimiter = ',')]
    pub(crate) base: Vec<usize>,

    /// Sensors subject to failure, as comma-separated labels
    #[arg(long, value_name = "V0,V1,...", value_delimiter = ',')]
    pub(crate) failable: Vec<usize>,

    /// Largest simultaneous failure count
    #[arg(long, value_name = "F", default_value_t = 0)]
    pub(crate) failure_budget: usize,

    /// Sensor, positive cost, and state support. Repeat `--candidate V COST STATES`
    #[arg(long = "candidate", value_names = ["V", "COST", "STATES"], num_args = 3)]
    pub(crate) candidates: Vec<String>,

    /// Largest selected sensor count
    #[arg(long, value_name = "N")]
    pub(crate) max_activations: usize,

    /// Coefficient field Z/p; must be a prime below 32768
    #[arg(long, value_name = "P", default_value_t = 2)]
    pub(crate) modulus: u32,

    /// Largest producer topology call count
    #[arg(long, value_name = "N", default_value_t = 2_000_000)]
    pub(crate) oracle_limit: usize,

    /// Largest producer branch-and-bound node count
    #[arg(long, value_name = "N", default_value_t = 2_000_000)]
    pub(crate) node_limit: usize,

    /// Worker budget for input parsing
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub(crate) threads: usize,

    /// Largest accepted output artifact
    #[arg(long, value_name = "BYTES", default_value_t = 1usize << 30)]
    pub(crate) max_artifact_bytes: usize,
}

#[derive(Parser)]
#[command(
    name = "holos cover-affine",
    version = version_string(),
    about = "Certify relative coverage over a complete affine communication schedule",
    after_help = "Physical coverage is conditional on the controlled-boundary domain and sensor-placement assumptions. Affine edge weights need not have a Euclidean realization. STATES is `all` or a comma-separated compiled state-index list. Check the result with: holos-check ARTIFACT"
)]
pub(crate) struct AffineCoverageCli {
    /// Affine trajectory with rows `u v intercept velocity`
    pub(crate) input: PathBuf,

    /// Output `HOLOSCOV` artifact
    pub(crate) output: PathBuf,

    /// Shared sensor count, including inactive sensors
    #[arg(long, value_name = "N")]
    pub(crate) vertices: usize,

    /// First trajectory time
    #[arg(long)]
    pub(crate) start: f64,

    /// Last trajectory time
    #[arg(long)]
    pub(crate) end: f64,

    /// Broadcast radius used to compile communication states
    #[arg(long, value_name = "R")]
    pub(crate) broadcast_radius: f64,

    /// Sensing-disc radius used by the controlled-boundary theorem
    #[arg(long, value_name = "R")]
    pub(crate) sensing_radius: f64,

    /// Ordered fence cycle as comma-separated sensor labels
    #[arg(long, value_name = "V0,V1,...", value_delimiter = ',', required = true)]
    pub(crate) fence: Vec<usize>,

    /// Initially active non-fence sensors, as comma-separated labels
    #[arg(long, value_name = "V0,V1,...", value_delimiter = ',')]
    pub(crate) base: Vec<usize>,

    /// Sensors subject to failure, as comma-separated labels
    #[arg(long, value_name = "V0,V1,...", value_delimiter = ',')]
    pub(crate) failable: Vec<usize>,

    /// Largest simultaneous failure count
    #[arg(long, value_name = "F", default_value_t = 0)]
    pub(crate) failure_budget: usize,

    /// Sensor, positive cost, and state support. Repeat `--candidate V COST STATES`
    #[arg(long = "candidate", value_names = ["V", "COST", "STATES"], num_args = 3)]
    pub(crate) candidates: Vec<String>,

    /// Largest selected sensor count
    #[arg(long, value_name = "N")]
    pub(crate) max_activations: usize,

    /// Coefficient field Z/p; must be a prime below 32768
    #[arg(long, value_name = "P", default_value_t = 2)]
    pub(crate) modulus: u32,

    /// Largest producer topology call count
    #[arg(long, value_name = "N", default_value_t = 2_000_000)]
    pub(crate) oracle_limit: usize,

    /// Largest producer branch-and-bound node count
    #[arg(long, value_name = "N", default_value_t = 2_000_000)]
    pub(crate) node_limit: usize,

    /// Largest accepted trajectory input or output artifact
    #[arg(long, value_name = "BYTES", default_value_t = 1usize << 30)]
    pub(crate) max_artifact_bytes: usize,
}
