//! Proof-carrying synthesis for finite temporal cohomology specifications.
//!
//! A specification contains state-indexed subspaces and upper bounds on their
//! surviving restriction-image rank. One selected action set must satisfy all
//! states. The proof tree certifies the global cost result without replaying the
//! producer's branch-and-bound search.

mod artifact;
mod model;
mod oracle;
mod proof;
mod proof_wire;
mod source_wire;
mod wire;

#[cfg(all(test, holos_repository_tests))]
mod tests;

pub use model::{
    SynthesisAction, SynthesisArtifact, SynthesisComponent, SynthesisCoordinate, SynthesisLimits,
    SynthesisSource, SynthesisState, SynthesisStatus, TopologicalSpecification,
};

pub(super) const MAGIC: &[u8; 8] = b"HOLOSSYN";
pub(super) const VERSION: u16 = 1;
pub(super) const F64_BITS_CODEC: u8 = 1;
pub(super) const FORMAT_MAX_STATES: usize = 4_096;
pub(super) const FORMAT_MAX_ACTIONS: usize = 65_536;
pub(super) const FORMAT_MAX_PROOF_NODES: usize = 10_000_000;
pub(super) const FORMAT_MAX_PROOF_TERMS: usize = 100_000_000;
