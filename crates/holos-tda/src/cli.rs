//! The `holos` command-line interface, callable as a library function.

use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::collapse::verify::{verify_dense_artifact, verify_sparse_artifact};
use crate::collapse::wire::{CollapseArtifact, DecodeLimits};
use crate::collapse::{AdaptiveCollapseParams, CollapseCompleteness, CollapseObjective};
use crate::io::{self, OutputFormat};
use crate::{
    AtlasArtifact, AtlasDecodeLimits, CertificateLimits, CohomologyInterventionArtifact,
    CohomologyInterventionCandidate, CohomologyInterventionLimits, CohomologyInterventionScenario,
    CohomologyLimits, CollapseSchedule, CorrespondenceMode, CoverageAction, CoverageFence,
    CoverageLimits, CoverageSpecification, CoverageState, CoverageSynthesisArtifact,
    CoverageSynthesisLimits, DenseStorage, DistanceMatrix, DurableInterfaceStore, Engine,
    ExplainedDiagram, GraphFactorization, IndexParams, IndexStream, IndexStreamProof,
    InterfacePolicy, InterventionArtifact, InterventionBudget, InterventionDecodeLimits,
    KineticEdge, KineticEventKind, KineticFiltration, KineticLimits, KineticZigzagArtifact,
    KineticZigzagArtifactLimits, PersistenceIndex, PlanarCoverageModel, PointCloudGraph,
    PointCloudParams, ProgramArtifact, ProgramDecodeLimits, ProgramTraceArtifact,
    ProgramTraceDecodeLimits, ProofArtifact, RelativeInterfaceCertificate, RipsParams,
    SparseDistanceMatrix, SynthesisAction, SynthesisArtifact, SynthesisLimits, SynthesisState,
    TopologicalSpecification, TrajectoryArtifact, TrajectoryDecodeLimits, cohomology_relation,
    cohomology_space, lift_h1_classes, rips_persistence_with_classes_sparse,
};
use clap::{Parser, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum InputFormat {
    PointCloud,
    LowerDistance,
    Sparse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Schedule {
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
enum ObjectiveArg {
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
enum EngineArg {
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
enum StorageArg {
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
enum FactorizationArg {
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
enum DiagramFormat {
    Ripser,
    Csv,
}

fn version_string() -> &'static str {
    let profile = crate::BUILD_PROFILE;
    // clap without its "string" feature wants &'static str. The one-time
    // leak lives for the whole process anyway.
    Box::leak(format!("{} ({}, {profile})", crate::VERSION, crate::GIT_HASH).into_boxed_str())
}

#[derive(Parser)]
#[command(
    name = "holos",
    version = version_string(),
    about = "Vietoris-Rips persistent homology over a prime field",
    after_help = "Additional workflows:\n  holos cohomology INPUT [OTHER] --at T --homology-dim D [options]\n  holos kinetic TRAJECTORY --vertices N --start A --end B [options]\n  holos cover OUTPUT --state GRAPH --fence VERTICES --candidate V COST STATES [options]\n  holos cover-affine TRAJECTORY OUTPUT --fence VERTICES --candidate V COST STATES [options]\n  holos synthesize OUTPUT --state GRAPH --candidate U V COST [options]\n  holos synthesize-kinetic TRAJECTORY OUTPUT --candidate U V COST [options]\n  holos intervene-cohomology INPUT OUTPUT --at T --homology-dim D --target N --candidate U V COST [options]\n  holos plan-links OUTPUT --scenario GRAPH --target N --candidate U V COST [options]\n  holos interface INPUT OUTPUT [options]\n  holos merge-interfaces STORE MANIFEST RESULT SHARD [SHARD ...] [options]\n  holos index INPUT SNAPSHOT --update NEXT --record RECORD [options]\n  holos prove INPUT OUTPUT [UPDATE ...] [options]\n  holos verify-collapse INPUT ARTIFACT [options]\n  holos verify-atlas INPUT ARTIFACT [options]\n  holos verify-trajectory ARTIFACT [options]\n  holos verify-program INPUT ARTIFACT [options]\n  holos verify-program-trace ARTIFACT [options]\n  holos intervene INPUT PROGRAM OUTPUT --space N --before T [options]\n  holos verify-intervention ARTIFACT [options]"
)]
struct Cli {
    /// Input file: point cloud, condensed lower-distance matrix, or sparse
    /// "i j d" triplets
    input: PathBuf,

    /// Input format. Inferred from the extension when omitted: .csv, .pts,
    /// and .xyz select point-cloud, anything else selects lower-distance.
    /// Sparse is never inferred
    #[arg(long, value_enum)]
    format: Option<InputFormat>,

    /// Compute homology up to this dimension
    #[arg(long, value_name = "D", default_value_t = 1)]
    dim: usize,

    /// Filtration threshold. Defaults to the enclosing radius for dense
    /// input, and to no threshold for sparse input
    #[arg(long, value_name = "T")]
    threshold: Option<f64>,

    /// Coefficient field Z/p; must be a prime below 32768
    #[arg(long, value_name = "P", default_value_t = 2)]
    modulus: u32,

    /// Worker threads for the input parse and the reduction, and for the
    /// collapse when --collapse-schedule is ordered or rounds (1 =
    /// serial). A file under one mebibyte parses serially whatever you set.
    /// The diagram is identical at any thread count
    #[arg(long, value_name = "N", default_value_t = 1)]
    threads: usize,

    /// Collapse dominated edges before the engine runs. The diagram is
    /// identical either way; collapse statistics go to stderr
    #[arg(long)]
    collapse_edges: bool,

    /// Collapse schedule; requires --collapse-edges. serial is the
    /// default and, in the registered studies, the fastest end to end on
    /// most inputs; ordered reproduces the serial result on parallel
    /// workers; rounds runs a different schedule, also the same at every
    /// thread count, that can keep far fewer edges; adaptive scores the
    /// removable edges once per pass and can stop at a work limit. The
    /// diagram is identical under every schedule
    #[arg(long, value_enum, value_name = "SCHEDULE")]
    collapse_schedule: Option<Schedule>,

    /// Downstream work ranked by the adaptive collapse schedule. The
    /// default follows --dim: h1 through dimension 1, h2 above it
    #[arg(long, value_enum, value_name = "OBJECTIVE")]
    collapse_objective: Option<ObjectiveArg>,

    /// Maximum removability tests for the adaptive collapse schedule. A
    /// run that reaches the limit returns a safe partial collapse
    #[arg(long, value_name = "N")]
    collapse_work_limit: Option<u64>,

    /// Write the reduced graph and collapse certificate to this file.
    /// Requires --collapse-edges
    #[arg(long, value_name = "FILE")]
    collapse_certificate: Option<PathBuf>,

    /// Write stable H1 classes and cocycles as JSON. This selects the
    /// explain profile and requires --dim of at least 1
    #[arg(long, value_name = "FILE")]
    representatives: Option<PathBuf>,

    /// Write a proof-carrying H0 and H1 atlas. The artifact supports checked
    /// reuse while the listed edge order stays fixed. Requires --dim 1
    #[arg(long, value_name = "FILE")]
    atlas: Option<PathBuf>,

    /// Write a compositional proof-carrying H0 and H1 program. The program
    /// supports result-sensitive reuse and local repair across checked sparse
    /// graph atoms. Requires --dim 1
    #[arg(long, value_name = "FILE")]
    program: Option<PathBuf>,

    /// Engine for a dense input: auto reduces a low-density input as a
    /// thresholded graph when the graph fits its memory budget, dense and
    /// sparse force one engine. Sparse input always uses the sparse
    /// engine. The diagram is identical either way
    #[arg(long, value_enum, value_name = "ENGINE", default_value_t = EngineArg::Auto)]
    engine: EngineArg,

    /// Storage form for a dense run: auto holds the condensed lower
    /// triangle and converts to a full row-major matrix when a frozen
    /// work and memory rule selects it, compact forbids the full form,
    /// square forces it. A run routed to the sparse engine builds no full
    /// matrix under any setting. The diagram is identical either way
    #[arg(long, value_enum, value_name = "FORM", default_value_t = StorageArg::Auto)]
    dense_storage: StorageArg,

    /// Structural factorization for sparse reduction. off reduces the whole
    /// graph, auto splits useful vertex-biconnected terminal graphs, and force
    /// always splits. The diagram is identical under every setting
    #[arg(
        long,
        value_enum,
        value_name = "MODE",
        default_value_t = FactorizationArg::Off
    )]
    factorization: FactorizationArg,

    /// Output format
    #[arg(long, value_enum, default_value_t = DiagramFormat::Ripser)]
    output: DiagramFormat,

    // Debug toggles. Each flag disables one pure optimization. Barcodes must
    // be identical either way (tested), so the flags are hidden from help.
    #[arg(long, hide = true)]
    no_emergent_pairs: bool,

    #[arg(long, hide = true)]
    no_apparent_pairs: bool,

    #[arg(long, hide = true)]
    no_adjacency_rows: bool,

    #[arg(long, hide = true)]
    no_clearing: bool,
}

#[derive(Parser)]
#[command(
    name = "holos prove",
    version = version_string(),
    about = "Build one checked proof DAG for a sparse persistence trajectory",
    after_help = "Check the result with: holos-check PROOF"
)]
struct ProveCli {
    /// Initial point cloud, lower-distance matrix, or sparse triplets
    input: PathBuf,

    /// Output `HOLOSPF` proof DAG
    output: PathBuf,

    /// Later inputs in trajectory order
    #[arg(value_name = "UPDATE")]
    updates: Vec<PathBuf>,

    /// Input format shared by the initial input and all updates. Inferred
    /// from each file extension when omitted. Sparse is never inferred
    #[arg(long, value_enum)]
    format: Option<InputFormat>,

    /// Filtration threshold. Defaults to the enclosing radius for each
    /// dense input, and to no threshold for sparse input
    #[arg(long, value_name = "T")]
    threshold: Option<f64>,

    /// Coefficient field Z/p; must be a prime below 32768
    #[arg(long, value_name = "P", default_value_t = 2)]
    modulus: u32,

    /// Worker threads for parsing and persistence reduction
    #[arg(long, value_name = "N", default_value_t = 1)]
    threads: usize,
}

#[derive(Parser)]
#[command(
    name = "holos index",
    version = version_string(),
    about = "Compile a versioned exact persistence index and proof stream",
    after_help = "Check the stream with: holos-check SNAPSHOT [RECORD ...]"
)]
struct IndexCli {
    /// Initial point cloud, lower-distance matrix, or sparse triplets
    input: PathBuf,

    /// Output initial `HOLOSIP` checkpoint
    snapshot: PathBuf,

    /// Later input. Repeat once for each --record in the same order
    #[arg(long, value_name = "FILE")]
    update: Vec<PathBuf>,

    /// Output proof record. A fixed envelope writes `HOLOSDP`; an envelope
    /// change writes a new `HOLOSIP` checkpoint. Repeat once per --update
    #[arg(long, value_name = "FILE")]
    record: Vec<PathBuf>,

    /// Input format shared by every input. Inferred from each extension when
    /// omitted. Sparse is never inferred
    #[arg(long, value_enum)]
    format: Option<InputFormat>,

    /// Fixed filtration threshold
    #[arg(long, value_name = "T")]
    threshold: Option<f64>,

    /// Coefficient field Z/p; must be a prime below 32768
    #[arg(long, value_name = "P", default_value_t = 2)]
    modulus: u32,

    /// Highest homology dimension to maintain and certify
    #[arg(long, value_name = "D", default_value_t = 1)]
    dim: usize,

    /// Worker budget for parsing, reduction, and independent alternatives
    #[arg(long, value_name = "N", default_value_t = 1)]
    threads: usize,

    /// Largest vertex separator considered by deterministic search
    #[arg(long, value_name = "W", default_value_t = 4)]
    separator_width: usize,

    /// Largest total separator candidate count
    #[arg(long, value_name = "N", default_value_t = 100_000)]
    separator_search_limit: usize,

