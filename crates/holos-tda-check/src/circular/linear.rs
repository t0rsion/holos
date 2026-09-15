use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::ProofError;
use crate::cohomology::Edge;

pub(super) fn adjacency(vertex_count: usize, edges: &[Edge]) -> Vec<BTreeSet<usize>> {
    let mut adjacency = vec![BTreeSet::new(); vertex_count];
    for edge in edges {
        adjacency[edge.u].insert(edge.v);
        adjacency[edge.v].insert(edge.u);
    }
    adjacency
}

pub(super) fn check_field_triangle_closure(
    vertex_count: usize,
    edges: &[Edge],
    terms: &[(Edge, u32)],
    modulus: u32,
) -> Result<(), ProofError> {
    if modulus < 2 {
        return Err(ProofError::new("circular field modulus is invalid"));
    }
    let coefficients = terms.iter().copied().collect::<BTreeMap<_, _>>();
    let adjacency = adjacency(vertex_count, edges);
    for u in 0..vertex_count {
        for &v in adjacency[u].range((std::ops::Bound::Excluded(u), std::ops::Bound::Unbounded)) {
            for &w in adjacency[v].range((std::ops::Bound::Excluded(v), std::ops::Bound::Unbounded))
            {
                if adjacency[u].contains(&w) {
                    let boundary = (u64::from(field_coefficient(&coefficients, u, v, modulus)?)
                        + u64::from(field_coefficient(&coefficients, v, w, modulus)?)
                        + u64::from(modulus)
                        - u64::from(field_coefficient(&coefficients, u, w, modulus)?))
                        % u64::from(modulus);
                    if boundary != 0 {
                        return Err(ProofError::new(
                            "circular source is not closed on an active triangle",
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn check_integer_triangle_closure(
    vertex_count: usize,
    edges: &[Edge],
    coefficients: &BTreeMap<Edge, i64>,
) -> Result<(), ProofError> {
    let adjacency = adjacency(vertex_count, edges);
    for u in 0..vertex_count {
        for &v in adjacency[u].range((std::ops::Bound::Excluded(u), std::ops::Bound::Unbounded)) {
            for &w in adjacency[v].range((std::ops::Bound::Excluded(v), std::ops::Bound::Unbounded))
            {
                if adjacency[u].contains(&w)
                    && integer_triangle_boundary(coefficients, u, v, w)? != 0
                {
                    return Err(ProofError::new(
                        "circular integer lift is not closed on an active triangle",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn field_coefficient(
    coefficients: &BTreeMap<Edge, u32>,
    u: usize,
    v: usize,
    modulus: u32,
) -> Result<u32, ProofError> {
    let value = coefficients
        .get(&Edge {
            u: u.min(v),
            v: u.max(v),
        })
        .copied()
        .unwrap_or(0);
    if value >= modulus {
        return Err(ProofError::new(
            "circular field coefficient is outside its modulus",
        ));
    }
    if u < v {
        Ok(value)
    } else if value == 0 {
        Ok(0)
    } else {
        Ok(modulus - value)
    }
}

fn integer_coefficient(
    coefficients: &BTreeMap<Edge, i64>,
    u: usize,
    v: usize,
) -> Result<i64, ProofError> {
    if u < v {
        Ok(coefficients.get(&Edge { u, v }).copied().unwrap_or(0))
    } else {
        coefficients
            .get(&Edge { u: v, v: u })
            .copied()
            .unwrap_or(0)
            .checked_neg()
            .ok_or_else(|| ProofError::new("circular integer coefficient cannot be reversed"))
    }
}

fn integer_triangle_boundary(
    coefficients: &BTreeMap<Edge, i64>,
    u: usize,
    v: usize,
    w: usize,
) -> Result<i64, ProofError> {
    let uv = integer_coefficient(coefficients, u, v)?;
    let vw = integer_coefficient(coefficients, v, w)?;
    let uw = integer_coefficient(coefficients, u, w)?;
    uv.checked_add(vw)
        .and_then(|boundary| boundary.checked_sub(uw))
        .ok_or_else(|| ProofError::new("circular integer triangle boundary overflows"))
}

pub(crate) fn check_reduction(
    edges: &[Edge],
    source: &[(Edge, u32)],
    integral: &BTreeMap<Edge, i64>,
    multiplier: u32,
    modulus: u32,
) -> Result<(), ProofError> {
    let source = source.iter().copied().collect::<BTreeMap<_, _>>();
    if modulus < 2 {
        return Err(ProofError::new("circular field modulus is invalid"));
    }
    let modulus64 = i64::from(modulus);
    let modulus_u64 = u64::from(modulus);
    for &edge in edges {
        let actual = integral
            .get(&edge)
            .copied()
            .unwrap_or(0)
            .rem_euclid(modulus64);
        let expected = (u64::from(source.get(&edge).copied().unwrap_or(0)) * u64::from(multiplier))
            % modulus_u64;
        if actual as u64 != expected {
            return Err(ProofError::new(
                "circular integer lift has the wrong field reduction",
            ));
        }
    }
    Ok(())
}

pub(crate) fn component_roots(vertex_count: usize, edges: &[Edge]) -> Vec<usize> {
    let adjacency = adjacency(vertex_count, edges);
    let mut seen = vec![false; vertex_count];
    let mut queue = VecDeque::new();
    let mut roots = Vec::new();
    for root in 0..vertex_count {
        if seen[root] {
            continue;
        }
        roots.push(root);
        seen[root] = true;
        queue.push_back(root);
        while let Some(vertex) = queue.pop_front() {
            for &neighbor in &adjacency[vertex] {
                if !seen[neighbor] {
                    seen[neighbor] = true;
                    queue.push_back(neighbor);
                }
            }
        }
    }
    roots
}

pub(crate) fn integral_divisibility(
    vertex_count: usize,
    edges: &[Edge],
    coefficients: &BTreeMap<Edge, i64>,
) -> Result<u64, ProofError> {
    let potential = integral_potentials(vertex_count, edges, coefficients)?;
    let divisor = integral_period_divisor(edges, coefficients, &potential)?;
    if divisor == 0 {
        Err(ProofError::new(
            "circular integer cocycle is the zero integer class",
        ))
    } else {
        Ok(divisor)
    }
}

fn integral_potentials(
    vertex_count: usize,
    edges: &[Edge],
    coefficients: &BTreeMap<Edge, i64>,
) -> Result<Vec<Option<i64>>, ProofError> {
    let adjacency = adjacency(vertex_count, edges);
    let mut potential = vec![None; vertex_count];
    let mut queue = VecDeque::new();
    for root in 0..vertex_count {
        if potential[root].is_some() {
            continue;
        }
        potential[root] = Some(0i64);
        queue.push_back(root);
        while let Some(u) = queue.pop_front() {
            let base = potential[u].expect("queued vertex has a potential");
            for &v in &adjacency[u] {
                if potential[v].is_none() {
                    let coefficient = integer_coefficient(coefficients, u, v)?;
                    potential[v] =
                        Some(base.checked_sub(coefficient).ok_or_else(|| {
                            ProofError::new("circular integer potential overflows")
                        })?);
                    queue.push_back(v);
                }
            }
        }
    }
    Ok(potential)
}

fn integral_period_divisor(
    edges: &[Edge],
    coefficients: &BTreeMap<Edge, i64>,
    potential: &[Option<i64>],
) -> Result<u64, ProofError> {
    let mut divisor = 0u64;
    for edge in edges {
        let coefficient = integer_coefficient(coefficients, edge.u, edge.v)?;
        let adjusted = coefficient
            .checked_add(potential[edge.v].unwrap_or(0))
            .and_then(|value| value.checked_sub(potential[edge.u].unwrap_or(0)))
            .ok_or_else(|| ProofError::new("circular integer period overflows"))?;
        divisor = gcd(divisor, adjusted.unsigned_abs());
    }
    Ok(divisor)
}

pub(crate) fn relative_residual(
    edges: &[Edge],
    coefficients: &BTreeMap<Edge, i64>,
    potential: &[f64],
    roots: &[usize],
) -> Result<f64, ProofError> {
    let mut right = vec![0.0; potential.len()];
    let mut residual = vec![0.0; potential.len()];
    let mut energy = 0.0;
    for edge in edges {
        let integral = integer_coefficient(coefficients, edge.u, edge.v)? as f64;
        right[edge.u] += integral;
        right[edge.v] -= integral;
        let harmonic = integral + potential[edge.v] - potential[edge.u];
        energy += harmonic * harmonic;
        residual[edge.u] -= harmonic;
        residual[edge.v] += harmonic;
    }
    for &root in roots {
        right[root] = 0.0;
        residual[root] = 0.0;
    }
    let scale = max_abs(&right).max(1.0);
    let relative = max_abs(&residual) / scale;
    if !energy.is_finite() || !relative.is_finite() {
        Err(ProofError::new("circular harmonic claim is not finite"))
    } else {
        Ok(relative)
    }
}

fn max_abs(values: &[f64]) -> f64 {
    values.iter().map(|value| value.abs()).fold(0.0, f64::max)
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let remainder = a % b;
        a = b;
        b = remainder;
    }
    a
}
