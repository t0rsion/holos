const MAGIC: &[u8; 8] = b"HOLOSRI\0";
const VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;

mod api;
mod cells;
mod decode;
mod digest;
mod leaf;
mod model;
mod reduction;
mod replay;
mod verify;

/// Return true when bytes start with the relative-interface certificate magic.
pub fn is_relative_interface(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

pub use api::{verify_relative_composition, verify_relative_interface};
pub use model::VerifiedRelativeInterface;

pub(crate) use decode::decode_verified;
pub(crate) use leaf::verify_index_leaf;
pub(crate) use model::{IndexLeafContext, VerifiedCertificate};
