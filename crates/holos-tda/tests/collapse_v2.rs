//! Version 2 collapse gates: the rounds schedule.
//!
//! The version 2 collapser tests every live edge against a frozen snapshot
//! of the graph, orders the successes by the frozen priority, takes a greedy
//! batch of pairwise non-conflicting edges, and deletes the batch. The gates
//! pin that schedule: an independent unpruned reference written from the
//! specification, byte-identical output at every worker count, and bar-for-bar
//! equality with the uncollapsed engine, the version 1 schedule, and the
//! brute-force oracle.
//!
//! The version 2 output is not the version 1 output. Neither graph is
//! canonical; only the barcode is.
//!
//! The named fixtures below each say which part of the round structure they
//! attack. Their expected certificates are derived from the specification,
//! not observed from a run.

mod common;
#[path = "collapse_v2/mod.rs"]
mod v2;
