mod model;
mod reduction;
mod wire;

use crate::{ProofBar, ProofError, ProofLimits};

use self::reduction::verify_decoded;
use self::wire::decode;

/// Summary of a checked explicit filtration.
#[derive(Debug, Clone, PartialEq)]
pub struct VerifiedExplicitPersistence {
    /// Highest checked homology dimension.
    pub max_homology_dimension: usize,
    /// Prime coefficient modulus.
    pub modulus: u32,
    /// Stable vertex-label count.
    pub vertices: usize,
    /// Simplex counts in dimensions zero through `max_homology_dimension + 1`.
    pub simplex_counts: Vec<usize>,
    /// Checked change-of-basis column count.
    pub change_columns: usize,
    /// Checked nonzero change-of-basis term count.
    pub change_terms: usize,
    /// Diagram derived from the checked pivots.
    pub bars: Vec<ProofBar>,
}

/// Return true when bytes start with an explicit-complex certificate envelope.
pub fn is_explicit_persistence(bytes: &[u8]) -> bool {
    bytes.starts_with(wire::MAGIC)
}

/// Verify one bounded `HOLOSEXP` certificate.
pub fn verify_explicit_persistence(
    bytes: &[u8],
    limits: ProofLimits,
) -> Result<VerifiedExplicitPersistence, ProofError> {
    verify_decoded(decode(bytes, limits)?, limits)
}

fn proof_error(message: impl Into<String>) -> ProofError {
    ProofError::new(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_probe_is_exact() {
        assert!(is_explicit_persistence(b"HOLOSEXPanything"));
        assert!(!is_explicit_persistence(b"HOLOSEXanything"));
    }
}
