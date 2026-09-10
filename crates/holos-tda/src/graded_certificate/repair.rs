use std::collections::{BTreeMap, BTreeSet};

use super::model::{
    FilteredSimplex, GradedComplex, GradedDimensionWork, GradedReductionRepairWork, SparseColumn,
};
use super::reduction::{reduce_with_prefix, validate_change_column};
use super::validation::checked_threshold;
use crate::SparseDistanceMatrix;
use crate::certificate::{
    CertificateError, CertificateLimits, CertificateTerm, ChangeColumn, ReductionRepairMode,
};

pub(super) fn repair_all_dimensions(
    old: &GradedComplex,
    new: &GradedComplex,
    old_columns: &[Vec<ChangeColumn>],
    modulus: u32,
    limits: CertificateLimits,
) -> Result<(Vec<Vec<ChangeColumn>>, Vec<GradedDimensionWork>), CertificateError> {
    let mut columns = Vec::with_capacity(old_columns.len());
    let mut work = Vec::with_capacity(old_columns.len());
    for dimension in 1..old.simplices.len() {
        let (next, dimension_work) = repair_dimension(
            dimension,
            old,
            new,
            &old_columns[dimension - 1],
            modulus,
            limits,
        )?;
        columns.push(next);
        work.push(dimension_work);
    }
    Ok((columns, work))
}

fn repair_dimension(
    dimension: usize,
    old: &GradedComplex,
    new: &GradedComplex,
    old_columns: &[ChangeColumn],
    modulus: u32,
    limits: CertificateLimits,
) -> Result<(Vec<ChangeColumn>, GradedDimensionWork), CertificateError> {
    let boundaries = new.boundaries(dimension, modulus)?;
    let candidates = reindexed_prefix_candidates(
        &old.simplices[dimension],
        &new.simplices[dimension],
        old_columns,
    )?;
    let prefix = valid_prefix_len(&boundaries, &candidates, modulus, limits)?;
    let (columns, additions) =
        reduce_with_prefix(&boundaries, &candidates[..prefix], modulus, limits)?;
    let work = GradedDimensionWork {
        simplex_dimension: dimension,
        columns_reused: prefix,
        columns_reduced: boundaries.len() - prefix,
        column_additions: additions,
    };
    Ok((columns, work))
}

pub(super) fn graded_repair_mode(work: &GradedReductionRepairWork) -> ReductionRepairMode {
    if work.columns_reduced() == 0 {
        ReductionRepairMode::Reused
    } else if work.columns_reused() == 0 {
        ReductionRepairMode::Rebuilt
    } else {
        ReductionRepairMode::SuffixRepaired
    }
}
fn valid_prefix_len(
    boundaries: &[SparseColumn],
    candidates: &[ChangeColumn],
    modulus: u32,
    limits: CertificateLimits,
) -> Result<usize, CertificateError> {
    let modulus64 = modulus as u64;
    let mut pivots = BTreeSet::new();
    let mut terms = 0usize;
    for (target, transform) in candidates.iter().enumerate() {
        validate_change_column(0, target, transform, modulus)?;
        terms += transform.terms.len();
        if terms > limits.max_terms {
            return Err(CertificateError::new(
                "graded repair candidates exceed the term limit",
            ));
        }
        let mut column = SparseColumn::default();
        for term in &transform.terms {
            column.add_scaled(&boundaries[term.index], term.coefficient as u64, modulus64);
        }
        if let Some((pivot, _)) = column.pivot() {
            if !pivots.insert(pivot) {
                return Ok(target);
            }
        }
    }
    Ok(candidates.len())
}

fn reindexed_prefix_candidates(
    old_simplices: &[FilteredSimplex],
    new_simplices: &[FilteredSimplex],
    old_columns: &[ChangeColumn],
) -> Result<Vec<ChangeColumn>, CertificateError> {
    if old_simplices.len() != old_columns.len() || old_simplices.len() != new_simplices.len() {
        return Err(CertificateError::new(
            "graded repair requires an unchanged simplex set",
        ));
    }
    let old_positions: BTreeMap<_, _> = old_simplices
        .iter()
        .enumerate()
        .map(|(position, simplex)| (simplex.key.clone(), position))
        .collect();
    let new_positions: BTreeMap<_, _> = new_simplices
        .iter()
        .enumerate()
        .map(|(position, simplex)| (simplex.key.clone(), position))
        .collect();
    if old_positions.keys().ne(new_positions.keys()) {
        return Err(CertificateError::new(
            "graded repair requires an unchanged simplex set",
        ));
    }
    let mut output = Vec::with_capacity(new_simplices.len());
    for (new_target, simplex) in new_simplices.iter().enumerate() {
        let old_target = old_positions[&simplex.key];
        let mut terms = Vec::with_capacity(old_columns[old_target].terms.len());
        for term in &old_columns[old_target].terms {
            let source = &old_simplices[term.index].key;
            let index = new_positions[source];
            if index > new_target {
                return Ok(output);
            }
            terms.push(CertificateTerm {
                index,
                coefficient: term.coefficient,
            });
        }
        terms.sort_unstable();
        if terms.windows(2).any(|pair| pair[0].index == pair[1].index) {
            return Err(CertificateError::new(
                "graded repair produced duplicate source positions",
            ));
        }
        output.push(ChangeColumn { terms });
    }
    Ok(output)
}

pub(super) fn require_fixed_envelope(
    current: &SparseDistanceMatrix,
    updated: &SparseDistanceMatrix,
    threshold: Option<f64>,
) -> Result<(), CertificateError> {
    if current.len() != updated.len() {
        return Err(CertificateError::new(
            "graded repair requires an unchanged vertex set",
        ));
    }
    let current_edges: Vec<_> = current.edges().map(|(u, v, _)| (u, v)).collect();
    let updated_edges: Vec<_> = updated.edges().map(|(u, v, _)| (u, v)).collect();
    if current_edges != updated_edges {
        return Err(CertificateError::new(
            "graded repair requires an unchanged listed edge set",
        ));
    }
    let threshold = checked_threshold(threshold)?;
    if current
        .edges()
        .zip(updated.edges())
        .any(|((_, _, old), (_, _, new))| (old <= threshold) != (new <= threshold))
    {
        return Err(CertificateError::new(
            "graded repair requires unchanged threshold membership",
        ));
    }
    Ok(())
}
