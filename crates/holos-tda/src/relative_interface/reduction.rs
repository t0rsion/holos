use std::collections::{BTreeMap, BTreeSet};

use rustc_hash::FxHashMap;

use crate::certificate::{CertificateError, CertificateLimits, CertificateTerm, ChangeColumn};
use crate::{Bar, Diagram};

use super::chain::boundary_matrices;
use super::digest::inverse_mod;
use super::model::InterfaceCell;

#[derive(Debug, Clone, Default)]
pub(super) struct SparseColumn(BTreeMap<usize, u64>);

impl SparseColumn {
    pub(super) fn insert(&mut self, row: usize, coefficient: u64) {
        if coefficient != 0 {
            self.0.insert(row, coefficient);
        }
    }

    fn pivot(&self) -> Option<(usize, u64)> {
        self.0.last_key_value().map(|(&row, &value)| (row, value))
    }

    fn add_scaled(&mut self, source: &Self, factor: u64, modulus: u64) {
        for (&row, &value) in &source.0 {
            let next = (self.0.get(&row).copied().unwrap_or(0) + factor * value) % modulus;
            if next == 0 {
                self.0.remove(&row);
            } else {
                self.0.insert(row, next);
            }
        }
    }
}

pub(super) fn reduce_core(
    cells: &[Vec<InterfaceCell>],
    modulus: u32,
    limits: CertificateLimits,
) -> Result<(Vec<Vec<ChangeColumn>>, Diagram, usize), CertificateError> {
    let boundaries = boundary_matrices(cells, modulus)?;
    let mut columns = Vec::with_capacity(boundaries.len());
    let mut additions = 0;
    for matrix in &boundaries {
        let (next, count) = reduce_matrix(matrix, modulus, limits)?;
        columns.push(next);
        additions += count;
    }
    let (diagram, _) = check_reduction(cells, modulus, &columns, limits)?;
    Ok((columns, diagram, additions))
}

fn reduce_matrix(
    boundaries: &[SparseColumn],
    modulus: u32,
    limits: CertificateLimits,
) -> Result<(Vec<ChangeColumn>, usize), CertificateError> {
    let modulus64 = modulus as u64;
    let mut reduced: Vec<SparseColumn> = Vec::with_capacity(boundaries.len());
    let mut bases: Vec<SparseColumn> = Vec::with_capacity(boundaries.len());
    let mut owners: FxHashMap<usize, usize> = FxHashMap::default();
    let mut additions = 0;
    let mut terms = 0;
    for (target, boundary) in boundaries.iter().enumerate() {
        let mut column = boundary.clone();
        let mut basis = SparseColumn::default();
        basis.insert(target, 1);
        while let Some((pivot, coefficient)) = column.pivot() {
            let Some(&owner) = owners.get(&pivot) else {
                break;
            };
            let owner_coefficient = reduced[owner].pivot().unwrap().1;
            let factor = (modulus64
                - coefficient * inverse_mod(owner_coefficient, modulus64) % modulus64)
                % modulus64;
            column.add_scaled(&reduced[owner], factor, modulus64);
            basis.add_scaled(&bases[owner], factor, modulus64);
            additions += 1;
        }
        if let Some((pivot, _)) = column.pivot() {
            owners.insert(pivot, target);
        }
        terms += basis.0.len();
        if terms > limits.max_terms {
            return Err(CertificateError::new(
                "relative core reduction exceeds the term limit",
            ));
        }
        reduced.push(column);
        bases.push(basis);
    }
    Ok((
        bases
            .into_iter()
            .map(|basis| ChangeColumn {
                terms: basis
                    .0
                    .into_iter()
                    .map(|(index, coefficient)| CertificateTerm {
                        index,
                        coefficient: coefficient as u32,
                    })
                    .collect(),
            })
            .collect(),
        additions,
    ))
}

pub(super) fn check_reduction(
    cells: &[Vec<InterfaceCell>],
    modulus: u32,
    columns: &[Vec<ChangeColumn>],
    limits: CertificateLimits,
) -> Result<(Diagram, Vec<Vec<SparseColumn>>), CertificateError> {
    let boundaries = boundary_matrices(cells, modulus)?;
    check_reduction_dimension_count(columns, &boundaries)?;
    let mut total_terms = 0;
    let reduced = replay_reduction(&boundaries, columns, modulus, limits, &mut total_terms)?;
    let mut diagram = diagram_from_reduction(cells, columns, &reduced);
    check_bar_count(&diagram, limits.max_bars)?;
    diagram.canonicalize();
    Ok((diagram, reduced))
}

fn check_reduction_dimension_count(
    columns: &[Vec<ChangeColumn>],
    boundaries: &[Vec<SparseColumn>],
) -> Result<(), CertificateError> {
    if columns.len() != boundaries.len() {
        return Err(CertificateError::new(
            "relative reduction has the wrong dimension count",
        ));
    }
    Ok(())
}

fn replay_reduction(
    boundaries: &[Vec<SparseColumn>],
    columns: &[Vec<ChangeColumn>],
    modulus: u32,
    limits: CertificateLimits,
    total_terms: &mut usize,
) -> Result<Vec<Vec<SparseColumn>>, CertificateError> {
    let mut reduced = Vec::with_capacity(columns.len());
    for (dimension, (matrix, transforms)) in boundaries.iter().zip(columns).enumerate() {
        reduced.push(replay_reduction_dimension(
            matrix,
            transforms,
            dimension,
            modulus,
            limits.max_terms,
            total_terms,
        )?);
    }
    Ok(reduced)
}

