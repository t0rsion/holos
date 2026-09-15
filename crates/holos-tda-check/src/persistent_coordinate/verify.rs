use std::collections::BTreeMap;

use crate::ProofError;
use crate::circular::{
    check_integer_triangle_closure, check_reduction, component_roots, integral_divisibility,
    relative_residual,
};
use crate::cohomology::Edge;
use crate::persistent_class::VerifiedPersistentClass;

use super::model::{DecodedPersistentCoordinate, VerifiedPersistentCoordinate};

pub(super) fn verify_decoded(
    claim: DecodedPersistentCoordinate,
    class: VerifiedPersistentClass,
) -> Result<VerifiedPersistentCoordinate, ProofError> {
    let (modulus, scale) = validate_header(&claim, &class)?;
    let source = source_terms(&class);
    let active = active_edges(&class, scale);
    let integral = checked_integral(&claim, &active)?;
    verify_lift(&class, &claim, &active, &source, &integral, modulus)?;
    let relative_residual = verify_potential(&class, &claim, &active, &integral)?;
    Ok(claim.into_verified(class, active.len(), relative_residual))
}

fn validate_header(
    claim: &DecodedPersistentCoordinate,
    class: &VerifiedPersistentClass,
) -> Result<(u32, f64), ProofError> {
    let modulus = class.modulus();
    if claim.field_multiplier == 0 || claim.field_multiplier >= modulus {
        return Err(ProofError::new(
            "persistent-coordinate lift multiplier is invalid",
        ));
    }
    if claim.potential.len() != class.vertex_count() {
        return Err(ProofError::new(
            "persistent-coordinate potential count differs from the checked class",
        ));
    }
    Ok((modulus, class.scale()))
}

fn source_terms(class: &VerifiedPersistentClass) -> Vec<(Edge, u32)> {
    class
        .cocycle()
        .iter()
        .map(|term| {
            (
                Edge {
                    u: term.u(),
                    v: term.v(),
                },
                term.coefficient(),
            )
        })
        .collect()
}

fn active_edges(class: &VerifiedPersistentClass, scale: f64) -> Vec<Edge> {
    class
        .source()
        .iter()
        .filter(|edge| edge.value() <= scale)
        .map(|edge| Edge {
            u: edge.u(),
            v: edge.v(),
        })
        .collect()
}

fn checked_integral(
    claim: &DecodedPersistentCoordinate,
    active: &[Edge],
) -> Result<BTreeMap<Edge, i64>, ProofError> {
    if claim
        .integral
        .iter()
        .any(|(edge, _)| active.binary_search(edge).is_err())
    {
        return Err(ProofError::new(
            "persistent-coordinate integral lift uses an inactive edge",
        ));
    }
    Ok(claim.integral.iter().copied().collect())
}

fn verify_lift(
    class: &VerifiedPersistentClass,
    claim: &DecodedPersistentCoordinate,
    active: &[Edge],
    source: &[(Edge, u32)],
    integral: &BTreeMap<Edge, i64>,
    modulus: u32,
) -> Result<(), ProofError> {
    check_integer_triangle_closure(class.vertex_count(), active, integral)?;
    check_reduction(active, source, integral, claim.field_multiplier, modulus)?;
    let divisibility = integral_divisibility(class.vertex_count(), active, integral)?;
    if divisibility != claim.divisibility {
        return Err(ProofError::new(
            "persistent-coordinate integral divisibility is wrong",
        ));
    }
    Ok(())
}

fn verify_potential(
    class: &VerifiedPersistentClass,
    claim: &DecodedPersistentCoordinate,
    active: &[Edge],
    integral: &BTreeMap<Edge, i64>,
) -> Result<f64, ProofError> {
    let roots = component_roots(class.vertex_count(), active);
    if roots
        .iter()
        .any(|&root| claim.potential[root].to_bits() != 0)
    {
        return Err(ProofError::new(
            "persistent-coordinate potential does not use the canonical component gauge",
        ));
    }
    let relative_residual = relative_residual(active, integral, &claim.potential, &roots)?;
    if relative_residual > claim.tolerance {
        return Err(ProofError::new(format!(
            "persistent-coordinate relative residual {relative_residual} exceeds {}",
            claim.tolerance
        )));
    }
    Ok(relative_residual)
}
