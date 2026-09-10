pub use holos_tda::collapse::verify::{verify_dense, verify_sparse};
pub use holos_tda::collapse::{
    CollapsedRips, collapse_dense, collapse_dense_ordered_parallel,
    collapse_dense_ordered_with_window, collapse_sparse, collapse_sparse_ordered_parallel,
    collapse_sparse_ordered_with_window,
};
pub use holos_tda::oracle::rips_persistence_oracle_mod;
pub use holos_tda::{
    Bar, CollapseSchedule, Diagram, DistanceMatrix, RipsParams, SparseDistanceMatrix,
    rips_persistence, rips_persistence_sparse,
};

mod diagram;
mod fixtures;
mod reference;
mod support;

pub(crate) use diagram::*;
pub(crate) use fixtures::*;
pub(crate) use reference::*;
pub(crate) use support::*;

mod invariance;
mod production;
mod scheduling;
