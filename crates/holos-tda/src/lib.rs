#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Vietoris-Rips persistent homology over a prime field (Z/2 by default)
//! with an implicit, ripser-style persistent cohomology engine.
//!
//! Tie-breaking and output conventions match ripser exactly. See README.md.

pub(crate) mod adjacency;
/// Certified local models of H1 persistence under changing inputs.
pub mod atlas;
/// Portable proof-carrying persistence atlases.
pub mod atlas_wire;
#[cfg(test)]
mod bit_keys;
pub(crate) mod budget;
/// Solver-independent algebraic reduction certificates.
pub mod certificate;
/// Stable H1 classes and cocycle validation.
pub mod classes;
/// The `holos` CLI as a library function (shared with the Python bindings).
pub mod cli;
/// Canonical cohomology and exact relations in any bounded dimension.
pub mod cohomology;
/// Weighted fixed-scale interventions across declared graph scenarios.
pub mod cohomology_intervention;
pub mod collapse;
pub(crate) mod combinadic;
/// Exact class-space correspondences across persistence updates.
pub mod correspondence;
/// Relative topological coverage criteria for fenced planar sensor networks.
pub mod coverage;
/// Exact component frontiers for failure-tolerant coverage synthesis.
pub mod coverage_frontier;
/// Exact planar geometry bindings for finite coverage specifications.
pub mod coverage_geometry;
/// Failure-tolerant finite and kinetic relative coverage specifications.
pub mod coverage_synthesis;
/// Distance-matrix construction and storage.
pub mod distances;
/// Durable content-addressed execution for relative interfaces.
pub mod distributed;
/// Proof-carrying persistence for explicit scalar filtered complexes.
pub mod explicit_certificate;
/// Vertex-biconnected factorization of sparse flag filtrations.
pub mod factorization;
pub(crate) mod field;
/// Explicit filtered simplicial complexes and filtration grades.
pub mod filtration;
/// Dimension-generic algebraic certificates for separator indexes.
pub mod graded_certificate;
/// Versioned exact persistence over checked separator interfaces.
pub mod index;
/// Cold snapshots and warm deltas for persistence indexes.
pub mod index_proof;
/// Stateful exact index streams with proof output.
pub mod index_stream;
/// Certified finite H1 lifetime interventions.
pub mod intervention;
/// File formats and diagram output.
pub mod io;
/// Exact events for affine edge-weight trajectories.
pub mod kinetic;
/// Self-contained certificates for exact kinetic cohomology zigzags.
pub mod kinetic_zigzag_artifact;
mod monotone_proof;
mod monotone_search;
/// Independent brute-force reference implementation used by the test gates.
pub mod oracle;
pub(crate) mod parallel;
/// Compositional, change-sensitive persistence programs.
pub mod program;
/// Portable independently checked persistence-program update traces.
pub mod program_trace;
/// Portable proof-carrying compositional persistence programs.
pub mod program_wire;
/// Unified proof DAGs for checked persistence trajectories.
pub mod proof;
pub(crate) mod reduce;
/// Exact filtered chain cores relative to separator subcomplexes.
pub mod relative_interface;
pub(crate) mod simplex;
pub(crate) mod solver;
/// Proof-carrying synthesis for finite temporal topology specifications.
pub mod synthesis;
/// Portable checked trajectories across atlas regions.
pub mod trajectory;
mod union_find;
/// Exact interval decomposition of finite zigzag modules.
pub mod zigzag;

use std::fmt;

pub use atlas::{
    AtlasEvaluation, AtlasUpdate, ClassSensitivity, CoordinateDerivative, EdgeKey,
    EndpointGradient, EvaluatedClassSpace, LineageId, PersistenceAtlas, PointAtlasUpdate,
    PointClassSensitivity, PointEndpointGradient, PointPersistenceAtlas, TopologyEvent,
    TopologyEventKind, UpdateMode,
};
pub use atlas_wire::{AtlasArtifact, AtlasArtifactError, AtlasArtifactRepair, AtlasDecodeLimits};
pub use certificate::{
    CertificateError, CertificateLimits, CertificateTerm, CertifiedReductionRegion,
    CertifiedRegionEvaluation, ChangeColumn, FiltrationSimplex, ReductionCertificate,
    ReductionGuard, ReductionGuardKind, ReductionRepair, ReductionRepairMode, ReductionRepairWork,
    RegionViolation, RegionViolationKind,
};
pub use classes::{
    BasisClassId, Cocycle, CocycleTerm, CriticalPair, CriticalSimplex, ExplainedDiagram,
    IntervalGroupId, PersistentClass, PersistentClassSpace, lift_h1_classes,
    rips_persistence_with_classes_sparse,
};
pub use cohomology::{
    CochainTerm, CohomologyClass, CohomologyClassId, CohomologyLimits, CohomologyMapColumn,
    CohomologyMapTerm, CohomologyRelation, CohomologyRelationTerm, CohomologyRelationVector,
    CohomologyRestriction, CohomologySpace, CohomologySpaceId, CohomologySubspace,
    CohomologySubspaceGenerator, CohomologySubspaceTerm, cohomology_relation,
    cohomology_restriction, cohomology_space,
};
pub use cohomology_intervention::{
    CohomologyInterventionArtifact, CohomologyInterventionCandidate, CohomologyInterventionLimits,
    CohomologyInterventionScenario, CohomologyInterventionStatus,
};
pub use correspondence::{
    ClassCorrespondence, CorrespondenceTerm, CorrespondenceVector, class_correspondences,
};
pub use coverage::{
    CoverageEvaluation, CoverageFence, CoverageLimits, CoverageTriangleTerm, PlanarCoverageModel,
    evaluate_planar_coverage,
};
pub use coverage_frontier::{
    CoverageComponentFrontier, CoverageComposition, CoverageCompositionStatus,
    CoverageFrontierEntry, compose_coverage_frontiers,
};
pub use coverage_geometry::{
    CoverageGeometry, CoverageGeometryLimits, GeometryBoundCoverageArtifact,
    GeometryBoundCoverageDecodeLimits, PlanarPoint,
};
pub use coverage_synthesis::{
    CoverageAction, CoverageComponent, CoverageCounterexample, CoveragePlanEvaluation,
    CoverageSource, CoverageSpecification, CoverageState, CoverageSynthesisArtifact,
    CoverageSynthesisLimits, CoverageSynthesisStatus, evaluate_coverage_plan,
};
pub use distances::{
    DistanceMatrix, PointCloudGraph, PointCloudParams, PointCloudStats, PointCloudStrategy,
    SparseDistanceMatrix,
};
pub use distributed::{
    ArtifactId, DistributedInterfaceCommit, DistributedInterfaceError,
    DistributedInterfaceManifest, DistributedInterfaceWork, DurableInterfaceStore,
};
pub use explicit_certificate::ExplicitReductionCertificate;
pub use filtration::{
    ComplexLimits, CoordinateProjection, FilteredSimplex, FilteredSimplicialComplex,
    FiltrationError, FiltrationGrade, FlagComplexParams, LinearFiltrationGrade, ProductGrade,
    ScalarGrade, ScalarProjection,
};
pub use graded_certificate::{
    GradedDimensionWork, GradedReductionCertificate, GradedReductionRepair,
    GradedReductionRepairWork,
};
pub use index::{
    DiagramDelta, IndexBranch, IndexDiff, IndexEdit, IndexEvent, IndexEventKind, IndexParams,
    IndexSummary, IndexTransition, IndexUpdateMode, IndexWork, InterfaceMode, InterfacePolicy,
    InterfaceSummary, PersistenceIndex, TopologyPatch,
};
pub use index_proof::{IndexDeltaProof, IndexProofError, IndexProofSummary, IndexSnapshotProof};
pub use index_stream::{IndexStream, IndexStreamProof, IndexStreamStep};
pub use intervention::{
    EdgeWeightEdit, H1Intervention, InterventionArtifact, InterventionBudget,
    InterventionDecodeLimits, InterventionError, InterventionStatus, VerifiedIntervention,
};
pub use kinetic::{
    KineticCohomologyEvent, KineticEdge, KineticEdgeKey, KineticEvent, KineticEventKind,
    KineticFiltration, KineticGraphState, KineticGraphStateKind, KineticLimits, KineticSchedule,
    KineticZigzag, KineticZigzagArrow, KineticZigzagNode, KineticZigzagNodeKind,
};
pub use kinetic_zigzag_artifact::{
    KineticZigzagArtifact, KineticZigzagArtifactLimits, KineticZigzagArtifactSummary,
    KineticZigzagIntervalClaim,
};
pub use program::{
    BasisTransport, ClassContinuation, ContinuationKind, CorrespondenceMode, PersistenceProgram,
    ProgramAtomInfo, ProgramBranch, ProgramCheckpoint, ProgramEvaluation, ProgramEvent,
    ProgramEventKind, ProgramSummary, ProgramUpdate, ProgramUpdateMode, ProgramWork,
};
pub use program_trace::{
    ProgramTraceArtifact, ProgramTraceDecodeLimits, ProgramTraceError, ProgramTraceStep,
    VerifiedProgramTrace, VerifiedProgramTraceStep,
};
pub use program_wire::{
    ProgramArtifact, ProgramArtifactError, ProgramAtomArtifact, ProgramDecodeLimits,
};
pub use proof::{ProofArtifact, ProofArtifactError, ProofArtifactSummary};
pub use relative_interface::{
    InterfaceCancellation, InterfaceCell, InterfaceChainTerm, RelativeInterfaceCertificate,
    RelativeInterfaceWork,
};
pub use synthesis::{
    SynthesisAction, SynthesisArtifact, SynthesisComponent, SynthesisCoordinate, SynthesisLimits,
    SynthesisSource, SynthesisState, SynthesisStatus, TopologicalSpecification,
};
pub use trajectory::{
    TrajectoryArtifact, TrajectoryDecodeLimits, TrajectoryError, TrajectoryStep,
    VerifiedTrajectory, VerifiedTrajectoryStep,
};
pub use zigzag::{
    ZigzagBarcode, ZigzagDirection, ZigzagInterval, ZigzagIntervalId, ZigzagLimits, ZigzagMap,
    ZigzagModule, ZigzagModuleId, ZigzagTerm,
};

