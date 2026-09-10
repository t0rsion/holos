use std::collections::BTreeSet;

use rustc_hash::FxHashMap;

use super::model::{CheckedGraded, GradedComplex, SparseColumn};
use crate::certificate::{CertificateError, CertificateLimits, CertificateTerm, ChangeColumn};
use crate::{Bar, Diagram};

pub(crate) fn reduce_all_dimensions(
    complex: &GradedComplex,
    max_dim: usize,
    modulus: u32,
    limits: CertificateLimits,
) -> Result<Vec<Vec<ChangeColumn>>, CertificateError> {
    let mut columns = Vec::with_capacity(max_dim + 1);
    for dimension in 1..=max_dim + 1 {
        let boundaries = complex.boundaries(dimension, modulus)?;
        columns.push(reduce_with_prefix(&boundaries, &[], modulus, limits)?.0);
    }
    Ok(columns)
}
impl SparseColumn {
    pub(super) fn insert(&mut self, index: usize, coefficient: u64) {
        if coefficient != 0 {
            self.0.insert(index, coefficient);
        }
    }

    pub(super) fn pivot(&self) -> Option<(usize, u64)> {
        self.0
            .last_key_value()
            .map(|(&index, &value)| (index, value))
    }

    pub(super) fn add_scaled(&mut self, source: &Self, factor: u64, modulus: u64) {
        for (&index, &value) in &source.0 {
            let next = (self.0.get(&index).copied().unwrap_or(0) + factor * value) % modulus;
            if next == 0 {
                self.0.remove(&index);
            } else {
                self.0.insert(index, next);
            }
        }
    }
}
pub(crate) fn check_all(
    complex: &GradedComplex,
    modulus: u32,
    columns: &[Vec<ChangeColumn>],
    limits: CertificateLimits,
) -> Result<CheckedGraded, CertificateError> {
    if columns.len() + 1 != complex.simplices.len() {
        return Err(CertificateError::new(
            "graded reduction count differs from the filtered complex",
        ));
    }
    let mut reduced = Vec::with_capacity(columns.len());
    let mut total_terms = 0usize;
    for dimension in 1..complex.simplices.len() {
        reduced.push(check_matrix(
            dimension,
            &complex.boundaries(dimension, modulus)?,
            &columns[dimension - 1],
            modulus,
            limits,
            &mut total_terms,
        )?);
    }
    let mut diagram = Diagram::default();
    for homology_dimension in 0..columns.len() {
        let births = if homology_dimension == 0 {
            vec![true; complex.simplices[0].len()]
        } else {
            reduced[homology_dimension - 1]
                .iter()
                .map(|column| column.0.is_empty())
                .collect()
        };
        let deaths: FxHashMap<_, _> = reduced[homology_dimension]
            .iter()
            .enumerate()
            .filter_map(|(column, reduction)| reduction.pivot().map(|(row, _)| (row, column)))
            .collect();
        for (birth_position, is_birth) in births.into_iter().enumerate() {
            if !is_birth {
                continue;
            }
            let birth = complex.simplices[homology_dimension][birth_position].value;
            let death = deaths
                .get(&birth_position)
                .map_or(f64::INFINITY, |&position| {
                    complex.simplices[homology_dimension + 1][position].value
                });
            if death > birth {
                diagram.bars.push(Bar {
                    dim: homology_dimension,
                    birth,
                    death,
                });
            }
        }
    }
    diagram.canonicalize();
    Ok(CheckedGraded { diagram })
}

fn check_matrix(
    dimension: usize,
    boundaries: &[SparseColumn],
    columns: &[ChangeColumn],
    modulus: u32,
    limits: CertificateLimits,
    total_terms: &mut usize,
) -> Result<Vec<SparseColumn>, CertificateError> {
    if boundaries.len() != columns.len() {
        return Err(CertificateError::new(format!(
            "dimension {dimension} has {} boundary columns but the certificate records {}",
            boundaries.len(),
            columns.len()
        )));
    }
    let modulus64 = modulus as u64;
    let mut reduced = Vec::with_capacity(columns.len());
    let mut pivots = BTreeSet::new();
    for (target, transform) in columns.iter().enumerate() {
        *total_terms = total_terms
            .checked_add(transform.terms.len())
            .ok_or_else(|| CertificateError::new("graded certificate term count overflows"))?;
        if *total_terms > limits.max_terms {
            return Err(CertificateError::new(format!(
                "{} graded certificate terms exceed the limit {}",
                *total_terms, limits.max_terms
            )));
        }
        validate_change_column(dimension, target, transform, modulus)?;
        let mut column = SparseColumn::default();
        for term in &transform.terms {
            column.add_scaled(&boundaries[term.index], term.coefficient as u64, modulus64);
        }
        if let Some((pivot, _)) = column.pivot() {
            if !pivots.insert(pivot) {
                return Err(CertificateError::new(format!(
                    "dimension {dimension} reduction repeats pivot {pivot}"
                )));
            }
        }
        reduced.push(column);
    }
    Ok(reduced)
}

