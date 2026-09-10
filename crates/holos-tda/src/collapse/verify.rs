//! Independent checker for [`crate::collapse::CollapseCertificate`].
//!
//! The checker re-derives the thresholded input and replays every
//! recorded removal. It shares no sweep, scheduling, or witness-selection
//! code with the collapser. It is slow by design: its job is to catch a
//! wrong collapse.
//!
//! The checker dispatches on the certificate's algorithm version.
//! Version 1 replays the serial schedule: each step is checked against the
//! graph left by the steps before it. Version 2 replays the rounds
//! schedule: steps are grouped by round, every check in a round runs
//! against the graph as it stood before the round, and the round's edges
//! are deleted only after the whole round passes. Version 3 replays an
//! unstructured adaptive sequence. It checks the safety of every removal.
//! It does not reproduce or certify the ranking policy.
//!
//! A passing certificate establishes:
//!
//! - Header consistency: vertex count, requested threshold, terminal
//!   level, and input and output edge counts all match the input, the
//!   output matrix, and each other.
//! - Replay safety: each step removes a live edge with the recorded
//!   value. At every critical value of the reference graph the active
//!   witness apex satisfies the domination inequalities. The reference
//!   graph is the current replay state for version 1 and the pre-round
//!   graph for version 2. Every witness segment starts at an independently
//!   recomputed critical value at or below the terminal level. Every
//!   segment is therefore the active segment at its own start, and no
//!   segment escapes the apex check.
//! - Witness-rule fidelity: the segments are exactly what the frozen
//!   selection rule produces on the reference graph. A kept apex still
//!   dominates. A new segment opens only where the previous apex stopped
//!   dominating, and its apex is the first dominating vertex of the
//!   candidate set in increasing vertex order.
//! - Schedule order: passes, rounds, and sequence positions are 1-based.
//!   Pass and round numbers never decrease or skip. Steps inside a pass or
//!   round follow the frozen edge order: value descending, ties by ascending
//!   combinadic index of the endpoint pair.
//! - Round independence, version 2 only: for every ordered pair of steps
//!   in one round, the closed common neighborhood of the first edge,
//!   taken in the pre-round graph, does not contain both endpoints of the
//!   second. A round that groups conflicting removals is rejected even
//!   when replaying its steps one after the other would succeed.
//! - Output and fixed point: after the last step the live edges equal the
//!   output matrix bit for bit. A certificate marked `CompleteFixedPoint`
//!   also proves that no live edge is still removable. A certificate marked
//!   `BudgetLimited` makes no fixed-point claim.
//!
//! The checker does not certify that a run followed the production
//! scheduling policy. A complete certificate proves a fixed-point result.
//! It may reach that result through a different safe trace. Version 2
//! does not require each round to be a greedy-maximal batch. Version 3
//! does not check that the highest-scoring removal came first. Only a full
//! production re-run establishes the canonical production trace.

mod input;
mod model;
mod replay;
mod validate;
mod witness;

#[cfg(test)]
mod tests;

pub use input::{verify_dense, verify_dense_artifact, verify_sparse, verify_sparse_artifact};
pub use model::VerifyError;