/// Short git commit hash recorded at build time ("unknown" outside a repo).
pub const GIT_HASH: &str = env!("HOLOS_GIT_HASH");
/// Crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Cargo build profile recorded at build time.
pub const BUILD_PROFILE: &str = env!("HOLOS_BUILD_PROFILE");

/// One persistence interval. `death` is `f64::INFINITY` for essential classes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bar {
    /// Homology dimension.
    pub dim: usize,
    /// Filtration value at which the class appears.
    pub birth: f64,
    /// Filtration value at which the class dies.
    pub death: f64,
}

impl Bar {
    /// True when the class never dies.
    pub fn is_essential(&self) -> bool {
        self.death == f64::INFINITY
    }
}

/// A persistence diagram: the multiset of bars across dimensions.
#[derive(Debug, Clone, Default)]
pub struct Diagram {
    /// All bars, in canonical order after [`Diagram::canonicalize`].
    pub bars: Vec<Bar>,
}

impl Diagram {
    /// Bars of one homology dimension.
    pub fn in_dim(&self, dim: usize) -> impl Iterator<Item = &Bar> {
        self.bars.iter().filter(move |b| b.dim == dim)
    }

    /// Sort bars into the canonical output order: by dimension, then birth,
    /// then death. The order is deterministic across runs and point
    /// permutations.
    pub fn canonicalize(&mut self) {
        self.bars.sort_by(|a, b| {
            a.dim
                .cmp(&b.dim)
                .then(a.birth.total_cmp(&b.birth))
                .then(a.death.total_cmp(&b.death))
        });
    }
}

/// Parameters for [`rips_persistence`].
///
/// The engine is dimension-generic. The differential gates cover
/// `max_dim <= 2`. Stress tests extend through dimension 4.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RipsParams {
    /// Highest homology dimension to compute.
    pub max_dim: usize,
    /// Filtration threshold. `None` means the input's default: the
    /// enclosing radius for dense matrices, no threshold for sparse ones.
    pub threshold: Option<f64>,
    /// Coefficient field Z/p; must be a prime below 32768. Default 2.
    pub modulus: u32,
    /// Worker threads for the run. 0 and 1 (the default) both run the
    /// serial engine. Higher values reduce each dimension concurrently.
    /// With [`RipsParams::collapse_edges`] set and the ordered or rounds
    /// [`RipsParams::collapse_schedule`], the same budget also drives the
    /// collapse: one pool serves the whole pipeline. The diagram is
    /// identical at any thread count.
    pub threads: usize,
    /// Optimization toggle. The diagram is identical with any combination
    /// disabled. For differential testing only.
    pub use_emergent_pairs: bool,
    /// See [`RipsParams::use_emergent_pairs`].
    pub use_apparent_pairs: bool,
    /// See [`RipsParams::use_emergent_pairs`].
    pub use_clearing: bool,
    /// See [`RipsParams::use_emergent_pairs`]. With this set, and the graph
    /// dense enough for the rows to fit their memory budget, the dim-0
    /// apparent test runs on adjacency bitsets instead of the neighbor
    /// lists.
    pub use_adjacency_rows: bool,
    /// Collapse dominated edges before the engine runs. Off by default.
    /// The diagram is identical either way. See [`collapse`].
    pub collapse_edges: bool,
    /// The schedule the collapse uses with `collapse_edges` set. Default
    /// [`CollapseSchedule::Serial`]. The diagram is identical under every
    /// schedule.
    pub collapse_schedule: CollapseSchedule,
    /// Objective and deterministic work limit for
    /// [`CollapseSchedule::Adaptive`]. Other schedules ignore this field.
    pub adaptive_collapse: collapse::AdaptiveCollapseParams,
    /// Which engine reduces a dense input. Default [`Engine::Auto`]. The
    /// diagram is identical under every setting. See [`Engine`].
    pub engine: Engine,
    /// Which storage form the dense engine reduces from. Default
    /// [`DenseStorage::Auto`]. The diagram is identical under every
    /// setting. See [`DenseStorage`].
    pub dense_storage: DenseStorage,
    /// Structural decomposition of a sparse terminal graph. Default
    /// [`GraphFactorization::Off`]. Dense runs use it only after routing to
    /// the sparse engine. See [`GraphFactorization`].
    pub factorization: GraphFactorization,
}

