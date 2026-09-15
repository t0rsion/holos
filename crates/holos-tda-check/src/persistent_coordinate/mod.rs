//! Independent verification of a selected persistent H1 circular coordinate.

mod model;
mod verify;
mod wire;

pub use model::{VerifiedIntegralTerm, VerifiedPersistentCoordinate};

use crate::persistent_class::verify_persistent_class;
use crate::{CircularProofLimits, ProofError};

/// Return true when bytes start with a `HOLOSPH` persistent-coordinate envelope.
pub fn is_persistent_coordinate(bytes: &[u8]) -> bool {
    bytes.starts_with(wire::MAGIC)
}

/// Verify a selected circular coordinate and its nested persistent-class artifact.
pub fn verify_persistent_coordinate(
    bytes: &[u8],
    limits: CircularProofLimits,
) -> Result<VerifiedPersistentCoordinate, ProofError> {
    limits.validate()?;
    let claim = wire::decode(bytes, limits)?;
    let class = verify_persistent_class(&claim.nested_bytes, limits.proof)?;
    verify::verify_decoded(claim, class)
}