fn replay_reduction_dimension(
    matrix: &[SparseColumn],
    transforms: &[ChangeColumn],
    dimension: usize,
    modulus: u32,
    term_limit: usize,
    total_terms: &mut usize,
) -> Result<Vec<SparseColumn>, CertificateError> {
    if matrix.len() != transforms.len() {
        return Err(CertificateError::new(format!(
            "relative boundary dimension {} has the wrong column count",
            dimension + 1
        )));
    }
    let mut reduced = Vec::with_capacity(matrix.len());
    let mut pivots = BTreeSet::new();
    for (target, transform) in transforms.iter().enumerate() {
        reduced.push(replay_transform(
            matrix,
            transform,
            target,
            modulus,
            term_limit,
            total_terms,
            &mut pivots,
        )?);
    }
    Ok(reduced)
}

fn replay_transform(
    matrix: &[SparseColumn],
    transform: &ChangeColumn,
    target: usize,
    modulus: u32,
    term_limit: usize,
    total_terms: &mut usize,
    pivots: &mut BTreeSet<usize>,
) -> Result<SparseColumn, CertificateError> {
    add_change_terms(total_terms, transform.terms.len(), term_limit)?;
    check_unit_triangular(transform, target)?;
    let column = apply_change_column(matrix, transform, target, modulus)?;
    check_unique_pivot(&column, pivots)?;
    Ok(column)
}

fn add_change_terms(total: &mut usize, count: usize, limit: usize) -> Result<(), CertificateError> {
    *total = total
        .checked_add(count)
        .ok_or_else(|| CertificateError::new("relative change term count overflows"))?;
    if *total > limit {
        return Err(CertificateError::new(
            "relative change of basis exceeds the term limit",
        ));
    }
    Ok(())
}

fn check_unit_triangular(transform: &ChangeColumn, target: usize) -> Result<(), CertificateError> {
    if transform.terms.last()
        != Some(&CertificateTerm {
            index: target,
            coefficient: 1,
        })
    {
        return Err(CertificateError::new(
            "relative change of basis is not bounded unit triangular",
        ));
    }
    Ok(())
}

fn apply_change_column(
    matrix: &[SparseColumn],
    transform: &ChangeColumn,
    target: usize,
    modulus: u32,
) -> Result<SparseColumn, CertificateError> {
    let mut previous = None;
    let mut column = SparseColumn::default();
    for term in &transform.terms {
        check_change_term(term, target, previous, modulus)?;
        previous = Some(term.index);
        column.add_scaled(&matrix[term.index], term.coefficient as u64, modulus as u64);
    }
    Ok(column)
}

fn check_change_term(
    term: &CertificateTerm,
    target: usize,
    previous: Option<usize>,
    modulus: u32,
) -> Result<(), CertificateError> {
    let invalid = term.index > target
        || previous.is_some_and(|value| value >= term.index)
        || term.coefficient == 0
        || term.coefficient >= modulus;
    if invalid {
        return Err(CertificateError::new(
            "relative change of basis is not canonical",
        ));
    }
    Ok(())
}

fn check_unique_pivot(
    column: &SparseColumn,
    pivots: &mut BTreeSet<usize>,
) -> Result<(), CertificateError> {
    if let Some((pivot, _)) = column.pivot() {
        if !pivots.insert(pivot) {
            return Err(CertificateError::new(
                "relative reduced matrix repeats a pivot",
            ));
        }
    }
    Ok(())
}

fn diagram_from_reduction(
    cells: &[Vec<InterfaceCell>],
    columns: &[Vec<ChangeColumn>],
    reduced: &[Vec<SparseColumn>],
) -> Diagram {
    let mut diagram = Diagram::default();
    for dimension in 0..columns.len() {
        add_dimension_bars(&mut diagram, cells, reduced, dimension);
    }
    diagram
}

fn add_dimension_bars(
    diagram: &mut Diagram,
    cells: &[Vec<InterfaceCell>],
    reduced: &[Vec<SparseColumn>],
    dimension: usize,
) {
    let births = birth_columns(cells, reduced, dimension);
    let deaths: FxHashMap<_, _> = reduced[dimension]
        .iter()
        .enumerate()
        .filter_map(|(column, value)| value.pivot().map(|(row, _)| (row, column)))
        .collect();
    for (position, is_birth) in births.into_iter().enumerate() {
        if is_birth {
            push_relative_bar(diagram, cells, dimension, position, &deaths);
        }
    }
}

fn birth_columns(
    cells: &[Vec<InterfaceCell>],
    reduced: &[Vec<SparseColumn>],
    dimension: usize,
) -> Vec<bool> {
    if dimension == 0 {
        return vec![true; cells[0].len()];
    }
    reduced[dimension - 1]
        .iter()
        .map(|column| column.0.is_empty())
        .collect()
}

fn push_relative_bar(
    diagram: &mut Diagram,
    cells: &[Vec<InterfaceCell>],
    dimension: usize,
    birth_position: usize,
    deaths: &FxHashMap<usize, usize>,
) {
    let birth = cells[dimension][birth_position].value;
    let death = deaths
        .get(&birth_position)
        .map_or(f64::INFINITY, |&position| {
            cells[dimension + 1][position].value
        });
    if death > birth {
        diagram.bars.push(Bar {
            dim: dimension,
            birth,
            death,
        });
    }
}

fn check_bar_count(diagram: &Diagram, limit: usize) -> Result<(), CertificateError> {
    if diagram.bars.len() > limit {
        return Err(CertificateError::new(
            "relative interface diagram exceeds the bar limit",
        ));
    }
    Ok(())
}