/// Which engine reduces a dense input.
///
/// A dense matrix can be reduced as it stands, or converted to the graph
/// of its edges at the threshold and reduced by the sparse engine. The
/// second is faster when few pairs are edges, because the sparse
/// enumerator walks a neighbor list where the dense one scans every
/// vertex. `Auto` picks between them from the edge density at the resolved
/// threshold and from the memory the conversion would take; `Dense` and
/// `Sparse` force one.
///
/// The diagram is identical under all three, bit for bit. Every simplex of
/// the complex has diameter at most the threshold, and a diameter is the
/// largest of the edge lengths, so every edge of every simplex survives
/// the conversion; conversely the conversion keeps only edges at or below
/// the threshold. The two complexes are therefore equal simplex for
/// simplex with equal diameters, and the vertex set carries over because
/// the conversion passes the point count explicitly.
///
/// An infinite threshold reads as `f64::MAX` in both engines. An absent
/// pair has distance `+inf`, so it enters neither complex, and the
/// conversion keeps exactly the pairs the dense engine admits.
///
/// The rule and its constants were frozen on 2026-08-18 from disclosed
/// engineering data. Performance assessment is WIP.
///
/// A sparse input is never routed, so this setting does not reach
/// [`rips_persistence_sparse`]. It also does not reach the collapse
/// pipeline: with [`RipsParams::collapse_edges`] set, the collapse already
/// produces a graph and the sparse engine already reduces it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Engine {
    /// Reduce a low-density dense input with the sparse engine, and every
    /// other dense input with the dense engine. The rule is frozen and
    /// takes no argument. It also holds the conversion to a memory budget:
    /// 32 MiB, or the bytes of the compact matrix, whichever is larger. A
    /// matrix of
    /// mostly absent pairs is low-density at any threshold, including an
    /// infinite one, so it routes too.
    #[default]
    Auto,
    /// Always reduce the distance matrix as it stands.
    Dense,
    /// Always convert to the thresholded graph and reduce that. The
    /// conversion costs one pass over the matrix and one triplet buffer.
    /// This is an explicit request, so the `Auto` memory budget does not
    /// apply.
    Sparse,
}

/// Which storage form the dense engine reduces from.
///
/// A [`DistanceMatrix`] is built compact: the condensed lower triangle,
/// `n(n-1)/2` entries. The full form holds both triangles row-major,
/// `n * n` entries, so that the cofacet diameter fold reads a contiguous
/// row per simplex vertex where the compact form reads a strided column.
/// The full form costs `n(n+1)/2` entries more.
///
/// The choice is per run and comes after the routing decision, so a run
/// the router sends to the sparse engine never builds the full form.
/// `Auto` selects it from the compact matrix size, a frozen budget on the
/// added bytes, the edge count at the resolved threshold, and how many
/// distances the fold reads. `Compact` forbids the conversion, which
/// bounds what a run spends on the matrix. `Square` forces it and skips
/// the budget.
///
/// The conversion runs once and the full form lives only as long as the
/// run. The caller keeps its compact matrix, so a run in the full form
/// holds `n * n + n(n-1)/2` entries: one and a half times the full form,
/// three times the compact one.
///
/// The diagram is identical under all three, bit for bit: the two forms
/// hold the same distances and answer every query with the same bits.
/// Performance and peak-memory assessment are WIP.
///
/// A sparse input holds no distance matrix, so this setting does not reach
/// [`rips_persistence_sparse`]. It also does not reach the collapse
/// pipeline: with [`RipsParams::collapse_edges`] set, the collapse
/// produces a graph and the sparse engine reduces it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum DenseStorage {
    /// Convert to the full form when the frozen rule selects it, and keep
    /// the compact form otherwise.
    #[default]
    Auto,
    /// Always reduce from the compact form. No run adds the second
    /// triangle.
    Compact,
    /// Always reduce from the full form. This is an explicit request, so
    /// the `Auto` budget on the added bytes does not apply.
    Square,
}

/// Structural routing for positive-dimensional sparse persistence.
///
/// Every terminal edge belongs to one vertex-biconnected block. Every
/// terminal clique with at least two vertices lies in one such block, and
/// every positive-dimensional cycle splits over the blocks. The engine can
/// therefore compute H0 once on the whole graph and compute H1 and above on
/// the cyclic blocks independently.
///
/// The diagram is identical under every setting. The automatic rule uses
/// factorization only when there are at least two cyclic blocks and the
/// largest holds at most nine tenths of their edges. A graph with one
/// dominant block stays on the existing reducer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum GraphFactorization {
    /// Use the frozen structural rule.
    Auto,
    /// Reduce the whole terminal graph.
    #[default]
    Off,
    /// Split every terminal graph. This is useful for exactness tests and
    /// controlled measurements.
    Force,
}

/// The collapse the pipeline runs with [`RipsParams::collapse_edges`] set.
///
/// Every schedule gives the same diagram. `Serial` is the default and, in
/// the registered studies, the fastest end to end on most inputs.
/// `Ordered` gives the serial result, bit for bit, from a parallel run.
/// `Rounds` gives a result that does not depend on the worker count and,
/// on some inputs, a smaller reduced graph; its cost grows faster with the
/// edge count than the serial cost. See [`collapse`] for the schedules and
/// their certificates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum CollapseSchedule {
    /// The serial schedule on one worker. It writes an algorithm
    /// version 1 certificate. The reduction still uses the whole thread
    /// budget.
    #[default]
    Serial,
    /// The ordered schedule on the run's worker budget. It gives the same
    /// reduced graph and the same certificate as `Serial`.
    Ordered,
    /// The rounds schedule on the run's worker budget. It writes an
    /// algorithm version 2 certificate and gives the same result at every
    /// worker count, but not the serial result.
    Rounds,
    /// The adaptive version 3 schedule. It ranks currently valid removals
    /// by estimated downstream H1 or H2 work and can stop at a declared
    /// work limit. It runs serially; the reduction still uses the whole
    /// thread budget.
    Adaptive,
}

impl Default for RipsParams {
    fn default() -> Self {
        Self {
            max_dim: 1,
            threshold: None,
            modulus: 2,
            threads: 1,
            use_emergent_pairs: true,
            use_apparent_pairs: true,
            use_clearing: true,
            use_adjacency_rows: true,
            collapse_edges: false,
            collapse_schedule: CollapseSchedule::Serial,
            adaptive_collapse: collapse::AdaptiveCollapseParams::default(),
            engine: Engine::Auto,
            dense_storage: DenseStorage::Auto,
            factorization: GraphFactorization::Off,
        }
    }
}