    /// A scope at or below this vertex count remains a leaf
    #[arg(long, value_name = "N", default_value_t = 4)]
    leaf_vertices: usize,

    /// Retain a full reduction at every parent interface
    #[arg(long)]
    materialize_interfaces: bool,
}

#[derive(Parser)]
#[command(
    name = "holos interface",
    version = version_string(),
    about = "Build a checked filtered chain core relative to protected vertices",
    after_help = "Check the result with: holos-check CERTIFICATE"
)]
struct InterfaceCli {
    /// Input point cloud, lower-distance matrix, or sparse triplets
    input: PathBuf,

    /// Output `HOLOSRI` relative-interface certificate
    output: PathBuf,

    /// Input format. Inferred from the extension when omitted. Sparse is
    /// never inferred
    #[arg(long, value_enum)]
    format: Option<InputFormat>,

    /// Fixed filtration threshold
    #[arg(long, value_name = "T")]
    threshold: Option<f64>,

    /// Highest homology dimension to certify
    #[arg(long, value_name = "D", default_value_t = 1)]
    dim: usize,

    /// Coefficient field Z/p; must be a prime below 32768
    #[arg(long, value_name = "P", default_value_t = 2)]
    modulus: u32,

    /// Vertex retained as part of the separator subcomplex. Repeat this flag
    /// for each protected vertex
    #[arg(long = "protect", value_name = "VERTEX")]
    protected: Vec<usize>,

    /// Worker budget for input parsing
    #[arg(long, value_name = "N", default_value_t = 1)]
    threads: usize,
}

#[derive(Parser)]
#[command(
    name = "holos merge-interfaces",
    version = version_string(),
    about = "Durably compose proof-carrying relative interface shards",
    after_help = "Check the result with: holos-check MANIFEST OBJECT [OBJECT ...]"
)]
struct MergeInterfacesCli {
    /// Content-addressed store directory
    store: PathBuf,

    /// Output `HOLOSDM` commit manifest
    manifest: PathBuf,

    /// Output composed `HOLOSRI` result
    result: PathBuf,

    /// Ordered child `HOLOSRI` artifacts
    #[arg(required = true)]
    shards: Vec<PathBuf>,

    /// Common separator vertex. Repeat for each separator vertex
    #[arg(long = "separator", value_name = "VERTEX")]
    separator: Vec<usize>,

    /// Vertex protected in the final output. Repeat for each vertex
    #[arg(long = "protect", value_name = "VERTEX")]
    protected: Vec<usize>,

    /// Largest artifact or manifest accepted
    #[arg(long, value_name = "BYTES", default_value_t = 1usize << 30)]
    max_artifact_bytes: usize,
}

#[derive(Parser)]
#[command(
    name = "holos cohomology",
    version = version_string(),
    about = "Compute canonical fixed-scale cohomology and an optional exact relation"
)]
struct CohomologyCli {
    /// First input graph
    input: PathBuf,

    /// Optional second graph related through the common active subcomplex
    other: Option<PathBuf>,

    /// Fixed filtration scale
    #[arg(long = "at", value_name = "T")]
    scale: f64,

    /// Cohomology dimension
    #[arg(long = "homology-dim", value_name = "D")]
    dimension: usize,

    /// Coefficient field Z/p; must be a prime below 32768
    #[arg(long, value_name = "P", default_value_t = 2)]
    modulus: u32,

    /// Input format. Inferred when omitted. Sparse is never inferred
    #[arg(long, value_enum)]
    format: Option<InputFormat>,

    /// Worker budget for input parsing
    #[arg(long, value_name = "N", default_value_t = 1)]
    threads: usize,
}

#[derive(Parser)]
#[command(
    name = "holos kinetic",
    version = version_string(),
    about = "Certify events in an affine edge-weight trajectory"
)]
struct KineticCli {
    /// Text file with `u v intercept velocity` on each line
    input: PathBuf,

    /// Vertex count, including isolated vertices
    #[arg(long, value_name = "N")]
    vertices: usize,

    /// First trajectory time
    #[arg(long)]
    start: f64,

    /// Last trajectory time
    #[arg(long)]
    end: f64,

    /// Fixed scale whose crossings are reported
    #[arg(long = "at", value_name = "T")]
    scale: Option<f64>,

    /// Also compute class relations in this cohomology dimension
    #[arg(long = "homology-dim", value_name = "D", requires = "scale")]
    dimension: Option<usize>,

    /// Coefficient field for class relations
    #[arg(long, value_name = "P", default_value_t = 2)]
    modulus: u32,

    /// Write a checked `HOLOSZZ` fixed-scale zigzag artifact
    #[arg(long, value_name = "FILE", requires = "scale", requires = "dimension")]
    zigzag: Option<PathBuf>,

    /// Largest accepted trajectory input or output artifact
    #[arg(long, value_name = "BYTES", default_value_t = 1usize << 30)]
    max_artifact_bytes: usize,
}

#[derive(Parser)]
#[command(
    name = "holos intervene-cohomology",
    version = version_string(),
    about = "Certify a minimum-cost fixed-scale cohomology intervention",
    after_help = "Check the result with: holos-check ARTIFACT"
)]
struct CohomologyInterventionCli {
    /// Input graph
    input: PathBuf,

    /// Output `HOLOSCI` artifact
    output: PathBuf,

    /// Fixed filtration scale
    #[arg(long = "at", value_name = "T")]
    scale: f64,

    /// Target cohomology dimension
    #[arg(long = "homology-dim", value_name = "D")]
    dimension: usize,

    /// Position in the canonical cohomology basis
    #[arg(long, value_name = "N")]
    target: usize,

    /// Candidate edge endpoints and positive cost. Repeat `--candidate U V COST`
    #[arg(long = "candidate", value_names = ["U", "V", "COST"], num_args = 3)]
    candidates: Vec<usize>,

    /// Largest selected edge count
    #[arg(long, value_name = "N")]
    max_edits: usize,

    /// Largest distinct topological oracle call count
    #[arg(long, value_name = "N", default_value_t = 1_000_000)]
    oracle_limit: usize,

    /// Largest branch-and-bound node count
    #[arg(long, value_name = "N", default_value_t = 1_000_000)]
    node_limit: usize,

    /// Coefficient field Z/p; must be a prime below 32768
    #[arg(long, value_name = "P", default_value_t = 2)]
    modulus: u32,

    /// Input format. Inferred when omitted. Sparse is never inferred
    #[arg(long, value_enum)]
    format: Option<InputFormat>,

    /// Worker budget for input parsing
    #[arg(long, value_name = "N", default_value_t = 1)]
    threads: usize,
}

#[derive(Parser)]
#[command(
    name = "holos plan-links",
    version = version_string(),
    about = "Plan one minimum-cost link set across network scenarios",
    after_help = "Check the result with: holos-check ARTIFACT"
)]
struct LinkPlanCli {
    /// Output `HOLOSCI` artifact
    output: PathBuf,

    /// Sparse scenario graph. Repeat once per target
    #[arg(long = "scenario", value_name = "GRAPH", required = true)]
    scenarios: Vec<PathBuf>,

    /// Canonical basis position for each scenario, in scenario order
    #[arg(long = "target", value_name = "N", required = true)]
    targets: Vec<usize>,

    /// Shared vertex count, including isolated vertices
    #[arg(long, value_name = "N")]
    vertices: usize,

    /// Fixed filtration scale
    #[arg(long = "at", value_name = "T")]
    scale: f64,

    /// Target cohomology dimension
    #[arg(long = "homology-dim", value_name = "D")]
    dimension: usize,

    /// Candidate link endpoints and positive cost. Repeat `--candidate U V COST`
    #[arg(long = "candidate", value_names = ["U", "V", "COST"], num_args = 3)]
    candidates: Vec<usize>,

    /// Largest selected link count
    #[arg(long, value_name = "N")]
    max_edits: usize,

    /// Largest distinct topological oracle call count
    #[arg(long, value_name = "N", default_value_t = 1_000_000)]
    oracle_limit: usize,

    /// Largest branch-and-bound node count
    #[arg(long, value_name = "N", default_value_t = 1_000_000)]
    node_limit: usize,

    /// Coefficient field Z/p; must be a prime below 32768
    #[arg(long, value_name = "P", default_value_t = 2)]
    modulus: u32,

    /// Worker budget for input parsing
    #[arg(long, value_name = "N", default_value_t = 1)]
    threads: usize,
}

#[derive(Parser)]
#[command(
    name = "holos synthesize",
    version = version_string(),
    about = "Synthesize a minimum-cost plan for finite cohomology rank bounds",
    after_help = "Each state uses the full canonical cohomology space as a subspace. Check the result with: holos-check ARTIFACT"
)]
struct SynthesisCli {
    /// Output `HOLOSSYN` artifact
    output: PathBuf,

    /// Sparse state graph. Repeat in temporal order
    #[arg(long = "state", value_name = "GRAPH", required = true)]
    states: Vec<PathBuf>,

    /// Shared vertex count, including isolated vertices
    #[arg(long, value_name = "N")]
    vertices: usize,

    /// Fixed filtration scale
    #[arg(long = "at", value_name = "T")]
    scale: f64,

    /// Cohomology dimension constrained in every state
    #[arg(long = "homology-dim", value_name = "D")]
    dimension: usize,

    /// Largest allowed surviving rank in each full target space
    #[arg(long, value_name = "R", default_value_t = 0)]
    max_rank: usize,

    /// Candidate edge endpoints and positive cost. Repeat `--candidate U V COST`
    #[arg(long = "candidate", value_names = ["U", "V", "COST"], num_args = 3)]
    candidates: Vec<usize>,

    /// Largest selected action count
    #[arg(long, value_name = "N")]
    max_edits: usize,

    /// Largest producer topology call count
    #[arg(long, value_name = "N", default_value_t = 2_000_000)]
    oracle_limit: usize,

    /// Largest producer branch-and-bound node count
    #[arg(long, value_name = "N", default_value_t = 2_000_000)]
    node_limit: usize,

    /// Coefficient field Z/p; must be a prime below 32768
    #[arg(long, value_name = "P", default_value_t = 2)]
    modulus: u32,

    /// Worker budget for input parsing
    #[arg(long, value_name = "N", default_value_t = 1)]
    threads: usize,
}

#[derive(Parser)]
#[command(
    name = "holos synthesize-kinetic",
    version = version_string(),
    about = "Synthesize a minimum-cost plan over an exact affine threshold schedule",
    after_help = "This command certifies a Rips cohomology rank condition. It does not by itself certify physical sensor coverage. Check the result with: holos-check ARTIFACT"
)]
struct KineticSynthesisCli {
    /// Affine trajectory with rows `u v intercept velocity`
    input: PathBuf,

    /// Output `HOLOSSYN` artifact
    output: PathBuf,

    /// Number of vertices, including isolated vertices
    #[arg(long, value_name = "N")]
    vertices: usize,

    /// First trajectory time
    #[arg(long)]
    start: f64,

    /// Last trajectory time
    #[arg(long)]
    end: f64,

    /// Fixed filtration scale
    #[arg(long = "at", value_name = "T")]
    scale: f64,

    /// Cohomology dimension constrained throughout the trajectory
    #[arg(long = "homology-dim", value_name = "D", default_value_t = 1)]
    dimension: usize,

    /// Largest allowed surviving rank in each full target space
    #[arg(long, value_name = "R", default_value_t = 0)]
    max_rank: usize,

    /// Candidate edge endpoints and positive cost. Repeat `--candidate U V COST`
    #[arg(long = "candidate", value_names = ["U", "V", "COST"], num_args = 3)]
    candidates: Vec<usize>,

    /// Largest selected action count
    #[arg(long, value_name = "N")]
    max_edits: usize,

    /// Largest producer topology call count
    #[arg(long, value_name = "N", default_value_t = 2_000_000)]
    oracle_limit: usize,

    /// Largest producer branch-and-bound node count
    #[arg(long, value_name = "N", default_value_t = 2_000_000)]
    node_limit: usize,

    /// Coefficient field Z/p; must be a prime below 32768
    #[arg(long, value_name = "P", default_value_t = 2)]
    modulus: u32,

