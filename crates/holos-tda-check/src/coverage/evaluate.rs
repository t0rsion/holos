use std::collections::{BTreeMap, BTreeSet};

use crate::{ProofError, ProofLimits, inverse_mod};

use super::FORMAT_MAX_PROOF_TERMS;
use super::model::{Claim, Edge, Evaluation, State};
use super::source::cycle_pairs;
use super::wire::visit_combinations;

pub(crate) fn evaluate(
    claim: &Claim,
    selected: &[usize],
    limits: ProofLimits,
) -> Result<Evaluation, ProofError> {
    let mut checks = 0usize;
    let mut minimum_witness = None;
    for (state_index, state) in claim.states.iter().enumerate() {
        let mut active = state.base.clone();
        for action in selected {
            if claim.actions[*action]
                .states
                .binary_search(&state_index)
                .is_ok()
            {
                insert_sorted(&mut active, claim.actions[*action].vertex);
            }
        }
        let failable = active
            .iter()
            .copied()
            .filter(|vertex| claim.failable.binary_search(vertex).is_ok())
            .collect::<Vec<_>>();
        let failure_count = claim.failure_budget.min(failable.len());
        let mut combination = Vec::with_capacity(failure_count);
        let mut failure = false;
        visit_combinations(
            &failable,
            failure_count,
            0,
            &mut combination,
            &mut |removed| {
                checks = checks
                    .checked_add(1)
                    .ok_or_else(|| ProofError::new("coverage failure check count overflows"))?;
                if checks > limits.max_snapshots {
                    return Err(ProofError::new(
                        "coverage failure checks exceed their limit",
                    ));
                }
                let remaining = active
                    .iter()
                    .copied()
                    .filter(|vertex| removed.binary_search(vertex).is_err())
                    .collect::<Vec<_>>();
                match relative_criterion(claim, state, &remaining, limits)? {
                    Some(support) => {
                        minimum_witness = Some(minimum_witness.unwrap_or(usize::MAX).min(support));
                        Ok(true)
                    }
                    None => {
                        failure = true;
                        Ok(false)
                    }
                }
            },
        )?;
        if failure {
            return Ok(Evaluation {
                criterion_holds: false,
                checks,
                minimum_witness: None,
            });
        }
    }
    Ok(Evaluation {
        criterion_holds: true,
        checks,
        minimum_witness,
    })
}

pub(crate) fn relative_criterion(
    claim: &Claim,
    state: &State,
    active: &[usize],
    limits: ProofLimits,
) -> Result<Option<usize>, ProofError> {
    let active_set = active.iter().copied().collect::<BTreeSet<_>>();
    let edges = state
        .edges
        .iter()
        .copied()
        .filter(|edge| active_set.contains(&edge.u) && active_set.contains(&edge.v))
        .collect::<Vec<_>>();
    for (u, v) in cycle_pairs(&claim.fence) {
        if edges.binary_search(&edge(u, v)).is_err() {
            return Err(ProofError::new(
                "coverage graph omits a consecutive fence edge",
            ));
        }
    }
    let triangles = flag_triangles(active, &edges, limits.max_triangles)?;
    let entries = edges
        .len()
        .checked_mul(triangles.len().saturating_add(1))
        .ok_or_else(|| ProofError::new("coverage linear-system size overflows"))?;
    if entries > limits.max_terms {
        return Err(ProofError::new(
            "coverage linear system exceeds its entry limit",
        ));
    }
    let target = fence_chain(&claim.fence, &edges, claim.modulus)?;
    Ok(solve_boundary(&edges, &triangles, &target, claim.modulus)
        .map(|solution| solution.into_iter().filter(|value| *value != 0).count()))
}

pub(crate) fn flag_triangles(
    vertices: &[usize],
    edges: &[Edge],
    maximum: usize,
) -> Result<Vec<[usize; 3]>, ProofError> {
    let edge_set = edges.iter().copied().collect::<BTreeSet<_>>();
    let mut triangles = Vec::new();
    for (first_position, &a) in vertices.iter().enumerate() {
        for (second_position, &b) in vertices.iter().enumerate().skip(first_position + 1) {
            if !edge_set.contains(&edge(a, b)) {
                continue;
            }
            for &c in vertices.iter().skip(second_position + 1) {
                if edge_set.contains(&edge(a, c)) && edge_set.contains(&edge(b, c)) {
                    triangles.push([a, b, c]);
                    if triangles.len() > maximum {
                        return Err(ProofError::new(
                            "coverage active triangles exceed their limit",
                        ));
                    }
                }
            }
        }
    }
    Ok(triangles)
}

pub(crate) fn fence_chain(
    fence: &[usize],
    edges: &[Edge],
    modulus: u32,
) -> Result<Vec<u32>, ProofError> {
    let positions = edges
        .iter()
        .copied()
        .enumerate()
        .map(|(position, edge)| (edge, position))
        .collect::<BTreeMap<_, _>>();
    let mut chain = vec![0u32; edges.len()];
    for (u, v) in cycle_pairs(fence) {
        let position = positions[&edge(u, v)];
        let coefficient = if u < v { 1 } else { modulus - 1 };
        chain[position] = add(chain[position], coefficient, modulus);
    }
    if chain.iter().all(|value| *value == 0) {
        return Err(ProofError::new(
            "coverage fence cycle is zero in the declared field",
        ));
    }
    Ok(chain)
}

pub(crate) fn solve_boundary(
    edges: &[Edge],
    triangles: &[[usize; 3]],
    target: &[u32],
    modulus: u32,
) -> Option<Vec<u32>> {
    let mut rows = boundary_rows(edges, triangles, target, modulus);
    let pivots = reduce_boundary_rows(&mut rows, triangles.len(), modulus);
    if boundary_inconsistent(&rows, triangles.len()) {
        return None;
    }
    Some(boundary_solution(&rows, &pivots, triangles.len()))
}

