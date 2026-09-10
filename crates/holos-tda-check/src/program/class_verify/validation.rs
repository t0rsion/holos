use std::collections::{BTreeMap, VecDeque};

use crate::proof::{Graph, ProofBar, ProofError};

use super::super::claim::{ClassClaim, ProvenanceClaim, SimplexClaim};
use super::super::model::ProgramProofLimits;
use super::identity::{bar_bits_equal, source_graph_digest, valid_interval_and_scale};

pub(super) fn check_provenance(
    provenance: &ProvenanceClaim,
    class: &ClassClaim,
    graph: &Graph,
    interval: ProofBar,
    modulus: u32,
) -> Result<(), ProofError> {
    if provenance.class_digest != class.id {
        return Err(ProofError::new(
            "atlas class identity differs from its provenance",
        ));
    }
    if !bar_bits_equal(provenance.interval, interval) {
        return Err(ProofError::new(
            "atlas class interval differs from its provenance",
        ));
    }
    if provenance.modulus != modulus {
        return Err(ProofError::new(
            "atlas class modulus differs from its provenance",
        ));
    }
    if provenance.scale.to_bits() != class.scale.to_bits() {
        return Err(ProofError::new(
            "atlas class scale differs from its provenance",
        ));
    }
    if !valid_interval_and_scale(provenance.interval, provenance.scale) {
        return Err(ProofError::new(
            "atlas class representative is outside its interval",
        ));
    }
    if provenance.source_graph_digest != source_graph_digest(graph, provenance.scale) {
        return Err(ProofError::new(
            "atlas class belongs to a different active graph",
        ));
    }
    Ok(())
}

pub(super) fn check_cocycle_shape(
    class: &ClassClaim,
    graph: &Graph,
    modulus: u32,
    limits: ProgramProofLimits,
) -> Result<(), ProofError> {
    check_cocycle_header(class, limits)?;
    let mut coefficients = BTreeMap::new();
    check_cocycle_terms(class, graph, modulus, &mut coefficients)?;
    let adjacency = active_adjacency(graph, class.scale);
    check_triangle_closure(&adjacency, &coefficients, modulus as u64)?;
    if is_vertex_coboundary(&adjacency, &coefficients, modulus as u64) {
        return Err(ProofError::new("atlas cocycle is a vertex coboundary"));
    }
    Ok(())
}

fn check_cocycle_header(class: &ClassClaim, limits: ProgramProofLimits) -> Result<(), ProofError> {
    if !class.scale.is_finite()
        || class.scale < 0.0
        || is_negative_zero(class.scale)
        || class.terms.is_empty()
    {
        return Err(ProofError::new(
            "atlas cocycle has an invalid scale or no terms",
        ));
    }
    if class.terms[0].coefficient != 1 {
        return Err(ProofError::new("atlas cocycle is not normalized"));
    }
    if class.terms.len() > limits.max_cocycle_terms {
        return Err(ProofError::new("atlas cocycle exceeds the term limit"));
    }
    Ok(())
}

fn check_cocycle_terms(
    class: &ClassClaim,
    graph: &Graph,
    modulus: u32,
    coefficients: &mut BTreeMap<(usize, usize), u64>,
) -> Result<(), ProofError> {
    let mut previous = None;
    for term in &class.terms {
        if !valid_cocycle_term(term, graph, class.scale, modulus, previous) {
            return Err(ProofError::new("atlas cocycle terms are not canonical"));
        }
        coefficients.insert((term.u, term.v), term.coefficient as u64);
        previous = Some((term.u, term.v));
    }
    Ok(())
}

fn valid_cocycle_term(
    term: &super::super::claim::CocycleTermClaim,
    graph: &Graph,
    scale: f64,
    modulus: u32,
    previous: Option<(usize, usize)>,
) -> bool {
    term.u < term.v
        && term.v < graph.vertex_count
        && term.coefficient != 0
        && term.coefficient < modulus
        && previous.is_none_or(|previous| previous < (term.u, term.v))
        && graph.get(term.u, term.v) <= scale
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
                                "atlas cocycle is not closed on triangle ({u}, {v}, {w})"
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

pub(super) fn check_simplex(
    simplex: &SimplexClaim,
    size: usize,
    graph: &Graph,
) -> Result<(), ProofError> {
    if simplex.vertices.len() != size
        || !simplex.vertices.windows(2).all(|pair| pair[0] < pair[1])
        || simplex
            .vertices
            .iter()
            .any(|&vertex| vertex >= graph.vertex_count)
        || !simplex.value.is_finite()
        || simplex.value < 0.0
    {
        return Err(ProofError::new("atlas critical simplex is not canonical"));
    }
    let mut value = 0.0f64;
    for right in 1..simplex.vertices.len() {
        for left in 0..right {
            let edge = graph.get(simplex.vertices[left], simplex.vertices[right]);
            if !edge.is_finite() {
                return Err(ProofError::new(
                    "atlas critical simplex contains an absent edge",
                ));
            }
            value = value.max(edge);
        }
    }
    if value.to_bits() != simplex.value.to_bits() {
        return Err(ProofError::new(
            "atlas critical simplex value differs from its graph",
        ));
    }
    Ok(())
}

pub(super) fn check_bar(bar: &ProofBar) -> Result<(), ProofError> {
    if bar.dimension > 1 || invalid_birth(bar.birth) || invalid_death(bar.death) {
        return Err(ProofError::new("atlas interval is invalid"));
    }
    if bar.death <= bar.birth {
        return Err(ProofError::new("atlas interval is invalid"));
    }
    Ok(())
}

fn invalid_birth(value: f64) -> bool {
    !value.is_finite() || value < 0.0 || is_negative_zero(value)
}

fn invalid_death(value: f64) -> bool {
    value.is_nan()
        || value < 0.0
        || is_negative_zero(value)
        || (value.is_infinite() && value.is_sign_negative())
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

fn is_negative_zero(value: f64) -> bool {
    value == 0.0 && value.to_bits() != 0
}
