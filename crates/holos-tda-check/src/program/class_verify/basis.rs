use std::collections::{BTreeMap, VecDeque};

use crate::{
    inverse_mod,
    proof::{Graph, ProofError},
};

use super::super::claim::CocycleTermClaim;

pub(crate) fn canonical_basis(
    graph: &Graph,
    modulus: u32,
    scale: f64,
    cocycles: &[Vec<CocycleTermClaim>],
) -> Result<Vec<Vec<CocycleTermClaim>>, ProofError> {
    let active_edges: Vec<_> = graph
        .edges
        .iter()
        .filter(|edge| edge.value <= scale)
        .map(|edge| (edge.u, edge.v))
        .collect();
    let edge_index: BTreeMap<_, _> = active_edges
        .iter()
        .copied()
        .enumerate()
        .map(|(index, edge)| (edge, index))
        .collect();
    let adjacency = active_adjacency(graph, scale);
    let mut rows: BTreeMap<usize, BTreeMap<usize, u64>> = BTreeMap::new();
    for cocycle in cocycles {
        let coefficients: BTreeMap<_, _> = cocycle
            .iter()
            .map(|term| ((term.u, term.v), term.coefficient as u64))
            .collect();
        let potential = gauge_potential(&adjacency, &coefficients, modulus as u64);
        let mut row = BTreeMap::new();
        for &(u, v) in &active_edges {
            let original = coefficient(&coefficients, u, v);
            let adjusted =
                (original + potential[u] + modulus as u64 - potential[v]) % modulus as u64;
            if adjusted != 0 {
                row.insert(edge_index[&(u, v)], adjusted);
            }
        }
        for (&pivot, existing) in &rows {
            if let Some(&coefficient) = row.get(&pivot) {
                add_scaled_row(
                    &mut row,
                    existing,
                    modulus as u64 - coefficient,
                    modulus as u64,
                );
            }
        }
        let Some((&pivot, &coefficient)) = row.first_key_value() else {
            return Err(ProofError::new(
                "atlas class-space basis contains a coboundary",
            ));
        };
        let inverse = inverse_mod(coefficient, modulus as u64);
        scale_row(&mut row, inverse, modulus as u64);
        for existing in rows.values_mut() {
            if let Some(&factor) = existing.get(&pivot) {
                add_scaled_row(existing, &row, modulus as u64 - factor, modulus as u64);
            }
        }
        rows.insert(pivot, row);
    }
    Ok(rows
        .into_values()
        .map(|row| {
            row.into_iter()
                .map(|(index, coefficient)| {
                    let (u, v) = active_edges[index];
                    CocycleTermClaim {
                        u,
                        v,
                        coefficient: coefficient as u32,
                    }
                })
                .collect()
        })
        .collect())
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

fn gauge_potential(
    adjacency: &[Vec<usize>],
    coefficients: &BTreeMap<(usize, usize), u64>,
    modulus: u64,
) -> Vec<u64> {
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
                if potential[v].is_none() {
                    potential[v] =
                        Some((base + oriented_coefficient(coefficients, u, v, modulus)) % modulus);
                    queue.push_back(v);
                }
            }
        }
    }
    potential.into_iter().map(Option::unwrap).collect()
}

fn add_scaled_row(
    target: &mut BTreeMap<usize, u64>,
    source: &BTreeMap<usize, u64>,
    factor: u64,
    modulus: u64,
) {
    if factor == 0 {
        return;
    }
    for (&index, &value) in source {
        let next = (target.get(&index).copied().unwrap_or(0) + factor * value) % modulus;
        if next == 0 {
            target.remove(&index);
        } else {
            target.insert(index, next);
        }
    }
}

fn scale_row(row: &mut BTreeMap<usize, u64>, factor: u64, modulus: u64) {
    for value in row.values_mut() {
        *value = *value * factor % modulus;
    }
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
