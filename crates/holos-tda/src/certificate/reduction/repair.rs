//! Change-of-basis repair and simplex reindexing.

use std::collections::BTreeMap;

use super::super::model::{CertificateError, CertificateResult, CertificateTerm, ChangeColumn};

pub(crate) struct DimensionRepair {
    pub(crate) columns: Vec<ChangeColumn>,
    pub(crate) prefix: usize,
    pub(crate) additions: usize,
}

pub(crate) fn reindexed_prefix_candidates<const N: usize>(
    old_simplices: &[[usize; N]],
    new_simplices: &[[usize; N]],
    old_columns: &[ChangeColumn],
) -> std::result::Result<Vec<ChangeColumn>, CertificateError> {
    check_simplex_counts(old_simplices, new_simplices, old_columns, "repair")?;
    let old_positions = simplex_positions(old_simplices);
    let new_positions = simplex_positions(new_simplices);
    check_repair_simplex_sets(old_simplices, new_simplices, &old_positions, &new_positions)?;
    let mut columns = Vec::new();
    for (new_target, simplex) in new_simplices.iter().enumerate() {
        let old_target = old_positions[simplex];
        let Some(column) = reindexed_prefix_column(
            new_target,
            old_simplices,
            &new_positions,
            &old_columns[old_target],
        )?
        else {
            break;
        };
        columns.push(column);
    }
    Ok(columns)
}

fn simplex_positions<const N: usize>(simplices: &[[usize; N]]) -> BTreeMap<[usize; N], usize> {
    simplices
        .iter()
        .copied()
        .enumerate()
        .map(|(position, simplex)| (simplex, position))
        .collect()
}

fn check_simplex_counts<const N: usize>(
    old_simplices: &[[usize; N]],
    new_simplices: &[[usize; N]],
    old_columns: &[ChangeColumn],
    operation: &str,
) -> CertificateResult<()> {
    if old_simplices.len() != new_simplices.len() || old_columns.len() != old_simplices.len() {
        return Err(CertificateError::new(format!(
            "reduction {operation} has inconsistent simplex counts"
        )));
    }
    Ok(())
}

fn check_repair_simplex_sets<const N: usize>(
    old_simplices: &[[usize; N]],
    new_simplices: &[[usize; N]],
    old_positions: &BTreeMap<[usize; N], usize>,
    new_positions: &BTreeMap<[usize; N], usize>,
) -> CertificateResult<()> {
    if old_positions.len() != old_simplices.len()
        || new_positions.len() != new_simplices.len()
        || old_positions.keys().ne(new_positions.keys())
    {
        return Err(CertificateError::new(
            "reduction repair found a changed simplex set",
        ));
    }
    Ok(())
}

fn reindexed_prefix_column<const N: usize>(
    new_target: usize,
    old_simplices: &[[usize; N]],
    new_positions: &BTreeMap<[usize; N], usize>,
    old_column: &ChangeColumn,
) -> CertificateResult<Option<ChangeColumn>> {
    let mut terms = Vec::with_capacity(old_column.terms.len());
    for term in &old_column.terms {
        let source = old_simplices.get(term.index).ok_or_else(|| {
            CertificateError::new("reduction repair found an invalid source position")
        })?;
        let index = new_positions[source];
        if index > new_target {
            return Ok(None);
        }
        terms.push(CertificateTerm {
            index,
            coefficient: term.coefficient,
        });
    }
    terms.sort_unstable();
    check_distinct_terms(&terms, "reduction repair")?;
    Ok(Some(ChangeColumn { terms }))
}

fn check_distinct_terms(terms: &[CertificateTerm], operation: &str) -> CertificateResult<()> {
    if terms.windows(2).any(|pair| pair[0].index == pair[1].index) {
        return Err(CertificateError::new(format!(
            "{operation} produced duplicate source positions"
        )));
    }
    Ok(())
}

pub(crate) fn reindex_change_columns<const N: usize>(
    old_simplices: &[[usize; N]],
    new_simplices: &[[usize; N]],
    old_columns: &[ChangeColumn],
) -> std::result::Result<Vec<ChangeColumn>, CertificateError> {
    check_simplex_counts(old_simplices, new_simplices, old_columns, "reindexing")?;
    let old_positions = simplex_positions(old_simplices);
    let new_positions = simplex_positions(new_simplices);
    if old_positions.len() != old_simplices.len() || new_positions.len() != new_simplices.len() {
        return Err(CertificateError::new(
            "reduction reindexing found duplicate simplex identities",
        ));
    }
    let mut columns = Vec::with_capacity(new_simplices.len());
    for (new_target, simplex) in new_simplices.iter().enumerate() {
        let old_target = old_positions.get(simplex).copied().ok_or_else(|| {
            CertificateError::new("reduction reindexing found a changed simplex set")
        })?;
        columns.push(reindex_column(
            new_target,
            old_simplices,
            &new_positions,
            &old_columns[old_target],
        )?);
    }
    Ok(columns)
}

fn reindex_column<const N: usize>(
    new_target: usize,
    old_simplices: &[[usize; N]],
    new_positions: &BTreeMap<[usize; N], usize>,
    old_column: &ChangeColumn,
) -> CertificateResult<ChangeColumn> {
    let mut terms = Vec::with_capacity(old_column.terms.len());
    for term in &old_column.terms {
        let source = old_simplices.get(term.index).ok_or_else(|| {
            CertificateError::new("reduction reindexing found an invalid source position")
        })?;
        let index = new_positions.get(source).copied().ok_or_else(|| {
            CertificateError::new("reduction reindexing found a changed simplex set")
        })?;
        if index > new_target {
            return Err(CertificateError::new(
                "accepted reduction is not filtration-compatible after reindexing",
            ));
        }
        terms.push(CertificateTerm {
            index,
            coefficient: term.coefficient,
        });
    }
    terms.sort_unstable();
    check_distinct_terms(&terms, "reduction reindexing")?;
    Ok(ChangeColumn { terms })
}
