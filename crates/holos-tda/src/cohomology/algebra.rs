use std::collections::{BTreeMap, BTreeSet};

use crate::{Error, Result};

use super::model::*;

impl SparseVector {
    pub(crate) fn insert(&mut self, position: usize, coefficient: u32) {
        if coefficient != 0 {
            self.0.insert(position, coefficient);
        }
    }

    pub(crate) fn leading(&self) -> Option<(usize, u32)> {
        self.0.first_key_value().map(|(&key, &value)| (key, value))
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.0.is_empty()
    }

    pub(crate) fn add_scaled(&mut self, source: &Self, factor: u64, modulus: u64) {
        if factor == 0 {
            return;
        }
        for (&position, &coefficient) in &source.0 {
            let current = u64::from(self.0.get(&position).copied().unwrap_or(0));
            let next = (current + factor * u64::from(coefficient)) % modulus;
            if next == 0 {
                self.0.remove(&position);
            } else {
                self.0.insert(position, next as u32);
            }
        }
    }

    pub(crate) fn scale(&mut self, factor: u64, modulus: u64) {
        for coefficient in self.0.values_mut() {
            *coefficient = (u64::from(*coefficient) * factor % modulus) as u32;
        }
    }
}

pub(crate) fn rref(rows: Vec<SparseVector>, modulus: u64) -> Vec<SparseVector> {
    let mut basis: Vec<SparseVector> = Vec::new();
    for mut row in rows {
        reduce_by_basis(&mut row, &basis, modulus);
        let Some((pivot, coefficient)) = row.leading() else {
            continue;
        };
        row.scale(inverse_mod(u64::from(coefficient), modulus), modulus);
        for existing in &mut basis {
            if let Some(&value) = existing.0.get(&pivot) {
                existing.add_scaled(&row, modulus - u64::from(value), modulus);
            }
        }
        basis.push(row);
        basis.sort_by_key(|item| item.leading().map(|(position, _)| position));
    }
    basis
}

pub(crate) fn reduce_by_basis(row: &mut SparseVector, basis: &[SparseVector], modulus: u64) {
    for existing in basis {
        let Some((pivot, _)) = existing.leading() else {
            continue;
        };
        if let Some(&coefficient) = row.0.get(&pivot) {
            row.add_scaled(existing, modulus - u64::from(coefficient), modulus);
        }
    }
}

pub(crate) fn nullspace(
    equations: Vec<SparseVector>,
    variables: usize,
    modulus: u64,
) -> Vec<SparseVector> {
    let equations = rref(equations, modulus);
    let pivots: BTreeSet<_> = equations
        .iter()
        .filter_map(|row| row.leading().map(|(position, _)| position))
        .collect();
    let mut basis = Vec::new();
    for free in (0..variables).filter(|position| !pivots.contains(position)) {
        let mut vector = SparseVector::default();
        vector.insert(free, 1);
        for equation in &equations {
            let pivot = equation.leading().unwrap().0;
            if let Some(&coefficient) = equation.0.get(&free) {
                vector.insert(pivot, (modulus - u64::from(coefficient)) as u32);
            }
        }
        basis.push(vector);
    }
    basis
}

pub(crate) fn restricted_rows(
    source: &CohomologySpace,
    common: &CohomologySpace,
) -> Vec<SparseVector> {
    let positions: BTreeMap<_, _> = common
        .simplices
        .iter()
        .cloned()
        .enumerate()
        .map(|(position, simplex)| (simplex, position))
        .collect();
    source
        .basis_vectors
        .iter()
        .map(|vector| {
            let mut restricted = SparseVector::default();
            for (&position, &coefficient) in &vector.0 {
                if let Some(&common_position) = positions.get(&source.simplices[position]) {
                    restricted.insert(common_position, coefficient);
                }
            }
            reduce_by_basis(&mut restricted, &common.coboundaries, common.modulus as u64);
            restricted
        })
        .collect()
}

pub(crate) fn checked_coordinate_vector(
    coordinates: &[(usize, u32)],
    rank: usize,
    modulus: u32,
    message: &str,
) -> Result<SparseVector> {
    if coordinates.windows(2).any(|pair| pair[0].0 >= pair[1].0)
        || coordinates.iter().any(|&(position, coefficient)| {
            position >= rank || coefficient == 0 || coefficient >= modulus
        })
    {
        return Err(Error::InvalidInput(message.into()));
    }
    let mut vector = SparseVector::default();
    for &(position, coefficient) in coordinates {
        vector.insert(position, coefficient);
    }
    Ok(vector)
}

pub(crate) fn basis_positions(space: &CohomologySpace) -> BTreeMap<CohomologyClassId, usize> {
    space
        .basis
        .iter()
        .enumerate()
        .map(|(position, class)| (class.id, position))
        .collect()
}

