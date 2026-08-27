//! Exact class-space correspondences across persistence updates.
//!
//! Two updated filtrations need not admit a map in either direction. At a
//! scale where an old and a new class space are both live, their active
//! graphs have a canonical common subgraph. Restricting cocycles to that
//! subgraph gives two linear images in its first cohomology. Their exact
//! intersection is the correspondence reported here.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::{
    Bar, BasisClassId, Error, IntervalGroupId, PersistentClassSpace, Result, SparseDistanceMatrix,
};

/// One nonzero coefficient on a declared canonical basis vector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CorrespondenceTerm {
    /// Basis vector named by the coefficient.
    pub basis: BasisClassId,
    /// Coefficient in the shared prime field.
    pub coefficient: u32,
}

/// One equality between old and new linear combinations after restriction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorrespondenceVector {
    /// Nonzero old-basis coefficients.
    pub old: Vec<CorrespondenceTerm>,
    /// Nonzero new-basis coefficients.
    pub new: Vec<CorrespondenceTerm>,
}

/// Exact relation between one old and one new persistent class space.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassCorrespondence {
    /// Old class space.
    pub old_space: IntervalGroupId,
    /// New class space.
    pub new_space: IntervalGroupId,
    /// Scale at which both spaces are restricted.
    pub scale: f64,
    /// Dimension of the declared old space.
    pub old_rank: usize,
    /// Dimension of the declared new space.
    pub new_rank: usize,
    /// Rank of the old image on the common subgraph.
    pub old_image_rank: usize,
    /// Rank of the new image on the common subgraph.
    pub new_image_rank: usize,
    /// Dimension of the exact image intersection.
    pub relation_rank: usize,
    /// Canonical basis for the relation.
    pub basis: Vec<CorrespondenceVector>,
}

impl ClassCorrespondence {
    /// True when the relation proves an isomorphism of the two full spaces.
    pub fn is_isomorphism(&self) -> bool {
        self.relation_rank == self.old_rank
            && self.relation_rank == self.new_rank
            && self.old_image_rank == self.old_rank
            && self.new_image_rank == self.new_rank
    }
}

/// Compute exact pairwise class-space correspondences across one update.
///
/// A record is returned only when the restricted images have a nonzero
/// intersection. Missing records do not prove that either class died. The
/// common subgraph can forget a class that exists on both full graphs.
pub fn class_correspondences(
    old_graph: &SparseDistanceMatrix,
    old_spaces: &[PersistentClassSpace],
    new_graph: &SparseDistanceMatrix,
    new_spaces: &[PersistentClassSpace],
    modulus: u32,
) -> Result<Vec<ClassCorrespondence>> {
    if old_graph.len() != new_graph.len() {
        return Ok(Vec::new());
    }
    let mut output = Vec::new();
    for old in old_spaces {
        for new in new_spaces {
            let Some(scale) = comparison_scale(old.interval, new.interval, old, new) else {
                continue;
            };
            let edges = common_edges(old_graph, new_graph, scale);
            let old_rows = restricted_rows(old_graph.len(), &edges, old, modulus)?;
            let new_rows = restricted_rows(new_graph.len(), &edges, new, modulus)?;
            let old_image = independent_image(&old_rows, modulus)?;
            let new_image = independent_image(&new_rows, modulus)?;
            let relation = image_intersection(&old_image, &new_image, edges.len(), modulus)?;
            if relation.is_empty() {
                continue;
            }
            let basis = relation
                .into_iter()
                .map(
                    |(old_coefficients, new_coefficients)| CorrespondenceVector {
                        old: terms(&old.basis, &old_coefficients),
                        new: terms(&new.basis, &new_coefficients),
                    },
                )
                .collect::<Vec<_>>();
            output.push(ClassCorrespondence {
                old_space: old.id,
                new_space: new.id,
                scale,
                old_rank: old.basis.len(),
                new_rank: new.basis.len(),
                old_image_rank: old_image.len(),
                new_image_rank: new_image.len(),
                relation_rank: basis.len(),
                basis,
            });
        }
    }
    output.sort_by(|left, right| {
        left.old_space
            .cmp(&right.old_space)
            .then(left.new_space.cmp(&right.new_space))
            .then(left.scale.total_cmp(&right.scale))
    });
    Ok(output)
}

fn comparison_scale(
    old_interval: Bar,
    new_interval: Bar,
    old: &PersistentClassSpace,
    new: &PersistentClassSpace,
) -> Option<f64> {
    let old_scale = old.basis.first()?.cocycle.scale;
    let new_scale = new.basis.first()?.cocycle.scale;
    let birth = old_interval.birth.max(new_interval.birth);
    let death = old_interval.death.min(new_interval.death);
    let scale = if death.is_infinite() {
        old_scale.max(new_scale).max(birth)
    } else {
        old_scale.min(new_scale)
    };
    (scale >= birth && scale < death).then_some(scale)
}

