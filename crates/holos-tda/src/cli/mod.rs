//! The `holos` command-line interface, callable as a library function.

mod args;
mod bipersistence;
mod class_record;
mod cohomology;
mod compute;
mod coverage;
mod coverage_geometry;
mod dispatch;
mod index;
mod input;
mod portfolio;
mod synthesis;
mod verify;

pub(crate) use args::*;
pub use dispatch::run_cli;
pub(crate) use input::{read_circular_cocycle, read_proof_input, write_via_temporary};