pub(crate) fn affine_solution(
    equations: &[SparseVector],
    right: &[u32],
    variables: usize,
    modulus: u64,
) -> Option<(SparseVector, Vec<SparseVector>)> {
    let augmented = equations
        .iter()
        .zip(right)
        .map(|(equation, &value)| {
            let mut row = equation.clone();
            row.insert(variables, value);
            row
        })
        .collect::<Vec<_>>();
    let reduced = rref(augmented, modulus);
    if reduced
        .iter()
        .any(|row| row.leading().is_some_and(|(pivot, _)| pivot == variables))
    {
        return None;
    }
    let mut particular = SparseVector::default();
    for row in reduced {
        let Some((pivot, _)) = row.leading() else {
            continue;
        };
        if let Some(&value) = row.0.get(&variables) {
            particular.insert(pivot, value);
        }
    }
    Some((
        particular,
        nullspace(equations.to_vec(), variables, modulus),
    ))
}

pub(crate) fn combine_relation_new(
    coefficients: &SparseVector,
    relation_rows: &[SparseVector],
    old_rank: usize,
    modulus: u64,
) -> SparseVector {
    let mut target = SparseVector::default();
    for (&relation_position, &coefficient) in &coefficients.0 {
        for (&position, &value) in relation_rows[relation_position].0.range(old_rank..) {
            let mut term = SparseVector::default();
            term.insert(position - old_rank, value);
            target.add_scaled(&term, u64::from(coefficient), modulus);
        }
    }
    target
}

pub(crate) fn continuation_result(
    old: &CohomologySpace,
    new: &CohomologySpace,
    selected: SparseVector,
    kind: CohomologyContinuationKind,
    target: SparseVector,
    ambiguity: Vec<SparseVector>,
) -> CohomologyContinuation {
    CohomologyContinuation {
        old_space: old.id,
        new_space: new.id,
        kind,
        old: relation_terms(&old.basis, &selected),
        new: relation_terms(&new.basis, &target),
        ambiguity: ambiguity
            .iter()
            .map(|vector| relation_terms(&new.basis, vector))
            .collect(),
    }
}

pub(crate) fn coordinates_in_basis(
    vector: &SparseVector,
    basis: &[SparseVector],
    modulus: u64,
) -> Result<SparseVector> {
    let mut residual = vector.clone();
    let mut coordinates = SparseVector::default();
    for (position, basis_vector) in basis.iter().enumerate() {
        let pivot = basis_vector
            .leading()
            .expect("a reduced basis does not contain zero")
            .0;
        if let Some(&coefficient) = residual.0.get(&pivot) {
            coordinates.insert(position, coefficient);
            residual.add_scaled(basis_vector, modulus - u64::from(coefficient), modulus);
        }
    }
    if !residual.is_zero() {
        return Err(Error::InvalidInput(
            "restricted cocycle is outside the target cohomology basis".into(),
        ));
    }
    Ok(coordinates)
}

pub(crate) fn full_relation(
    old: &[SparseVector],
    new: &[SparseVector],
    coordinates: usize,
    modulus: u64,
) -> Vec<(SparseVector, SparseVector)> {
    let mut equations = vec![SparseVector::default(); coordinates];
    for (variable, image) in old.iter().enumerate() {
        for (&coordinate, &coefficient) in &image.0 {
            equations[coordinate].insert(variable, coefficient);
        }
    }
    for (offset, image) in new.iter().enumerate() {
        for (&coordinate, &coefficient) in &image.0 {
            equations[coordinate].insert(
                old.len() + offset,
                (modulus - u64::from(coefficient)) as u32,
            );
        }
    }
    nullspace(equations, old.len() + new.len(), modulus)
        .into_iter()
        .map(|relation| {
            let mut old_source = SparseVector::default();
            for (&position, &coefficient) in relation.0.range(..old.len()) {
                old_source.insert(position, coefficient);
            }
            let mut new_source = SparseVector::default();
            for (&position, &coefficient) in relation.0.range(old.len()..) {
                new_source.insert(position - old.len(), coefficient);
            }
            (old_source, new_source)
        })
        .collect()
}

pub(crate) fn relation_terms(
    basis: &[CohomologyClass],
    coefficients: &SparseVector,
) -> Vec<CohomologyRelationTerm> {
    coefficients
        .0
        .iter()
        .map(|(&position, &coefficient)| CohomologyRelationTerm {
            class: basis[position].id,
            coefficient,
        })
        .collect()
}

pub(crate) fn inverse_mod(value: u64, modulus: u64) -> u64 {
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
