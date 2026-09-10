//! Command-line argument types and value conversions.

mod cohomology;
mod coverage;
mod intervention;
mod proof;
mod synthesis;
mod verify;

pub(crate) use cohomology::*;
pub(crate) use coverage::*;
pub(crate) use intervention::*;
pub(crate) use proof::*;
pub(crate) use synthesis::*;
pub(crate) use verify::*;

use std::path::PathBuf;

use clap::{Parser, ValueEnum};

use crate::collapse::CollapseObjective;
use crate::{CollapseSchedule, DenseStorage, Engine, GraphFactorization};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum InputFormat {
    PointCloud,
    LowerDistance,
    Sparse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Schedule {
    Serial,
    Ordered,
    Rounds,
    Adaptive,
}

impl From<Schedule> for CollapseSchedule {
    fn from(s: Schedule) -> Self {
        match s {
            Schedule::Serial => CollapseSchedule::Serial,
            Schedule::Ordered => CollapseSchedule::Ordered,
            Schedule::Rounds => CollapseSchedule::Rounds,
            Schedule::Adaptive => CollapseSchedule::Adaptive,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum ObjectiveArg {
    H1,
    H2,
}

impl From<ObjectiveArg> for CollapseObjective {
    fn from(objective: ObjectiveArg) -> Self {
        match objective {
            ObjectiveArg::H1 => CollapseObjective::H1,
            ObjectiveArg::H2 => CollapseObjective::H2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum EngineArg {
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
pub(crate) enum StorageArg {
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
pub(crate) enum FactorizationArg {
    Auto,
    Off,
    Force,
}

impl From<FactorizationArg> for GraphFactorization {
    fn from(value: FactorizationArg) -> Self {
        match value {
            FactorizationArg::Auto => GraphFactorization::Auto,
            FactorizationArg::Off => GraphFactorization::Off,
            FactorizationArg::Force => GraphFactorization::Force,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum DiagramFormat {
    Ripser,
    Csv,
}

pub(crate) fn version_string() -> &'static str {
    let profile = crate::BUILD_PROFILE;
    // clap without its "string" feature wants &'static str. The one-time
    // leak lives for the whole process.
    Box::leak(format!("{} ({}, {profile})", crate::VERSION, crate::GIT_HASH).into_boxed_str())
}

#[derive(Parser)]
#[command(
    name = "holos",
    version = version_string(),
    about = "Vietoris-Rips persistent homology over a prime field",
    after_help = "Additional workflows:\n  holos bipersistence INPUT OUTPUT [options]\n  holos collapse-portfolio INPUT OUTPUT [options]\n  holos cohomology INPUT [OTHER] --at T --homology-dim D [options]\n  holos circular INPUT COCYCLE OUTPUT --at T [options]\n  holos kinetic TRAJECTORY --vertices N --start A --end B [options]\n  holos cover OUTPUT --state GRAPH --fence VERTICES --candidate V COST STATES [options]\n  holos cover-affine TRAJECTORY OUTPUT --fence VERTICES --candidate V COST STATES [options]\n  holos synthesize OUTPUT --state GRAPH --candidate U V COST [options]\n  holos synthesize-kinetic TRAJECTORY OUTPUT --candidate U V COST [options]\n  holos intervene-cohomology INPUT OUTPUT --at T --homology-dim D --target N --candidate U V COST [options]\n  holos plan-links OUTPUT --scenario GRAPH --target N --candidate U V COST [options]\n  holos interface INPUT OUTPUT [options]\n  holos merge-interfaces STORE MANIFEST RESULT SHARD [SHARD ...] [options]\n  holos index INPUT SNAPSHOT --update NEXT --record RECORD [options]\n  holos prove INPUT OUTPUT [UPDATE ...] [options]\n  holos verify-collapse INPUT ARTIFACT [options]\n  holos verify-atlas INPUT ARTIFACT [options]\n  holos verify-trajectory ARTIFACT [options]\n  holos verify-program INPUT ARTIFACT [options]\n  holos verify-program-trace ARTIFACT [options]\n  holos intervene INPUT PROGRAM OUTPUT --space N --before T [options]\n  holos verify-intervention ARTIFACT [options]"
)]
pub(crate) struct Cli {
    /// Input file: point cloud, condensed lower-distance matrix, or sparse
    /// "i j d" triplets
    pub(crate) input: PathBuf,

    /// Input format. Inferred from the extension when omitted: .csv, .pts,
    /// and .xyz select point-cloud, anything else selects lower-distance.
    /// Sparse is never inferred
    #[arg(long, value_enum)]
    pub(crate) format: Option<InputFormat>,

    /// Compute homology up to this dimension
    #[arg(long, value_name = "D", default_value_t = 1)]
    pub(crate) dim: usize,

    /// Filtration threshold. Defaults to the enclosing radius for dense
    /// input, and to no threshold for sparse input
    #[arg(long, value_name = "T")]
    pub(crate) threshold: Option<f64>,

    /// Coefficient field Z/p; must be a prime below 32768
    #[arg(long, value_name = "P", default_value_t = 2)]
    pub(crate) modulus: u32,

    /// Worker threads for the input parse and the reduction, and for the
    /// collapse when --collapse-schedule is ordered or rounds (1 =
    /// serial). A file under one mebibyte parses serially whatever you set.
    /// The diagram is identical at any thread count
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub(crate) threads: usize,

    /// Collapse dominated edges before the engine runs. The diagram is
    /// identical either way; collapse statistics go to stderr
    #[arg(long)]
    pub(crate) collapse_edges: bool,

    /// Collapse schedule; requires --collapse-edges. serial is the
    /// default and, in the registered studies, the fastest end to end on
    /// most inputs; ordered reproduces the serial result on parallel
    /// workers; rounds runs a different schedule, also the same at every
    /// thread count, that can keep far fewer edges; adaptive scores the
    /// removable edges once per pass and can stop at a work limit. The
    /// diagram is identical under every schedule
    #[arg(long, value_enum, value_name = "SCHEDULE")]
    pub(crate) collapse_schedule: Option<Schedule>,

    /// Downstream work ranked by the adaptive collapse schedule. The
    /// default follows --dim: h1 through dimension 1, h2 above it
    #[arg(long, value_enum, value_name = "OBJECTIVE")]
    pub(crate) collapse_objective: Option<ObjectiveArg>,

    /// Maximum removability tests for the adaptive collapse schedule. A
    /// run that reaches the limit returns a safe partial collapse
    #[arg(long, value_name = "N")]
    pub(crate) collapse_work_limit: Option<u64>,

    /// Write the reduced graph and collapse certificate to this file.
    /// Requires --collapse-edges
    #[arg(long, value_name = "FILE")]
    pub(crate) collapse_certificate: Option<PathBuf>,

    /// Write stable H1 classes and cocycles as JSON. This selects the
    /// explain profile and requires --dim of at least 1
    #[arg(long, value_name = "FILE")]
    pub(crate) representatives: Option<PathBuf>,

    /// Write a proof-carrying H0 and H1 atlas. The artifact supports checked
    /// reuse while the listed edge order stays fixed. Requires --dim 1
    #[arg(long, value_name = "FILE")]
    pub(crate) atlas: Option<PathBuf>,

    /// Write a compositional proof-carrying H0 and H1 program. The program
    /// supports result-sensitive reuse and local repair across checked sparse
    /// graph atoms. Requires --dim 1
    #[arg(long, value_name = "FILE")]
    pub(crate) program: Option<PathBuf>,

    /// Engine for a dense input: auto reduces a low-density input as a
    /// thresholded graph when the graph fits its memory budget, dense and
    /// sparse force one engine. Sparse input always uses the sparse
    /// engine. The diagram is identical either way
    #[arg(long, value_enum, value_name = "ENGINE", default_value_t = EngineArg::Auto)]
    pub(crate) engine: EngineArg,

    /// Storage form for a dense run: auto holds the condensed lower
    /// triangle and converts to a full row-major matrix when a frozen
    /// work and memory rule selects it, compact forbids the full form,
    /// square forces it. A run routed to the sparse engine builds no full
    /// matrix under any setting. The diagram is identical either way
    #[arg(long, value_enum, value_name = "FORM", default_value_t = StorageArg::Auto)]
    pub(crate) dense_storage: StorageArg,

    /// Structural factorization for sparse reduction. off reduces the whole
    /// graph, auto splits useful vertex-biconnected terminal graphs, and force
    /// always splits. The diagram is identical under every setting
    #[arg(
        long,
        value_enum,
        value_name = "MODE",
        default_value_t = FactorizationArg::Off
    )]
    pub(crate) factorization: FactorizationArg,

    /// Output format
    #[arg(long, value_enum, default_value_t = DiagramFormat::Ripser)]
    pub(crate) output: DiagramFormat,

    // Debug flags. Each disables one optimization. Barcodes must be identical
    // either way (tested).
    #[arg(long, hide = true)]
    pub(crate) no_emergent_pairs: bool,

    #[arg(long, hide = true)]
    pub(crate) no_apparent_pairs: bool,

    #[arg(long, hide = true)]
    pub(crate) no_adjacency_rows: bool,

    #[arg(long, hide = true)]
    pub(crate) no_clearing: bool,
}