    /// Largest accepted trajectory input or output artifact
    #[arg(long, value_name = "BYTES", default_value_t = 1usize << 30)]
    max_artifact_bytes: usize,
}

#[derive(Parser)]
#[command(
    name = "holos cover",
    version = version_string(),
    about = "Certify a minimum-cost failure-tolerant relative coverage plan",
    after_help = "Physical coverage is conditional on the controlled-boundary domain and sensor-placement assumptions. STATES is `all` or a comma-separated state-index list. Check the result with: holos-check ARTIFACT"
)]
struct CoverageCli {
    /// Output `HOLOSCOV` artifact
    output: PathBuf,

    /// Sparse graph of possible communication edges. Repeat in state order
    #[arg(long = "state", value_name = "GRAPH", required = true)]
    states: Vec<PathBuf>,

    /// Shared sensor count, including inactive sensors
    #[arg(long, value_name = "N")]
    vertices: usize,

    /// Broadcast radius used to retain communication edges
    #[arg(long, value_name = "R")]
    broadcast_radius: f64,

    /// Sensing-disc radius used by the controlled-boundary theorem
    #[arg(long, value_name = "R")]
    sensing_radius: f64,

    /// Ordered fence cycle as comma-separated sensor labels
    #[arg(long, value_name = "V0,V1,...", value_delimiter = ',', required = true)]
    fence: Vec<usize>,

    /// Initially active non-fence sensors, as comma-separated labels
    #[arg(long, value_name = "V0,V1,...", value_delimiter = ',')]
    base: Vec<usize>,

    /// Sensors subject to failure, as comma-separated labels
    #[arg(long, value_name = "V0,V1,...", value_delimiter = ',')]
    failable: Vec<usize>,

    /// Largest simultaneous failure count
    #[arg(long, value_name = "F", default_value_t = 0)]
    failure_budget: usize,

    /// Sensor, positive cost, and state support. Repeat `--candidate V COST STATES`
    #[arg(long = "candidate", value_names = ["V", "COST", "STATES"], num_args = 3)]
    candidates: Vec<String>,

    /// Largest selected sensor count
    #[arg(long, value_name = "N")]
    max_activations: usize,

    /// Coefficient field Z/p; must be a prime below 32768
    #[arg(long, value_name = "P", default_value_t = 2)]
    modulus: u32,

    /// Largest producer topology call count
    #[arg(long, value_name = "N", default_value_t = 2_000_000)]
    oracle_limit: usize,

    /// Largest producer branch-and-bound node count
    #[arg(long, value_name = "N", default_value_t = 2_000_000)]
    node_limit: usize,

    /// Worker budget for input parsing
    #[arg(long, value_name = "N", default_value_t = 1)]
    threads: usize,

    /// Largest accepted output artifact
    #[arg(long, value_name = "BYTES", default_value_t = 1usize << 30)]
    max_artifact_bytes: usize,
}

#[derive(Parser)]
#[command(
    name = "holos cover-affine",
    version = version_string(),
    about = "Certify relative coverage over a complete affine communication schedule",
    after_help = "Physical coverage is conditional on the controlled-boundary domain and sensor-placement assumptions. Affine edge weights need not have a Euclidean realization. STATES is `all` or a comma-separated compiled state-index list. Check the result with: holos-check ARTIFACT"
)]
struct AffineCoverageCli {
    /// Affine trajectory with rows `u v intercept velocity`
    input: PathBuf,

    /// Output `HOLOSCOV` artifact
    output: PathBuf,

    /// Shared sensor count, including inactive sensors
    #[arg(long, value_name = "N")]
    vertices: usize,

    /// First trajectory time
    #[arg(long)]
    start: f64,

    /// Last trajectory time
    #[arg(long)]
    end: f64,

    /// Broadcast radius used to compile communication states
    #[arg(long, value_name = "R")]
    broadcast_radius: f64,

    /// Sensing-disc radius used by the controlled-boundary theorem
    #[arg(long, value_name = "R")]
    sensing_radius: f64,

    /// Ordered fence cycle as comma-separated sensor labels
    #[arg(long, value_name = "V0,V1,...", value_delimiter = ',', required = true)]
    fence: Vec<usize>,

    /// Initially active non-fence sensors, as comma-separated labels
    #[arg(long, value_name = "V0,V1,...", value_delimiter = ',')]
    base: Vec<usize>,

    /// Sensors subject to failure, as comma-separated labels
    #[arg(long, value_name = "V0,V1,...", value_delimiter = ',')]
    failable: Vec<usize>,

    /// Largest simultaneous failure count
    #[arg(long, value_name = "F", default_value_t = 0)]
    failure_budget: usize,

    /// Sensor, positive cost, and state support. Repeat `--candidate V COST STATES`
    #[arg(long = "candidate", value_names = ["V", "COST", "STATES"], num_args = 3)]
    candidates: Vec<String>,

    /// Largest selected sensor count
    #[arg(long, value_name = "N")]
    max_activations: usize,

    /// Coefficient field Z/p; must be a prime below 32768
    #[arg(long, value_name = "P", default_value_t = 2)]
    modulus: u32,

    /// Largest producer topology call count
    #[arg(long, value_name = "N", default_value_t = 2_000_000)]
    oracle_limit: usize,

    /// Largest producer branch-and-bound node count
    #[arg(long, value_name = "N", default_value_t = 2_000_000)]
    node_limit: usize,

    /// Largest accepted trajectory input or output artifact
    #[arg(long, value_name = "BYTES", default_value_t = 1usize << 30)]
    max_artifact_bytes: usize,
}

#[derive(Parser)]
#[command(
    name = "holos verify-collapse",
    version = version_string(),
    about = "Verify a portable collapse artifact against its input"
)]
struct VerifyCli {
    /// Original point cloud, lower-distance matrix, or sparse triplets
    input: PathBuf,

    /// Portable collapse artifact written by --collapse-certificate
    artifact: PathBuf,

    /// Input format. Inferred from the input extension when omitted.
    /// Sparse is never inferred
    #[arg(long, value_enum)]
    format: Option<InputFormat>,

    /// Filtration threshold used for the collapse
    #[arg(long, value_name = "T")]
    threshold: Option<f64>,

    /// Worker threads for parsing the original input
    #[arg(long, value_name = "N", default_value_t = 1)]
    threads: usize,

    /// Largest artifact accepted before the file is read
    #[arg(long, value_name = "BYTES", default_value_t = 1usize << 30)]
    max_artifact_bytes: usize,
}

#[derive(Parser)]
#[command(
    name = "holos verify-atlas",
    version = version_string(),
    about = "Verify a proof-carrying H0 and H1 atlas against its input"
)]
struct VerifyAtlasCli {
    /// Original point cloud, lower-distance matrix, or sparse triplets
    input: PathBuf,

    /// Portable atlas written by --atlas
    artifact: PathBuf,

    /// Input format. Inferred from the input extension when omitted.
    /// Sparse is never inferred
    #[arg(long, value_enum)]
    format: Option<InputFormat>,

    /// Worker threads for parsing and point-graph construction
    #[arg(long, value_name = "N", default_value_t = 1)]
    threads: usize,

    /// Largest artifact accepted while reading the file
    #[arg(long, value_name = "BYTES", default_value_t = 1usize << 30)]
    max_artifact_bytes: usize,
}

#[derive(Parser)]
#[command(
    name = "holos verify-trajectory",
    version = version_string(),
    about = "Verify a self-contained persistence trajectory"
)]
struct VerifyTrajectoryCli {
    /// Portable trajectory artifact
    artifact: PathBuf,

    /// Largest artifact accepted while reading the file
    #[arg(long, value_name = "BYTES", default_value_t = 1usize << 30)]
    max_artifact_bytes: usize,
}

#[derive(Parser)]
#[command(
    name = "holos verify-program",
    version = version_string(),
    about = "Verify a compositional H0 and H1 program against its input"
)]
struct VerifyProgramCli {
    /// Original point cloud, lower-distance matrix, or sparse triplets
    input: PathBuf,

    /// Portable program written by --program
    artifact: PathBuf,

    /// Input format. Inferred from the input extension when omitted.
    /// Sparse is never inferred
    #[arg(long, value_enum)]
    format: Option<InputFormat>,

    /// Worker threads for parsing and point-graph construction
    #[arg(long, value_name = "N", default_value_t = 1)]
    threads: usize,

    /// Largest artifact accepted while reading the file
    #[arg(long, value_name = "BYTES", default_value_t = 1usize << 30)]
    max_artifact_bytes: usize,
}

#[derive(Parser)]
#[command(
    name = "holos verify-program-trace",
    version = version_string(),
    about = "Verify a self-contained compositional persistence trace"
)]
struct VerifyProgramTraceCli {
    /// Portable program trace artifact
    artifact: PathBuf,

    /// Largest artifact accepted while reading the file
    #[arg(long, value_name = "BYTES", default_value_t = 1usize << 30)]
    max_artifact_bytes: usize,
}

#[derive(Parser)]
#[command(
    name = "holos intervene",
    version = version_string(),
    about = "Certify a finite H1 lifetime intervention"
)]
struct InterveneCli {
    /// Original input bound to the program
    input: PathBuf,

    /// Checked compositional program written by --program
    program: PathBuf,

    /// Output `HOLOSINT` artifact
    output: PathBuf,

    /// Zero-based H1 class-space index in the program result
    #[arg(long, value_name = "N")]
    space: usize,

    /// Requested latest death scale
    #[arg(long, value_name = "T")]
    before: f64,

    /// Largest candidate count. The current intervention uses one candidate
    #[arg(long, value_name = "N", default_value_t = 1)]
    budget: usize,

    /// Input format. Inferred from the input extension when omitted.
    /// Sparse is never inferred
    #[arg(long, value_enum)]
    format: Option<InputFormat>,

    /// Worker threads for parsing and point-graph construction
    #[arg(long, value_name = "N", default_value_t = 1)]
    threads: usize,

    /// Largest program accepted while reading the file
    #[arg(long, value_name = "BYTES", default_value_t = 1usize << 30)]
    max_artifact_bytes: usize,
}

#[derive(Parser)]
#[command(
    name = "holos verify-intervention",
    version = version_string(),
    about = "Verify a self-contained finite H1 intervention"
)]
struct VerifyInterventionCli {
    /// Portable intervention artifact
    artifact: PathBuf,

    /// Largest artifact accepted while reading the file
    #[arg(long, value_name = "BYTES", default_value_t = 1usize << 30)]
    max_artifact_bytes: usize,
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

// Report the collapse before the reduction starts. The pipeline owns the
// collapse result and, for a parallel schedule, shares one worker pool
// across both phases, so the statistics come out of it rather than from a
// separate standalone call.
fn report_collapse(collapsed: &crate::collapse::CollapsedRips) {
    let s = &collapsed.stats;
    let epoch = match collapsed.certificate.algorithm_version() {
        1 => "passes",
        2 => "rounds",
        3 => "passes",
        _ => "epochs",
    };
    eprintln!(
        "collapse: kept {} of {} edges, removed {}, {} {epoch}",
        s.output_edges, s.input_edges, s.removed_edges, s.epochs
    );
    eprintln!(
        "collapse detail: {} edge tests, {} witness segments, \
         max common neighborhood {}",
        s.edge_tests, s.witness_segments, s.max_common_neighborhood
    );
    if collapsed.certificate.algorithm_version() == 3 {
        let completeness = match collapsed.certificate.completeness() {
            CollapseCompleteness::CompleteFixedPoint => "complete fixed point",
            CollapseCompleteness::BudgetLimited => "budget-limited partial collapse",
        };
        eprintln!(
            "collapse adaptive: {completeness}, {} work units, {} score evaluations",
            collapsed.certificate.work_used(),
            s.adaptive_score_evaluations
        );
    }
}

fn write_collapse_artifact(
    collapsed: &crate::collapse::CollapsedRips,
    path: Option<&Path>,
) -> crate::Result<()> {
    report_collapse(collapsed);
    let Some(path) = path else {
        return Ok(());
    };
    let artifact = CollapseArtifact::from_result(collapsed)
        .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
    let bytes = artifact
        .encode()
        .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
    write_via_temporary(path, &bytes)?;
    eprintln!(
        "collapse artifact: wrote {} bytes to {}",
        bytes.len(),
        path.display()
    );
    Ok(())
}

fn write_via_temporary(path: &Path, bytes: &[u8]) -> crate::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path.file_name().ok_or_else(|| {
        crate::Error::InvalidInput(format!("output path {} has no file name", path.display()))
    })?;
    for nonce in 0..100u32 {
        let temporary = parent.join(format!(
            ".{}.{}.{}.tmp",
            name.to_string_lossy(),
            std::process::id(),
            nonce
        ));
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary);
        let mut file = match file {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(crate::Error::Io(format!(
                    "cannot create output file {}: {error}",
                    path.display()
                )));
            }
        };
        let result = file
            .write_all(bytes)
            .and_then(|()| file.sync_all())
            .and_then(|()| match std::fs::rename(&temporary, path) {
                Ok(()) => Ok(()),
                Err(_) if path.is_file() => {
                    std::fs::remove_file(path).and_then(|()| std::fs::rename(&temporary, path))
                }
                Err(error) => Err(error),
            });
        if let Err(error) = result {
            let _ = std::fs::remove_file(&temporary);
            return Err(crate::Error::Io(format!(
                "cannot write output file {}: {error}",
                path.display()
            )));
        }
        return Ok(());
    }
    Err(crate::Error::Io(format!(
        "cannot create a temporary file for output {}",
        path.display()
    )))
}

