//! Wire format for checked finite degree-Rips bipersistence modules.

mod build;
mod digest;
mod model;
mod verify;
mod wire;

pub use model::{
    BipersistenceArtifact, BipersistenceArtifactLimits, BipersistenceArtifactSummary,
    BipersistenceRectangleClaim, BipersistenceRegionClaim,
};

const MAGIC: &[u8; 8] = b"HOLOSBP\0";
const VERSION: u16 = 2;
const F64_BITS_CODEC: u8 = 1;

#[cfg(all(test, holos_repository_tests))]
mod tests;
