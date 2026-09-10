mod linear;
mod model;
mod verify;
mod wire;

pub use model::{
    CircularProofLimits, VerifiedCircularContinuationKind, VerifiedCircularCoordinate,
};

use crate::ProofError;
use model::VerifiedCircularBinding;

/// Return true when bytes start with the circular-coordinate magic.
pub fn is_circular_coordinate(bytes: &[u8]) -> bool {
    bytes.starts_with(model::MAGIC)
}

/// Verify a bounded `HOLOSCC` circular-coordinate artifact.
///
/// The checker rebuilds each fixed-scale H1 space. It checks the finite-field
/// class, integer lift, harmonic potential, and optional continuation without
/// linking `holos-tda`.
pub fn verify_circular_coordinate(
    bytes: &[u8],
    limits: CircularProofLimits,
) -> Result<VerifiedCircularCoordinate, ProofError> {
    limits.validate()?;
    let claim = wire::decode_claim(bytes, limits)?;
    let checked = claim
        .states
        .iter()
        .map(|state| verify::verify_state(state, &claim, limits))
        .collect::<Result<Vec<_>, _>>()?;
    let continuation = verify::verify_continuation(&claim, &checked, limits)?;
    let relative_residuals = checked
        .iter()
        .filter_map(|state| state.relative_residual)
        .collect::<Vec<_>>();
    Ok(VerifiedCircularCoordinate {
        modulus: claim.modulus,
        scale: claim.scale,
        tolerance: claim.tolerance,
        states: claim.states.len(),
        coordinates: relative_residuals.len(),
        edges: claim.states.iter().map(|state| state.edges.len()).sum(),
        max_relative_residual: relative_residuals.into_iter().fold(0.0, f64::max),
        divisibilities: checked
            .iter()
            .filter_map(|state| state.divisibility)
            .collect(),
        continuation,
        ambiguity_rank: claim
            .continuation
            .as_ref()
            .map(|value| value.ambiguity.len())
            .unwrap_or(0),
    })
}

pub(crate) fn verify_single_binding(
    bytes: &[u8],
    limits: CircularProofLimits,
) -> Result<VerifiedCircularBinding, ProofError> {
    limits.validate()?;
    let claim = wire::decode_claim(bytes, limits)?;
    if claim.states.len() != 1 || claim.continuation.is_some() {
        return Err(ProofError::new(
            "a circular-family entry needs one coordinate state",
        ));
    }
    verify::verify_state(&claim.states[0], &claim, limits)?;
    let coordinate = claim.states[0]
        .coordinate
        .as_ref()
        .ok_or_else(|| ProofError::new("a circular-family entry has no coordinate"))?;
    Ok(VerifiedCircularBinding {
        vertex_count: claim.states[0].vertex_count,
        edges: claim.states[0].edges.clone(),
        modulus: claim.modulus,
        scale: claim.scale,
        tolerance: claim.tolerance,
        space: coordinate.space,
        class: coordinate.class.clone(),
    })
}

#[cfg(test)]
mod tests;