fn collapse_for_explain(
    matrix: &SparseDistanceMatrix,
    params: &RipsParams,
) -> crate::Result<crate::collapse::CollapsedRips> {
    match params.collapse_schedule {
        CollapseSchedule::Serial => crate::collapse::collapse_sparse(matrix, params.threshold),
        CollapseSchedule::Ordered => crate::collapse::collapse_sparse_ordered_parallel(
            matrix,
            params.threshold,
            params.threads,
        ),
        CollapseSchedule::Rounds => crate::collapse::collapse_sparse_rounds_parallel(
            matrix,
            params.threshold,
            params.threads,
        ),
        CollapseSchedule::Adaptive => crate::collapse::collapse_sparse_adaptive(
            matrix,
            params.threshold,
            params.adaptive_collapse,
        ),
    }
}

fn explain_sparse(
    matrix: &SparseDistanceMatrix,
    params: &RipsParams,
    cli: &Cli,
) -> crate::Result<ExplainedDiagram> {
    if let Some(path) = &cli.program {
        return explain_with_program(matrix, params, cli, path);
    }
    if let Some(path) = &cli.atlas {
        return explain_with_atlas(matrix, params, cli, path);
    }
    explain_direct(matrix, params, cli)
}

fn record_explain_collapse(
    matrix: &SparseDistanceMatrix,
    params: &RipsParams,
    cli: &Cli,
) -> crate::Result<()> {
    if cli.collapse_edges {
        let collapsed = collapse_for_explain(matrix, params)?;
        write_collapse_artifact(&collapsed, cli.collapse_certificate.as_deref())?;
    }
    Ok(())
}

fn explain_with_program(
    matrix: &SparseDistanceMatrix,
    params: &RipsParams,
    cli: &Cli,
    path: &Path,
) -> crate::Result<ExplainedDiagram> {
    record_explain_collapse(matrix, params, cli)?;
    let (artifact, program) =
        ProgramArtifact::compile(matrix, params, CertificateLimits::default())
            .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
    if let Some(path) = &cli.representatives {
        write_representatives(path, program.result())?;
    }
    let explained = program.result().clone();
    let bytes = artifact
        .encode()
        .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
    write_via_temporary(path, &bytes)?;
    eprintln!(
        "persistence program: wrote {} bytes and {} cyclic atoms to {}",
        bytes.len(),
        artifact.atoms().len(),
        path.display()
    );
    Ok(explained)
}

fn explain_with_atlas(
    matrix: &SparseDistanceMatrix,
    params: &RipsParams,
    cli: &Cli,
    path: &Path,
) -> crate::Result<ExplainedDiagram> {
    record_explain_collapse(matrix, params, cli)?;
    let artifact = AtlasArtifact::build(matrix, params, CertificateLimits::default())
        .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
    if let Some(path) = &cli.representatives {
        write_representatives(path, artifact.explained())?;
    }
    let explained = artifact.explained().clone();
    let bytes = artifact
        .encode()
        .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
    write_via_temporary(path, &bytes)?;
    eprintln!(
        "persistence atlas: wrote {} bytes to {}",
        bytes.len(),
        path.display()
    );
    Ok(explained)
}

fn explain_direct(
    matrix: &SparseDistanceMatrix,
    params: &RipsParams,
    cli: &Cli,
) -> crate::Result<ExplainedDiagram> {
    let explained = if cli.collapse_edges {
        let collapsed = collapse_for_explain(matrix, params)?;
        write_collapse_artifact(&collapsed, cli.collapse_certificate.as_deref())?;
        let mut inner = params.clone();
        inner.collapse_edges = false;
        inner.threshold = Some(collapsed.certificate.terminal_level());
        let reduced = rips_persistence_with_classes_sparse(&collapsed.matrix, &inner)?;
        lift_h1_classes(&collapsed, reduced)?
    } else {
        rips_persistence_with_classes_sparse(matrix, params)?
    };
    if let Some(path) = &cli.representatives {
        write_representatives(path, &explained)?;
    }
    Ok(explained)
}

fn write_representatives(path: &Path, explained: &ExplainedDiagram) -> crate::Result<()> {
    let mut json = String::new();
    json.push_str("{\n  \"format\": \"holos-h1-class-spaces-v1\",\n  \"spaces\": [");
    for (space_index, space) in explained.spaces.iter().enumerate() {
        if space_index != 0 {
            json.push(',');
        }
        write!(
            json,
            "\n    {{\"id\":\"{}\",\"birth\":{},\"death\":",
            space.id, space.interval.birth
        )
        .expect("writing to a string cannot fail");
        if space.interval.death.is_infinite() {
            json.push_str("null");
        } else {
            write!(json, "{}", space.interval.death).expect("writing to a string cannot fail");
        }
        write!(
            json,
            ",\"essential\":{},\"multiplicity\":{},\"basis\":[",
            space.interval.is_essential(),
            space.basis.len()
        )
        .expect("writing to a string cannot fail");
        for (basis_index, class) in space.basis.iter().enumerate() {
            if basis_index != 0 {
                json.push(',');
            }
            write!(
                json,
                "{{\"id\":\"{}\",\"index\":{},\"modulus\":{},\"scale\":{},\"terms\":[",
                class.id, class.basis_index, class.cocycle.modulus, class.cocycle.scale
            )
            .expect("writing to a string cannot fail");
            for (term_index, term) in class.cocycle.terms.iter().enumerate() {
                if term_index != 0 {
                    json.push(',');
                }
                write!(json, "[{},{},{}]", term.u, term.v, term.coefficient)
                    .expect("writing to a string cannot fail");
            }
            json.push_str("]}");
        }
        json.push_str("]}");
    }
    json.push_str("\n  ]\n}\n");
    write_via_temporary(path, json.as_bytes())?;
    eprintln!(
        "H1 representatives: wrote {} classes to {}",
        explained.class_count(),
        path.display()
    );
    Ok(())
}

fn run(cli: Cli) -> crate::Result<()> {
    let format = cli.format.unwrap_or_else(|| infer_format(&cli.input));
    let params = compute_params(&cli);
    validate_compute_options(&cli)?;
    let explain = explain_enabled(&cli);
    let (mut diagram, n_points) = match format {
        InputFormat::Sparse => compute_sparse_input(&cli, &params, explain)?,
        InputFormat::PointCloud => compute_point_input(&cli, &params, explain)?,
        InputFormat::LowerDistance => compute_lower_input(&cli, &params, explain)?,
    };
    diagram.canonicalize();
    write_cli_diagram(&cli, &diagram, n_points)
}

fn compute_params(cli: &Cli) -> RipsParams {
    let adaptive_collapse = AdaptiveCollapseParams {
        objective: cli
            .collapse_objective
            .map(Into::into)
            .unwrap_or(if cli.dim <= 1 {
                CollapseObjective::H1
            } else {
                CollapseObjective::H2
            }),
        work_limit: cli.collapse_work_limit,
    };
    RipsParams {
        max_dim: cli.dim,
        // None lets the library apply the input's own default.
        threshold: cli.threshold,
        modulus: cli.modulus,
        threads: cli.threads.max(1),
        use_emergent_pairs: !cli.no_emergent_pairs,
        use_apparent_pairs: !cli.no_apparent_pairs,
        use_clearing: !cli.no_clearing,
        use_adjacency_rows: !cli.no_adjacency_rows,
        collapse_edges: false,
        collapse_schedule: cli.collapse_schedule.unwrap_or(Schedule::Serial).into(),
        adaptive_collapse,
        engine: cli.engine.into(),
        dense_storage: cli.dense_storage.into(),
        factorization: cli.factorization.into(),
    }
}

fn validate_compute_options(cli: &Cli) -> crate::Result<()> {
    validate_collapse_options(cli)?;
    validate_explain_options(cli)
}

fn validate_collapse_options(cli: &Cli) -> crate::Result<()> {
    if cli.collapse_schedule.is_some() && !cli.collapse_edges {
        return Err(crate::Error::InvalidInput(
            "--collapse-schedule requires --collapse-edges".into(),
        ));
    }
    if cli.collapse_certificate.is_some() && !cli.collapse_edges {
        return Err(crate::Error::InvalidInput(
            "--collapse-certificate requires --collapse-edges".into(),
        ));
    }
    if (cli.collapse_objective.is_some() || cli.collapse_work_limit.is_some())
        && cli.collapse_schedule != Some(Schedule::Adaptive)
    {
        return Err(crate::Error::InvalidInput(
            "--collapse-objective and --collapse-work-limit require --collapse-schedule adaptive"
                .into(),
        ));
    }
    Ok(())
}

fn validate_explain_options(cli: &Cli) -> crate::Result<()> {
    if cli.atlas.is_some() && cli.program.is_some() {
        return Err(crate::Error::InvalidInput(
            "--atlas and --program cannot be used together".into(),
        ));
    }
    let explain = explain_enabled(cli);
    if explain && cli.dim < 1 {
        return Err(crate::Error::InvalidInput(
            "--representatives, --atlas, and --program require --dim of at least 1".into(),
        ));
    }
    if (cli.atlas.is_some() || cli.program.is_some()) && cli.dim != 1 {
        return Err(crate::Error::InvalidInput(
            "--atlas and --program require --dim 1".into(),
        ));
    }
    Ok(())
}

fn explain_enabled(cli: &Cli) -> bool {
    cli.representatives.is_some() || cli.atlas.is_some() || cli.program.is_some()
}

fn compute_sparse_input(
    cli: &Cli,
    params: &RipsParams,
    explain: bool,
) -> crate::Result<(crate::Diagram, usize)> {
    let matrix = io::read_sparse_matrix(&cli.input, params.threads)?;
    report_sparse_input(&matrix, cli.threshold);
    let diagram = compute_sparse_matrix(&matrix, params, cli, explain)?;
    Ok((diagram, matrix.len()))
}

fn report_sparse_input(matrix: &SparseDistanceMatrix, threshold: Option<f64>) {
    match threshold {
        Some(value) => eprintln!(
            "{} points, {} edges, threshold {value}",
            matrix.len(),
            matrix.num_edges()
        ),
        None => eprintln!(
            "{} points, {} edges, no threshold (all listed edges)",
            matrix.len(),
            matrix.num_edges()
        ),
    }
}