pub(super) fn validate_change_column(
    dimension: usize,
    target: usize,
    column: &ChangeColumn,
    modulus: u32,
) -> Result<(), CertificateError> {
    if column.terms.is_empty()
        || column.terms.last()
            != Some(&CertificateTerm {
                index: target,
                coefficient: 1,
            })
    {
        return Err(CertificateError::new(format!(
            "dimension {dimension} change column {target} is not unit triangular"
        )));
    }
    let mut previous = None;
    for term in &column.terms {
        if term.index > target
            || previous.is_some_and(|value| value >= term.index)
            || term.coefficient == 0
            || term.coefficient >= modulus
        {
            return Err(CertificateError::new(format!(
                "dimension {dimension} change column {target} is not canonical"
            )));
        }
        previous = Some(term.index);
    }
    Ok(())
}

pub(super) fn reduce_with_prefix(
    boundaries: &[SparseColumn],
    prefix: &[ChangeColumn],
    modulus: u32,
    limits: CertificateLimits,
) -> Result<(Vec<ChangeColumn>, usize), CertificateError> {
    if prefix.len() > boundaries.len() {
        return Err(CertificateError::new(
            "graded reduction prefix exceeds its boundary matrix",
        ));
    }
    let mut state = GradedReductionState::new(boundaries.len(), modulus);
    for (target, transform) in prefix.iter().enumerate() {
        state.retain(target, transform, boundaries, limits.max_terms)?;
    }
    for (target, boundary) in boundaries.iter().enumerate().skip(prefix.len()) {
        state.reduce(target, boundary, limits.max_terms)?;
    }
    Ok(state.finish())
}

struct GradedReductionState {
    modulus: u32,
    reduced: Vec<SparseColumn>,
    bases: Vec<SparseColumn>,
    owners: FxHashMap<usize, usize>,
    term_count: usize,
    additions: usize,
}

impl GradedReductionState {
    fn new(capacity: usize, modulus: u32) -> Self {
        Self {
            modulus,
            reduced: Vec::with_capacity(capacity),
            bases: Vec::with_capacity(capacity),
            owners: FxHashMap::default(),
            term_count: 0,
            additions: 0,
        }
    }

    fn retain(
        &mut self,
        target: usize,
        transform: &ChangeColumn,
        boundaries: &[SparseColumn],
        maximum: usize,
    ) -> Result<(), CertificateError> {
        validate_change_column(0, target, transform, self.modulus)?;
        let (column, basis) = apply_transform(transform, boundaries, self.modulus as u64);
        if column
            .pivot()
            .is_some_and(|(pivot, _)| self.owners.insert(pivot, target).is_some())
        {
            return Err(CertificateError::new(
                "graded reduction prefix repeats a pivot",
            ));
        }
        self.add_prefix_terms(basis.0.len(), maximum)?;
        self.reduced.push(column);
        self.bases.push(basis);
        Ok(())
    }

    fn reduce(
        &mut self,
        target: usize,
        boundary: &SparseColumn,
        maximum: usize,
    ) -> Result<(), CertificateError> {
        let modulus = self.modulus as u64;
        let mut column = boundary.clone();
        let mut basis = SparseColumn::default();
        basis.insert(target, 1);
        while let Some((pivot, coefficient)) = column.pivot() {
            let Some(&owner) = self.owners.get(&pivot) else {
                break;
            };
            let owner_coefficient = self.reduced[owner].pivot().expect("owner has a pivot").1;
            let factor = (modulus
                - coefficient * inverse_mod(owner_coefficient, modulus) % modulus)
                % modulus;
            column.add_scaled(&self.reduced[owner], factor, modulus);
            basis.add_scaled(&self.bases[owner], factor, modulus);
            self.additions += 1;
        }
        if let Some((pivot, _)) = column.pivot() {
            self.owners.insert(pivot, target);
        }
        self.add_terms(basis.0.len(), maximum)?;
        self.reduced.push(column);
        self.bases.push(basis);
        Ok(())
    }

    fn add_prefix_terms(&mut self, count: usize, maximum: usize) -> Result<(), CertificateError> {
        self.term_count += count;
        if self.term_count > maximum {
            return Err(CertificateError::new(
                "graded reduction prefix exceeds the term limit",
            ));
        }
        Ok(())
    }

    fn add_terms(&mut self, count: usize, maximum: usize) -> Result<(), CertificateError> {
        self.term_count += count;
        if self.term_count > maximum {
            return Err(CertificateError::new(format!(
                "{} graded change terms exceed the limit {maximum}",
                self.term_count
            )));
        }
        Ok(())
    }

    fn finish(self) -> (Vec<ChangeColumn>, usize) {
        let columns = self.bases.into_iter().map(sparse_change_column).collect();
        (columns, self.additions)
    }
}

pub(super) fn apply_transform(
    transform: &ChangeColumn,
    boundaries: &[SparseColumn],
    modulus: u64,
) -> (SparseColumn, SparseColumn) {
    let mut column = SparseColumn::default();
    let mut basis = SparseColumn::default();
    for term in &transform.terms {
        column.add_scaled(&boundaries[term.index], term.coefficient as u64, modulus);
        basis.insert(term.index, term.coefficient as u64);
    }
    (column, basis)
}

fn sparse_change_column(column: SparseColumn) -> ChangeColumn {
    ChangeColumn {
        terms: column
            .0
            .into_iter()
            .map(|(index, coefficient)| CertificateTerm {
                index,
                coefficient: coefficient as u32,
            })
            .collect(),
    }
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