pub(crate) fn boundary_rows(
    edges: &[Edge],
    triangles: &[[usize; 3]],
    target: &[u32],
    modulus: u32,
) -> Vec<Vec<u32>> {
    let positions = edges
        .iter()
        .copied()
        .enumerate()
        .map(|(position, edge)| (edge, position))
        .collect::<BTreeMap<_, _>>();
    let mut rows = vec![vec![0u32; triangles.len() + 1]; edges.len()];
    for (column, &[a, b, c]) in triangles.iter().enumerate() {
        rows[positions[&edge(b, c)]][column] = 1;
        rows[positions[&edge(a, c)]][column] = modulus - 1;
        rows[positions[&edge(a, b)]][column] = 1;
    }
    for (row, value) in rows.iter_mut().zip(target) {
        row[triangles.len()] = *value;
    }
    rows
}

pub(crate) fn reduce_boundary_rows(
    rows: &mut [Vec<u32>],
    columns: usize,
    modulus: u32,
) -> Vec<usize> {
    let mut pivot_row = 0usize;
    let mut pivots = Vec::new();
    for column in 0..columns {
        let Some(found) = (pivot_row..rows.len()).find(|row| rows[*row][column] != 0) else {
            continue;
        };
        rows.swap(pivot_row, found);
        normalize_boundary_pivot(rows, pivot_row, column, modulus);
        eliminate_boundary_pivot(rows, pivot_row, column, modulus);
        pivots.push(column);
        pivot_row += 1;
        if pivot_row == rows.len() {
            break;
        }
    }
    pivots
}

pub(crate) fn normalize_boundary_pivot(
    rows: &mut [Vec<u32>],
    pivot_row: usize,
    column: usize,
    modulus: u32,
) {
    let inverse = inverse_mod(u64::from(rows[pivot_row][column]), u64::from(modulus)) as u32;
    for value in &mut rows[pivot_row][column..] {
        *value = multiply(*value, inverse, modulus);
    }
}

pub(crate) fn eliminate_boundary_pivot(
    rows: &mut [Vec<u32>],
    pivot_row: usize,
    column: usize,
    modulus: u32,
) {
    let pivot = rows[pivot_row][column..].to_vec();
    for (row_index, row) in rows.iter_mut().enumerate() {
        if row_index == pivot_row || row[column] == 0 {
            continue;
        }
        let factor = row[column];
        for (value, pivot_value) in row[column..].iter_mut().zip(&pivot) {
            *value = subtract(*value, multiply(factor, *pivot_value, modulus), modulus);
        }
    }
}

pub(crate) fn boundary_inconsistent(rows: &[Vec<u32>], columns: usize) -> bool {
    rows.iter()
        .any(|row| row[..columns].iter().all(|value| *value == 0) && row[columns] != 0)
}

pub(crate) fn boundary_solution(rows: &[Vec<u32>], pivots: &[usize], columns: usize) -> Vec<u32> {
    let mut solution = vec![0u32; columns];
    for (row, &column) in pivots.iter().enumerate() {
        solution[column] = rows[row][columns];
    }
    solution
}

pub(crate) fn edge(u: usize, v: usize) -> Edge {
    Edge {
        u: u.min(v),
        v: u.max(v),
    }
}

pub(crate) fn add(left: u32, right: u32, modulus: u32) -> u32 {
    ((u64::from(left) + u64::from(right)) % u64::from(modulus)) as u32
}

pub(crate) fn subtract(left: u32, right: u32, modulus: u32) -> u32 {
    ((u64::from(left) + u64::from(modulus) - u64::from(right)) % u64::from(modulus)) as u32
}

pub(crate) fn multiply(left: u32, right: u32, modulus: u32) -> u32 {
    (u64::from(left) * u64::from(right) % u64::from(modulus)) as u32
}

pub(crate) fn selected_cost(costs: &[u64], selected: &[usize]) -> Result<u64, ProofError> {
    selected.iter().try_fold(0u64, |sum, candidate| {
        sum.checked_add(costs[*candidate])
            .ok_or_else(|| ProofError::new("coverage selected cost overflows"))
    })
}

pub(crate) fn blocker_min_cost(costs: &[u64], blocker: &[usize]) -> u64 {
    blocker
        .iter()
        .map(|candidate| costs[*candidate])
        .min()
        .unwrap_or(0)
}

pub(crate) fn blocker_bound(costs: &[u64], blockers: &[Vec<usize>]) -> Result<u64, ProofError> {
    blockers.iter().try_fold(0u64, |sum, blocker| {
        sum.checked_add(blocker_min_cost(costs, blocker))
            .ok_or_else(|| ProofError::new("coverage blocker bound overflows"))
    })
}

pub(crate) fn add_terms(
    total: &mut usize,
    count: usize,
    limits: ProofLimits,
) -> Result<(), ProofError> {
    *total = total
        .checked_add(count)
        .ok_or_else(|| ProofError::new("coverage proof term count overflows"))?;
    if *total > limits.max_terms.min(FORMAT_MAX_PROOF_TERMS) {
        return Err(ProofError::new("coverage proof terms exceed their limit"));
    }
    Ok(())
}

pub(crate) fn insert_sorted(values: &mut Vec<usize>, value: usize) {
    if let Err(position) = values.binary_search(&value) {
        values.insert(position, value);
    }
}

pub(crate) fn difference(values: &[usize], removed: &[usize]) -> Vec<usize> {
    values
        .iter()
        .copied()
        .filter(|value| removed.binary_search(value).is_err())
        .collect()
}

pub(crate) fn merge(left: &[usize], right: &[usize]) -> Vec<usize> {
    left.iter()
        .chain(right)
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