fn compute_sparse_matrix(
    matrix: &SparseDistanceMatrix,
    params: &RipsParams,
    cli: &Cli,
    explain: bool,
) -> crate::Result<crate::Diagram> {
    if explain {
        return Ok(explain_sparse(matrix, params, cli)?.diagram);
    }
    if cli.collapse_edges {
        return crate::collapse_and_solve(matrix, params, |collapsed| {
            write_collapse_artifact(collapsed, cli.collapse_certificate.as_deref())
        });
    }
    crate::rips_persistence_sparse(matrix, params)
}

fn compute_point_input(
    cli: &Cli,
    params: &RipsParams,
    explain: bool,
) -> crate::Result<(crate::Diagram, usize)> {
    let points = io::read_point_cloud(&cli.input, params.threads)?;
    let Some(threshold) = cli.threshold else {
        let matrix = DistanceMatrix::from_points(&points)?;
        report_dense_input(&matrix, None);
        let diagram = compute_dense_matrix(&matrix, params, cli, explain)?;
        return Ok((diagram, matrix.len()));
    };
    let built = PointCloudGraph::build(
        &points,
        PointCloudParams::new(threshold).with_threads(params.threads),
    )?;
    let matrix = built.matrix();
    eprintln!(
        "{} points, {} edges, threshold {threshold}, {:?} point construction",
        matrix.len(),
        matrix.num_edges(),
        built.stats().strategy
    );
    let diagram = compute_sparse_matrix(matrix, params, cli, explain)?;
    Ok((diagram, matrix.len()))
}

fn compute_lower_input(
    cli: &Cli,
    params: &RipsParams,
    explain: bool,
) -> crate::Result<(crate::Diagram, usize)> {
    let matrix = io::read_lower_distance_matrix(&cli.input, params.threads)?;
    report_dense_input(&matrix, cli.threshold);
    let diagram = compute_dense_matrix(&matrix, params, cli, explain)?;
    Ok((diagram, matrix.len()))
}

fn report_dense_input(matrix: &DistanceMatrix, threshold: Option<f64>) {
    match threshold {
        Some(value) => eprintln!("{} points, threshold {value}", matrix.len()),
        None => eprintln!(
            "{} points, threshold {} (enclosing radius)",
            matrix.len(),
            matrix.enclosing_radius()
        ),
    }
}

fn compute_dense_matrix(
    matrix: &DistanceMatrix,
    params: &RipsParams,
    cli: &Cli,
    explain: bool,
) -> crate::Result<crate::Diagram> {
    if explain {
        let threshold = cli.threshold.unwrap_or_else(|| matrix.enclosing_radius());
        let sparse = matrix.to_sparse_at(threshold)?;
        return Ok(explain_sparse(&sparse, params, cli)?.diagram);
    }
    if cli.collapse_edges {
        return crate::collapse_and_solve(matrix, params, |collapsed| {
            write_collapse_artifact(collapsed, cli.collapse_certificate.as_deref())
        });
    }
    crate::rips_persistence(matrix, params)
}

fn write_cli_diagram(cli: &Cli, diagram: &crate::Diagram, n_points: usize) -> crate::Result<()> {
    let output = match cli.output {
        DiagramFormat::Ripser => OutputFormat::Ripser,
        DiagramFormat::Csv => OutputFormat::Csv,
    };
    // Stdout flushes on every line; a diagram of many bars is written once
    // through a buffer.
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::with_capacity(1 << 16, stdout.lock());
    io::write_diagram(
        &mut out,
        diagram,
        output,
        cli.dim.min(n_points.saturating_sub(1)),
    )?;
    out.flush()
        .map_err(|error| crate::Error::Io(error.to_string()))
}

fn run_verify(cli: VerifyCli) -> crate::Result<()> {
    let bytes = read_bounded_artifact(&cli.artifact, cli.max_artifact_bytes, "collapse artifact")?;
    let limits = DecodeLimits {
        max_bytes: cli.max_artifact_bytes,
        ..DecodeLimits::default()
    };
    let artifact = CollapseArtifact::decode(&bytes, limits)
        .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
    verify_collapse_input(&cli, &artifact)?;
    report_verified_collapse(&artifact);
    Ok(())
}

fn verify_collapse_input(cli: &VerifyCli, artifact: &CollapseArtifact) -> crate::Result<()> {
    let format = cli.format.unwrap_or_else(|| infer_format(&cli.input));
    match format {
        InputFormat::Sparse => verify_sparse_collapse_input(cli, artifact),
        InputFormat::PointCloud => verify_point_collapse_input(cli, artifact),
        InputFormat::LowerDistance => verify_lower_collapse_input(cli, artifact),
    }
}

fn report_verified_collapse(artifact: &CollapseArtifact) {
    let completeness = match artifact.certificate().completeness() {
        CollapseCompleteness::CompleteFixedPoint => "complete fixed point",
        CollapseCompleteness::BudgetLimited => "budget-limited partial collapse",
    };
    println!(
        "verified collapse artifact: algorithm version {}, {completeness}, {} removals",
        artifact.certificate().algorithm_version(),
        artifact.certificate().steps().len()
    );
}

fn verify_sparse_collapse_input(cli: &VerifyCli, artifact: &CollapseArtifact) -> crate::Result<()> {
    let matrix = io::read_sparse_matrix(&cli.input, cli.threads.max(1))?;
    verify_sparse_artifact(&matrix, cli.threshold, artifact)
        .map_err(|error| crate::Error::InvalidInput(error.to_string()))
}

fn verify_point_collapse_input(cli: &VerifyCli, artifact: &CollapseArtifact) -> crate::Result<()> {
    let points = io::read_point_cloud(&cli.input, cli.threads.max(1))?;
    if let Some(threshold) = cli.threshold {
        let graph = PointCloudGraph::build(
            &points,
            PointCloudParams::new(threshold).with_threads(cli.threads),
        )?;
        return verify_sparse_artifact(graph.matrix(), Some(threshold), artifact)
            .map_err(|error| crate::Error::InvalidInput(error.to_string()));
    }
    let matrix = DistanceMatrix::from_points(&points)?;
    verify_dense_artifact(&matrix, None, artifact)
        .map_err(|error| crate::Error::InvalidInput(error.to_string()))
}

fn verify_lower_collapse_input(cli: &VerifyCli, artifact: &CollapseArtifact) -> crate::Result<()> {
    let matrix = io::read_lower_distance_matrix(&cli.input, cli.threads.max(1))?;
    verify_dense_artifact(&matrix, cli.threshold, artifact)
        .map_err(|error| crate::Error::InvalidInput(error.to_string()))
}

fn read_bounded_artifact(path: &Path, maximum: usize, label: &str) -> crate::Result<Vec<u8>> {
    use std::io::Read;

    let file = std::fs::File::open(path).map_err(|error| {
        crate::Error::Io(format!("cannot open {label} {}: {error}", path.display()))
    })?;
    let read_limit = u64::try_from(maximum).unwrap_or(u64::MAX).saturating_add(1);
    let mut bytes = Vec::new();
    file.take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            crate::Error::Io(format!("cannot read {label} {}: {error}", path.display()))
        })?;
    if bytes.len() > maximum {
        return Err(crate::Error::InvalidInput(format!(
            "{label} has {} bytes, above the limit {}",
            bytes.len(),
            maximum
        )));
    }
    Ok(bytes)
}

fn read_proof_input(
    input: &Path,
    format: Option<InputFormat>,
    threads: usize,
    threshold: Option<f64>,
) -> crate::Result<SparseDistanceMatrix> {
    let format = format.unwrap_or_else(|| infer_format(input));
    match format {
        InputFormat::Sparse => io::read_sparse_matrix(input, threads.max(1)),
        InputFormat::PointCloud => {
            let points = io::read_point_cloud(input, threads.max(1))?;
            if let Some(threshold) = threshold {
                PointCloudGraph::build(
                    &points,
                    PointCloudParams::new(threshold).with_threads(threads),
                )
                .map(PointCloudGraph::into_matrix)
            } else {
                let dense = DistanceMatrix::from_points(&points)?;
                dense.to_sparse_at(dense.enclosing_radius())
            }
        }
        InputFormat::LowerDistance => {
            let dense = io::read_lower_distance_matrix(input, threads.max(1))?;
            dense.to_sparse_at(threshold.unwrap_or_else(|| dense.enclosing_radius()))
        }
    }
}

fn run_prove(cli: ProveCli) -> crate::Result<()> {
    let threads = cli.threads.max(1);
    let initial = read_proof_input(&cli.input, cli.format, threads, cli.threshold)?;
    let updates = cli
        .updates
        .iter()
        .map(|path| read_proof_input(path, cli.format, threads, cli.threshold))
        .collect::<crate::Result<Vec<_>>>()?;
    let params = RipsParams {
        max_dim: 1,
        threshold: cli.threshold,
        modulus: cli.modulus,
        threads,
        ..RipsParams::default()
    };
    let proof = ProofArtifact::build(&initial, &updates, &params, CertificateLimits::default())
        .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
    let summary = proof.summary();
    let bytes = proof
        .encode()
        .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
    write_via_temporary(&cli.output, &bytes)?;
    println!(
        "wrote {} snapshots through {} unique reduction nodes; {} references reused",
        summary.snapshots, summary.unique_nodes, summary.reused_references
    );
    Ok(())
}

fn run_index(cli: IndexCli) -> crate::Result<()> {
    validate_index_paths(&cli)?;
    let threads = cli.threads.max(1);
    let initial = read_proof_input(&cli.input, cli.format, threads, cli.threshold)?;
    let params = RipsParams {
        max_dim: cli.dim,
        threshold: cli.threshold,
        modulus: cli.modulus,
        threads,
        ..RipsParams::default()
    };
    let index_params = IndexParams {
        max_separator_width: cli.separator_width,
        separator_search_limit: cli.separator_search_limit,
        leaf_vertices: cli.leaf_vertices,
        interface_policy: if cli.materialize_interfaces {
            InterfacePolicy::Materialize
        } else {
            InterfacePolicy::Relative
        },
    };
    let limits = CertificateLimits::default();
    let index = PersistenceIndex::compile(&initial, &params, index_params, limits)?;
    let mut stream = IndexStream::new(index);
    let snapshot = stream
        .checkpoint()
        .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
    let encoded = snapshot
        .encode()
        .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
    write_via_temporary(&cli.snapshot, &encoded)?;
    let snapshot_summary = snapshot.summary();
    let work = write_index_updates(&cli, threads, &mut stream)?;
    println!(
        "wrote one initial checkpoint through dimension {} with {} interfaces and {} records with {} changed interfaces, {} edge changes, and {} envelope checkpoints",
        cli.dim,
        snapshot_summary.nodes,
        cli.record.len(),
        work.changed_nodes,
        work.edge_changes,
        work.cold_records
    );
    Ok(())
}

fn validate_index_paths(cli: &IndexCli) -> crate::Result<()> {
    if cli.update.len() != cli.record.len() {
        return Err(crate::Error::InvalidInput(format!(
            "--update occurs {} times but --record occurs {} times",
            cli.update.len(),
            cli.record.len()
        )));
    }
    Ok(())
}

#[derive(Default)]
struct IndexCliWork {
    changed_nodes: usize,
    edge_changes: usize,
    cold_records: usize,
}

fn write_index_updates(
    cli: &IndexCli,
    threads: usize,
    stream: &mut IndexStream,
) -> crate::Result<IndexCliWork> {
    let mut work = IndexCliWork::default();
    for (input, output) in cli.update.iter().zip(&cli.record) {
        let graph = read_proof_input(input, cli.format, threads, cli.threshold)?;
        let step = stream.apply_graph(&graph, CorrespondenceMode::Omit)?;
        let summary = match &step.proof {
            IndexStreamProof::Delta(proof) => proof.summary(),
            IndexStreamProof::Snapshot(proof) => {
                work.cold_records += 1;
                proof.summary()
            }
        };
        let bytes = step
            .proof
            .encode()
            .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
        write_via_temporary(output, &bytes)?;
        work.changed_nodes += summary.nodes;
        work.edge_changes += summary.edge_changes;
    }
    Ok(work)
}

