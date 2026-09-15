use std::collections::BTreeMap;

use crate::classes::{Cocycle, CocycleTerm, PersistentClass, validate_h1_cocycle};
use crate::cohomology::{
    CochainTerm, CohomologyContinuation, CohomologyContinuationKind, CohomologySpace,
    cohomology_continuation, cohomology_relation, cohomology_space,
};
use crate::field::{MODULUS_LIMIT, is_prime};
use crate::{Error, Result, SparseDistanceMatrix};

use super::harmonic::{canonical_phase, harmonic_potential, integral_divisibility};
use super::lift::{
    active_edges, centered_integral_lift, check_integral_lift, infer_field_multiplier, inverse_mod,
};
use super::model::{
    CircularClassTerm, CircularCoordinate, CircularCoordinateContinuation,
    CircularCoordinateParams, IntegralCocycleTerm, SelectedCircularCoordinate,
};

/// Normalize Ripser-shaped H1 terms into a canonical holos cocycle.
///
/// Each input row is `(u, v, coefficient)`. Reversed endpoints are accepted
/// and change the coefficient sign. Terms above `scale` are removed before
/// the result is normalized projectively. Its first coefficient is one.
pub fn cocycle_from_ripser_terms(
    graph: &SparseDistanceMatrix,
    modulus: u32,
    scale: f64,
    terms: &[(usize, usize, u32)],
) -> Result<Cocycle> {
    if !is_prime(u64::from(modulus)) || u64::from(modulus) >= MODULUS_LIMIT {
        return Err(Error::InvalidInput(
            "circular cocycle modulus must be a supported prime".into(),
        ));
    }
    if !scale.is_finite() || scale < 0.0 {
        return Err(Error::InvalidInput(
            "circular cocycle scale must be finite and non-negative".into(),
        ));
    }
    let modulus64 = u64::from(modulus);
    let mut coefficients = BTreeMap::<(usize, usize), u64>::new();
    for &(a, b, coefficient) in terms {
        add_ripser_term(graph, scale, modulus, &mut coefficients, a, b, coefficient)?;
    }
    let Some(&first) = coefficients.values().next() else {
        return Err(Error::InvalidInput("Ripser cocycle is zero".into()));
    };
    let inverse = inverse_mod(first, modulus64);
    Ok(Cocycle {
        modulus,
        scale,
        terms: coefficients
            .into_iter()
            .map(|((u, v), coefficient)| CocycleTerm {
                u,
                v,
                coefficient: (coefficient * inverse % modulus64) as u32,
            })
            .collect(),
    })
}

fn add_ripser_term(
    graph: &SparseDistanceMatrix,
    scale: f64,
    modulus: u32,
    coefficients: &mut BTreeMap<(usize, usize), u64>,
    a: usize,
    b: usize,
    coefficient: u32,
) -> Result<()> {
    let modulus64 = u64::from(modulus);
    if a == b || a >= graph.len() || b >= graph.len() || coefficient >= modulus {
        return Err(Error::InvalidInput(
            "Ripser cocycle term is not a valid oriented edge coefficient".into(),
        ));
    }
    if graph.get(a, b) > scale {
        return Ok(());
    }
    let (edge, value) = if a < b {
        ((a, b), u64::from(coefficient))
    } else {
        ((b, a), (modulus64 - u64::from(coefficient)) % modulus64)
    };
    let next = (coefficients.get(&edge).copied().unwrap_or(0) + value) % modulus64;
    if next == 0 {
        coefficients.remove(&edge);
    } else {
        coefficients.insert(edge, next);
    }
    Ok(())
}

/// Compute a checked harmonic circular coordinate from one H1 cocycle.
pub fn circular_coordinate(
    graph: &SparseDistanceMatrix,
    cocycle: &Cocycle,
    params: CircularCoordinateParams,
) -> Result<CircularCoordinate> {
    validate_params(params)?;
    validate_h1_cocycle(graph, cocycle)?;
    circular_coordinate_from_checked_cocycle(graph, cocycle, params)
}

fn circular_coordinate_from_checked_cocycle(
    graph: &SparseDistanceMatrix,
    cocycle: &Cocycle,
    params: CircularCoordinateParams,
) -> Result<CircularCoordinate> {
    automatic_coordinate(graph, cocycle, params).map_err(CircularCoordinateFailure::into_error)
}

#[derive(Debug)]
pub(crate) enum CircularCoordinateFailure {
    Lift(Error),
    Solve(Error),
    Other(Error),
}

impl CircularCoordinateFailure {
    pub(crate) fn into_error(self) -> Error {
        match self {
            Self::Lift(error) | Self::Solve(error) | Self::Other(error) => error,
        }
    }
}

impl From<Error> for CircularCoordinateFailure {
    fn from(error: Error) -> Self {
        Self::Other(error)
    }
}

