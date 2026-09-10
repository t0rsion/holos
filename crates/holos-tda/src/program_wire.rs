//! Wire format for persistence programs.
//!
//! A `HOLOSPRG` envelope binds the complete listed graph, its articulation
//! decomposition, one atlas per cyclic atom, and the composed H0 and H1
//! diagram. Verification does not call the persistence solver.

mod decode;
mod encode;
mod model;
mod primitives;
mod verification;

#[cfg(test)]
mod tests;

pub use model::{ProgramArtifact, ProgramArtifactError, ProgramAtomArtifact, ProgramDecodeLimits};

#[cfg(test)]
use verification::diagram_bits_equal;

const MAGIC: &[u8; 8] = b"HOLOSPRG";
const WIRE_VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;