fn run_interface(cli: InterfaceCli) -> crate::Result<()> {
    let input = read_proof_input(&cli.input, cli.format, cli.threads, cli.threshold)?;
    let mut params = RipsParams::new(cli.dim).with_modulus(cli.modulus);
    params.threshold = cli.threshold;
    let certificate = RelativeInterfaceCertificate::build(
        &input,
        &params,
        &cli.protected,
        CertificateLimits::default(),
    )
    .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
    let bytes = certificate
        .encode(CertificateLimits::default())
        .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
    write_via_temporary(&cli.output, &bytes)?;
    let work = certificate.work();
    println!(
        "wrote relative interface through dimension {} with {} input cells, {} cancellations, {} retained cells, and {} bytes",
        cli.dim,
        work.input_cells,
        work.cancellations,
        work.core_cells,
        bytes.len(),
    );
    Ok(())
}

fn run_merge_interfaces(cli: MergeInterfacesCli) -> crate::Result<()> {
    let store = DurableInterfaceStore::open(&cli.store)
        .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
    let mut shard_ids = Vec::with_capacity(cli.shards.len());
    for path in &cli.shards {
        let bytes = read_bounded_artifact(path, cli.max_artifact_bytes, "interface shard")?;
        let (id, _) = store
            .put(&bytes)
            .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
        shard_ids.push(id);
    }
    let limits = certificate_limits(cli.max_artifact_bytes);
    let commit = store
        .commit_stored(&shard_ids, &cli.separator, &cli.protected, limits)
        .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
    let manifest = commit
        .manifest()
        .encode()
        .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
    let result = commit
        .certificate()
        .encode(limits)
        .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
    write_via_temporary(&cli.manifest, &manifest)?;
    write_via_temporary(&cli.result, &result)?;
    let work = commit.work();
    println!(
        "committed distributed interface {} with {} shards, {} reused folds, {} computed folds, and {} result bytes",
        commit.manifest().job(),
        work.shards,
        work.folds_reused,
        work.folds_computed,
        result.len(),
    );
    Ok(())
}

fn run_cohomology(cli: CohomologyCli) -> crate::Result<()> {
    let graph = read_proof_input(&cli.input, cli.format, cli.threads, Some(cli.scale))?;
    let limits = CohomologyLimits::default();
    let space = cohomology_space(&graph, cli.dimension, cli.scale, cli.modulus, limits)?;
    println!(
        "H{} at {} over Z/{} has rank {} and space id {}",
        cli.dimension,
        cli.scale,
        cli.modulus,
        space.rank(),
        space.id()
    );
    for class in space.basis() {
        let mut terms = String::new();
        for (position, term) in class.terms.iter().enumerate() {
            if position != 0 {
                terms.push_str(", ");
            }
            write!(terms, "{:?}:{}", term.simplex, term.coefficient)
                .expect("writing to a string cannot fail");
        }
        println!("basis {} {} [{}]", class.basis_index, class.id, terms);
    }
    if let Some(other) = cli.other {
        let other_graph = read_proof_input(&other, cli.format, cli.threads, Some(cli.scale))?;
        let other_space =
            cohomology_space(&other_graph, cli.dimension, cli.scale, cli.modulus, limits)?;
        let relation = cohomology_relation(&graph, &space, &other_graph, &other_space, limits)?;
        println!(
            "relation rank {} from image ranks {} and {}; isomorphism {}",
            relation.relation_rank,
            relation.old_image_rank,
            relation.new_image_rank,
            relation.is_isomorphism()
        );
    }
    Ok(())
}

fn read_kinetic_edges(path: &Path, maximum_bytes: usize) -> crate::Result<Vec<KineticEdge>> {
    let bytes = read_bounded_artifact(path, maximum_bytes, "affine trajectory")?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|error| crate::Error::InvalidInput(format!("trajectory is not UTF-8: {error}")))?;
    let mut edges = Vec::new();
    for (line_index, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 4 {
            return Err(crate::Error::InvalidInput(format!(
                "trajectory line {} needs u, v, intercept, and velocity",
                line_index + 1
            )));
        }
        let parse_usize = |position: usize| {
            fields[position].parse::<usize>().map_err(|error| {
                crate::Error::InvalidInput(format!(
                    "trajectory line {} has an invalid vertex: {error}",
                    line_index + 1
                ))
            })
        };
        let parse_f64 = |position: usize| {
            fields[position].parse::<f64>().map_err(|error| {
                crate::Error::InvalidInput(format!(
                    "trajectory line {} has an invalid coefficient: {error}",
                    line_index + 1
                ))
            })
        };
        edges.push(KineticEdge {
            u: parse_usize(0)?,
            v: parse_usize(1)?,
            intercept: parse_f64(2)?,
            velocity: parse_f64(3)?,
        });
    }
    Ok(edges)
}

fn kinetic_kind(kind: &KineticEventKind) -> String {
    match kind {
        KineticEventKind::ThresholdCrossing { edge } => {
            format!("threshold ({}, {})", edge.u, edge.v)
        }
        KineticEventKind::EdgeOrderSwap { first, second } => format!(
            "order ({}, {}) with ({}, {})",
            first.u, first.v, second.u, second.v
        ),
    }
}

fn run_kinetic(cli: KineticCli) -> crate::Result<()> {
    let edges = read_kinetic_edges(&cli.input, cli.max_artifact_bytes)?;
    let trajectory = KineticFiltration::new(
        cli.vertices,
        edges,
        cli.start,
        cli.end,
        KineticLimits::default(),
    )?;
    let schedule = trajectory.events(cli.scale)?;
    report_kinetic_schedule(&schedule);
    if let (Some(dimension), Some(scale)) = (cli.dimension, cli.scale) {
        run_kinetic_cohomology(&cli, &trajectory, dimension, scale)?;
    }
    Ok(())
}

fn report_kinetic_schedule(schedule: &crate::KineticSchedule) {
    println!(
        "certified {} isolated events and {} persistent ties on [{}, {}]",
        schedule.events.len(),
        schedule.persistent_ties,
        schedule.start,
        schedule.end
    );
    for event in &schedule.events {
        let kinds = event
            .kinds
            .iter()
            .map(kinetic_kind)
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "event {} in [{}, {}]: {}",
            event.time, event.lower, event.upper, kinds
        );
    }
}

fn run_kinetic_cohomology(
    cli: &KineticCli,
    trajectory: &KineticFiltration,
    dimension: usize,
    scale: f64,
) -> crate::Result<()> {
    let relations =
        trajectory.cohomology_events(dimension, scale, cli.modulus, CohomologyLimits::default())?;
    for event in relations {
        println!(
            "H{} event {}: rank {} to {}, relation rank {}",
            dimension,
            event.event.time,
            event.before_rank,
            event.after_rank,
            event.relation.relation_rank
        );
    }
    if let Some(output) = &cli.zigzag {
        write_kinetic_zigzag(cli, trajectory, dimension, scale, output)?;
    }
    Ok(())
}

fn write_kinetic_zigzag(
    cli: &KineticCli,
    trajectory: &KineticFiltration,
    dimension: usize,
    scale: f64,
    output: &Path,
) -> crate::Result<()> {
    let limits = KineticZigzagArtifactLimits {
        max_bytes: cli.max_artifact_bytes,
        ..KineticZigzagArtifactLimits::default()
    };
    let (artifact, zigzag) =
        KineticZigzagArtifact::build(trajectory, dimension, scale, cli.modulus, limits)?;
    let bytes = artifact.encode(limits)?;
    write_via_temporary(output, &bytes)?;
    let summary = artifact.summary();
    println!(
        "wrote H{} kinetic zigzag with {} nodes, {} arrows, {} interval spaces, {} interval copies, and {} bytes",
        dimension,
        summary.nodes,
        summary.arrows,
        summary.intervals,
        summary.interval_copies,
        bytes.len()
    );
    for interval in zigzag.barcode.intervals {
        println!(
            "zigzag [{}..={}] multiplicity {} id {}",
            interval.start, interval.end, interval.multiplicity, interval.id
        );
    }
    Ok(())
}

fn run_cohomology_intervention(cli: CohomologyInterventionCli) -> crate::Result<()> {
    let graph = read_proof_input(&cli.input, cli.format, cli.threads, Some(cli.scale))?;
    let candidates = parse_weighted_candidates(&cli.candidates)?;
    let limits = CohomologyInterventionLimits {
        max_oracle_calls: cli.oracle_limit,
        max_search_nodes: cli.node_limit,
        ..CohomologyInterventionLimits::default()
    };
    let scenario = CohomologyInterventionScenario::from_graph(&graph, cli.scale, cli.target)?;
    let artifact = CohomologyInterventionArtifact::build(
        graph.len(),
        cli.dimension,
        cli.scale,
        cli.modulus,
        &[scenario],
        &candidates,
        cli.max_edits,
        limits,
    )?;
    let bytes = artifact.encode(limits)?;
    write_via_temporary(&cli.output, &bytes)?;
    println!(
        "certified H{} intervention: {}, {} edits, {} oracle calls, cost bounds {:?} to {:?}, wrote {} bytes",
        cli.dimension,
        artifact.status(),
        artifact.edits().len(),
        artifact.oracle_calls(),
        artifact.lower_bound_cost(),
        artifact.upper_bound_cost(),
        bytes.len()
    );
    Ok(())
}

fn run_link_plan(cli: LinkPlanCli) -> crate::Result<()> {
    if cli.scenarios.len() != cli.targets.len() {
        return Err(crate::Error::InvalidInput(
            "--scenario and --target counts must match".into(),
        ));
    }
    let scenarios = cli
        .scenarios
        .iter()
        .zip(&cli.targets)
        .map(|(path, target)| {
            let parsed = io::read_sparse_matrix(path, cli.threads)?;
            if parsed.len() > cli.vertices {
                return Err(crate::Error::InvalidInput(format!(
                    "scenario {} uses a vertex above --vertices {}",
                    path.display(),
                    cli.vertices
                )));
            }
            let triplets = parsed.edges().collect::<Vec<_>>();
            let graph = SparseDistanceMatrix::from_triplets(cli.vertices, &triplets)?;
            CohomologyInterventionScenario::from_graph(&graph, cli.scale, *target)
        })
        .collect::<crate::Result<Vec<_>>>()?;
    let candidates = parse_weighted_candidates(&cli.candidates)?;
    let limits = CohomologyInterventionLimits {
        max_oracle_calls: cli.oracle_limit,
        max_search_nodes: cli.node_limit,
        ..CohomologyInterventionLimits::default()
    };
    let artifact = CohomologyInterventionArtifact::build(
        cli.vertices,
        cli.dimension,
        cli.scale,
        cli.modulus,
        &scenarios,
        &candidates,
        cli.max_edits,
        limits,
    )?;
    let bytes = artifact.encode(limits)?;
    write_via_temporary(&cli.output, &bytes)?;
    println!(
        "certified H{} link plan across {} scenarios: {}, {} links, cost bounds {:?} to {:?}, {} oracle calls, wrote {} bytes",
        cli.dimension,
        scenarios.len(),
        artifact.status(),
        artifact.edits().len(),
        artifact.lower_bound_cost(),
        artifact.upper_bound_cost(),
        artifact.oracle_calls(),
        bytes.len(),
    );
    Ok(())
}

fn run_synthesis(cli: SynthesisCli) -> crate::Result<()> {
    let declared_states = cli.states.len();
    let states = synthesis_states(&cli)?;
    let specification =
        TopologicalSpecification::new(cli.vertices, cli.dimension, cli.scale, cli.modulus, states);
    let actions = parse_synthesis_actions(&cli.candidates, &specification)?;
    let limits = SynthesisLimits {
        max_oracle_calls: cli.oracle_limit,
        max_search_nodes: cli.node_limit,
        ..SynthesisLimits::default()
    };
    let artifact = SynthesisArtifact::build(specification, actions, cli.max_edits, limits)?;
    let bytes = artifact.encode(limits)?;
    write_via_temporary(&cli.output, &bytes)?;
    println!(
        "certified H{} synthesis across {} of {} constrained states: {}, {} actions, cost bounds {:?} to {:?}, {} producer topology calls, {} proof topology checks, wrote {} bytes",
        cli.dimension,
        artifact.specification().states().len(),
        declared_states,
        artifact.status(),
        artifact.selected().len(),
        artifact.lower_bound_cost(),
        artifact.upper_bound_cost(),
        artifact.producer_oracle_calls(),
        artifact.proof_topology_checks(),
        bytes.len(),
    );
    Ok(())
}