impl RipsParams {
    /// Defaults with the given `max_dim`.
    ///
    /// The threshold is the enclosing radius. Reduction shortcuts are on.
    /// Edge collapse and structural factorization are off.
    pub fn new(max_dim: usize) -> Self {
        Self {
            max_dim,
            ..Self::default()
        }
    }

    /// Truncate the filtration at `threshold`.
    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.threshold = Some(threshold);
        self
    }

    /// Compute over Z/p instead of Z/2. `modulus` must be a prime below
    /// 32768.
    pub fn with_modulus(mut self, modulus: u32) -> Self {
        self.modulus = modulus;
        self
    }

    /// Reduce with `threads` workers. 1 keeps the serial engine. The diagram
    /// is identical at any thread count.
    pub fn with_threads(mut self, threads: usize) -> Self {
        self.threads = threads.max(1);
        self
    }

    /// Collapse dominated edges before the engine runs. The diagram is
    /// identical either way. See [`collapse`].
    pub fn with_edge_collapse(mut self) -> Self {
        self.collapse_edges = true;
        self
    }

    /// Collapse dominated edges with the given schedule before the engine
    /// runs. Also sets [`RipsParams::collapse_edges`]. The diagram is
    /// identical under every schedule.
    pub fn with_collapse_schedule(mut self, schedule: CollapseSchedule) -> Self {
        self.collapse_edges = true;
        self.collapse_schedule = schedule;
        self
    }

    /// Collapse with the adaptive version 3 schedule and the given
    /// objective and work limit.
    pub fn with_adaptive_collapse(mut self, params: collapse::AdaptiveCollapseParams) -> Self {
        self.collapse_edges = true;
        self.collapse_schedule = CollapseSchedule::Adaptive;
        self.adaptive_collapse = params;
        self
    }

    /// Choose the engine for a dense input. The diagram is identical under
    /// every setting. See [`Engine`].
    pub fn with_engine(mut self, engine: Engine) -> Self {
        self.engine = engine;
        self
    }

    /// Choose the storage form the dense engine reduces from. The diagram
    /// is identical under every setting. See [`DenseStorage`].
    pub fn with_dense_storage(mut self, storage: DenseStorage) -> Self {
        self.dense_storage = storage;
        self
    }

    /// Choose structural factorization for sparse reduction. The diagram is
    /// identical under every setting.
    pub fn with_factorization(mut self, factorization: GraphFactorization) -> Self {
        self.factorization = factorization;
        self
    }
}

/// Errors surfaced by construction, validation, and IO.
#[derive(Debug, Clone, PartialEq)]
#[allow(missing_docs)]
pub enum Error {
    InvalidDistance(String),
    InvalidInput(String),
    IndexOverflow { n: usize, dim: usize },
    Io(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidDistance(msg) => write!(f, "invalid distance: {msg}"),
            Error::InvalidInput(msg) => write!(f, "invalid input: {msg}"),
            Error::IndexOverflow { n, dim } => write!(
                f,
                "simplex index space overflows u64 for {n} points in dimension {dim}"
            ),
            Error::Io(msg) => write!(f, "io error: {msg}"),
        }
    }
}

impl std::error::Error for Error {}

/// Crate-wide result alias.
pub type Result<T> = std::result::Result<T, Error>;

/// Smallest point count [`Engine::Auto`] routes. The conversion costs one
/// pass over the matrix and one triplet buffer, and a short reduction
/// cannot earn that back.
///
/// Frozen on 2026-08-18 from disclosed engineering data. Performance
/// assessment is WIP.
const N_MIN: usize = 32;

/// Numerator of the highest edge density [`Engine::Auto`] routes:
/// `m / C(n, 2)` at the resolved threshold, with `m` the finite pairs at or
/// below it. The cutoff is four fifths, and [`density_routes`] applies it
/// as `5 * m <= 4 * C(n, 2)` so that no rounding of `0.8` and no rounding
/// of a large count enters the decision.
///
/// Frozen with [`N_MIN`] on 2026-08-18 from the same disclosed engineering
/// data. Above the cutoff the graph would only spend memory on edges the
/// matrix already holds. Performance assessment is WIP.
const RHO_MAX_NUM: u128 = 4;

/// Denominator of [`RHO_MAX_NUM`].
const RHO_MAX_DEN: u128 = 5;

/// The threshold a dense run applies: the caller's, or the enclosing
/// radius, which is what the engine resolves for itself.
fn resolved_threshold(dist: &DistanceMatrix, params: &RipsParams) -> f64 {
    params.threshold.unwrap_or_else(|| dist.enclosing_radius())
}

/// True when a dense input can route at all. Below [`N_MIN`] points the
/// answer is dense whatever the matrix holds, so the counting pass never
/// runs. A negative threshold is an error the engine reports, and a NaN
/// threshold fails the comparison, so neither routes.
///
/// An infinite threshold does route. It admits every finite pair and no
/// absent one, and a matrix of mostly absent pairs has a sparse graph at
/// that threshold, so the density cutoff is what decides. A matrix with no
/// absent pair has every pair as an edge and fails the cutoff.
fn may_route(n: usize, threshold: f64) -> bool {
    n >= N_MIN && threshold >= 0.0
}

/// True when the edge density at the threshold is at or below the frozen
/// cutoff. `edges` is the count at the same threshold.
///
/// A dense matrix stores every pair, so `C(n, 2)` is bounded by the address
/// space and both products fit u128 with room to spare.
fn density_routes(n: usize, edges: usize) -> bool {
    RHO_MAX_DEN * edges as u128 <= RHO_MAX_NUM * pair_count(n)
}

/// Bytes the conversion holds per retained edge at its peak.
///
/// `DistanceMatrix::to_sparse_at` writes both directed entries of an edge
/// into one index array (`u32`, 4 bytes each) and one value array (`f64`,
/// 8 bytes each), so an edge costs 24 bytes. No triplet buffer exists.
const CONVERSION_BYTES_PER_EDGE: u128 = 24;

/// Bytes the conversion holds per point at its peak: the degree count, the
/// row offset, and the fill cursor, 8 bytes each. One more offset closes
/// the last row.
const CONVERSION_BYTES_PER_POINT: u128 = 24;

/// Smallest conversion budget [`Engine::Auto`] grants, in bytes. A small
/// matrix is a few kilobytes, too little for any useful graph, so the
/// budget never falls below this.
const MIN_CONVERSION_BYTES: u128 = 32 * 1024 * 1024;

/// The pairs of `n` points, `C(n, 2)`, wide enough not to overflow.
fn pair_count(n: usize) -> u128 {
    let n = n as u128;
    n * n.saturating_sub(1) / 2
}

