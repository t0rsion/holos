//! Wire format for persistence atlases.
//!
//! A `HOLOSATL` version 2 envelope binds the complete listed graph, its
//! canonical H1 class spaces and critical simplices, optional class
//! provenance, and a nested algebraic reduction certificate. Verification
//! checks the reduction without calling the persistence solver, validates each
//! cocycle on the caller's graph, and reconstructs the reusable validity
//! region.

mod artifact;
mod codec;
mod model;
#[cfg(test)]
mod tests;
mod validation;
mod wire;

const MAGIC: &[u8; 8] = b"HOLOSATL";
const WIRE_VERSION: u16 = 2;
const F64_BITS_CODEC: u8 = 1;

pub use model::{AtlasArtifact, AtlasArtifactError, AtlasArtifactRepair, AtlasDecodeLimits};
