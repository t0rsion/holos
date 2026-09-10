use std::collections::{BTreeMap, BTreeSet};

use crate::{ProofBar, ProofError, ProofLimits};

use super::model::{DecodedExplicit, Simplex, SparseColumn};
use super::proof_error;

pub(crate) fn verify_decoded(
    decoded: DecodedExplicit,
    limits: ProofLimits,
) -> Result<super::VerifiedExplicitPersistence, ProofError> {
    let ordered = order_complex(&decoded.complex);
    let boundaries = build_boundaries(&ordered, decoded.modulus)?;
    let reduced = verify_reductions(
        &boundaries,
        &decoded.columns,
        decoded.modulus,
        limits.max_terms,
    )?;
    let bars = derive_bars(&ordered, &reduced);
    if !same_bars(&bars, &decoded.bars) {
        return Err(proof_error(
            "explicit diagram differs from the checked reductions",
        ));
    }
    let simplex_counts = decoded.complex.iter().map(Vec::len).collect();
    let change_columns = decoded.columns.iter().map(Vec::len).sum();
    Ok(super::VerifiedExplicitPersistence {
        max_homology_dimension: decoded.max_homology_dimension,
        modulus: decoded.modulus,
        vertices: decoded.labels.len(),
        simplex_counts,
        change_columns,
        change_terms: decoded.change_terms,
        bars,
    })
}

fn order_complex(complex: &[Vec<Simplex>]) -> Vec<Vec<Simplex>> {
    complex
        .iter()
        .map(|dimension| {
            let mut ordered = dimension.clone();
            ordered.sort_by(|left, right| {
                left.grade
                    .total_cmp(&right.grade)
                    .then_with(|| right.vertices.iter().rev().cmp(left.vertices.iter().rev()))
            });
            ordered
        })
        .collect()
}

fn build_boundaries(
    complex: &[Vec<Simplex>],
    modulus: u32,
) -> Result<Vec<Vec<SparseColumn>>, ProofError> {
    let rows = complex
        .iter()
        .map(|dimension| {
            dimension
                .iter()
                .enumerate()
                .map(|(position, simplex)| (simplex.vertices.clone(), position))
                .collect::<BTreeMap<_, _>>()
        })
        .collect::<Vec<_>>();
    let mut boundaries = Vec::with_capacity(complex.len().saturating_sub(1));
    for dimension in 1..complex.len() {
        boundaries.push(boundary_dimension(
            &complex[dimension],
            &rows[dimension - 1],
            modulus,
        )?);
    }
    Ok(boundaries)
}

fn boundary_dimension(
    simplices: &[Simplex],
    rows: &BTreeMap<Vec<usize>, usize>,
    modulus: u32,
) -> Result<Vec<SparseColumn>, ProofError> {
    simplices
        .iter()
        .map(|simplex| simplex_boundary(simplex, rows, modulus))
        .collect()
}

fn simplex_boundary(
    simplex: &Simplex,
    rows: &BTreeMap<Vec<usize>, usize>,
    modulus: u32,
) -> Result<SparseColumn, ProofError> {
    let mut column = SparseColumn::default();
    for removed in 0..simplex.vertices.len() {
        let mut face = simplex.vertices.clone();
        face.remove(removed);
        let row = rows
            .get(&face)
            .copied()
            .ok_or_else(|| proof_error("explicit simplex boundary omits a face"))?;
        let coefficient = if removed % 2 == 0 {
            1
        } else {
            u64::from(modulus - 1)
        };
        column.insert(row, coefficient);
    }
    Ok(column)
}

fn verify_reductions(
    boundaries: &[Vec<SparseColumn>],
    dimensions: &[Vec<super::model::ChangeColumn>],
    modulus: u32,
    maximum_terms: usize,
) -> Result<Vec<Vec<SparseColumn>>, ProofError> {
    if boundaries.len() != dimensions.len() {
        return Err(proof_error(
            "explicit reduction count differs from the complex",
        ));
    }
    let mut total_terms = 0usize;
    boundaries
        .iter()
        .zip(dimensions)
        .enumerate()
        .map(|(offset, (boundary, columns))| {
            verify_matrix(
                offset + 1,
                boundary,
                columns,
                modulus,
                maximum_terms,
                &mut total_terms,
            )
        })
        .collect()
}

