use std::collections::{BTreeMap, BTreeSet};

use crate::ProofError;
use crate::cohomology::{self, ContinuationKind, Space};

use super::linear;
use super::model::{
    CheckedState, CircularProofLimits, Claim, CoordinateClaim, StateClaim,
    VerifiedCircularContinuationKind,
};

pub(super) fn verify_state(
    state: &StateClaim,
    claim: &Claim,
    limits: CircularProofLimits,
) -> Result<CheckedState, ProofError> {
    let space = Space::build(
        state.vertex_count,
        1,
        &state.edges,
        claim.modulus,
        limits.proof,
    )?;
    let Some(coordinate) = &state.coordinate else {
        return Ok(CheckedState {
            space,
            relative_residual: None,
            divisibility: None,
        });
    };
    let (divisibility, relative_residual) = verify_coordinate(state, claim, &space, coordinate)?;
    Ok(CheckedState {
        space,
        relative_residual: Some(relative_residual),
        divisibility: Some(divisibility),
    })
}

fn verify_coordinate(
    state: &StateClaim,
    claim: &Claim,
    space: &Space,
    coordinate: &CoordinateClaim,
) -> Result<(u64, f64), ProofError> {
    verify_space_identifier(state, claim, space, coordinate)?;
    verify_active_terms(state, coordinate)?;
    verify_source_class(state, claim, space, coordinate)?;
    let integral = coordinate
        .integral
        .iter()
        .copied()
        .collect::<BTreeMap<_, _>>();
    let divisibility = verify_integral_claim(state, claim, coordinate, &integral)?;
    let residual = verify_potential_claim(state, claim, coordinate, &integral)?;
    Ok((divisibility, residual))
}

fn verify_space_identifier(
    state: &StateClaim,
    claim: &Claim,
    space: &Space,
    coordinate: &CoordinateClaim,
) -> Result<(), ProofError> {
    if space.id(
        state.vertex_count,
        1,
        claim.scale,
        claim.modulus,
        &state.edges,
    ) != coordinate.space
    {
        return Err(ProofError::new(
            "circular cohomology-space identifier is wrong",
        ));
    }
    Ok(())
}

fn verify_active_terms(state: &StateClaim, coordinate: &CoordinateClaim) -> Result<(), ProofError> {
    let active = state.edges.iter().copied().collect::<BTreeSet<_>>();
    if coordinate
        .source
        .iter()
        .any(|term| !active.contains(&term.0))
        || coordinate
            .integral
            .iter()
            .any(|term| !active.contains(&term.0))
    {
        return Err(ProofError::new("circular cocycle uses an inactive edge"));
    }
    Ok(())
}

fn verify_source_class(
    state: &StateClaim,
    claim: &Claim,
    space: &Space,
    coordinate: &CoordinateClaim,
) -> Result<(), ProofError> {
    linear::check_field_triangle_closure(
        state.vertex_count,
        &state.edges,
        &coordinate.source,
        claim.modulus,
    )?;
    let class = space.coordinates_of_edge_cocycle(&coordinate.source, claim.modulus)?;
    if class.is_empty() || class != coordinate.class {
        return Err(ProofError::new(
            "circular source does not represent the declared class",
        ));
    }
    Ok(())
}

fn verify_integral_claim(
    state: &StateClaim,
    claim: &Claim,
    coordinate: &CoordinateClaim,
    integral: &BTreeMap<crate::cohomology::Edge, i64>,
) -> Result<u64, ProofError> {
    linear::check_integer_triangle_closure(state.vertex_count, &state.edges, integral)?;
    linear::check_reduction(
        &state.edges,
        &coordinate.source,
        integral,
        coordinate.field_multiplier,
        claim.modulus,
    )?;
    let divisibility = linear::integral_divisibility(state.vertex_count, &state.edges, integral)?;
    if divisibility != coordinate.divisibility {
        return Err(ProofError::new("circular integral divisibility is wrong"));
    }
    Ok(divisibility)
}

fn verify_potential_claim(
    state: &StateClaim,
    claim: &Claim,
    coordinate: &CoordinateClaim,
    integral: &BTreeMap<crate::cohomology::Edge, i64>,
) -> Result<f64, ProofError> {
    let roots = linear::component_roots(state.vertex_count, &state.edges);
    if roots
        .iter()
        .any(|&root| coordinate.potential[root].to_bits() != 0)
    {
        return Err(ProofError::new(
            "circular potential does not use the canonical component gauge",
        ));
    }
    let relative_residual =
        linear::relative_residual(&state.edges, integral, &coordinate.potential, &roots)?;
    if relative_residual > claim.tolerance {
        return Err(ProofError::new(format!(
            "circular relative residual {relative_residual} exceeds {}",
            claim.tolerance
        )));
    }
    Ok(relative_residual)
}

pub(super) fn verify_continuation(
    claim: &Claim,
    checked: &[CheckedState],
    limits: CircularProofLimits,
) -> Result<Option<VerifiedCircularContinuationKind>, ProofError> {
    let Some(declared) = &claim.continuation else {
        return Ok(None);
    };
    let old_state = &claim.states[0];
    let new_state = &claim.states[1];
    if old_state.vertex_count != new_state.vertex_count {
        return Err(ProofError::new(
            "circular continuation changes the labeled vertex set",
        ));
    }
    let common_edges = old_state
        .edges
        .iter()
        .copied()
        .filter(|edge| new_state.edges.binary_search(edge).is_ok())
        .collect::<Vec<_>>();
    let common = Space::build(
        old_state.vertex_count,
        1,
        &common_edges,
        claim.modulus,
        limits.proof,
    )?;
    let selected = &old_state
        .coordinate
        .as_ref()
        .ok_or_else(|| ProofError::new("circular continuation has no old coordinate"))?
        .class;
    let actual = cohomology::continuation(
        &checked[0].space,
        &checked[1].space,
        &common,
        selected,
        claim.modulus,
    )?;
    let actual_kind = verified_kind(actual.kind);
    if actual_kind != declared.kind
        || actual.target != declared.target
        || actual.ambiguity != declared.ambiguity
    {
        return Err(ProofError::new(
            "circular continuation claim differs from the exact relation",
        ));
    }
    let new_coordinate = new_state.coordinate.as_ref();
    if (actual_kind == VerifiedCircularContinuationKind::Unique) != new_coordinate.is_some() {
        return Err(ProofError::new(
            "circular continuation coordinate does not match its status",
        ));
    }
    if new_coordinate.is_some_and(|coordinate| coordinate.class != actual.target) {
        return Err(ProofError::new(
            "continued circular coordinate names the wrong target class",
        ));
    }
    Ok(Some(actual_kind))
}

fn verified_kind(kind: ContinuationKind) -> VerifiedCircularContinuationKind {
    match kind {
        ContinuationKind::Unique => VerifiedCircularContinuationKind::Unique,
        ContinuationKind::Ambiguous => VerifiedCircularContinuationKind::Ambiguous,
        ContinuationKind::NoExtension => VerifiedCircularContinuationKind::NoExtension,
        ContinuationKind::NoNonzeroContinuation => {
            VerifiedCircularContinuationKind::NoNonzeroContinuation
        }
    }
}
