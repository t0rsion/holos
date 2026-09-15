#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Independent replay of holos proof artifacts.
//!
//! The crate does not depend on `holos-tda`. It shares the mathematical
//! specification and byte formats with the producer. Public functions
//! reconstruct each declared claim from a bounded envelope.
//!
//! The checker does not certify a producer algorithm in a proof assistant.
//! SHA-256 digests detect content changes. They do not authenticate a producer.
//! Collapse and portfolio artifacts use the linked producer verifier.

mod bipersistence;
mod circular;
mod cohomology;
mod cohomology_intervention;
mod coverage;
mod coverage_geometry;
mod distributed;
mod explicit;
mod finite_field;
mod index;
mod kinetic_zigzag;
mod persistent_class;
mod persistent_coordinate;
mod program;
mod proof;
mod relative;
mod synthesis;

pub use bipersistence::{
    BipersistenceProofLimits, VerifiedBipersistence, is_bipersistence, verify_bipersistence,
};
pub use circular::{
    CircularProofLimits, VerifiedCircularContinuationKind, VerifiedCircularCoordinate,
    is_circular_coordinate, verify_circular_coordinate,
};
pub use cohomology_intervention::{
    VerifiedCohomologyIntervention, VerifiedCohomologyInterventionStatus,
    is_cohomology_intervention, verify_cohomology_intervention,
};
pub use coverage::{
    VerifiedCoverage, VerifiedCoverageSource, VerifiedCoverageStatus, is_coverage, verify_coverage,
};
pub use coverage_geometry::{
    VerifiedGeometryBoundCoverage, is_geometry_bound_coverage, verify_geometry_bound_coverage,
};
pub use distributed::{
    VerifiedDistributedInterface, is_distributed_interface, verify_distributed_interface,
    verify_distributed_interface_with,
};
pub use explicit::{
    VerifiedExplicitPersistence, is_explicit_persistence, verify_explicit_persistence,
};
pub use index::{IndexProofState, VerifiedIndexDelta, VerifiedIndexSnapshot, is_index_snapshot};
pub use kinetic_zigzag::{VerifiedKineticZigzag, is_kinetic_zigzag, verify_kinetic_zigzag};
pub use persistent_class::{
    PersistenceCycleTerm, PersistenceTriangleTerm, PersistentCocycleTerm, PersistentCriticalPair,
    PersistentSourceEdge, VerifiedPersistentClass, is_persistent_class, verify_persistent_class,
};
pub use persistent_coordinate::{
    VerifiedIntegralTerm, VerifiedPersistentCoordinate, is_persistent_coordinate,
    verify_persistent_coordinate,
};
pub use program::{
    ProgramEdge, ProgramGraph, ProgramProofLimits, ProgramTraceProofLimits, VerifiedProgram,
    VerifiedProgramTrace, is_program, is_program_trace, verify_program, verify_program_trace,
};
pub use proof::{
    AtomProof, ProofBar, ProofBundle, ProofColumn, ProofEdge, ProofError, ProofLimits, ProofTerm,
    SnapshotProof, VerifiedProof,
};
pub use relative::{
    VerifiedRelativeInterface, is_relative_interface, verify_relative_composition,
    verify_relative_interface,
};
pub use synthesis::{
    VerifiedSynthesis, VerifiedSynthesisSource, VerifiedSynthesisStatus, is_synthesis,
    verify_synthesis,
};

pub(crate) use finite_field::{inverse_mod, is_prime};
pub(crate) use proof::{
    Graph, Reader, SparseColumn, canonicalize_diagram, check_column, check_matrix,
    checked_threshold, diagrams_equal,
};

const MAGIC: &[u8; 8] = b"HOLOSPF\0";
const VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;
const MODULUS_LIMIT: u64 = 32_768;
const SEPARATOR_WIDTH: usize = 3;
const SEPARATOR_SEARCH_LIMIT: usize = 100_000;