fn synthesis_states(cli: &SynthesisCli) -> crate::Result<Vec<SynthesisState>> {
    let mut states = Vec::new();
    for (step, path) in cli.states.iter().enumerate() {
        let graph = read_synthesis_state(path, cli.vertices, cli.threads)?;
        let space = cohomology_space(
            &graph,
            cli.dimension,
            cli.scale,
            cli.modulus,
            CohomologyLimits::default(),
        )?;
        if space.rank() <= cli.max_rank {
            continue;
        }
        let target = space.full_subspace();
        states.push(SynthesisState::from_subspace(
            0,
            step as u64,
            &graph,
            cli.scale,
            &space,
            &target,
            cli.max_rank,
        )?);
    }
    Ok(states)
}

fn read_synthesis_state(
    path: &Path,
    vertices: usize,
    threads: usize,
) -> crate::Result<SparseDistanceMatrix> {
    let parsed = io::read_sparse_matrix(path, threads)?;
    if parsed.len() > vertices {
        return Err(crate::Error::InvalidInput(format!(
            "state {} uses a vertex above --vertices {vertices}",
            path.display()
        )));
    }
    SparseDistanceMatrix::from_triplets(vertices, &parsed.edges().collect::<Vec<_>>())
}

fn run_kinetic_synthesis(cli: KineticSynthesisCli) -> crate::Result<()> {
    let edges = read_kinetic_edges(&cli.input, cli.max_artifact_bytes)?;
    let trajectory = KineticFiltration::new(
        cli.vertices,
        edges,
        cli.start,
        cli.end,
        KineticLimits::default(),
    )?;
    let specification = TopologicalSpecification::from_kinetic_rank_ceiling(
        &trajectory,
        0,
        cli.dimension,
        cli.scale,
        cli.modulus,
        cli.max_rank,
        CohomologyLimits::default(),
    )?;
    let actions = parse_synthesis_actions(&cli.candidates, &specification)?;
    let limits = SynthesisLimits {
        max_bytes: cli.max_artifact_bytes,
        max_oracle_calls: cli.oracle_limit,
        max_search_nodes: cli.node_limit,
        ..SynthesisLimits::default()
    };
    let artifact = SynthesisArtifact::build(specification, actions, cli.max_edits, limits)?;
    let bytes = artifact.encode(limits)?;
    write_via_temporary(&cli.output, &bytes)?;
    println!(
        "certified H{} all-time Rips rank plan across {} constrained critical states: {}, {} actions, cost bounds {:?} to {:?}, {} producer topology calls, {} proof topology checks, wrote {} bytes",
        cli.dimension,
        artifact.specification().states().len(),
        artifact.status(),
        artifact.selected().len(),
        artifact.lower_bound_cost(),
        artifact.upper_bound_cost(),
        artifact.producer_oracle_calls(),
        artifact.proof_topology_checks(),
        bytes.len(),
    );
    Ok(())
}

fn run_coverage(cli: CoverageCli) -> crate::Result<()> {
    let model = PlanarCoverageModel::new(cli.broadcast_radius, cli.sensing_radius)?;
    let fence = CoverageFence::new(cli.fence)?;
    let base = coverage_base(fence.vertices(), &cli.base);
    let mut states = Vec::with_capacity(cli.states.len());
    for (step, path) in cli.states.iter().enumerate() {
        let parsed = io::read_sparse_matrix(path, cli.threads)?;
        if parsed.len() > cli.vertices {
            return Err(crate::Error::InvalidInput(format!(
                "state {} uses a vertex above --vertices {}",
                path.display(),
                cli.vertices
            )));
        }
        let graph =
            SparseDistanceMatrix::from_triplets(cli.vertices, &parsed.edges().collect::<Vec<_>>())?;
        states.push(CoverageState::new(
            0,
            step as u64,
            &graph,
            base.clone(),
            cli.broadcast_radius,
        )?);
    }
    let specification = CoverageSpecification::new(
        cli.vertices,
        model,
        cli.modulus,
        fence,
        cli.failable,
        cli.failure_budget,
        states,
        CoverageLimits::default(),
    )?;
    let actions = parse_coverage_actions(&cli.candidates, &specification)?;
    write_coverage_artifact(
        specification,
        actions,
        cli.max_activations,
        cli.oracle_limit,
        cli.node_limit,
        cli.max_artifact_bytes,
        &cli.output,
        "finite",
    )
}

fn run_affine_coverage(cli: AffineCoverageCli) -> crate::Result<()> {
    let edges = read_kinetic_edges(&cli.input, cli.max_artifact_bytes)?;
    let trajectory = KineticFiltration::new(
        cli.vertices,
        edges,
        cli.start,
        cli.end,
        KineticLimits::default(),
    )?;
    let model = PlanarCoverageModel::new(cli.broadcast_radius, cli.sensing_radius)?;
    let fence = CoverageFence::new(cli.fence)?;
    let base = coverage_base(fence.vertices(), &cli.base);
    let specification = CoverageSpecification::from_kinetic(
        &trajectory,
        0,
        model,
        cli.modulus,
        fence,
        cli.failable,
        cli.failure_budget,
        base,
        CoverageLimits::default(),
    )?;
    let actions = parse_coverage_actions(&cli.candidates, &specification)?;
    write_coverage_artifact(
        specification,
        actions,
        cli.max_activations,
        cli.oracle_limit,
        cli.node_limit,
        cli.max_artifact_bytes,
        &cli.output,
        "complete affine",
    )
}

fn coverage_base(fence: &[usize], additional: &[usize]) -> Vec<usize> {
    let mut base = fence.iter().chain(additional).copied().collect::<Vec<_>>();
    base.sort_unstable();
    base.dedup();
    base
}

fn parse_coverage_actions(
    values: &[String],
    specification: &CoverageSpecification,
) -> crate::Result<Vec<CoverageAction>> {
    if values.len() % 3 != 0 {
        return Err(crate::Error::InvalidInput(
            "--candidate requires V COST STATES".into(),
        ));
    }
    let mut actions = values
        .chunks_exact(3)
        .map(|candidate| {
            let vertex = candidate[0].parse::<usize>().map_err(|_| {
                crate::Error::InvalidInput(format!(
                    "candidate vertex {} is not an integer",
                    candidate[0]
                ))
            })?;
            let cost = candidate[1].parse::<u64>().map_err(|_| {
                crate::Error::InvalidInput(format!(
                    "candidate cost {} is not an integer",
                    candidate[1]
                ))
            })?;
            let states = if candidate[2] == "all" {
                (0..specification.states().len()).collect()
            } else {
                candidate[2]
                    .split(',')
                    .map(|value| {
                        value.parse::<usize>().map_err(|_| {
                            crate::Error::InvalidInput(format!(
                                "candidate state {value} is not an integer"
                            ))
                        })
                    })
                    .collect::<crate::Result<Vec<_>>>()?
            };
            Ok(CoverageAction::new(vertex, cost, states))
        })
        .collect::<crate::Result<Vec<_>>>()?;
    actions.sort_by_key(|action| action.vertex);
    if actions
        .windows(2)
        .any(|pair| pair[0].vertex == pair[1].vertex)
    {
        return Err(crate::Error::InvalidInput(
            "--candidate repeats a sensor vertex".into(),
        ));
    }
    Ok(actions)
}

#[allow(clippy::too_many_arguments)]
fn write_coverage_artifact(
    specification: CoverageSpecification,
    actions: Vec<CoverageAction>,
    max_activations: usize,
    oracle_limit: usize,
    node_limit: usize,
    max_artifact_bytes: usize,
    output: &Path,
    scope: &str,
) -> crate::Result<()> {
    let limits = CoverageSynthesisLimits {
        max_bytes: max_artifact_bytes,
        max_oracle_calls: oracle_limit,
        max_search_nodes: node_limit,
        ..CoverageSynthesisLimits::default()
    };
    let artifact =
        CoverageSynthesisArtifact::build(specification, actions, max_activations, limits)?;
    let bytes = artifact.encode(limits)?;
    write_via_temporary(output, &bytes)?;
    println!(
        "certified {scope} relative coverage across {} states and failure budget {}: {}, {} activations, cost bounds {:?} to {:?}, {} producer topology calls, {} proof topology checks, wrote {} bytes",
        artifact.specification().states().len(),
        artifact.specification().failure_budget(),
        artifact.status(),
        artifact.selected().len(),
        artifact.lower_bound_cost(),
        artifact.upper_bound_cost(),
        artifact.producer_oracle_calls(),
        artifact.proof_topology_checks(),
        bytes.len(),
    );
    eprintln!(
        "physical coverage requires the declared planar domain, sensor placement, fence, and communication assumptions"
    );
    Ok(())
}

fn parse_synthesis_actions(
    values: &[usize],
    specification: &TopologicalSpecification,
) -> crate::Result<Vec<SynthesisAction>> {
    if specification.states().is_empty() {
        return Ok(Vec::new());
    }
    let mut actions = values
        .chunks_exact(3)
        .map(|candidate| {
            SynthesisAction::throughout(
                candidate[0],
                candidate[1],
                candidate[2] as u64,
                specification,
            )
        })
        .collect::<Vec<_>>();
    actions.sort();
    let original_count = actions.len();
    actions.dedup_by_key(|action| action.edge);
    if actions.len() != original_count {
        return Err(crate::Error::InvalidInput(
            "--candidate repeats an edge".into(),
        ));
    }
    Ok(actions)
}

