use std::collections::{BTreeMap, VecDeque};

use crate::{Error, Result};

fn active_adjacency(vertex_count: usize, edges: &[(usize, usize)]) -> Vec<Vec<usize>> {
    let mut adjacency = vec![Vec::new(); vertex_count];
    for &(u, v) in edges {
        adjacency[u].push(v);
        adjacency[v].push(u);
    }
    for neighbors in &mut adjacency {
        neighbors.sort_unstable();
    }
    adjacency
}

pub(super) fn check_integer_triangle_closure(
    vertex_count: usize,
    edges: &[(usize, usize)],
    coefficients: &BTreeMap<(usize, usize), i64>,
) -> Result<()> {
    let adjacency = active_adjacency(vertex_count, edges);
    for u in 0..vertex_count {
        for &v in adjacency[u].iter().filter(|&&v| v > u) {
            for &w in adjacency[v].iter().filter(|&&w| w > v) {
                if adjacency[u].binary_search(&w).is_ok() {
                    let boundary = coefficient(coefficients, u, v)
                        + coefficient(coefficients, v, w)
                        - coefficient(coefficients, u, w);
                    if boundary != 0 {
                        return Err(Error::InvalidInput(format!(
                            "integral cocycle is not closed on triangle ({u}, {v}, {w})"
                        )));
                    }
                }
            }
        }
    }
    Ok(())
}

pub(super) fn coefficient(coefficients: &BTreeMap<(usize, usize), i64>, u: usize, v: usize) -> i64 {
    if u < v {
        coefficients.get(&(u, v)).copied().unwrap_or(0)
    } else {
        -coefficients.get(&(v, u)).copied().unwrap_or(0)
    }
}

pub(super) fn integral_divisibility(
    vertex_count: usize,
    edges: &[(usize, usize)],
    coefficients: &BTreeMap<(usize, usize), i64>,
) -> Result<u64> {
    let adjacency = active_adjacency(vertex_count, edges);
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
                    potential[v] = Some(
                        base.checked_sub(coefficient(coefficients, u, v))
                            .ok_or_else(|| {
                                Error::InvalidInput("integral cocycle potential overflows".into())
                            })?,
                    );
                    queue.push_back(v);
                }
            }
        }
    }
    let mut divisor = 0u64;
    for &(u, v) in edges {
        let adjusted = coefficient(coefficients, u, v)
            .checked_add(potential[v].unwrap_or(0))
            .and_then(|value| value.checked_sub(potential[u].unwrap_or(0)))
            .ok_or_else(|| Error::InvalidInput("integral cocycle period overflows".into()))?;
        divisor = gcd(divisor, adjusted.unsigned_abs());
    }
    if divisor == 0 {
        return Err(Error::InvalidInput(
            "integral cocycle represents the zero integer class".into(),
        ));
    }
    Ok(divisor)
}

pub(super) struct HarmonicSolve {
    pub(super) potential: Vec<f64>,
    pub(super) energy: f64,
    pub(super) max_residual: f64,
    pub(super) relative_residual: f64,
    pub(super) iterations: usize,
}

pub(super) fn harmonic_potential(
    vertex_count: usize,
    edges: &[(usize, usize)],
    coefficients: &BTreeMap<(usize, usize), i64>,
    tolerance: f64,
    max_iterations: usize,
) -> Result<HarmonicSolve> {
    let adjacency = active_adjacency(vertex_count, edges);
    let roots = component_roots(&adjacency);
    let right = harmonic_rhs(vertex_count, edges, coefficients, &roots);
    let residual_scale = max_abs(&right).max(1.0);
    let mut potential = vec![0.0; vertex_count];
    let mut residual = right.clone();
    let mut direction = residual.clone();
    let mut squared = dot(&residual, &residual);
    let mut iterations = 0usize;
    while max_abs(&residual) / residual_scale > tolerance && iterations < max_iterations {
        conjugate_gradient_step(
            &mut potential,
            &mut residual,
            &mut direction,
            &mut squared,
            edges,
            &roots,
        )?;
        iterations += 1;
    }
    let (energy, checked_residual) = harmonic_claim(edges, coefficients, &potential, &roots);
    let max_residual = max_abs(&checked_residual);
    let relative_residual = max_residual / residual_scale;
    if !energy.is_finite() || relative_residual > tolerance {
        return Err(Error::InvalidInput(format!(
            "circular relative harmonic residual {relative_residual} exceeds tolerance {tolerance} after {iterations} iterations"
        )));
    }
    Ok(HarmonicSolve {
        potential,
        energy,
        max_residual,
        relative_residual,
        iterations,
    })
}

