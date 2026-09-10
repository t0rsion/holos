//! Canonical artifacts for edge collapse.
//!
//! An artifact contains the reduced graph and its collapse certificate. It
//! binds both the thresholded input graph and the reduced graph with SHA-256.
//! Decoding checks the envelope, resource limits, graph structure, and both
//! bindings. [`crate::collapse::verify`] then checks every removal.

mod decode;
mod encode;
mod model;
mod primitives;
#[cfg(test)]
mod tests;
mod validation;

pub use model::{ArtifactError, CollapseArtifact, DecodeLimits};
pub(crate) use primitives::graph_digest;

const MAGIC: &[u8; 8] = b"HOLOSCOL";
const WIRE_VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;
