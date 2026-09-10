use std::collections::VecDeque;

use rustc_hash::FxHashMap;

use crate::field::{MODULUS_LIMIT, is_prime};
use crate::{Error, Result, SparseDistanceMatrix};

use super::model::{Cocycle, CocycleTerm};

/// Check that an H1 cocycle is canonical, closed, and nontrivial at its
/// declared scale.
pub fn validate_h1_cocycle(matrix: &SparseDistanceMatrix, cocycle: &Cocycle) -> Result<()> {
    let modulus = cocycle.modulus as u64;
    check_cocycle_header(cocycle, modulus)?;
    let coefficients = checked_cocycle_terms(matrix, cocycle)?;
    let adjacency = active_adjacency(matrix, cocycle.scale);
    check_triangle_closure(&adjacency, &coefficients, modulus)?;
    if is_vertex_coboundary(&adjacency, &coefficients, modulus) {
        return Err(Error::InvalidInput(
            "cocycle is a vertex coboundary at its scale".into(),
        ));
    }
    Ok(())
}

fn check_cocycle_header(cocycle: &Cocycle, modulus: u64) -> Result<()> {
    if !is_prime(modulus) || modulus >= MODULUS_LIMIT {
        return Err(Error::InvalidInput(format!(
            "modulus must be a prime below {MODULUS_LIMIT}, got {modulus}"
        )));
    }
    if cocycle.scale.is_nan() || cocycle.scale < 0.0 || !cocycle.scale.is_finite() {
        return Err(Error::InvalidInput(format!(
            "cocycle scale must be finite and non-negative, got {}",
            cocycle.scale
        )));
    }
    if cocycle.terms.is_empty() {
        return Err(Error::InvalidInput("cocycle has no terms".into()));
    }
    if cocycle.terms[0].coefficient != 1 {
        return Err(Error::InvalidInput(
            "cocycle first coefficient must be one".into(),
        ));
    }
    Ok(())
}

fn checked_cocycle_terms(
    matrix: &SparseDistanceMatrix,
    cocycle: &Cocycle,
) -> Result<FxHashMap<(usize, usize), u64>> {
    let mut coefficients: FxHashMap<(usize, usize), u64> = FxHashMap::default();
    let mut previous = None;
    for (index, term) in cocycle.terms.iter().enumerate() {
        check_cocycle_term(matrix, cocycle, index, term, previous)?;
        coefficients.insert((term.u, term.v), term.coefficient as u64);
        previous = Some((term.u, term.v));
    }
    Ok(coefficients)
}

fn check_cocycle_term(
    matrix: &SparseDistanceMatrix,
    cocycle: &Cocycle,
    index: usize,
    term: &CocycleTerm,
    previous: Option<(usize, usize)>,
) -> Result<()> {
    if term.u >= term.v || term.v >= matrix.len() {
        return Err(Error::InvalidInput(format!(
            "cocycle term {index} has invalid edge ({}, {})",
            term.u, term.v
        )));
    }
    if term.coefficient == 0 || term.coefficient >= cocycle.modulus {
        return Err(Error::InvalidInput(format!(
            "cocycle term {index} has coefficient {} outside 1..{}",
            term.coefficient, cocycle.modulus
        )));
    }
    if previous.is_some_and(|edge| edge >= (term.u, term.v)) {
        return Err(Error::InvalidInput(
            "cocycle terms are not in strict endpoint order".into(),
        ));
    }
    let distance = matrix.get(term.u, term.v);
    if !distance.is_finite() || distance > cocycle.scale {
        return Err(Error::InvalidInput(format!(
            "cocycle term ({}, {}) is absent at scale {}",
            term.u, term.v, cocycle.scale
        )));
    }
    Ok(())
}

fn active_adjacency(matrix: &SparseDistanceMatrix, scale: f64) -> Vec<Vec<usize>> {
    let mut adjacency = vec![Vec::new(); matrix.len()];
    for (u, v, _) in matrix.edges().filter(|&(_, _, value)| value <= scale) {
        adjacency[u].push(v);
        adjacency[v].push(u);
    }
    adjacency
}

fn check_triangle_closure(
    adjacency: &[Vec<usize>],
    coefficients: &FxHashMap<(usize, usize), u64>,
    modulus: u64,
) -> Result<()> {
    for u in 0..adjacency.len() {
        for &v in adjacency[u].iter().filter(|&&v| v > u) {
            check_edge_triangles(adjacency, coefficients, modulus, u, v)?;
        }
    }
    Ok(())
}

fn check_edge_triangles(
    adjacency: &[Vec<usize>],
    coefficients: &FxHashMap<(usize, usize), u64>,
    modulus: u64,
    u: usize,
    v: usize,
) -> Result<()> {
    let mut a = adjacency[u].partition_point(|&w| w <= v);
    let mut b = adjacency[v].partition_point(|&w| w <= v);
    while a < adjacency[u].len() && b < adjacency[v].len() {
        match adjacency[u][a].cmp(&adjacency[v][b]) {
            std::cmp::Ordering::Less => a += 1,
            std::cmp::Ordering::Greater => b += 1,
            std::cmp::Ordering::Equal => {
                check_triangle(coefficients, modulus, u, v, adjacency[u][a])?;
                a += 1;
                b += 1;
            }
        }
    }
    Ok(())
}

fn check_triangle(
    coefficients: &FxHashMap<(usize, usize), u64>,
    modulus: u64,
    u: usize,
    v: usize,
    w: usize,
) -> Result<()> {
    let uv = coefficient(coefficients, u, v);
    let uw = coefficient(coefficients, u, w);
    let vw = coefficient(coefficients, v, w);
    if (uv + vw + modulus - uw) % modulus != 0 {
        return Err(Error::InvalidInput(format!(
            "cocycle is not closed on triangle ({u}, {v}, {w})"
        )));
    }
    Ok(())
}

pub(super) fn coefficient(
    coefficients: &FxHashMap<(usize, usize), u64>,
    u: usize,
    v: usize,
) -> u64 {
    *coefficients.get(&(u, v)).unwrap_or(&0)
}

pub(super) fn oriented_coefficient(
    coefficients: &FxHashMap<(usize, usize), u64>,
    from: usize,
    to: usize,
    modulus: u64,
) -> u64 {
    if from < to {
        coefficient(coefficients, from, to)
    } else {
        let value = coefficient(coefficients, to, from);
        if value == 0 { 0 } else { modulus - value }
    }
}

fn is_vertex_coboundary(
    adjacency: &[Vec<usize>],
    coefficients: &FxHashMap<(usize, usize), u64>,
    modulus: u64,
) -> bool {
    let mut potential = vec![None; adjacency.len()];
    let mut queue = VecDeque::new();
    for root in 0..adjacency.len() {
        if potential[root].is_some() {
            continue;
        }
        potential[root] = Some(0u64);
        queue.push_back(root);
        while let Some(u) = queue.pop_front() {
            let base = potential[u].expect("queued vertex has a potential");
            for &v in &adjacency[u] {
                let expected = (base + oriented_coefficient(coefficients, u, v, modulus)) % modulus;
                match potential[v] {
                    None => {
                        potential[v] = Some(expected);
                        queue.push_back(v);
                    }
                    Some(value) if value != expected => return false,
                    Some(_) => {}
                }
            }
        }
    }
    true
}