fn harmonic_rhs(
    vertex_count: usize,
    edges: &[(usize, usize)],
    coefficients: &BTreeMap<(usize, usize), i64>,
    roots: &[usize],
) -> Vec<f64> {
    let mut right = vec![0.0; vertex_count];
    for &(u, v) in edges {
        let value = coefficient(coefficients, u, v) as f64;
        right[u] += value;
        right[v] -= value;
    }
    for &root in roots {
        right[root] = 0.0;
    }
    right
}

fn conjugate_gradient_step(
    potential: &mut [f64],
    residual: &mut [f64],
    direction: &mut [f64],
    squared: &mut f64,
    edges: &[(usize, usize)],
    roots: &[usize],
) -> Result<()> {
    let applied = laplacian(direction, edges, roots);
    let denominator = dot(direction, &applied);
    if !denominator.is_finite() || denominator <= 0.0 {
        return Err(Error::InvalidInput(
            "circular harmonic solve lost positive definiteness".into(),
        ));
    }
    let step = *squared / denominator;
    for position in 0..potential.len() {
        potential[position] += step * direction[position];
        residual[position] -= step * applied[position];
    }
    let next_squared = dot(residual, residual);
    if !next_squared.is_finite() {
        return Err(Error::InvalidInput(
            "circular harmonic solve produced a non-finite residual".into(),
        ));
    }
    let ratio = if *squared == 0.0 {
        0.0
    } else {
        next_squared / *squared
    };
    for position in 0..direction.len() {
        direction[position] = residual[position] + ratio * direction[position];
    }
    *squared = next_squared;
    Ok(())
}

fn component_roots(adjacency: &[Vec<usize>]) -> Vec<usize> {
    let mut seen = vec![false; adjacency.len()];
    let mut roots = Vec::new();
    let mut queue = VecDeque::new();
    for root in 0..adjacency.len() {
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

fn laplacian(values: &[f64], edges: &[(usize, usize)], roots: &[usize]) -> Vec<f64> {
    let mut output = vec![0.0; values.len()];
    for &(u, v) in edges {
        let difference = values[u] - values[v];
        output[u] += difference;
        output[v] -= difference;
    }
    for &root in roots {
        output[root] = 0.0;
    }
    output
}

fn harmonic_claim(
    edges: &[(usize, usize)],
    coefficients: &BTreeMap<(usize, usize), i64>,
    potential: &[f64],
    roots: &[usize],
) -> (f64, Vec<f64>) {
    let mut energy = 0.0;
    let mut residual = vec![0.0; potential.len()];
    for &(u, v) in edges {
        let harmonic = coefficient(coefficients, u, v) as f64 + potential[v] - potential[u];
        energy += harmonic * harmonic;
        residual[u] -= harmonic;
        residual[v] += harmonic;
    }
    for &root in roots {
        residual[root] = 0.0;
    }
    (energy, residual)
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}

fn max_abs(values: &[f64]) -> f64 {
    values.iter().map(|value| value.abs()).fold(0.0, f64::max)
}

pub(super) fn canonical_phase(value: f64) -> f64 {
    let phase = value.rem_euclid(1.0);
    if phase == 0.0 { 0.0 } else { phase }
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let remainder = a % b;
        a = b;
        b = remainder;
    }
    a
}
