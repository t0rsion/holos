use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::{inverse_mod, proof::ProofError};

use super::trace_model::{ResultClass, ResultSpace, TraceBasisTerm};

pub(super) type ImageRelation = (Vec<u64>, Vec<u64>);

#[derive(Debug, Clone, Default)]
pub(super) struct SparseRow(BTreeMap<usize, u64>);

impl SparseRow {
    fn leading(&self) -> Option<(usize, u64)> {
        self.0
            .first_key_value()
            .map(|(&index, &value)| (index, value))
    }

    fn add_scaled(&mut self, source: &Self, factor: u64, modulus: u64) {
        if factor == 0 {
            return;
        }
        for (&index, &value) in &source.0 {
            let next = (self.0.get(&index).copied().unwrap_or(0) + factor * value) % modulus;
            if next == 0 {
                self.0.remove(&index);
            } else {
                self.0.insert(index, next);
            }
        }
    }

    fn scale(&mut self, factor: u64, modulus: u64) {
        for value in self.0.values_mut() {
            *value = *value * factor % modulus;
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct ImageVector {
    pub(super) row: SparseRow,
    pub(super) coefficients: Vec<u64>,
}

pub(super) fn restricted_rows(
    vertices: usize,
    edges: &[(usize, usize)],
    space: &ResultSpace,
    modulus: u32,
) -> Result<Vec<SparseRow>, ProofError> {
    let edge_indices: BTreeMap<_, _> = edges
        .iter()
        .copied()
        .enumerate()
        .map(|(index, edge)| (edge, index))
        .collect();
    let mut adjacency = vec![Vec::new(); vertices];
    for &(u, v) in edges {
        adjacency[u].push(v);
        adjacency[v].push(u);
    }
    for neighbors in &mut adjacency {
        neighbors.sort_unstable();
    }
    space
        .basis
        .iter()
        .map(|class| {
            let coefficients: BTreeMap<_, _> = class
                .terms
                .iter()
                .filter_map(|term| {
                    edge_indices
                        .contains_key(&(term.u, term.v))
                        .then_some(((term.u, term.v), term.coefficient as u64))
                })
                .collect();
            let mut potential = vec![None; vertices];
            let mut queue = VecDeque::new();
            for root in 0..vertices {
                if potential[root].is_some() {
                    continue;
                }
                potential[root] = Some(0);
                queue.push_back(root);
                while let Some(u) = queue.pop_front() {
                    let base = potential[u].expect("queued vertex has a potential");
                    for &v in &adjacency[u] {
                        if potential[v].is_none() {
                            potential[v] = Some(
                                (base + oriented_coefficient(&coefficients, u, v, modulus as u64))
                                    % modulus as u64,
                            );
                            queue.push_back(v);
                        }
                    }
                }
            }
            let mut row = SparseRow::default();
            for &(u, v) in edges {
                let original = coefficients.get(&(u, v)).copied().unwrap_or(0);
                let adjusted = (original + potential[u].unwrap_or(0) + modulus as u64
                    - potential[v].unwrap_or(0))
                    % modulus as u64;
                if adjusted != 0 {
                    row.0.insert(edge_indices[&(u, v)], adjusted);
                }
            }
            Ok(row)
        })
        .collect()
}

fn oriented_coefficient(
    coefficients: &BTreeMap<(usize, usize), u64>,
    u: usize,
    v: usize,
    modulus: u64,
) -> u64 {
    if u < v {
        coefficients.get(&(u, v)).copied().unwrap_or(0)
    } else {
        (modulus - coefficients.get(&(v, u)).copied().unwrap_or(0)) % modulus
    }
}

pub(super) fn independent_image(
    rows: &[SparseRow],
    modulus: u32,
    max_relation_cells: usize,
) -> Result<Vec<ImageVector>, ProofError> {
    let cells = rows
        .len()
        .checked_mul(rows.len())
        .ok_or_else(|| ProofError::new("correspondence image matrix size overflows usize"))?;
    if cells > max_relation_cells {
        return Err(ProofError::new(
            "correspondence image matrix exceeds the relation-cell limit",
        ));
    }
    let modulus = modulus as u64;
    let mut image: Vec<ImageVector> = Vec::new();
    for (index, source) in rows.iter().enumerate() {
        let mut row = source.clone();
        let mut coefficients = vec![0; rows.len()];
        coefficients[index] = 1;
        for existing in &image {
            let Some((pivot, _)) = existing.row.leading() else {
                continue;
            };
            let Some(&coefficient) = row.0.get(&pivot) else {
                continue;
            };
            row.add_scaled(&existing.row, modulus - coefficient, modulus);
            add_scaled_dense(
                &mut coefficients,
                &existing.coefficients,
                modulus - coefficient,
                modulus,
            );
        }
        let Some((_, coefficient)) = row.leading() else {
            continue;
        };
        let inverse = inverse_mod(coefficient, modulus);
        row.scale(inverse, modulus);
        for value in &mut coefficients {
            *value = *value * inverse % modulus;
        }
        image.push(ImageVector { row, coefficients });
        image.sort_by_key(|vector| vector.row.leading().map(|(pivot, _)| pivot));
    }
    Ok(image)
}

pub(super) fn image_intersection(
    old: &[ImageVector],
    new: &[ImageVector],
    coordinates: usize,
    modulus: u32,
    max_relation_cells: usize,
) -> Result<Vec<ImageRelation>, ProofError> {
    if old.is_empty() || new.is_empty() {
        return Ok(Vec::new());
    }
    let modulus = modulus as u64;
    let variables = relation_width(old.len(), new.len())?;
    check_relation_cells(coordinates, variables, max_relation_cells)?;
    let equations = relation_equations(old, new, coordinates, variables, modulus);
    let kernel = nullspace(equations, variables, modulus);
    Ok(kernel_relations(kernel, old, new, modulus))
}

fn relation_width(old: usize, new: usize) -> Result<usize, ProofError> {
    old.checked_add(new)
        .ok_or_else(|| ProofError::new("correspondence relation width overflows usize"))
}

fn check_relation_cells(
    coordinates: usize,
    variables: usize,
    max_relation_cells: usize,
) -> Result<(), ProofError> {
    let equation_cells = coordinates
        .checked_mul(variables)
        .ok_or_else(|| ProofError::new("correspondence relation matrix size overflows usize"))?;
    let nullspace_cells = variables
        .checked_mul(variables)
        .ok_or_else(|| ProofError::new("correspondence nullspace size overflows usize"))?;
    let cells = equation_cells
        .checked_add(nullspace_cells)
        .ok_or_else(|| ProofError::new("correspondence relation size overflows usize"))?;
    if cells > max_relation_cells {
        return Err(ProofError::new(
            "correspondence relation matrix exceeds the relation-cell limit",
        ));
    }
    Ok(())
}

fn relation_equations(
    old: &[ImageVector],
    new: &[ImageVector],
    coordinates: usize,
    variables: usize,
    modulus: u64,
) -> Vec<Vec<u64>> {
    let mut equations = vec![vec![0; variables]; coordinates];
    for (index, vector) in old.iter().enumerate() {
        for (&coordinate, &value) in &vector.row.0 {
            equations[coordinate][index] = value;
        }
    }
    for (index, vector) in new.iter().enumerate() {
        for (&coordinate, &value) in &vector.row.0 {
            equations[coordinate][old.len() + index] = (modulus - value) % modulus;
        }
    }
    equations
}

fn kernel_relations(
    kernel: Vec<Vec<u64>>,
    old: &[ImageVector],
    new: &[ImageVector],
    modulus: u64,
) -> Vec<ImageRelation> {
    kernel
        .into_iter()
        .filter_map(|relation| {
            let left = combine_image(old, &relation[..old.len()], modulus);
            let right = combine_image(new, &relation[old.len()..], modulus);
            (left.iter().any(|&value| value != 0) && right.iter().any(|&value| value != 0))
                .then_some((left, right))
        })
        .collect()
}

fn nullspace(mut equations: Vec<Vec<u64>>, variables: usize, modulus: u64) -> Vec<Vec<u64>> {
    let mut pivots = Vec::new();
    let mut pivot_row = 0;
    for column in 0..variables {
        let Some(row) = (pivot_row..equations.len()).find(|&row| equations[row][column] != 0)
        else {
            continue;
        };
        equations.swap(pivot_row, row);
        let inverse = inverse_mod(equations[pivot_row][column], modulus);
        for value in &mut equations[pivot_row][column..] {
            *value = *value * inverse % modulus;
        }
        let normalized = equations[pivot_row].clone();
        for (row, equation) in equations.iter_mut().enumerate() {
            if row == pivot_row || equation[column] == 0 {
                continue;
            }
            let factor = modulus - equation[column];
            for (target, &source) in equation[column..].iter_mut().zip(&normalized[column..]) {
                *target = (*target + factor * source) % modulus;
            }
        }
        pivots.push(column);
        pivot_row += 1;
        if pivot_row == equations.len() {
            break;
        }
    }
    let pivot_set: BTreeSet<_> = pivots.iter().copied().collect();
    let mut basis = Vec::new();
    for free in (0..variables).filter(|column| !pivot_set.contains(column)) {
        let mut vector = vec![0; variables];
        vector[free] = 1;
        for (row, &pivot) in pivots.iter().enumerate().rev() {
            vector[pivot] = (modulus - equations[row][free]) % modulus;
        }
        basis.push(vector);
    }
    basis
}

fn combine_image(vectors: &[ImageVector], factors: &[u64], modulus: u64) -> Vec<u64> {
    let mut result = vec![0; vectors[0].coefficients.len()];
    for (factor, vector) in factors.iter().zip(vectors) {
        add_scaled_dense(&mut result, &vector.coefficients, *factor, modulus);
    }
    result
}

fn add_scaled_dense(target: &mut [u64], source: &[u64], factor: u64, modulus: u64) {
    for (target, source) in target.iter_mut().zip(source) {
        *target = (*target + factor * *source) % modulus;
    }
}

pub(super) fn terms_for(basis: &[ResultClass], coefficients: &[u64]) -> Vec<TraceBasisTerm> {
    let mut terms: Vec<_> = basis
        .iter()
        .zip(coefficients)
        .filter_map(|(class, &coefficient)| {
            (coefficient != 0).then_some((class.id, coefficient as u32))
        })
        .collect();
    terms.sort_unstable();
    terms
}

#[cfg(test)]
#[path = "trace_correspondence_tests.rs"]
mod tests;
