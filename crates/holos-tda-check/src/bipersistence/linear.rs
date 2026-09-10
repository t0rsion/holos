use std::collections::{BTreeMap, BTreeSet};

use crate::{ProofError, inverse_mod};

use super::claims::Term;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LinearMap {
    pub(crate) source_rank: usize,
    pub(crate) target_rank: usize,
    pub(crate) columns: Vec<Vector>,
}

impl LinearMap {
    pub(crate) fn identity(rank: usize) -> Self {
        Self {
            source_rank: rank,
            target_rank: rank,
            columns: (0..rank)
                .map(|position| Vector(BTreeMap::from([(position, 1)])))
                .collect(),
        }
    }

    pub(crate) fn compose(after: &Self, before: &Self, modulus: u32) -> Result<Self, ProofError> {
        if before.target_rank != after.source_rank {
            return Err(ProofError::new(
                "bipersistence map composition has incompatible ranks",
            ));
        }
        let columns = before
            .columns
            .iter()
            .map(|column| {
                let mut image = Vector::default();
                for (&middle, &coefficient) in &column.0 {
                    image.add_scaled(&after.columns[middle], coefficient, modulus);
                }
                image
            })
            .collect();
        Ok(Self {
            source_rank: before.source_rank,
            target_rank: after.target_rank,
            columns,
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Vector(pub(crate) BTreeMap<usize, u32>);

impl Vector {
    pub(crate) fn insert(&mut self, position: usize, coefficient: u32, modulus: u32) {
        let coefficient = coefficient % modulus;
        if coefficient == 0 {
            self.0.remove(&position);
        } else {
            self.0.insert(position, coefficient);
        }
    }

    pub(crate) fn add_scaled(&mut self, other: &Self, scale: u32, modulus: u32) {
        for (&position, &coefficient) in &other.0 {
            let old = self.0.get(&position).copied().unwrap_or(0);
            let product = u64::from(coefficient) * u64::from(scale) % u64::from(modulus);
            let next = (u64::from(old) + product) % u64::from(modulus);
            self.insert(position, next as u32, modulus);
        }
    }

    pub(crate) fn scale(&mut self, coefficient: u32, modulus: u32) {
        for value in self.0.values_mut() {
            *value = (u64::from(*value) * u64::from(coefficient) % u64::from(modulus)) as u32;
        }
    }
}

pub(crate) fn coordinate_vector(
    terms: &[Term],
    dimension: usize,
    modulus: u32,
    message: &str,
) -> Result<Vector, ProofError> {
    if terms.windows(2).any(|pair| pair[0].basis >= pair[1].basis)
        || terms.iter().any(|term| {
            term.basis >= dimension || term.coefficient == 0 || term.coefficient >= modulus
        })
    {
        return Err(ProofError::new(message));
    }
    Ok(Vector(
        terms
            .iter()
            .map(|term| (term.basis, term.coefficient))
            .collect(),
    ))
}

pub(crate) fn public_terms(vector: &Vector) -> Vec<Term> {
    vector
        .0
        .iter()
        .map(|(&basis, &coefficient)| Term { basis, coefficient })
        .collect()
}

pub(crate) fn rank(rows: Vec<Vector>, variables: usize, modulus: u32) -> usize {
    rref(rows, variables, modulus).0.len()
}

pub(crate) fn rref(
    mut rows: Vec<Vector>,
    variables: usize,
    modulus: u32,
) -> (Vec<Vector>, Vec<usize>) {
    let mut pivot_row = 0usize;
    let mut pivots = Vec::new();
    for column in 0..variables {
        let Some(found) = (pivot_row..rows.len()).find(|&row| rows[row].0.contains_key(&column))
        else {
            continue;
        };
        rows.swap(pivot_row, found);
        let pivot = rows[pivot_row].0[&column];
        rows[pivot_row].scale(
            inverse_mod(u64::from(pivot), u64::from(modulus)) as u32,
            modulus,
        );
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

pub(crate) fn nullspace(equations: Vec<Vector>, variables: usize, modulus: u32) -> Vec<Vector> {
    let (rows, pivots) = rref(equations, variables, modulus);
    let pivot_set = pivots.iter().copied().collect::<BTreeSet<_>>();
    (0..variables)
        .filter(|variable| !pivot_set.contains(variable))
        .map(|free| {
            let mut vector = Vector::default();
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

pub(crate) fn affine_solution(
    columns: &[Vector],
    target: &Vector,
    modulus: u32,
) -> Option<(Vector, Vec<Vector>)> {
    let variables = columns.len();
    let target_rank = columns
        .iter()
        .flat_map(|column| column.0.keys().copied())
        .chain(target.0.keys().copied())
        .max()
        .map_or(0, |maximum| maximum + 1);
    let mut equations = Vec::with_capacity(target_rank);
    for row in 0..target_rank {
        let mut equation = Vector::default();
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
    let mut particular = Vector::default();
    for (row, &pivot) in rows.iter().zip(&pivots) {
        if let Some(&right) = row.0.get(&variables) {
            particular.insert(pivot, right, modulus);
        }
    }
    let pivot_set = pivots.iter().copied().collect::<BTreeSet<_>>();
    let kernel = (0..variables)
        .filter(|variable| !pivot_set.contains(variable))
        .map(|free| {
            let mut vector = Vector::default();
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

pub(crate) fn augmented_rref(
    mut rows: Vec<Vector>,
    variables: usize,
    modulus: u32,
) -> (Vec<Vector>, Vec<usize>, bool) {
    let mut pivot_row = 0usize;
    let mut pivots = Vec::new();
    for column in 0..variables {
        let Some(found) = (pivot_row..rows.len()).find(|&row| rows[row].0.contains_key(&column))
        else {
            continue;
        };
        rows.swap(pivot_row, found);
        let pivot = rows[pivot_row].0[&column];
        rows[pivot_row].scale(
            inverse_mod(u64::from(pivot), u64::from(modulus)) as u32,
            modulus,
        );
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

pub(crate) fn negate(value: u32, modulus: u32) -> u32 {
    if value == 0 { 0 } else { modulus - value }
}
