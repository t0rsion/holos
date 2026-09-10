#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Vietoris-Rips persistent homology and finite H1 degree-Rips modules over a
//! prime field.
//!
//! The scalar engine uses implicit persistent cohomology. Its tie-breaking and
//! output conventions match Ripser. The degree-Rips path constructs every node
//! and cover map on a declared finite grid.

pub(crate) mod adjacency;
/// Certified local models of H1 persistence under changing inputs.
pub mod atlas;
/// Wire format for persistence atlases.
pub mod atlas_wire;
/// Finite multicritical bifiltrations and exact degree-Rips construction.
pub mod bifiltration;
/// Exact finite H1 modules over degree-Rips parameter grids.
pub mod bipersistence;
/// Wire format for checked finite degree-Rips bipersistence modules.
pub mod bipersistence_artifact;
#[cfg(test)]
mod bit_keys;
pub(crate) mod budget;
/// Algebraic reduction certificates.
pub mod certificate;
/// Checked circular coordinates and conservative continuation.
pub mod circular;
/// Wire format for checked circular coordinates and continuation.
pub mod circular_artifact;
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
/// Certificates for kinetic cohomology zigzags.
pub mod kinetic_zigzag_artifact;
mod monotone_proof;
mod monotone_search;
/// Brute-force reference implementation used by the test gates.
pub mod oracle;
pub(crate) mod parallel;
/// Compositional H0 and H1 persistence programs.
pub mod program;
/// Update traces for persistence programs.
pub mod program_trace;
/// Wire format for persistence programs.
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
/// Trajectories across atlas regions.
pub mod trajectory;
mod union_find;
/// Exact interval decomposition of finite zigzag modules.
pub mod zigzag;

pub use atlas::{
    AtlasEvaluation, AtlasUpdate, ClassSensitivity, CoordinateDerivative, EdgeKey,
    EndpointGradient, EvaluatedClassSpace, LineageId, PersistenceAtlas, PointAtlasUpdate,
    PointClassSensitivity, PointEndpointGradient, PointPersistenceAtlas, TopologyEvent,
    TopologyEventKind, UpdateMode,
};
pub use atlas_wire::{AtlasArtifact, AtlasArtifactError, AtlasArtifactRepair, AtlasDecodeLimits};
pub use bifiltration::{
    BifiltrationLimits, BifiltrationSlice, Bigrade, BirthAntichain, DegreeRipsBifiltration,
    DegreeRipsParams, MulticriticalBifiltration, MulticriticalSimplex,
};
pub use bipersistence::{
    BipersistenceLimits, BipersistenceMap, BipersistenceMapColumn, BipersistenceModule,
    BipersistenceNode, BipersistenceRectangle, BipersistenceRegion, BipersistenceTerm,
    CircularCoordinateFamily, CircularCoordinateFamilyEntry, ClassExtension, ClassExtensionKind,
    ClassExtensionRegion, CohomologyClassAtlas,
};
pub use bipersistence_artifact::{
    BipersistenceArtifact, BipersistenceArtifactLimits, BipersistenceArtifactSummary,
    BipersistenceRectangleClaim, BipersistenceRegionClaim,
};
pub use certificate::{
    CertificateError, CertificateLimits, CertificateTerm, CertifiedReductionRegion,
    CertifiedRegionEvaluation, ChangeColumn, FiltrationSimplex, ReductionCertificate,
    ReductionGuard, ReductionGuardKind, ReductionRepair, ReductionRepairMode, ReductionRepairWork,
    RegionViolation, RegionViolationKind,
};
pub use circular::{
    CircularClassTerm, CircularCoordinate, CircularCoordinateContinuation,
    CircularCoordinateParams, IntegralCocycleTerm, circular_coordinate,
    circular_coordinate_for_class, circular_coordinate_with_integral_lift,
    cocycle_from_ripser_terms, continue_circular_coordinate,
};
pub use circular_artifact::{
    CircularArtifactError, CircularArtifactSummary, CircularCoordinateArtifact,
};
pub use classes::{
    BasisClassId, Cocycle, CocycleTerm, CriticalPair, CriticalSimplex, ExplainedDiagram,
    IntervalGroupId, PersistentClass, PersistentClassProvenance, PersistentClassSpace,
    lift_h1_classes, rips_persistence_with_classes_sparse,
};
pub use cohomology::{
    CochainTerm, CohomologyClass, CohomologyClassId, CohomologyContinuation,
    CohomologyContinuationKind, CohomologyLimits, CohomologyMapColumn, CohomologyMapTerm,
    CohomologyRelation, CohomologyRelationTerm, CohomologyRelationVector, CohomologyRestriction,
    CohomologySpace, CohomologySpaceId, CohomologySubspace, CohomologySubspaceGenerator,
    CohomologySubspaceTerm, cohomology_continuation, cohomology_relation, cohomology_restriction,
    cohomology_space,
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

mod scalar;

pub(crate) use scalar::collapse_and_solve;
pub use scalar::{
    Bar, CollapseSchedule, DenseStorage, Diagram, Engine, Error, GraphFactorization, Result,
    RipsParams, rips_persistence, rips_persistence_sparse, rips_persistence_with_classes,
};
