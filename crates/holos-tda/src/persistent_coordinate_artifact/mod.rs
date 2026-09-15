//! Wire format for one selected persistent circular coordinate.

mod build;
mod model;
mod wire;

#[cfg(all(test, holos_repository_tests))]
mod tests;

pub use model::{
    PersistentCoordinateArtifact, PersistentCoordinateArtifactError,
    PersistentCoordinateArtifactSummary,
};

pub(crate) const MAGIC: &[u8; 8] = b"HOLOSPH\0";
pub(crate) const VERSION: u16 = 1;
pub(crate) const F64_BITS_CODEC: u8 = 1;