pub(crate) fn circular_coordinate_with_failure_stage(
    graph: &SparseDistanceMatrix,
    cocycle: &Cocycle,
    params: CircularCoordinateParams,
) -> std::result::Result<CircularCoordinate, CircularCoordinateFailure> {
    validate_params(params)?;
    validate_h1_cocycle(graph, cocycle)?;
    automatic_coordinate(graph, cocycle, params)
}

fn automatic_coordinate(
    graph: &SparseDistanceMatrix,
    cocycle: &Cocycle,
    params: CircularCoordinateParams,
) -> std::result::Result<CircularCoordinate, CircularCoordinateFailure> {
    if cocycle.modulus == 2 {
        return Err(Error::InvalidInput(
            "automatic circular lifting requires an odd prime; use modulus 47 or supply an integral lift"
                .into(),
        ).into());
    }
    let (field_multiplier, integral) =
        centered_integral_lift(graph, cocycle).map_err(CircularCoordinateFailure::Lift)?;
    build_coordinate(graph, cocycle, field_multiplier, integral, params)
}

/// Compute a circular coordinate from one source-bound persistent H1 basis class.
///
/// The class must carry interval provenance for the supplied graph. Automatic
/// lifting requires an odd prime; modulus two needs a caller-supplied lift.
pub fn circular_coordinate_for_class(
    graph: &SparseDistanceMatrix,
    class: &PersistentClass,
    params: CircularCoordinateParams,
) -> Result<CircularCoordinate> {
    class.validate_provenance(graph)?;
    circular_coordinate(graph, &class.cocycle, params)
}

/// Compute a coordinate from a caller-supplied checked integral lift.
///
/// The field multiplier is inferred from the first source term and checked on
/// every active edge.
pub fn circular_coordinate_with_integral_lift(
    graph: &SparseDistanceMatrix,
    cocycle: &Cocycle,
    integral: &[IntegralCocycleTerm],
    params: CircularCoordinateParams,
) -> Result<CircularCoordinate> {
    validate_params(params)?;
    validate_h1_cocycle(graph, cocycle)?;
    let field_multiplier = infer_field_multiplier(cocycle, integral)?;
    build_coordinate(graph, cocycle, field_multiplier, integral.to_vec(), params)
        .map_err(CircularCoordinateFailure::into_error)
}

/// Compute the harmonic part of a selected source without constructing a
/// canonical fixed-scale cohomology space.
pub(crate) fn selected_coordinate(
    graph: &SparseDistanceMatrix,
    cocycle: &Cocycle,
    integral: Option<&[IntegralCocycleTerm]>,
    params: CircularCoordinateParams,
) -> std::result::Result<SelectedCircularCoordinate, CircularCoordinateFailure> {
    validate_params(params)?;
    validate_h1_cocycle(graph, cocycle)?;
    let (field_multiplier, integral) = match integral {
        Some(integral) => {
            let multiplier = infer_field_multiplier(cocycle, integral)?;
            (multiplier, integral.to_vec())
        }
        None => {
            if cocycle.modulus == 2 {
                return Err(Error::InvalidInput(
                    "automatic selected circular lifting requires an odd prime; supply an integral lift"
                        .into(),
                )
                .into());
            }
            let (multiplier, integral) =
                centered_integral_lift(graph, cocycle).map_err(CircularCoordinateFailure::Lift)?;
            (multiplier, integral)
        }
    };
    build_selected_coordinate(graph, cocycle, field_multiplier, integral, params)
}

/// Continue a coordinate through the exact common-subcomplex relation.
///
/// The function computes a new coordinate only for a unique nonzero target.
pub fn continue_circular_coordinate(
    old_graph: &SparseDistanceMatrix,
    old_coordinate: &CircularCoordinate,
    new_graph: &SparseDistanceMatrix,
    params: CircularCoordinateParams,
) -> Result<CircularCoordinateContinuation> {
    validate_params(params)?;
    if old_graph.len() != new_graph.len() {
        return Err(Error::InvalidInput(
            "circular continuation requires one labeled vertex set".into(),
        ));
    }
    let old = cohomology_space(
        old_graph,
        1,
        old_coordinate.scale,
        old_coordinate.modulus,
        params.cohomology,
    )?;
    if old.id() != old_coordinate.space {
        return Err(Error::InvalidInput(
            "circular coordinate belongs to a different old graph".into(),
        ));
    }
    let new = cohomology_space(
        new_graph,
        1,
        old_coordinate.scale,
        old_coordinate.modulus,
        params.cohomology,
    )?;
    let relation = cohomology_relation(old_graph, &old, new_graph, &new, params.cohomology)?;
    let selected = old_coordinate
        .class
        .iter()
        .map(|term| (term.basis_index, term.coefficient))
        .collect::<Vec<_>>();
    let topology = cohomology_continuation(&old, &new, &relation, &selected)?;
    let coordinate = coordinate_from_continuation(new_graph, &new, &topology, params)?;
    Ok(CircularCoordinateContinuation {
        topology,
        coordinate,
    })
}