/// True when the conversion peak fits the budget: [`MIN_CONVERSION_BYTES`],
/// or the bytes of the compact matrix (8 per pair), whichever is larger.
/// `edges` is the count at the resolved threshold.
///
/// The density cutoff bounds the graph in proportion to the matrix, and
/// this bounds it in bytes: a routed run then peaks at about twice the
/// compact matrix, below the dense engine's peak in the square form.
/// [`Engine::Sparse`] is an explicit request and ignores the budget.
fn memory_routes(n: usize, edges: usize) -> bool {
    let extra =
        CONVERSION_BYTES_PER_EDGE * edges as u128 + CONVERSION_BYTES_PER_POINT * n as u128 + 8;
    let budget = MIN_CONVERSION_BYTES.max(pair_count(n) * 8);
    extra <= budget
}

/// True when the counted graph is worth building: its density is at or
/// below the cutoff and its conversion fits the budget. This is the whole
/// decision [`Engine::Auto`] makes after the counting pass.
fn graph_routes(n: usize, edges: usize) -> bool {
    density_routes(n, edges) && memory_routes(n, edges)
}

/// Smallest compact matrix [`DenseStorage::Auto`] converts, in bytes. The
/// bound is 1025 points.
///
/// The full form doubles the matrix. Below this size both the matrix and
/// the reduction are small in absolute terms, so `Auto` keeps the compact
/// form and leaves the second triangle to a caller who asks for it.
///
/// Frozen on 2026-08-18 from disclosed engineering data. Performance
/// assessment is WIP.
const SQUARE_MIN_BYTES: u128 = 4 << 20;

/// Most bytes [`DenseStorage::Auto`] adds for the full form. The full form
/// adds `n(n+1)/2` entries, the compact matrix again plus its diagonal, so
/// this bounds the point count as well: 8191 points.
///
/// A policy bound, not a crossover. It caps what a run spends on a storage
/// form nobody asked for. [`DenseStorage::Square`] is an explicit request
/// and ignores it.
///
/// Frozen on 2026-08-18. Performance assessment is WIP.
const SQUARE_EXTRA_MAX_BYTES: u128 = 256 << 20;

/// Distance reads per matrix cell the fold must make before
/// [`DenseStorage::Auto`] converts. The conversion writes `n * n` cells,
/// and those reads are what the full form makes contiguous, so the ratio
/// is what the conversion has to earn back. The dim-0 columns supply one
/// read per cell on their own, so the test asks the columns above them for
/// three more.
///
/// Frozen on 2026-08-18 from disclosed engineering data. Performance
/// assessment is WIP.
const SQUARE_READS_PER_CELL: u128 = 4;

/// Bytes the full form adds over the compact one: the entries above the
/// diagonal and the diagonal itself.
fn square_extra_bytes(n: usize) -> u128 {
    let n = n as u128;
    n * (n + 1) / 2 * 8
}

/// Distances the cofacet diameter fold reads, as the rule estimates them.
///
/// Two terms. The dim-0 columns classify the cofacets of every vertex, so
/// they read the whole matrix once: `n * n`. Each dim-1 column then walks
/// the candidate range and reads one distance per candidate, so the dim-1
/// columns read `edges * n` between them, and from `max_dim` 2 up the
/// assembly enumerates the same cofacets once more per further dimension.
fn fold_reads(n: usize, edges: usize, max_dim: usize) -> u128 {
    let n = n as u128;
    n * n + edges as u128 * n * max_dim.max(1) as u128
}

/// True when the matrix is large enough for the full form to pay and its
/// added bytes fit the budget.
fn square_size_fits(n: usize) -> bool {
    pair_count(n) * 8 >= SQUARE_MIN_BYTES && square_extra_bytes(n) <= SQUARE_EXTRA_MAX_BYTES
}

/// True when the fold reads enough distances to earn the conversion.
fn square_work_pays(n: usize, edges: usize, max_dim: usize) -> bool {
    let cells = n as u128 * n as u128;
    fold_reads(n, edges, max_dim) >= SQUARE_READS_PER_CELL * cells
}

/// True when the dense run reduces from the full form. `edges` is the
/// count at `threshold` when the caller has already made it; the rule
/// counts for itself only when the size test has already passed, so a run
/// that cannot convert never pays for a pass.
fn square_selected(
    dist: &DistanceMatrix,
    params: &RipsParams,
    threshold: f64,
    edges: Option<usize>,
) -> bool {
    match params.dense_storage {
        DenseStorage::Compact => false,
        DenseStorage::Square => true,
        DenseStorage::Auto => {
            let n = dist.len();
            if !square_size_fits(n) {
                return false;
            }
            let edges = edges.unwrap_or_else(|| dist.count_edges_at(threshold));
            square_work_pays(n, edges, params.max_dim)
        }
    }
}

/// Reduce a dense input with the dense engine, from the storage form the
/// rule selects. The conversion runs once, and the full form lives no
/// longer than the run.
fn solve_dense(
    dist: &DistanceMatrix,
    params: &RipsParams,
    threshold: f64,
    edges: Option<usize>,
) -> Result<Diagram> {
    if square_selected(dist, params, threshold, edges) {
        return solver::compute(&dist.to_square(), params);
    }
    solver::compute(dist, params)
}

/// Reduce the thresholded graph of a dense input with the sparse engine.
/// The threshold passes explicitly: a sparse input keeps every listed edge
/// by default, while a dense one stops at the enclosing radius, so an
/// unset threshold here would change the filtration.
fn solve_thresholded(
    dist: &DistanceMatrix,
    params: &RipsParams,
    threshold: f64,
) -> Result<Diagram> {
    let sparse = dist.to_sparse_at(threshold)?;
    let mut inner = params.clone();
    inner.threshold = Some(threshold);
    factorization::compute_sparse(&sparse, &inner)
}

/// Compute the Rips persistence diagram of a distance matrix.
///
/// [`RipsParams::engine`] selects the engine. Under [`Engine::Auto`] an
/// input above a frozen point count, whose edge density at the resolved
/// threshold is at or below a frozen cutoff, and whose graph fits the
/// conversion memory budget, is converted to its thresholded graph and
/// reduced by the sparse engine. The diagram is the same either way, bit
/// for bit: see [`Engine`].
///
/// A run that stays dense then selects its storage form, which
/// [`RipsParams::dense_storage`] governs. A routed run never converts the
/// matrix, so the two decisions come in that order: see [`DenseStorage`].
pub fn rips_persistence(dist: &DistanceMatrix, params: &RipsParams) -> Result<Diagram> {
    if params.collapse_edges {
        return collapse_and_solve(dist, params, |_| Ok(()));
    }
    // The engine resolves the same threshold, so resolving it here and
    // handing it back adds no pass over the matrix.
    let threshold = resolved_threshold(dist, params);
    let mut resolved = params.clone();
    resolved.threshold = Some(threshold);
    match params.engine {
        Engine::Dense => solve_dense(dist, &resolved, threshold, None),
        Engine::Sparse => solve_thresholded(dist, params, threshold),
        Engine::Auto => {
            // The counting pass is the whole cost of a refused route, and
            // the storage rule reads the same count.
            let mut counted = None;
            if may_route(dist.len(), threshold) {
                let edges = dist.count_edges_at(threshold);
                if graph_routes(dist.len(), edges) {
                    return solve_thresholded(dist, &resolved, threshold);
                }
                counted = Some(edges);
            }
            solve_dense(dist, &resolved, threshold, counted)
        }
    }
}