fn verify_matrix(
    dimension: usize,
    boundaries: &[SparseColumn],
    columns: &[super::model::ChangeColumn],
    modulus: u32,
    maximum_terms: usize,
    total_terms: &mut usize,
) -> Result<Vec<SparseColumn>, ProofError> {
    if boundaries.len() != columns.len() {
        return Err(proof_error(format!(
            "explicit dimension {dimension} has the wrong change column count"
        )));
    }
    let mut pivots = BTreeSet::new();
    let mut reduced = Vec::with_capacity(columns.len());
    for (target, transform) in columns.iter().enumerate() {
        charge_terms(transform.terms.len(), maximum_terms, total_terms)?;
        validate_change_column(dimension, target, transform, modulus)?;
        let column = apply_change_column(transform, boundaries, modulus);
        if column
            .pivot()
            .is_some_and(|(pivot, _)| !pivots.insert(pivot))
        {
            return Err(proof_error(format!(
                "explicit dimension {dimension} repeats a reduced pivot"
            )));
        }
        reduced.push(column);
    }
    Ok(reduced)
}

fn charge_terms(count: usize, maximum: usize, total: &mut usize) -> Result<(), ProofError> {
    *total = total
        .checked_add(count)
        .ok_or_else(|| proof_error("explicit change term count overflows"))?;
    if *total > maximum {
        return Err(proof_error(
            "explicit change terms exceed their verification limit",
        ));
    }
    Ok(())
}

fn validate_change_column(
    dimension: usize,
    target: usize,
    column: &super::model::ChangeColumn,
    modulus: u32,
) -> Result<(), ProofError> {
    let unit = column
        .terms
        .last()
        .is_some_and(|term| term.index == target && term.coefficient == 1);
    if column.terms.is_empty() || !unit {
        return Err(proof_error(format!(
            "explicit dimension {dimension} change column {target} is not unit triangular"
        )));
    }
    let canonical = column
        .terms
        .iter()
        .all(|term| term.index <= target && term.coefficient > 0 && term.coefficient < modulus)
        && column
            .terms
            .windows(2)
            .all(|pair| pair[0].index < pair[1].index);
    if !canonical {
        return Err(proof_error(format!(
            "explicit dimension {dimension} change column {target} is not canonical"
        )));
    }
    Ok(())
}

fn apply_change_column(
    transform: &super::model::ChangeColumn,
    boundaries: &[SparseColumn],
    modulus: u32,
) -> SparseColumn {
    let mut column = SparseColumn::default();
    for term in &transform.terms {
        column.add_scaled(
            &boundaries[term.index],
            u64::from(term.coefficient),
            u64::from(modulus),
        );
    }
    column
}

fn derive_bars(complex: &[Vec<Simplex>], reduced: &[Vec<SparseColumn>]) -> Vec<ProofBar> {
    let mut bars = Vec::new();
    for dimension in 0..reduced.len() {
        let births = birth_columns(dimension, complex[dimension].len(), reduced);
        let deaths = death_columns(&reduced[dimension]);
        append_bars(dimension, &births, &deaths, complex, &mut bars);
    }
    bars.sort_by(compare_bars);
    bars
}

fn birth_columns(dimension: usize, count: usize, reduced: &[Vec<SparseColumn>]) -> Vec<bool> {
    if dimension == 0 {
        vec![true; count]
    } else {
        reduced[dimension - 1]
            .iter()
            .map(|column| column.0.is_empty())
            .collect()
    }
}

fn death_columns(reduced: &[SparseColumn]) -> BTreeMap<usize, usize> {
    reduced
        .iter()
        .enumerate()
        .filter_map(|(column, reduction)| reduction.pivot().map(|(row, _)| (row, column)))
        .collect()
}

fn append_bars(
    dimension: usize,
    births: &[bool],
    deaths: &BTreeMap<usize, usize>,
    complex: &[Vec<Simplex>],
    output: &mut Vec<ProofBar>,
) {
    for (position, &is_birth) in births.iter().enumerate() {
        if !is_birth {
            continue;
        }
        let birth = complex[dimension][position].grade;
        let death = deaths.get(&position).map_or(f64::INFINITY, |&column| {
            complex[dimension + 1][column].grade
        });
        if death > birth {
            output.push(ProofBar {
                dimension,
                birth,
                death,
            });
        }
    }
}

fn compare_bars(left: &ProofBar, right: &ProofBar) -> std::cmp::Ordering {
    left.dimension
        .cmp(&right.dimension)
        .then(left.birth.total_cmp(&right.birth))
        .then(left.death.total_cmp(&right.death))
}

fn same_bars(left: &[ProofBar], right: &[ProofBar]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.dimension == right.dimension
                && left.birth.to_bits() == right.birth.to_bits()
                && left.death.to_bits() == right.death.to_bits()
        })
}
