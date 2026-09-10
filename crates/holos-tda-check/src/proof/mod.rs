//! Core proof model, wire format, and independent replay.

mod graph;
mod model;
mod verify;
mod wire;

#[cfg(test)]
mod tests;

pub use model::{
    AtomProof, ProofBar, ProofBundle, ProofColumn, ProofEdge, ProofError, ProofLimits, ProofTerm,
    SnapshotProof,
};
pub use verify::VerifiedProof;

pub(crate) use graph::{
    Block, Graph, SparseColumn, canonicalize_diagram, check_column, check_diagram, check_matrix,
    checked_threshold, diagrams_equal, h0_diagram, program_blocks,
};
pub(crate) use wire::Reader;