fn coordinate_from_continuation(
    new_graph: &SparseDistanceMatrix,
    new: &CohomologySpace,
    topology: &CohomologyContinuation,
    params: CircularCoordinateParams,
) -> Result<Option<CircularCoordinate>> {
    if topology.kind != CohomologyContinuationKind::Unique {
        return Ok(None);
    }
    let positions = new
        .basis()
        .iter()
        .enumerate()
        .map(|(position, class)| (class.id, position))
        .collect::<BTreeMap<_, _>>();
    let target = topology
        .new
        .iter()
        .map(|term| (positions[&term.class], term.coefficient))
        .collect::<Vec<_>>();
    let terms = new.cocycle_from_coordinates(&target)?;
    let cocycle = Cocycle {
        modulus: new.modulus(),
        scale: new.scale(),
        terms: terms
            .into_iter()
            .map(|term| CocycleTerm {
                u: term.simplex[0],
                v: term.simplex[1],
                coefficient: term.coefficient,
            })
            .collect(),
    };
    Ok(Some(circular_coordinate_from_checked_cocycle(
        new_graph, &cocycle, params,
    )?))
}

pub(super) fn build_coordinate(
    graph: &SparseDistanceMatrix,
    cocycle: &Cocycle,
    field_multiplier: u32,
    integral: Vec<IntegralCocycleTerm>,
    params: CircularCoordinateParams,
) -> std::result::Result<CircularCoordinate, CircularCoordinateFailure> {
    check_integral_lift(graph, cocycle, field_multiplier, &integral)?;
    let space = cohomology_space(graph, 1, cocycle.scale, cocycle.modulus, params.cohomology)?;
    let source_terms = cocycle
        .terms
        .iter()
        .map(|term| CochainTerm {
            simplex: vec![term.u, term.v],
            coefficient: term.coefficient,
        })
        .collect::<Vec<_>>();
    let class = space.coordinates_of_cocycle(&source_terms)?;
    if class.is_empty() {
        return Err(Error::InvalidInput(
            "circular coordinate needs a nonzero cohomology class".into(),
        )
        .into());
    }
    let selected = solve_selected_coordinate(graph, cocycle, field_multiplier, integral, params)?;
    Ok(CircularCoordinate {
        space: space.id(),
        modulus: cocycle.modulus,
        scale: cocycle.scale,
        field_multiplier,
        class: class
            .into_iter()
            .map(|(basis_index, coefficient)| CircularClassTerm {
                basis_index,
                coefficient,
            })
            .collect(),
        source: cocycle.terms.clone(),
        integral: selected.integral,
        divisibility: selected.divisibility,
        potential: selected.potential,
        phase: selected.phase,
        energy: selected.energy,
        max_residual: selected.max_residual,
        relative_residual: selected.relative_residual,
        iterations: selected.iterations,
        tolerance: params.tolerance,
    })
}

pub(crate) fn build_selected_coordinate(
    graph: &SparseDistanceMatrix,
    cocycle: &Cocycle,
    field_multiplier: u32,
    integral: Vec<IntegralCocycleTerm>,
    params: CircularCoordinateParams,
) -> std::result::Result<SelectedCircularCoordinate, CircularCoordinateFailure> {
    check_integral_lift(graph, cocycle, field_multiplier, &integral)?;
    solve_selected_coordinate(graph, cocycle, field_multiplier, integral, params)
}

fn solve_selected_coordinate(
    graph: &SparseDistanceMatrix,
    cocycle: &Cocycle,
    field_multiplier: u32,
    integral: Vec<IntegralCocycleTerm>,
    params: CircularCoordinateParams,
) -> std::result::Result<SelectedCircularCoordinate, CircularCoordinateFailure> {
    let active = active_edges(graph, cocycle.scale);
    let integral_map = integral
        .iter()
        .map(|term| ((term.u, term.v), term.coefficient))
        .collect::<BTreeMap<_, _>>();
    let divisibility = integral_divisibility(graph.len(), &active, &integral_map)?;
    let solve = harmonic_potential(
        graph.len(),
        &active,
        &integral_map,
        params.tolerance,
        params.max_iterations,
    )
    .map_err(CircularCoordinateFailure::Solve)?;
    let phase = solve
        .potential
        .iter()
        .map(|&value| canonical_phase(value))
        .collect();
    Ok(SelectedCircularCoordinate {
        modulus: cocycle.modulus,
        scale: cocycle.scale,
        field_multiplier,
        integral,
        divisibility,
        potential: solve.potential,
        phase,
        energy: solve.energy,
        max_residual: solve.max_residual,
        relative_residual: solve.relative_residual,
        iterations: solve.iterations,
        tolerance: params.tolerance,
    })
}

pub(crate) fn validate_params(params: CircularCoordinateParams) -> Result<()> {
    if !params.tolerance.is_finite() || params.tolerance <= 0.0 {
        return Err(Error::InvalidInput(
            "circular residual tolerance must be finite and positive".into(),
        ));
    }
    if params.max_iterations == 0 {
        return Err(Error::InvalidInput(
            "circular maximum iteration count must be positive".into(),
        ));
    }
    Ok(())
}
