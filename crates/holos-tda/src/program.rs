//! Compositional, change-sensitive H0 and H1 persistence programs.
//!
//! A program splits positive-dimensional persistence at articulation
//! separators. It compiles each cyclic block into a reduction region and
//! computes H0 on the complete active graph. An update rebuilds only cyclic
//! blocks touched by changed weights. A topology or threshold-membership
//! change rebuilds the complete program.

mod composition;
mod continuation;
mod diagram;
mod model;
mod topology;
mod update;

#[cfg(test)]
mod tests;

pub use model::{
    BasisTransport, ClassContinuation, ContinuationKind, CorrespondenceMode, PersistenceProgram,
    ProgramAtomInfo, ProgramBranch, ProgramCheckpoint, ProgramDiagramState, ProgramDiagramUpdate,
    ProgramDiagramUpdateMode, ProgramEvaluation, ProgramEvent, ProgramEventKind, ProgramSummary,
    ProgramUpdate, ProgramUpdateMode, ProgramWork,
};

pub(crate) use composition::{atom_infos, compose_result, local_matrix};
pub(crate) use continuation::class_continuation;
pub(crate) use model::ProgramAtomState;
