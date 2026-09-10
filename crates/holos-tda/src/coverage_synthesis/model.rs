//! Coverage specification, action, and artifact data models.

#[path = "artifact_data.rs"]
mod artifact_data;
#[path = "specification.rs"]
mod specification;
#[path = "types.rs"]
mod types;

pub use artifact_data::CoverageSynthesisArtifact;
pub(super) use artifact_data::{
    BuiltCoverageProof, CoverageHeader, CoverageProducerWork, CoverageSearchData,
    CoverageSelectionData, CoverageWorkLimits, DecodedCoverageProof, DecodedCoverageSearch,
};
pub use specification::{
    CoverageSource, CoverageSpecification, CoverageState, CoverageSynthesisLimits,
};
pub(super) use specification::{FORMAT_MAX_PROOF_NODES, FORMAT_MAX_PROOF_TERMS};
pub(super) use types::EvaluationClaim;
pub use types::{
    CoverageAction, CoverageComponent, CoverageCounterexample, CoveragePlanEvaluation,
    CoverageSynthesisStatus,
};
