use std::collections::{BTreeMap, VecDeque};

use crate::ProofError;
use crate::proof::{Graph, ProofBar};

use super::claim::CocycleTermClaim;

pub(super) fn validate_cocycle(
    graph: &Graph,
    modulus: u32,
    scale: f64,
    terms: &[CocycleTermClaim],
) -> Result<(), ProofError> {
    check_cocycle_header(scale, terms)?;
    let mut coefficients = BTreeMap::new();
    check_cocycle_terms(graph, modulus, scale, terms, &mut coefficients)?;
    let adjacency = active_adjacency(graph, scale);
    check_triangle_closure(&adjacency, &coefficients, modulus as u64)?;
    if is_vertex_coboundary(&adjacency, &coefficients, modulus as u64) {
        return Err(ProofError::new("trace cocycle is a vertex coboundary"));
    }
    Ok(())
}

fn check_cocycle_header(scale: f64, terms: &[CocycleTermClaim]) -> Result<(), ProofError> {
    if !scale.is_finite() || scale < 0.0 || is_negative_zero(scale) || terms.is_empty() {
        return Err(ProofError::new(
            "trace cocycle has an invalid scale or no terms",
        ));
    }
    if terms[0].coefficient != 1 {
        return Err(ProofError::new("trace cocycle is not normalized"));
    }
    Ok(())
}

fn check_cocycle_terms(
    graph: &Graph,
    modulus: u32,
    scale: f64,
    terms: &[CocycleTermClaim],
    coefficients: &mut BTreeMap<(usize, usize), u64>,
) -> Result<(), ProofError> {
    let mut previous = None;
    for term in terms {
        if !valid_cocycle_term(term, graph, modulus, scale, previous) {
            return Err(ProofError::new("trace cocycle terms are not canonical"));
        }
        coefficients.insert((term.u, term.v), term.coefficient as u64);
        previous = Some((term.u, term.v));
    }
    Ok(())
}

fn valid_cocycle_term(
    term: &CocycleTermClaim,
    graph: &Graph,
    modulus: u32,
    scale: f64,
    previous: Option<(usize, usize)>,
) -> bool {
    term.u < term.v
        && term.v < graph.vertex_count
        && term.coefficient != 0
        && term.coefficient < modulus
        && previous.is_none_or(|previous| previous < (term.u, term.v))
        && graph.get(term.u, term.v) <= scale
}

pub(super) fn validate_space_basis(
    graph: &Graph,
    modulus: u32,
    interval: ProofBar,
    basis: &[Vec<CocycleTermClaim>],
) -> Result<(), ProofError> {
    let scale = if interval.death.is_finite() {
        f64::from_bits(interval.death.to_bits() - 1)
    } else {
        graph
            .edges
            .iter()
            .map(|edge| edge.value)
            .fold(0.0, f64::max)
    };
    for terms in basis {
        validate_cocycle(graph, modulus, scale, terms)?;
    }
    Ok(())
}

fn active_adjacency(graph: &Graph, scale: f64) -> Vec<Vec<usize>> {
    let mut adjacency = vec![Vec::new(); graph.vertex_count];
    for edge in graph.edges.iter().filter(|edge| edge.value <= scale) {
        adjacency[edge.u].push(edge.v);
        adjacency[edge.v].push(edge.u);
    }
    for neighbors in &mut adjacency {
        neighbors.sort_unstable();
    }
    adjacency
}

fn check_triangle_closure(
    adjacency: &[Vec<usize>],
    coefficients: &BTreeMap<(usize, usize), u64>,
    modulus: u64,
) -> Result<(), ProofError> {
    for u in 0..adjacency.len() {
        for &v in adjacency[u].iter().filter(|&&v| v > u) {
            let mut left = adjacency[u].partition_point(|&w| w <= v);
            let mut right = adjacency[v].partition_point(|&w| w <= v);
            while left < adjacency[u].len() && right < adjacency[v].len() {
                match adjacency[u][left].cmp(&adjacency[v][right]) {
                    std::cmp::Ordering::Less => left += 1,
                    std::cmp::Ordering::Greater => right += 1,
                    std::cmp::Ordering::Equal => {
                        let w = adjacency[u][left];
                        let uv = coefficient(coefficients, u, v);
                        let uw = coefficient(coefficients, u, w);
                        let vw = coefficient(coefficients, v, w);
                        if (uv + vw + modulus - uw) % modulus != 0 {
                            return Err(ProofError::new(format!(
                                "trace cocycle is not closed on triangle ({u}, {v}, {w})"
                            )));
                        }
                        left += 1;
                        right += 1;
                    }
                }
            }
        }
    }
    Ok(())
}

fn coefficient(coefficients: &BTreeMap<(usize, usize), u64>, u: usize, v: usize) -> u64 {
    coefficients.get(&(u, v)).copied().unwrap_or(0)
}

fn oriented_coefficient(
    coefficients: &BTreeMap<(usize, usize), u64>,
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
    coefficients: &BTreeMap<(usize, usize), u64>,
    modulus: u64,
) -> bool {
    let mut potential = vec![None; adjacency.len()];
    let mut queue = VecDeque::new();
    for root in 0..adjacency.len() {
        if potential[root].is_some() {
            continue;
        }
        potential[root] = Some(0);
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

fn is_negative_zero(value: f64) -> bool {
    value == 0.0 && value.to_bits() != 0
}
