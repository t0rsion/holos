//! Sparse column arithmetic and reduction state.

use std::collections::BTreeMap;

use rustc_hash::FxHashMap;

use super::super::model::{
    CertificateError, CertificateLimits, CertificateResult, CertificateTerm, ChangeColumn,
};
use super::super::verify::inverse_mod;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SparseColumn(pub(crate) BTreeMap<usize, u64>);

impl SparseColumn {
    pub(crate) fn insert(&mut self, index: usize, coefficient: u64) {
        if coefficient != 0 {
            self.0.insert(index, coefficient);
        }
    }

    pub(crate) fn pivot(&self) -> Option<(usize, u64)> {
        self.0
            .last_key_value()
            .map(|(&index, &value)| (index, value))
    }

    pub(crate) fn add_scaled(&mut self, source: &Self, factor: u64, modulus: u64) {
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
}

pub(crate) fn reduce_with_basis(
    boundaries: &[SparseColumn],
    modulus: u32,
    limits: CertificateLimits,
) -> std::result::Result<Vec<ChangeColumn>, CertificateError> {
    reduce_with_prefix(boundaries, &[], modulus, limits).map(|(columns, _)| columns)
}

pub(crate) fn reduce_with_prefix(
    boundaries: &[SparseColumn],
    prefix: &[ChangeColumn],
    modulus: u32,
    limits: CertificateLimits,
) -> std::result::Result<(Vec<ChangeColumn>, usize), CertificateError> {
    if prefix.len() > boundaries.len() {
        return Err(CertificateError::new(
            "reduction prefix is longer than the boundary matrix",
        ));
    }
    let mut state = ReductionState::new(boundaries.len(), modulus as u64);
    for (index, transform) in prefix.iter().enumerate() {
        state.retain(index, transform, boundaries, limits.max_terms)?;
    }
    for (index, boundary) in boundaries.iter().enumerate().skip(prefix.len()) {
        state.reduce(index, boundary, limits.max_terms)?;
    }
    Ok(state.finish())
}

struct ReductionState {
    modulus: u64,
    reduced: Vec<SparseColumn>,
    basis: Vec<SparseColumn>,
    pivot_owner: FxHashMap<usize, usize>,
    total_terms: usize,
    additions: usize,
}

impl ReductionState {
    fn new(capacity: usize, modulus: u64) -> Self {
        Self {
            modulus,
            reduced: Vec::with_capacity(capacity),
            basis: Vec::with_capacity(capacity),
            pivot_owner: FxHashMap::default(),
            total_terms: 0,
            additions: 0,
        }
    }

    fn retain(
        &mut self,
        index: usize,
        transform: &ChangeColumn,
        boundaries: &[SparseColumn],
        max_terms: usize,
    ) -> CertificateResult<()> {
        let (column, basis_column) = retained_column(index, transform, boundaries, self.modulus)?;
        if let Some((pivot, _)) = column.pivot() {
            if self.pivot_owner.insert(pivot, index).is_some() {
                return Err(CertificateError::new(
                    "retained reduction prefix has duplicate pivots",
                ));
            }
        }
        self.add_terms(basis_column.0.len(), max_terms)?;
        self.reduced.push(column);
        self.basis.push(basis_column);
        Ok(())
    }

    fn reduce(
        &mut self,
        index: usize,
        boundary: &SparseColumn,
        max_terms: usize,
    ) -> CertificateResult<()> {
        let mut column = boundary.clone();
        let mut transform = SparseColumn::default();
        transform.insert(index, 1);
        while let Some((pivot, coefficient)) = column.pivot() {
            let Some(&owner) = self.pivot_owner.get(&pivot) else {
                break;
            };
            let owner_coefficient = self.reduced[owner]
                .pivot()
                .expect("pivot owner is nonempty")
                .1;
            let factor = (self.modulus
                - coefficient * inverse_mod(owner_coefficient, self.modulus) % self.modulus)
                % self.modulus;
            column.add_scaled(&self.reduced[owner], factor, self.modulus);
            transform.add_scaled(&self.basis[owner], factor, self.modulus);
            self.additions = self
                .additions
                .checked_add(1)
                .ok_or_else(|| CertificateError::new("column addition count overflows usize"))?;
        }
        if let Some((pivot, _)) = column.pivot() {
            self.pivot_owner.insert(pivot, index);
        }
        self.add_terms(transform.0.len(), max_terms)?;
        self.reduced.push(column);
        self.basis.push(transform);
        Ok(())
    }

