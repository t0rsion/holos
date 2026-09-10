mod atlas;
mod claims;
mod grid;
mod linear;
mod module;
mod node;
mod queries;
mod wire;

pub use claims::{BipersistenceProofLimits, VerifiedBipersistence};

use crate::ProofError;
use module::CheckedModule;
use wire::MAGIC;

/// Return true when bytes start with the bipersistence magic.
pub fn is_bipersistence(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

/// Verify a bounded `HOLOSBP` finite bipersistence artifact.
///
/// The checker validates the declared finite degree-Rips grid. It rebuilds
/// every `H¹` space and cover restriction, checks every square, and recomputes
/// each rectangle rank and class-extension atlas.
pub fn verify_bipersistence(
    bytes: &[u8],
    limits: BipersistenceProofLimits,
) -> Result<VerifiedBipersistence, ProofError> {
    let claim = wire::decode_claim(bytes, limits)?;
    let checked = CheckedModule::build(&claim, limits)?;
    checked.verify_claims(&claim, limits)?;
    Ok(VerifiedBipersistence {
        vertices: claim.vertex_count,
        edges: claim.edges.len(),
        scales: claim.scale_bits.len(),
        density_levels: claim.minimum_degrees.len(),
        nodes: claim.nodes.len(),
        cover_maps: claim.cover_maps.len(),
        rectangles: claim.rectangles.len(),
        regions: claim.rank_regions.len(),
        class_atlases: claim.class_atlases.len(),
        circular_families: claim.circular_families.len(),
        modulus: claim.modulus,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_is_exact() {
        assert!(is_bipersistence(b"HOLOSBP\0anything"));
        assert!(!is_bipersistence(b"HOLOSBanything"));
    }

    #[test]
    fn truncated_inputs_are_errors() {
        for bytes in [&[][..], &[0u8; 31], &[0u8; 32], b"HOLOSBP\0"] {
            assert!(verify_bipersistence(bytes, BipersistenceProofLimits::default()).is_err());
        }
    }
}