fn parse_weighted_candidates(
    values: &[usize],
) -> crate::Result<Vec<CohomologyInterventionCandidate>> {
    let mut candidates = values
        .chunks_exact(3)
        .map(|candidate| {
            CohomologyInterventionCandidate::new(candidate[0], candidate[1], candidate[2] as u64)
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|candidate| candidate.edge);
    let original_count = candidates.len();
    candidates.dedup_by_key(|candidate| candidate.edge);
    if candidates.len() != original_count {
        return Err(crate::Error::InvalidInput(
            "--candidate repeats an edge".into(),
        ));
    }
    Ok(candidates)
}

fn run_verify_atlas(cli: VerifyAtlasCli) -> crate::Result<()> {
    let bytes = read_bounded_artifact(&cli.artifact, cli.max_artifact_bytes, "persistence atlas")?;
    let atlas_limits = AtlasDecodeLimits {
        max_bytes: cli.max_artifact_bytes,
        max_certificate_bytes: cli.max_artifact_bytes,
        ..AtlasDecodeLimits::default()
    };
    let certificate_limits = CertificateLimits {
        max_bytes: cli.max_artifact_bytes,
        ..CertificateLimits::default()
    };
    let artifact = AtlasArtifact::decode(&bytes, atlas_limits, certificate_limits)
        .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
    let matrix = read_proof_input(&cli.input, cli.format, cli.threads, artifact.threshold())?;
    artifact
        .verify(&matrix, certificate_limits)
        .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
    let classes: usize = artifact
        .spaces()
        .iter()
        .map(|space| space.basis.len())
        .sum();
    println!(
        "verified persistence atlas: Z/{}, {} bars, {} H1 class spaces, {} basis classes",
        artifact.modulus(),
        artifact.diagram().bars.len(),
        artifact.spaces().len(),
        classes
    );
    Ok(())
}

fn run_verify_trajectory(cli: VerifyTrajectoryCli) -> crate::Result<()> {
    let bytes = read_bounded_artifact(
        &cli.artifact,
        cli.max_artifact_bytes,
        "persistence trajectory",
    )?;
    let trace_limits = TrajectoryDecodeLimits {
        max_bytes: cli.max_artifact_bytes,
        max_atlas_bytes: cli.max_artifact_bytes,
        ..TrajectoryDecodeLimits::default()
    };
    let atlas_limits = AtlasDecodeLimits {
        max_bytes: cli.max_artifact_bytes,
        max_certificate_bytes: cli.max_artifact_bytes,
        ..AtlasDecodeLimits::default()
    };
    let certificate_limits = CertificateLimits {
        max_bytes: cli.max_artifact_bytes,
        ..CertificateLimits::default()
    };
    let artifact =
        TrajectoryArtifact::decode(&bytes, trace_limits, atlas_limits, certificate_limits)
            .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
    let verified = artifact
        .verify(certificate_limits)
        .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
    let reused = verified
        .steps
        .iter()
        .filter(|step| step.mode == crate::UpdateMode::Reused)
        .count();
    let events: usize = verified.steps.iter().map(|step| step.events.len()).sum();
    println!(
        "verified persistence trajectory: {} steps, {reused} reused, {events} region events",
        verified.steps.len()
    );
    Ok(())
}

fn program_limits(maximum: usize) -> ProgramDecodeLimits {
    ProgramDecodeLimits {
        max_bytes: maximum,
        max_atlas_bytes: maximum,
        atlas: AtlasDecodeLimits {
            max_bytes: maximum,
            max_certificate_bytes: maximum,
            ..AtlasDecodeLimits::default()
        },
        ..ProgramDecodeLimits::default()
    }
}

fn program_trace_limits(maximum: usize) -> ProgramTraceDecodeLimits {
    ProgramTraceDecodeLimits {
        max_bytes: maximum,
        max_checkpoint_bytes: maximum,
        program: program_limits(maximum),
        ..ProgramTraceDecodeLimits::default()
    }
}

fn certificate_limits(maximum: usize) -> CertificateLimits {
    CertificateLimits {
        max_bytes: maximum,
        ..CertificateLimits::default()
    }
}

fn run_verify_program(cli: VerifyProgramCli) -> crate::Result<()> {
    let bytes =
        read_bounded_artifact(&cli.artifact, cli.max_artifact_bytes, "persistence program")?;
    let limits = certificate_limits(cli.max_artifact_bytes);
    let artifact = ProgramArtifact::decode(&bytes, program_limits(cli.max_artifact_bytes), limits)
        .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
    let matrix = read_proof_input(&cli.input, cli.format, cli.threads, artifact.threshold())?;
    let program = artifact
        .verify(&matrix, limits)
        .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
    let summary = program.summary();
    println!(
        "verified persistence program: Z/{}, {} bars, {} atoms, {} cyclic atoms, {} guards",
        artifact.modulus(),
        artifact.diagram().bars.len(),
        summary.atoms,
        summary.cyclic_atoms,
        summary.guards
    );
    Ok(())
}

fn run_verify_program_trace(cli: VerifyProgramTraceCli) -> crate::Result<()> {
    let bytes = read_bounded_artifact(
        &cli.artifact,
        cli.max_artifact_bytes,
        "persistence program trace",
    )?;
    let limits = certificate_limits(cli.max_artifact_bytes);
    let artifact =
        ProgramTraceArtifact::decode(&bytes, program_trace_limits(cli.max_artifact_bytes), limits)
            .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
    let verified = artifact
        .verify(limits)
        .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
    let reused = verified
        .steps
        .iter()
        .filter(|step| step.mode == crate::ProgramUpdateMode::Reused)
        .count();
    let repaired = verified
        .steps
        .iter()
        .filter(|step| step.mode == crate::ProgramUpdateMode::Repaired)
        .count();
    println!(
        "verified persistence program trace: {} steps, {reused} reused, {repaired} repaired",
        verified.steps.len()
    );
    Ok(())
}

fn run_intervene(cli: InterveneCli) -> crate::Result<()> {
    validate_intervention_budget(cli.budget)?;
    let program = read_intervention_program(&cli)?;
    let target = intervention_target(&program, cli.space)?;
    let intervention =
        program.kill_h1_before(target, cli.before, InterventionBudget::new(cli.budget))?;
    let proof = intervention.artifact.ok_or_else(|| {
        crate::Error::InvalidInput(
            "the candidate budget ended without a certified intervention".into(),
        )
    })?;
    let encoded = proof
        .encode()
        .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
    write_via_temporary(&cli.output, &encoded)?;
    println!(
        "certified H1 intervention: {:?}, {} edits, lower bound {}, upper bound {}, wrote {} bytes to {}",
        intervention.status,
        intervention.edits.len(),
        intervention.lower_bound,
        intervention
            .upper_bound
            .expect("a feasible intervention has an upper bound"),
        encoded.len(),
        cli.output.display()
    );
    Ok(())
}

fn validate_intervention_budget(budget: usize) -> crate::Result<()> {
    if budget == 0 {
        return Err(crate::Error::InvalidInput(
            "--budget must be at least 1 when an output artifact is requested".into(),
        ));
    }
    Ok(())
}

fn read_intervention_program(cli: &InterveneCli) -> crate::Result<crate::PersistenceProgram> {
    let bytes = read_bounded_artifact(&cli.program, cli.max_artifact_bytes, "persistence program")?;
    let limits = certificate_limits(cli.max_artifact_bytes);
    let artifact = ProgramArtifact::decode(&bytes, program_limits(cli.max_artifact_bytes), limits)
        .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
    let matrix = read_proof_input(&cli.input, cli.format, cli.threads, artifact.threshold())?;
    artifact
        .verify(&matrix, limits)
        .map_err(|error| crate::Error::InvalidInput(error.to_string()))
}

fn intervention_target(
    program: &crate::PersistenceProgram,
    space: usize,
) -> crate::Result<crate::IntervalGroupId> {
    program
        .result()
        .spaces
        .get(space)
        .map(|target| target.id)
        .ok_or_else(|| {
            crate::Error::InvalidInput(format!(
                "H1 class-space index {} is out of range for {} spaces",
                space,
                program.result().spaces.len()
            ))
        })
}

fn run_verify_intervention(cli: VerifyInterventionCli) -> crate::Result<()> {
    let bytes = read_bounded_artifact(
        &cli.artifact,
        cli.max_artifact_bytes,
        "persistence intervention",
    )?;
    let limits = certificate_limits(cli.max_artifact_bytes);
    let artifact = InterventionArtifact::decode(
        &bytes,
        InterventionDecodeLimits {
            max_bytes: cli.max_artifact_bytes,
            max_trace_bytes: cli.max_artifact_bytes,
            trace: program_trace_limits(cli.max_artifact_bytes),
            ..InterventionDecodeLimits::default()
        },
        limits,
    )
    .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
    let verified = artifact
        .verify(limits)
        .map_err(|error| crate::Error::InvalidInput(error.to_string()))?;
    println!(
        "verified H1 intervention: {:?}, {} edits, lower bound {}, upper bound {}",
        verified.status,
        verified.edits.len(),
        verified.lower_bound,
        verified.upper_bound
    );
    Ok(())
}

/// Run the `holos` CLI on `argv` and return the process exit code.
///
/// `argv[0]` is the program name. The binary and the Python bindings both
/// enter here, so the CLI behaves the same either way.
pub fn run_cli<I, T>(argv: I) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let argv: Vec<std::ffi::OsString> = argv.into_iter().map(Into::into).collect();
    let handler = subcommand_handler(argv.get(1).map(std::ffi::OsString::as_os_str));
    match handler {
        Some(handler) => handler(argv),
        None => run_main_command(argv),
    }
}

type CommandHandler = fn(Vec<std::ffi::OsString>) -> i32;

fn subcommand_handler(command: Option<&std::ffi::OsStr>) -> Option<CommandHandler> {
    let command = command?;
    SUBCOMMANDS
        .iter()
        .find_map(|(name, handler)| (command == std::ffi::OsStr::new(name)).then_some(*handler))
}

fn parse_command<C: Parser>(
    argv: Vec<std::ffi::OsString>,
    command: &str,
    run: fn(C) -> crate::Result<()>,
) -> i32 {
    let mut command_argv = Vec::with_capacity(argv.len().saturating_sub(1));
    command_argv.push(std::ffi::OsString::from(format!("holos {command}")));
    command_argv.extend(argv.into_iter().skip(2));
    match C::try_parse_from(command_argv) {
        Ok(cli) => finish_command(run(cli)),
        Err(error) => print_parse_error(error),
    }
}

fn run_main_command(argv: Vec<std::ffi::OsString>) -> i32 {
    match Cli::try_parse_from(argv) {
        Ok(cli) => finish_command(run(cli)),
        Err(error) => print_parse_error(error),
    }
}

fn finish_command(result: crate::Result<()>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("holos: {error}");
            1
        }
    }
}

fn print_parse_error(error: clap::Error) -> i32 {
    let code = error.exit_code();
    let _ = error.print();
    code
}

macro_rules! command_handler {
    ($handler:ident, $parser:ty, $run:path, $command:literal) => {
        fn $handler(argv: Vec<std::ffi::OsString>) -> i32 {
            parse_command::<$parser>(argv, $command, $run)
        }
    };
}

command_handler!(
    cohomology_command,
    CohomologyCli,
    run_cohomology,
    "cohomology"
);
command_handler!(kinetic_command, KineticCli, run_kinetic, "kinetic");
command_handler!(coverage_command, CoverageCli, run_coverage, "cover");
command_handler!(
    affine_coverage_command,
    AffineCoverageCli,
    run_affine_coverage,
    "cover-affine"
);
command_handler!(synthesis_command, SynthesisCli, run_synthesis, "synthesize");
command_handler!(
    kinetic_synthesis_command,
    KineticSynthesisCli,
    run_kinetic_synthesis,
    "synthesize-kinetic"
);
command_handler!(
    cohomology_intervention_command,
    CohomologyInterventionCli,
    run_cohomology_intervention,
    "intervene-cohomology"
);
command_handler!(link_plan_command, LinkPlanCli, run_link_plan, "plan-links");
command_handler!(
    merge_interfaces_command,
    MergeInterfacesCli,
    run_merge_interfaces,
    "merge-interfaces"
);
command_handler!(interface_command, InterfaceCli, run_interface, "interface");
command_handler!(index_command, IndexCli, run_index, "index");
command_handler!(prove_command, ProveCli, run_prove, "prove");
command_handler!(
    verify_program_command,
    VerifyProgramCli,
    run_verify_program,
    "verify-program"
);
command_handler!(
    verify_program_trace_command,
    VerifyProgramTraceCli,
    run_verify_program_trace,
    "verify-program-trace"
);
command_handler!(intervene_command, InterveneCli, run_intervene, "intervene");
command_handler!(
    verify_intervention_command,
    VerifyInterventionCli,
    run_verify_intervention,
    "verify-intervention"
);
command_handler!(
    verify_atlas_command,
    VerifyAtlasCli,
    run_verify_atlas,
    "verify-atlas"
);
command_handler!(
    verify_trajectory_command,
    VerifyTrajectoryCli,
    run_verify_trajectory,
    "verify-trajectory"
);
command_handler!(
    verify_collapse_command,
    VerifyCli,
    run_verify,
    "verify-collapse"
);

const SUBCOMMANDS: &[(&str, CommandHandler)] = &[
    ("cohomology", cohomology_command),
    ("kinetic", kinetic_command),
    ("cover", coverage_command),
    ("cover-affine", affine_coverage_command),
    ("synthesize", synthesis_command),
    ("synthesize-kinetic", kinetic_synthesis_command),
    ("intervene-cohomology", cohomology_intervention_command),
    ("plan-links", link_plan_command),
    ("merge-interfaces", merge_interfaces_command),
    ("interface", interface_command),
    ("index", index_command),
    ("prove", prove_command),
    ("verify-program", verify_program_command),
    ("verify-program-trace", verify_program_trace_command),
    ("intervene", intervene_command),
    ("verify-intervention", verify_intervention_command),
    ("verify-atlas", verify_atlas_command),
    ("verify-trajectory", verify_trajectory_command),
    ("verify-collapse", verify_collapse_command),
];