    fn add_terms(&mut self, count: usize, maximum: usize) -> CertificateResult<()> {
        self.total_terms = self
            .total_terms
            .checked_add(count)
            .ok_or_else(|| CertificateError::new("change-of-basis term count overflows usize"))?;
        if self.total_terms > maximum {
            return Err(CertificateError::new(format!(
                "{} change-of-basis terms exceed the limit {maximum}",
                self.total_terms
            )));
        }
        Ok(())
    }

    fn finish(self) -> (Vec<ChangeColumn>, usize) {
        let columns = self
            .basis
            .into_iter()
            .map(change_column_from_sparse)
            .collect();
        (columns, self.additions)
    }
}

fn retained_column(
    index: usize,
    transform: &ChangeColumn,
    boundaries: &[SparseColumn],
    modulus: u64,
) -> CertificateResult<(SparseColumn, SparseColumn)> {
    let mut column = SparseColumn::default();
    let mut basis_column = SparseColumn::default();
    let mut previous = None;
    for term in &transform.terms {
        check_retained_term(index, term, previous, modulus)?;
        column.add_scaled(&boundaries[term.index], term.coefficient as u64, modulus);
        basis_column.insert(term.index, term.coefficient as u64);
        previous = Some(term.index);
    }
    if basis_column.0.get(&index) != Some(&1) {
        return Err(CertificateError::new(
            "retained reduction prefix is not unit triangular",
        ));
    }
    Ok((column, basis_column))
}

fn check_retained_term(
    target: usize,
    term: &CertificateTerm,
    previous: Option<usize>,
    modulus: u64,
) -> CertificateResult<()> {
    if term.index > target || previous.is_some_and(|value| value >= term.index) {
        return Err(CertificateError::new(
            "retained reduction prefix is not unit triangular",
        ));
    }
    if term.coefficient == 0 || term.coefficient as u64 >= modulus {
        return Err(CertificateError::new(
            "retained reduction prefix has an invalid coefficient",
        ));
    }
    Ok(())
}

fn change_column_from_sparse(column: SparseColumn) -> ChangeColumn {
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

pub(crate) fn valid_reduction_prefix_len(
    boundaries: &[SparseColumn],
    candidates: &[ChangeColumn],
    modulus: u32,
    limits: CertificateLimits,
) -> std::result::Result<usize, CertificateError> {
    let modulus = modulus as u64;
    let mut pivots = FxHashMap::<usize, usize>::default();
    let mut total_terms = 0usize;
    for (target, transform) in candidates.iter().enumerate() {
        let Some(reduced) = candidate_reduction(target, transform, boundaries, modulus)? else {
            return Ok(target);
        };
        if reduced
            .pivot()
            .is_some_and(|(pivot, _)| pivots.insert(pivot, target).is_some())
        {
            return Ok(target);
        }
        total_terms = total_terms
            .checked_add(transform.terms.len())
            .ok_or_else(|| CertificateError::new("change-of-basis term count overflows usize"))?;
        if total_terms > limits.max_terms {
            return Err(CertificateError::new(format!(
                "{total_terms} change-of-basis terms exceed the limit {}",
                limits.max_terms
            )));
        }
    }
    Ok(candidates.len())
}

fn candidate_reduction(
    target: usize,
    transform: &ChangeColumn,
    boundaries: &[SparseColumn],
    modulus: u64,
) -> CertificateResult<Option<SparseColumn>> {
    let mut reduced = SparseColumn::default();
    let mut previous = None;
    for term in &transform.terms {
        if term.index > target || previous.is_some_and(|value| value >= term.index) {
            return Ok(None);
        }
        if term.coefficient == 0 || term.coefficient as u64 >= modulus {
            return Err(CertificateError::new(
                "reduction repair found an invalid coefficient",
            ));
        }
        reduced.add_scaled(&boundaries[term.index], term.coefficient as u64, modulus);
        previous = Some(term.index);
    }
    let unit_diagonal = transform
        .terms
        .last()
        .is_some_and(|term| (term.index, term.coefficient) == (target, 1));
    Ok(unit_diagonal.then_some(reduced))
}
