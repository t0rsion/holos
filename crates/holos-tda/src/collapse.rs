//! Filtered edge collapse for flag filtrations.
//!
//! The collapse removes edges that are dominated at every scale from their
//! birth to the terminal level. That is the filtration-wide multi-witness
//! criterion of Boissonnat and Pritam. The flag filtration of the reduced
//! graph has the same persistence diagram as the input, in every dimension. Each
//! removal is recorded in a replayable [`CollapseCertificate`] that the
//! independent checker in [`verify`] can validate.
//!
//! Four schedules exist. [`collapse_dense`] and [`collapse_sparse`] run
//! the serial schedule: passes over the edges with immediate deletion.
//! [`crate::rips_persistence`] and the CLI use this schedule by default.
//! In the registered studies it is the fastest on most inputs.
//! [`crate::CollapseSchedule`] selects the others.
//! [`collapse_dense_ordered_parallel`] and
//! [`collapse_sparse_ordered_parallel`] run the ordered schedule. They
//! perform the same removals, tested speculatively in parallel. Their
//! output is the serial one, field for field, at every worker count.
//! Serial and ordered both write an algorithm version 1 certificate.
//! [`collapse_dense_rounds_parallel`] and
//! [`collapse_sparse_rounds_parallel`] run the rounds schedule. It writes
//! a version 2 certificate. Each round tests live edges against a frozen
//! graph and deletes a batch of provably independent removals. The output is
//! byte-identical at every worker count. All four schedules are
//! deterministic given the vertex labeling.
//! [`collapse_dense_adaptive`] and [`collapse_sparse_adaptive`] run the
//! version 3 schedule. It ranks live removals by the triangles or
//! tetrahedra they remove. With a declared work limit it can return a
//! certified partial collapse. No reduced graph is canonical. A
//! relabeling or schedule change can move the surviving set. The barcode
//! does not change.

mod adaptive;
mod domination;
mod execution;
mod model;
mod ordered;
mod parallel;
mod portfolio;
mod preparation;
#[cfg(test)]
mod tests;
pub mod verify;
/// Collapse certificates and their reduced graphs.
pub mod wire;

pub(crate) use adaptive::collapse_adaptive_in;
pub use adaptive::{collapse_dense_adaptive, collapse_sparse_adaptive};
use domination::{Scratch, for_each_induced_edge, mark_dirty, test_edge};
use execution::collapse_impl;
pub(crate) use execution::collapse_serial_in;
pub use execution::{collapse_dense, collapse_sparse};
pub(crate) use ordered::collapse_ordered_in;
pub use ordered::{
    collapse_dense_ordered_parallel, collapse_dense_ordered_with_window,
    collapse_sparse_ordered_parallel, collapse_sparse_ordered_with_window,
};
pub(crate) use parallel::collapse_rounds_in;
pub use parallel::{collapse_dense_rounds_parallel, collapse_sparse_rounds_parallel};
pub use portfolio::{
    CollapsePortfolio, CollapsePortfolioArtifact, CollapsePortfolioArtifactEntry,
    CollapsePortfolioCandidate, CollapsePortfolioDecodeLimits, CollapsePortfolioEntry,
    CollapsePortfolioLimits, CollapsePortfolioObjective, CollapsePortfolioScore,
    collapse_sparse_portfolio,
};
use preparation::{
    AdjEntry, EdgeRec, Execution, Prepared, Run, build_pool, finish, prepare, tombstone,
};

pub(crate) use crate::distances::Distances;
pub(crate) use crate::{DistanceMatrix, Result, SparseDistanceMatrix};

pub use model::{
    AdaptiveCollapseParams, CollapseCertificate, CollapseCompleteness, CollapseObjective,
    CollapseStats, CollapseTimings, CollapsedRips, RemovalStep, SchedulePosition,
};