/// Compute a diagram and stable H1 classes from a dense distance matrix.
///
/// The explain path constructs the exact terminal graph, then uses the fixed
/// representative profile described by
/// [`rips_persistence_with_classes_sparse`]. The ordinary compute path keeps
/// its dense and sparse routing choices.
pub fn rips_persistence_with_classes(
    dist: &DistanceMatrix,
    params: &RipsParams,
) -> Result<ExplainedDiagram> {
    let threshold = resolved_threshold(dist, params);
    if params.collapse_edges {
        return dense_collapsed_classes(dist, params);
    }
    let sparse = dist.to_sparse_at(threshold)?;
    let mut fixed = params.clone();
    fixed.threshold = Some(threshold);
    classes::rips_persistence_with_classes_sparse(&sparse, &fixed)
}

fn dense_collapsed_classes(dist: &DistanceMatrix, params: &RipsParams) -> Result<ExplainedDiagram> {
    let collapsed = match params.collapse_schedule {
        CollapseSchedule::Serial => collapse::collapse_dense(dist, params.threshold)?,
        CollapseSchedule::Ordered => {
            collapse::collapse_dense_ordered_parallel(dist, params.threshold, params.threads)?
        }
        CollapseSchedule::Rounds => {
            collapse::collapse_dense_rounds_parallel(dist, params.threshold, params.threads)?
        }
        CollapseSchedule::Adaptive => {
            collapse::collapse_dense_adaptive(dist, params.threshold, params.adaptive_collapse)?
        }
    };
    let mut inner = params.clone();
    inner.collapse_edges = false;
    inner.threshold = Some(collapsed.certificate.terminal_level());
    let explained = classes::rips_persistence_with_classes_sparse(&collapsed.matrix, &inner)?;
    classes::lift_h1_classes(&collapsed, explained)
}

/// Compute the Rips persistence diagram of a sparse distance matrix.
///
/// Pairs not listed in the input are absent at every scale. With no
/// threshold set, all listed edges enter the filtration.
pub fn rips_persistence_sparse(
    dist: &SparseDistanceMatrix,
    params: &RipsParams,
) -> Result<Diagram> {
    if params.collapse_edges {
        return collapse_and_solve(dist, params, |_| Ok(()));
    }
    factorization::compute_sparse(dist, params)
}

/// The collapse pipeline behind [`rips_persistence`]. One run-wide pool
/// serves the selected collapse and then the reduction. The serial
/// schedule collapses before the pool exists, so the pool goes to the
/// reduction alone. Every surviving edge lies at or below the terminal
/// level, so the terminal level is the exact threshold for the reduced
/// complex. `report` sees the collapse result before the reduction
/// starts, which is how the CLI prints its statistics without building a
/// second pool.
pub(crate) fn collapse_and_solve<D: distances::Distances + Sync>(
    dist: &D,
    params: &RipsParams,
    report: impl FnOnce(&collapse::CollapsedRips) -> Result<()>,
) -> Result<Diagram> {
    let (collapsed, pool) = execute_collapse(dist, params)?;
    report(&collapsed)?;
    solve_collapsed(collapsed, pool, params)
}

fn collapse_pool(threads: usize) -> Result<Option<rayon::ThreadPool>> {
    if threads <= 1 {
        return Ok(None);
    }
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .map(Some)
        .map_err(|error| Error::Io(format!("thread pool: {error}")))
}

fn execute_collapse<D: distances::Distances + Sync>(
    dist: &D,
    params: &RipsParams,
) -> Result<(collapse::CollapsedRips, Option<rayon::ThreadPool>)> {
    match params.collapse_schedule {
        CollapseSchedule::Serial => collapse_serial_run(dist, params),
        CollapseSchedule::Ordered => collapse_ordered_run(dist, params),
        CollapseSchedule::Rounds => collapse_rounds_run(dist, params),
        CollapseSchedule::Adaptive => collapse_adaptive_run(dist, params),
    }
}

fn collapse_serial_run<D: distances::Distances>(
    dist: &D,
    params: &RipsParams,
) -> Result<(collapse::CollapsedRips, Option<rayon::ThreadPool>)> {
    Ok((
        collapse::collapse_serial_in(dist, params.threshold)?,
        collapse_pool(params.threads)?,
    ))
}

fn collapse_ordered_run<D: distances::Distances + Sync>(
    dist: &D,
    params: &RipsParams,
) -> Result<(collapse::CollapsedRips, Option<rayon::ThreadPool>)> {
    let pool = collapse_pool(params.threads)?;
    let collapsed = collapse::collapse_ordered_in(dist, params.threshold, pool.as_ref())?;
    Ok((collapsed, pool))
}

fn collapse_rounds_run<D: distances::Distances + Sync>(
    dist: &D,
    params: &RipsParams,
) -> Result<(collapse::CollapsedRips, Option<rayon::ThreadPool>)> {
    let pool = collapse_pool(params.threads)?;
    let collapsed = collapse::collapse_rounds_in(dist, params.threshold, pool.as_ref())?;
    Ok((collapsed, pool))
}

fn collapse_adaptive_run<D: distances::Distances>(
    dist: &D,
    params: &RipsParams,
) -> Result<(collapse::CollapsedRips, Option<rayon::ThreadPool>)> {
    Ok((
        collapse::collapse_adaptive_in(dist, params.threshold, params.adaptive_collapse)?,
        collapse_pool(params.threads)?,
    ))
}

