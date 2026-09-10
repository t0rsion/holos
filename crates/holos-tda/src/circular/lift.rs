use std::collections::BTreeMap;

use crate::classes::Cocycle;
use crate::{Error, Result, SparseDistanceMatrix};

use super::harmonic::check_integer_triangle_closure;
use super::model::IntegralCocycleTerm;

const INTEGRAL_COEFFICIENT_LIMIT: i64 = 1i64 << 31;

pub(super) fn centered_integral_lift(
    graph: &SparseDistanceMatrix,
    cocycle: &Cocycle,
) -> Result<(u32, Vec<IntegralCocycleTerm>)> {
    let modulus = u64::from(cocycle.modulus);
    for multiplier in 1..modulus {
        let terms = cocycle
            .terms
            .iter()
            .filter_map(|term| {
                let residue = u64::from(term.coefficient) * multiplier % modulus;
                let centered = if residue > modulus / 2 {
                    residue as i64 - modulus as i64
                } else {
                    residue as i64
                };
                (centered != 0).then_some(IntegralCocycleTerm {
                    u: term.u,
                    v: term.v,
                    coefficient: centered,
                })
            })
            .collect::<Vec<_>>();
        if check_integral_lift(graph, cocycle, multiplier as u32, &terms).is_ok() {
            return Ok((multiplier as u32, terms));
        }
    }
    Err(Error::InvalidInput(
        "no centered scalar integral lift is closed; supply a checked integral lift".into(),
    ))
}

pub(super) fn check_integral_lift(
    graph: &SparseDistanceMatrix,
    cocycle: &Cocycle,
    field_multiplier: u32,
    terms: &[IntegralCocycleTerm],
) -> Result<()> {
    if field_multiplier == 0 || field_multiplier >= cocycle.modulus {
        return Err(Error::InvalidInput(
            "circular field multiplier must be nonzero and below the modulus".into(),
        ));
    }
    if terms.is_empty()
        || terms
            .windows(2)
            .any(|pair| (pair[0].u, pair[0].v) >= (pair[1].u, pair[1].v))
    {
        return Err(Error::InvalidInput(
            "integral cocycle terms are empty or not canonical".into(),
        ));
    }
    let active = active_edges(graph, cocycle.scale);
    let active_set = active
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    if terms.iter().any(|term| {
        term.u >= term.v
            || term.v >= graph.len()
            || term.coefficient == 0
            || term.coefficient.unsigned_abs() > INTEGRAL_COEFFICIENT_LIMIT as u64
            || !active_set.contains(&(term.u, term.v))
    }) {
        return Err(Error::InvalidInput(
            "integral cocycle has an invalid active-edge term".into(),
        ));
    }
    let integral = terms
        .iter()
        .map(|term| ((term.u, term.v), term.coefficient))
        .collect::<BTreeMap<_, _>>();
    check_integer_triangle_closure(graph.len(), &active, &integral)?;
    let modulus = i64::from(cocycle.modulus);
    let source = cocycle
        .terms
        .iter()
        .map(|term| ((term.u, term.v), i64::from(term.coefficient)))
        .collect::<BTreeMap<_, _>>();
    for &(u, v) in &active {
        let actual = integral
            .get(&(u, v))
            .copied()
            .unwrap_or(0)
            .rem_euclid(modulus);
        let expected = (source.get(&(u, v)).copied().unwrap_or(0) * i64::from(field_multiplier))
            .rem_euclid(modulus);
        if actual != expected {
            return Err(Error::InvalidInput(
                "integral cocycle does not reduce to the multiplied field cocycle".into(),
            ));
        }
    }
    Ok(())
}

pub(super) fn infer_field_multiplier(
    cocycle: &Cocycle,
    terms: &[IntegralCocycleTerm],
) -> Result<u32> {
    let first = cocycle
        .terms
        .first()
        .expect("a validated cocycle has a term");
    let integral = terms
        .iter()
        .find(|term| (term.u, term.v) == (first.u, first.v))
        .map(|term| term.coefficient)
        .unwrap_or(0);
    let modulus = i64::from(cocycle.modulus);
    let residue = integral.rem_euclid(modulus) as u64;
    if residue == 0 {
        return Err(Error::InvalidInput(
            "integral cocycle has no nonzero field multiplier".into(),
        ));
    }
    Ok(
        (residue * inverse_mod(u64::from(first.coefficient), u64::from(cocycle.modulus))
            % u64::from(cocycle.modulus)) as u32,
    )
}

pub(super) fn active_edges(graph: &SparseDistanceMatrix, scale: f64) -> Vec<(usize, usize)> {
    graph
        .edges()
        .filter(|&(_, _, value)| value <= scale)
        .map(|(u, v, _)| (u, v))
        .collect()
}

pub(super) fn inverse_mod(value: u64, modulus: u64) -> u64 {
    let mut result = 1u64;
    let mut base = value;
    let mut exponent = modulus - 2;
    while exponent > 0 {
        if exponent & 1 == 1 {
            result = result * base % modulus;
        }
        base = base * base % modulus;
        exponent >>= 1;
    }
    result
}