fn common_edges(
    old: &SparseDistanceMatrix,
    new: &SparseDistanceMatrix,
    scale: f64,
) -> Vec<(usize, usize)> {
    old.edges()
        .filter(|&(u, v, value)| value <= scale && new.get(u, v) <= scale)
        .map(|(u, v, _)| (u, v))
        .collect()
}

fn restricted_rows(
    vertex_count: usize,
    edges: &[(usize, usize)],
    space: &PersistentClassSpace,
    modulus: u32,
) -> Result<Vec<SparseRow>> {
    let edge_indices: BTreeMap<_, _> = edges
        .iter()
        .copied()
        .enumerate()
        .map(|(index, edge)| (edge, index))
        .collect();
    let mut adjacency = vec![Vec::new(); vertex_count];
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
            if class.cocycle.modulus != modulus {
                return Err(Error::InvalidInput(
                    "class correspondence has inconsistent coefficient fields".into(),
                ));
            }
            let coefficients: BTreeMap<_, _> = class
                .cocycle
                .terms
                .iter()
                .filter_map(|term| {
                    edge_indices
                        .contains_key(&(term.u, term.v))
                        .then_some(((term.u, term.v), term.coefficient as u64))
                })
                .collect();
            gauge_fixed_row(
                vertex_count,
                edges,
                &edge_indices,
                &adjacency,
                &coefficients,
                modulus as u64,
            )
        })
        .collect()
}

fn gauge_fixed_row(
    vertex_count: usize,
    edges: &[(usize, usize)],
    edge_indices: &BTreeMap<(usize, usize), usize>,
    adjacency: &[Vec<usize>],
    coefficients: &BTreeMap<(usize, usize), u64>,
    modulus: u64,
) -> Result<SparseRow> {
    let mut potential = vec![None; vertex_count];
    let mut queue = VecDeque::new();
    for root in 0..vertex_count {
        if potential[root].is_some() {
            continue;
        }
        potential[root] = Some(0u64);
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
    let mut row = SparseRow::default();
    for &(u, v) in edges {
        let original = coefficients.get(&(u, v)).copied().unwrap_or(0);
        let adjusted =
            (original + potential[u].unwrap_or(0) + modulus - potential[v].unwrap_or(0)) % modulus;
        if adjusted != 0 {
            let index = edge_indices.get(&(u, v)).copied().ok_or_else(|| {
                Error::InvalidInput(format!("common edge ({u}, {v}) has no index"))
            })?;
            row.0.insert(index, adjusted);
        }
    }
    Ok(row)
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
        let value = coefficients.get(&(v, u)).copied().unwrap_or(0);
        (modulus - value) % modulus
    }
}

#[derive(Debug, Clone, Default)]
struct SparseRow(BTreeMap<usize, u64>);

impl SparseRow {
    fn leading(&self) -> Option<(usize, u64)> {
        self.0.first_key_value().map(|(&key, &value)| (key, value))
    }

