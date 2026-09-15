//! Independent checking for persistence-index snapshots and warm deltas.

mod delta;
mod digest;
mod graded;
mod model;
mod scope;
mod state;
mod wire;
mod wire_reader;

#[cfg(test)]
mod tests;

pub use model::{IndexProofState, VerifiedIndexDelta, VerifiedIndexSnapshot};

/// Return true when bytes start with the versioned-index snapshot magic.
pub fn is_index_snapshot(bytes: &[u8]) -> bool {
    bytes.starts_with(wire::SNAPSHOT_MAGIC)
}