fn solve_collapsed(
    collapsed: collapse::CollapsedRips,
    pool: Option<rayon::ThreadPool>,
    params: &RipsParams,
) -> Result<Diagram> {
    let mut inner = params.clone();
    inner.collapse_edges = false;
    inner.threshold = Some(collapsed.certificate.terminal_level());
    if inner.factorization == GraphFactorization::Off {
        solver::compute_in(&collapsed.matrix, &inner, pool)
    } else {
        drop(pool);
        factorization::compute_sparse(&collapsed.matrix, &inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collapse::verify::verify_dense;

    fn grid(side: usize) -> DistanceMatrix {
        let mut points = Vec::new();
        for i in 0..side {
            for j in 0..side {
                points.push(vec![i as f64, j as f64]);
            }
        }
        DistanceMatrix::from_points(&points).unwrap()
    }

    fn bits(d: &Diagram) -> Vec<(usize, u64, u64)> {
        d.bars
            .iter()
            .map(|b| (b.dim, b.birth.to_bits(), b.death.to_bits()))
            .collect()
    }

    /// Run `f` and report how many conversions to the full storage form it
    /// made. The counter is thread-local and each test owns its thread.
    fn counting<T>(f: impl FnOnce() -> T) -> (usize, T) {
        distances::SQUARE_BUILDS.with(|c| c.set(0));
        let value = f();
        (distances::SQUARE_BUILDS.with(|c| c.get()), value)
    }

    // The frozen rule, pinned against its constants. A change to either
    // constant fails here first, before it reaches a measurement.
    #[test]
    fn the_routing_rule_holds_its_constants() {
        assert!(!may_route(N_MIN - 1, 1.0), "under N_MIN nothing routes");
        assert!(may_route(N_MIN, 1.0));
        assert!(
            may_route(N_MIN, f64::INFINITY),
            "an infinite threshold routes"
        );
        for bad in [f64::NAN, -1.0] {
            assert!(!may_route(N_MIN, bad), "threshold {bad} must not route");
        }
        // The last accepted and the first rejected edge count, to the
        // edge. Four fifths of C(n, 2) is not always an integer, so the
        // cutoff can fall between two counts, and each case names the two
        // counts it falls between.
        for (n, last_routed) in [
            (N_MIN, 396usize),
            (33, 422),
            (100, 3960),
            (1001, 400_400),
            (4000, 6_398_400),
        ] {
            let pairs = n * (n - 1) / 2;
            assert_eq!(last_routed, 4 * pairs / 5, "{n}: the cutoff moved");
            assert!(density_routes(n, 0), "{n}: an empty graph routes");
            assert!(density_routes(n, last_routed), "{n}: the cutoff routes");
            assert!(!density_routes(n, last_routed + 1), "{n}: one edge over");
            assert!(!density_routes(n, pairs), "{n}: a complete graph");
        }
    }

    // The budget, at the byte, in both regimes. Below 2897 points the
    // matrix is under 32 MiB and the floor decides; above it the matrix
    // does. The last edge that fits is `(budget - 24 n - 8) / 24`.
    #[test]
    fn the_conversion_budget_gates_at_the_byte() {
        let small = 1000;
        assert!(pair_count(small) * 8 < MIN_CONVERSION_BYTES);
        let last_fit = ((MIN_CONVERSION_BYTES - 24 * small as u128 - 8) / 24) as usize;
        assert_eq!(last_fit, 1_397_101);
        assert!(memory_routes(small, last_fit));
        assert!(!memory_routes(small, last_fit + 1));

        let large = 20_000;
        let budget = pair_count(large) * 8;
        assert!(budget > MIN_CONVERSION_BYTES);
        let last_fit = ((budget - 24 * large as u128 - 8) / 24) as usize;
        assert_eq!(last_fit, 66_643_333);
        assert!(memory_routes(large, last_fit));
        assert!(!memory_routes(large, last_fit + 1));
    }

    // A near-clique the density cutoff alone would route: 2000 points with
    // three quarters of the pairs at or below the threshold. The graph
    // keeps over a million edges and about 36 MB against a 32 MiB budget,
    // so Auto refuses it. The test counts the edges of a real matrix and
    // stops at the decision, because the reduction itself is not cheap.
    #[test]
    fn a_large_near_clique_stays_dense() {
        let n = 2000;
        let data: Vec<f64> = (0..n * (n - 1) / 2)
            .map(|k| if k % 4 == 0 { 3.0 } else { 1.0 })
            .collect();
        let dist = DistanceMatrix::from_condensed(data).unwrap();
        let threshold = 1.0;
        let edges = dist.count_edges_at(threshold);
        assert!(edges > 1_000_000, "{edges} edges");
        assert!(may_route(n, threshold));
        assert!(density_routes(n, edges), "the density cutoff accepts it");
        assert!(!memory_routes(n, edges), "the budget must refuse it");
        assert!(!graph_routes(n, edges));
    }

    // The frozen storage rule, pinned against its constants, the same way
    // the routing rule is.
    #[test]
    fn the_storage_rule_holds_its_constants() {
        assert!(!square_size_fits(1024), "1024 points stay compact");
        assert!(
            square_size_fits(1025),
            "1025 points may take both triangles"
        );
        assert!(square_size_fits(8191), "8191 points fit the byte budget");
        assert!(!square_size_fits(8192), "8192 points exceed it");
        assert_eq!(square_extra_bytes(8191), 268_402_688);

        // The work test is `edges * max_dim >= (READS_PER_CELL - 1) * n`:
        // the dim-0 walk over the whole matrix contributes one read per
        // cell on its own, and the rest has to come from the columns above
        // it.
        for n in [1025usize, 2400, 8191] {
            let last_refused = (SQUARE_READS_PER_CELL as usize - 1) * n - 1;
            assert!(!square_work_pays(n, last_refused, 1), "{n}: one edge under");
            assert!(
                square_work_pays(n, last_refused + 1, 1),
                "{n}: at the cutoff"
            );
            // A second dimension doubles the reads, so half the edges do.
            assert!(
                square_work_pays(n, last_refused / 2 + 1, 2),
                "{n}: max_dim 2"
            );
        }
        assert!(!square_work_pays(2400, 863, 1), "a low threshold refuses");
        assert!(
            square_work_pays(2400, 14_273, 1),
            "a sparse block graph pays"
        );
    }

    // The storage form follows the routing decision. A routed run holds no
    // distance matrix at all, so it builds no full form, not even when the
    // caller forces one. The fixture asserts that the rule would have
    // selected the full form, so it cannot stop testing the interaction
    // when a constant moves.
    #[test]
    fn a_routed_run_never_builds_the_full_form() {
        let n = 1030;
        let dist = band(n, 4);
        let threshold = resolved_threshold(&dist, &RipsParams::new(1));
        let edges = dist.count_edges_at(threshold);
        assert!(
            may_route(n, threshold) && graph_routes(n, edges),
            "the fixture must route: {edges} edges"
        );
        assert!(
            square_size_fits(n) && square_work_pays(n, edges, 1),
            "the storage rule must want the full form here"
        );

        let params = RipsParams::new(1);
        let mut reference = None;
        for storage in [
            DenseStorage::Auto,
            DenseStorage::Compact,
            DenseStorage::Square,
        ] {
            let p = params.clone().with_dense_storage(storage);
            let (built, diagram) = counting(|| rips_persistence(&dist, &p).unwrap());
            assert_eq!(built, 0, "{storage:?}: a routed run stays compact");
            let bits = bits(&diagram);
            assert_eq!(*reference.get_or_insert(bits.clone()), bits, "{storage:?}");
        }

        // The same input on the dense engine, where the rule does decide.
        for (storage, want) in [
            (DenseStorage::Auto, 1),
            (DenseStorage::Compact, 0),
            (DenseStorage::Square, 1),
        ] {
            let p = params
                .clone()
                .with_engine(Engine::Dense)
                .with_dense_storage(storage);
            let (built, diagram) = counting(|| rips_persistence(&dist, &p).unwrap());
            assert_eq!(built, want, "{storage:?}: conversions");
            assert_eq!(bits(&diagram), *reference.as_ref().unwrap(), "{storage:?}");
        }
    }

    // Below the size bound `Auto` keeps the compact form, `Square` still
    // converts, and every form gives one diagram. The matrix is small, so
    // this covers the whole engine and storage product cheaply.
    #[test]
    fn every_storage_form_gives_one_diagram() {
        let dist = grid(6);
        assert!(
            !square_size_fits(dist.len()),
            "the fixture is under the bound"
        );
        let params = RipsParams::new(2);
        let mut reference = None;
        for engine in [Engine::Auto, Engine::Dense, Engine::Sparse] {
            for storage in [
                DenseStorage::Auto,
                DenseStorage::Compact,
                DenseStorage::Square,
            ] {
                let p = params
                    .clone()
                    .with_engine(engine)
                    .with_dense_storage(storage);
                let (built, diagram) = counting(|| rips_persistence(&dist, &p).unwrap());
                let dense_run = engine == Engine::Dense
                    || (engine == Engine::Auto
                        && !graph_routes(
                            dist.len(),
                            dist.count_edges_at(resolved_threshold(&dist, &p)),
                        ));
                let want = usize::from(dense_run && storage == DenseStorage::Square);
                assert_eq!(built, want, "{engine:?}, {storage:?}: conversions");
                let bits = bits(&diagram);
                assert_eq!(
                    *reference.get_or_insert(bits.clone()),
                    bits,
                    "{engine:?}, {storage:?}"
                );
            }
        }
    }

    /// A band matrix: `d(i, j)` is `|i - j|` within `width` and absent
    /// outside it. Every row holds an absent pair, so the enclosing radius
    /// is infinite and the graph stays sparse at any threshold.
    fn band(n: usize, width: usize) -> DistanceMatrix {
        let mut data = Vec::with_capacity(n * (n - 1) / 2);
        for i in 1..n {
            for j in 0..i {
                data.push(if i - j <= width {
                    (i - j) as f64
                } else {
                    f64::INFINITY
                });
            }
        }
        DistanceMatrix::from_condensed(data).unwrap()
    }

    // A dense matrix of mostly absent pairs has a sparse graph even with no
    // threshold, and its enclosing radius is infinite, so the default and
    // the explicit infinite threshold are the same run. A matrix with no
    // absent pair keeps every pair and stays dense.
    #[test]
    fn an_infinite_threshold_routes_on_density() {
        let n = 40;
        let dist = band(n, 3);
        assert_eq!(dist.enclosing_radius(), f64::INFINITY);
        let edges = dist.count_edges_at(f64::INFINITY);
        assert_eq!(edges, 3 * n - 6, "the band holds its own edges");
        assert!(may_route(n, f64::INFINITY) && graph_routes(n, edges));

        let complete = grid(7);
        let n = complete.len();
        let all = complete.count_edges_at(f64::INFINITY);
        assert_eq!(all, n * (n - 1) / 2, "every pair of a grid is finite");
        assert!(!graph_routes(n, all), "a complete matrix stays dense");

        // The routed diagram, against the dense one, on both spellings of
        // the infinite threshold.
        let params = RipsParams::new(2);
        let dense = rips_persistence(&dist, &params.clone().with_engine(Engine::Dense)).unwrap();
        for threshold in [None, Some(f64::INFINITY)] {
            let mut p = params.clone();
            p.threshold = threshold;
            for engine in [Engine::Auto, Engine::Sparse] {
                let got = rips_persistence(&dist, &p.clone().with_engine(engine)).unwrap();
                assert_eq!(bits(&got), bits(&dense), "{threshold:?}, {engine:?}");
            }
        }
    }

    // The routed path against the dense one on an input the rule accepts.
    // The fixture asserts its own routing, so it cannot quietly stop
    // testing the conversion when a constant moves.
    #[test]
    fn a_routed_input_gives_the_dense_diagram() {
        let side = (N_MIN as f64).sqrt().ceil() as usize + 1;
        let dist = grid(side);
        let threshold = 1.5;
        let edges = dist.count_edges_at(threshold);
        assert!(
            may_route(dist.len(), threshold) && graph_routes(dist.len(), edges),
            "the fixture must route: {} points, {edges} edges",
            dist.len()
        );
        let params = RipsParams::new(1).with_threshold(threshold);
        let dense = rips_persistence(&dist, &params.clone().with_engine(Engine::Dense)).unwrap();
        for engine in [Engine::Auto, Engine::Sparse] {
            let got = rips_persistence(&dist, &params.clone().with_engine(engine)).unwrap();
            assert_eq!(bits(&got), bits(&dense), "{engine:?}");
        }
    }

    // The default threshold of a dense input is its enclosing radius, and
    // of a sparse one is no threshold at all. Routing must carry the dense
    // default across, or the routed filtration would be larger.
    #[test]
    fn routing_carries_the_dense_default_threshold() {
        let side = (N_MIN as f64).sqrt().ceil() as usize + 1;
        let dist = grid(side);
        let radius = dist.enclosing_radius();
        let params = RipsParams::new(1);
        let auto = rips_persistence(&dist, &params).unwrap();
        let explicit = rips_persistence(
            &dist,
            &params
                .clone()
                .with_engine(Engine::Sparse)
                .with_threshold(radius),
        )
        .unwrap();
        assert_eq!(bits(&auto), bits(&explicit));
        // Every pair of this grid is finite, so no threshold at all would
        // admit strictly more edges than the enclosing radius does.
        assert!(dist.count_edges_at(radius) < dist.len() * (dist.len() - 1) / 2);
    }

    #[test]
    fn pipeline_runs_the_selected_schedule() {
        // The diagram is the same under every schedule, so only the
        // certificate the pipeline hands to `report` shows which collapse
        // ran: version 2 for rounds, version 1 otherwise, and the ordered
        // run at four workers tests more edges than the serial one on this
        // grid. Every certificate must pass the independent verifier.
        let dist = grid(5);
        let plain = rips_persistence(&dist, &RipsParams::new(2)).unwrap();
        let mut seen = Vec::new();
        for schedule in [
            CollapseSchedule::Serial,
            CollapseSchedule::Ordered,
            CollapseSchedule::Rounds,
            CollapseSchedule::Adaptive,
        ] {
            let params = RipsParams::new(2)
                .with_threads(4)
                .with_collapse_schedule(schedule);
            let mut captured = None;
            let diagram = collapse_and_solve(&dist, &params, |c| {
                captured = Some(c.clone());
                Ok(())
            })
            .unwrap();
            let captured = captured.expect("report must see the collapse");
            verify_dense(&dist, None, &captured).unwrap();
            let expected_version = match schedule {
                CollapseSchedule::Serial | CollapseSchedule::Ordered => 1,
                CollapseSchedule::Rounds => 2,
                CollapseSchedule::Adaptive => 3,
            };
            assert_eq!(captured.certificate.algorithm_version(), expected_version);
            let mut a = diagram.clone();
            let mut b = plain.clone();
            a.canonicalize();
            b.canonicalize();
            assert_eq!(a.bars, b.bars, "{schedule:?}");
            seen.push((schedule, captured.stats));
        }
        let serial = seen[0].1;
        let ordered = seen[1].1;
        assert_eq!(ordered.logical_tests, serial.edge_tests);
        assert!(
            ordered.edge_tests > serial.edge_tests,
            "the ordered schedule did not speculate: {} vs {}",
            ordered.edge_tests,
            serial.edge_tests
        );
        assert_eq!(serial.window_batches, 0);
        assert!(ordered.window_batches > 0);
    }
}