    fn add_scaled(&mut self, source: &Self, factor: u64, modulus: u64) {
        if factor == 0 {
            return;
        }
        for (&column, &value) in &source.0 {
            let next = (self.0.get(&column).copied().unwrap_or(0) + factor * value) % modulus;
            if next == 0 {
                self.0.remove(&column);
            } else {
                self.0.insert(column, next);
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
struct ImageVector {
    row: SparseRow,
    coefficients: Vec<u64>,
}

fn independent_image(rows: &[SparseRow], modulus: u32) -> Result<Vec<ImageVector>> {
    let modulus = modulus as u64;
    if modulus < 2 {
        return Err(Error::InvalidInput(
            "class correspondence requires a prime coefficient field".into(),
        ));
    }
    let mut image: Vec<ImageVector> = Vec::new();
    for (index, source) in rows.iter().enumerate() {
        let mut row = source.clone();
        let mut coefficients = vec![0u64; rows.len()];
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

fn image_intersection(
    old: &[ImageVector],
    new: &[ImageVector],
    coordinates: usize,
    modulus: u32,
) -> Result<Vec<(Vec<u64>, Vec<u64>)>> {
    if old.is_empty() || new.is_empty() {
        return Ok(Vec::new());
    }
    let variables = old.len() + new.len();
    let mut equations = vec![vec![0u64; variables]; coordinates];
    let modulus64 = modulus as u64;
    for (variable, vector) in old.iter().enumerate() {
        for (&coordinate, &value) in &vector.row.0 {
            equations[coordinate][variable] = value;
        }
    }
    for (offset, vector) in new.iter().enumerate() {
        for (&coordinate, &value) in &vector.row.0 {
            equations[coordinate][old.len() + offset] = (modulus64 - value) % modulus64;
        }
    }
    let kernel = nullspace(equations, variables, modulus64)?;
    let mut output = Vec::with_capacity(kernel.len());
    for relation in kernel {
        let mut old_coefficients = vec![0u64; old[0].coefficients.len()];
        for (coefficient, vector) in relation[..old.len()].iter().zip(old) {
            add_scaled_dense(
                &mut old_coefficients,
                &vector.coefficients,
                *coefficient,
                modulus64,
            );
        }
        let mut new_coefficients = vec![0u64; new[0].coefficients.len()];
        for (coefficient, vector) in relation[old.len()..].iter().zip(new) {
            add_scaled_dense(
                &mut new_coefficients,
                &vector.coefficients,
                *coefficient,
                modulus64,
            );
        }
        if old_coefficients.iter().any(|&value| value != 0)
            && new_coefficients.iter().any(|&value| value != 0)
        {
            output.push((old_coefficients, new_coefficients));
        }
    }
    Ok(output)
}

fn nullspace(
    mut equations: Vec<Vec<u64>>,
    variables: usize,
    modulus: u64,
) -> Result<Vec<Vec<u64>>> {
    let mut pivot_columns = Vec::new();
    let mut pivot_row = 0usize;
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
        pivot_columns.push(column);
        pivot_row += 1;
        if pivot_row == equations.len() {
            break;
        }
    }
    let pivots: BTreeSet<_> = pivot_columns.iter().copied().collect();
    let mut basis = Vec::new();
    for free in (0..variables).filter(|column| !pivots.contains(column)) {
        let mut vector = vec![0u64; variables];
        vector[free] = 1;
        for (row, &pivot) in pivot_columns.iter().enumerate().rev() {
            vector[pivot] = (modulus - equations[row][free]) % modulus;
        }
        basis.push(vector);
    }
    Ok(basis)
}

fn add_scaled_dense(target: &mut [u64], source: &[u64], factor: u64, modulus: u64) {
    for (target, source) in target.iter_mut().zip(source) {
        *target = (*target + factor * *source) % modulus;
    }
}

fn terms(basis: &[crate::PersistentClass], coefficients: &[u64]) -> Vec<CorrespondenceTerm> {
    let mut terms: Vec<_> = basis
        .iter()
        .zip(coefficients)
        .filter_map(|(class, &coefficient)| {
            (coefficient != 0).then_some(CorrespondenceTerm {
                basis: class.id,
                coefficient: coefficient as u32,
            })
        })
        .collect();
    terms.sort_unstable();
    terms
}

fn inverse_mod(value: u64, modulus: u64) -> u64 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RipsParams, rips_persistence_with_classes_sparse};

    fn two_squares() -> SparseDistanceMatrix {
        SparseDistanceMatrix::from_triplets(
            7,
            &[
                (0, 1, 1.0),
                (1, 2, 1.0),
                (2, 3, 1.0),
                (0, 3, 1.0),
                (0, 2, 2.0),
                (1, 3, 2.0),
                (3, 4, 1.0),
                (4, 5, 1.0),
                (5, 6, 1.0),
                (3, 6, 1.0),
                (3, 5, 2.0),
                (4, 6, 2.0),
            ],
        )
        .unwrap()
    }

    #[test]
    fn identity_update_proves_the_full_rank_two_space() {
        let graph = two_squares();
        for modulus in [2, 3, 5] {
            let explained = rips_persistence_with_classes_sparse(
                &graph,
                &RipsParams::new(1).with_modulus(modulus),
            )
            .unwrap();
            let relation = class_correspondences(
                &graph,
                &explained.spaces,
                &graph,
                &explained.spaces,
                modulus,
            )
            .unwrap();
            assert_eq!(relation.len(), 1);
            assert_eq!(relation[0].old_rank, 2);
            assert_eq!(relation[0].relation_rank, 2);
            assert!(relation[0].is_isomorphism());
        }
    }

    #[test]
    fn disjoint_active_subgraphs_do_not_invent_a_relation() {
        let old = two_squares();
        let new = SparseDistanceMatrix::from_triplets(
            7,
            &[
                (0, 1, 3.0),
                (1, 2, 3.0),
                (2, 3, 3.0),
                (0, 3, 3.0),
                (0, 2, 4.0),
                (1, 3, 4.0),
                (3, 4, 3.0),
                (4, 5, 3.0),
                (5, 6, 3.0),
                (3, 6, 3.0),
                (3, 5, 4.0),
                (4, 6, 4.0),
            ],
        )
        .unwrap();
        let params = RipsParams::new(1);
        let old_result = rips_persistence_with_classes_sparse(&old, &params).unwrap();
        let new_result = rips_persistence_with_classes_sparse(&new, &params).unwrap();
        let relation =
            class_correspondences(&old, &old_result.spaces, &new, &new_result.spaces, 2).unwrap();
        assert!(relation.is_empty());
    }
}
