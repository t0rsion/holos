mod artifact;
mod specification;
mod types;

pub use artifact::SynthesisArtifact;
pub use specification::{
    SynthesisAction, SynthesisComponent, SynthesisState, TopologicalSpecification,
};
pub use types::{SynthesisCoordinate, SynthesisLimits, SynthesisSource, SynthesisStatus};

pub(super) use artifact::{
    BuiltProof, ProducerWork, ProofData, SearchData, SelectionData, SynthesisHeader, WorkLimits,
};
pub(super) use types::{BoundKind, ProofNode};
