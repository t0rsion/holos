//! Sparse finite-field linear algebra for bipersistence maps and queries.

use std::collections::{BTreeMap, BTreeSet};

use crate::cohomology::{CohomologyRestriction, CohomologySpace};
use crate::{Error, Result};

use super::BipersistenceTerm;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LinearMap {
    pub(super) source_rank: usize,
    pub(super) target_rank: usize,
    pub(super) columns: Vec<SparseVector>,
}

impl LinearMap {
    pub(super) fn identity(rank: usize) -> Self {
        Self {
            source_rank: rank,
            target_rank: rank,
            columns: (0..rank)
                .map(|position| SparseVector(BTreeMap::from([(position, 1)])))
                .collect(),
        }
    }

    pub(super) fn compose(after: &Self, before: &Self, modulus: u32) -> Result<Self> {
        if before.target_rank != after.source_rank {
            return Err(Error::InvalidInput(
                "bipersistence map composition has incompatible ranks".into(),
            ));
        }
        let mut columns = Vec::with_capacity(before.source_rank);
        for column in &before.columns {
            let mut image = SparseVector::default();
            for (&middle, &coefficient) in &column.0 {
                image.add_scaled(&after.columns[middle], coefficient, modulus);
            }
            columns.push(image);
        }
        Ok(Self {
            source_rank: before.source_rank,
            target_rank: after.target_rank,
            columns,
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct SparseVector(pub(super) BTreeMap<usize, u32>);

impl SparseVector {
    pub(super) fn insert(&mut self, position: usize, coefficient: u32, modulus: u32) {
        let coefficient = coefficient % modulus;
        if coefficient == 0 {
            self.0.remove(&position);
        } else {
            self.0.insert(position, coefficient);
        }
    }

    pub(super) fn add_scaled(&mut self, other: &Self, scale: u32, modulus: u32) {
        if scale == 0 {
            return;
        }
        for (&position, &coefficient) in &other.0 {
            let old = self.0.get(&position).copied().unwrap_or(0);
            let product = (u64::from(coefficient) * u64::from(scale)) % u64::from(modulus);
            let next = (u64::from(old) + product) % u64::from(modulus);
            self.insert(position, next as u32, modulus);
        }
    }

    pub(super) fn scale(&mut self, coefficient: u32, modulus: u32) {
        for value in self.0.values_mut() {
            *value = ((u64::from(*value) * u64::from(coefficient)) % u64::from(modulus)) as u32;
        }
        self.0.retain(|_, value| *value != 0);
    }

    pub(super) fn len(&self) -> usize {
        self.0.len()
    }

    pub(super) fn is_zero(&self) -> bool {
        self.0.is_empty()
    }
}

pub(super) fn linear_from_restriction(
    restriction: &CohomologyRestriction,
    source: &CohomologySpace,
    target: &CohomologySpace,
) -> Result<LinearMap> {
    if restriction.source_space != source.id()
        || restriction.target_space != target.id()
        || restriction.columns.len() != source.rank()
    {
        return Err(Error::InvalidInput(
            "cohomology restriction does not match its bipersistence nodes".into(),
        ));
    }
    let source_positions = source
        .basis()
        .iter()
        .enumerate()
        .map(|(position, class)| (class.id, position))
        .collect::<BTreeMap<_, _>>();
    let target_positions = target
        .basis()
        .iter()
        .enumerate()
        .map(|(position, class)| (class.id, position))
        .collect::<BTreeMap<_, _>>();
    let mut columns = Vec::with_capacity(source.rank());
    for (position, column) in restriction.columns.iter().enumerate() {
        if source_positions.get(&column.source).copied() != Some(position) {
            return Err(Error::InvalidInput(
                "cohomology restriction source order is not canonical".into(),
            ));
        }
        let mut image = SparseVector::default();
        for term in &column.image {
            let target_position = target_positions.get(&term.class).copied().ok_or_else(|| {
                Error::InvalidInput("cohomology restriction names an unknown target class".into())
            })?;
            image.insert(target_position, term.coefficient, restriction.modulus);
        }
        columns.push(image);
    }
    Ok(LinearMap {
        source_rank: source.rank(),
        target_rank: target.rank(),
        columns,
    })
}

pub(super) fn coordinate_vector(
    terms: &[BipersistenceTerm],
    rank: usize,
    modulus: u32,
    message: &str,
) -> Result<SparseVector> {
    if terms
        .windows(2)
        .any(|pair| pair[0].basis_index >= pair[1].basis_index)
        || terms.iter().any(|term| {
            term.basis_index >= rank || term.coefficient == 0 || term.coefficient >= modulus
        })
    {
        return Err(Error::InvalidInput(message.into()));
    }
    Ok(SparseVector(
        terms
            .iter()
            .map(|term| (term.basis_index, term.coefficient))
            .collect(),
    ))
}

pub(super) fn public_terms(vector: &SparseVector) -> Vec<BipersistenceTerm> {
    vector
        .0
        .iter()
        .map(|(&basis_index, &coefficient)| BipersistenceTerm {
            basis_index,
            coefficient,
        })
        .collect()
}

pub(super) fn rank(rows: Vec<SparseVector>, variables: usize, modulus: u32) -> usize {
    rref(rows, variables, modulus).0.len()
}

fn rref(
    mut rows: Vec<SparseVector>,
    variables: usize,
    modulus: u32,
) -> (Vec<SparseVector>, Vec<usize>) {
    let mut pivot_row = 0usize;
    let mut pivots = Vec::new();
    for column in 0..variables {
        let Some(found) = (pivot_row..rows.len()).find(|&row| rows[row].0.contains_key(&column))
        else {
            continue;
        };
        rows.swap(pivot_row, found);
        let pivot = rows[pivot_row].0[&column];
        rows[pivot_row].scale(inverse(pivot, modulus), modulus);
        let normalized = rows[pivot_row].clone();
        for (position, row) in rows.iter_mut().enumerate() {
            if position == pivot_row {
                continue;
            }
            if let Some(&coefficient) = row.0.get(&column) {
                row.add_scaled(&normalized, negate(coefficient, modulus), modulus);
            }
        }
        pivots.push(column);
        pivot_row += 1;
        if pivot_row == rows.len() {
            break;
        }
    }
    rows.truncate(pivot_row);
    (rows, pivots)
}

pub(super) fn nullspace(
    equations: Vec<SparseVector>,
    variables: usize,
    modulus: u32,
) -> Vec<SparseVector> {
    let (rows, pivots) = rref(equations, variables, modulus);
    let pivot_set = pivots.iter().copied().collect::<BTreeSet<_>>();
    (0..variables)
        .filter(|variable| !pivot_set.contains(variable))
        .map(|free| {
            let mut vector = SparseVector::default();
            vector.insert(free, 1, modulus);
            for (row, &pivot) in rows.iter().zip(&pivots) {
                if let Some(&coefficient) = row.0.get(&free) {
                    vector.insert(pivot, negate(coefficient, modulus), modulus);
                }
            }
            vector
        })
        .collect()
}

pub(super) fn affine_solution(
    columns: &[SparseVector],
    target: &SparseVector,
    modulus: u32,
) -> Option<(SparseVector, Vec<SparseVector>)> {
    let variables = columns.len();
    let target_rank = columns
        .iter()
        .flat_map(|column| column.0.keys().copied())
        .chain(target.0.keys().copied())
        .max()
        .map_or(0, |maximum| maximum + 1);
    let mut equations = Vec::with_capacity(target_rank);
    for row in 0..target_rank {
        let mut equation = SparseVector::default();
        for (variable, column) in columns.iter().enumerate() {
            if let Some(&coefficient) = column.0.get(&row) {
                equation.insert(variable, coefficient, modulus);
            }
        }
        if let Some(&right) = target.0.get(&row) {
            equation.insert(variables, right, modulus);
        }
        equations.push(equation);
    }
    let (rows, pivots, inconsistent) = augmented_rref(equations, variables, modulus);
    if inconsistent {
        return None;
    }
    let mut particular = SparseVector::default();
    for (row, &pivot) in rows.iter().zip(&pivots) {
        if let Some(&right) = row.0.get(&variables) {
            particular.insert(pivot, right, modulus);
        }
    }
    let pivot_set = pivots.iter().copied().collect::<BTreeSet<_>>();
    let kernel = (0..variables)
        .filter(|variable| !pivot_set.contains(variable))
        .map(|free| {
            let mut vector = SparseVector::default();
            vector.insert(free, 1, modulus);
            for (row, &pivot) in rows.iter().zip(&pivots) {
                if let Some(&coefficient) = row.0.get(&free) {
                    vector.insert(pivot, negate(coefficient, modulus), modulus);
                }
            }
            vector
        })
        .collect();
    Some((particular, kernel))
}

fn augmented_rref(
    mut rows: Vec<SparseVector>,
    variables: usize,
    modulus: u32,
) -> (Vec<SparseVector>, Vec<usize>, bool) {
    let mut pivot_row = 0usize;
    let mut pivots = Vec::new();
    for column in 0..variables {
        let Some(found) = (pivot_row..rows.len()).find(|&row| rows[row].0.contains_key(&column))
        else {
            continue;
        };
        rows.swap(pivot_row, found);
        let pivot = rows[pivot_row].0[&column];
        rows[pivot_row].scale(inverse(pivot, modulus), modulus);
        let normalized = rows[pivot_row].clone();
        for (position, row) in rows.iter_mut().enumerate() {
            if position == pivot_row {
                continue;
            }
            if let Some(&coefficient) = row.0.get(&column) {
                row.add_scaled(&normalized, negate(coefficient, modulus), modulus);
            }
        }
        pivots.push(column);
        pivot_row += 1;
        if pivot_row == rows.len() {
            break;
        }
    }
    let inconsistent = rows[pivot_row..]
        .iter()
        .any(|row| row.0.contains_key(&variables));
    rows.truncate(pivot_row);
    (rows, pivots, inconsistent)
}

fn inverse(value: u32, modulus: u32) -> u32 {
    let mut result = 1u64;
    let mut base = u64::from(value);
    let mut exponent = u64::from(modulus - 2);
    let modulus64 = u64::from(modulus);
    while exponent > 0 {
        if exponent & 1 == 1 {
            result = result * base % modulus64;
        }
        base = base * base % modulus64;
        exponent >>= 1;
    }
    result as u32
}

pub(super) fn negate(value: u32, modulus: u32) -> u32 {
    if value == 0 { 0 } else { modulus - value }
}

pub(super) fn checked_term_sum(
    total: usize,
    count: usize,
    limits: super::BipersistenceLimits,
) -> Result<usize> {
    let next = total
        .checked_add(count)
        .ok_or_else(|| Error::InvalidInput("rectangle coefficient count overflows".into()))?;
    if next > limits.max_linear_terms {
        return Err(Error::InvalidInput(format!(
            "rectangle coefficient count exceeds the limit {}",
            limits.max_linear_terms
        )));
    }
    Ok(next)
}
