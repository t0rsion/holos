//! Wire format and producer for one source-bound persistent H1 class.

mod build;
mod model;
mod wire;

#[cfg(test)]
mod tests;

pub use model::{PersistenceCycleTerm, PersistenceTriangleTerm, PersistentClassArtifact};

pub(crate) const MAGIC: &[u8; 8] = b"HOLOSPC\0";
pub(crate) const VERSION: u16 = 1;
pub(crate) const F64_BITS_CODEC: u8 = 1;
