//! Ordered speculative collapse gates.
//!
//! The ordered execution runs the version 1 schedule with staged windows.
//! Its output must be the version 1 output: the same matrix, the same
//! certificate field for field with floats compared by bits, the same pass
//! count, at every worker count and every window size. Only the work
//! counters may move.
//!
//! The suite uses three references: the shipped serial version 1 collapser,
//! an unpruned version 1 reference rebuilt from the specification, and the
//! uncollapsed engine with its brute-force oracle.
//!
//! The named fixtures at the end attack the scheduler itself. Each one is
//! built against the frozen edge order (value descending, ties by
//! `(v, u)` ascending), and its expectations come from hand-simulating the
//! serial schedule, not from a run.

mod common;
#[path = "collapse_ordered/mod.rs"]
mod ordered;
