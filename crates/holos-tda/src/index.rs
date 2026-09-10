//! Versioned exact persistence over checked separator interfaces.
//!
//! An index decomposes the fixed listed-edge envelope of a sparse graph.
//! Relative filtered cores compose through arbitrary protected separators.
//! `InterfacePolicy::Compose` and `InterfacePolicy::Materialize` are
//! alternative parent policies. A transition path-copies the affected route
//! and shares every untouched subtree.

mod compile;
mod model;
mod summary;
mod transition;

#[cfg(test)]
mod tests;

pub(crate) use model::InterfaceNode;
pub use model::{
    DiagramDelta, IndexBranch, IndexDiff, IndexEdit, IndexEvent, IndexEventKind, IndexParams,
    IndexSummary, IndexTransition, IndexUpdateMode, IndexWork, InterfaceMode, InterfacePolicy,
    InterfaceSummary, PersistenceIndex, TopologyPatch,
};
