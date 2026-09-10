use std::cmp::Ordering;
use std::collections::BTreeMap;

use super::model::{FilteredSimplex, GradedComplex, SimplexKey, SparseColumn};
use crate::SparseDistanceMatrix;
use crate::certificate::{CertificateError, CertificateLimits};
use crate::filtration::{FilteredSimplicialComplex, ScalarGrade};

impl GradedComplex {
    pub(super) fn build(
        input: &SparseDistanceMatrix,
        max_dim: usize,
        threshold: f64,
        limits: CertificateLimits,
    ) -> Result<Self, CertificateError> {
        let vertices: Vec<_> = (0..input.len())
            .map(|vertex| FilteredSimplex {
                key: SimplexKey(vec![vertex]),
                value: 0.0,
            })
            .collect();
        let mut simplices = vec![vertices];
        for dimension in 1..=max_dim + 1 {
            let mut next = next_simplices(
                input,
                &simplices[dimension - 1],
                dimension,
                threshold,
                simplex_limit(dimension, limits),
            )?;
            next.sort_by(filtered_simplex_order);
            simplices.push(next);
        }
        let rows = simplex_rows(&simplices);
        Ok(Self { simplices, rows })
    }

    pub(crate) fn from_filtered(
        input: &FilteredSimplicialComplex<ScalarGrade>,
        max_dim: usize,
        limits: CertificateLimits,
    ) -> Result<Self, CertificateError> {
        if max_dim > limits.max_dimension || input.max_dimension() < max_dim + 1 {
            return Err(CertificateError::new(
                "explicit complex does not cover the requested homology dimensions",
            ));
        }
        if input.vertex_labels().len() > limits.max_vertices {
            return Err(CertificateError::new(
                "explicit complex exceeds the vertex limit",
            ));
        }
        let mut simplices = Vec::with_capacity(max_dim + 2);
        for dimension in 0..=max_dim + 1 {
            let source = &input.simplices()[dimension];
            if source.len() > explicit_simplex_limit(dimension, limits) {
                return Err(CertificateError::new(format!(
                    "explicit complex dimension {dimension} exceeds its simplex limit"
                )));
            }
            let mut ordered = source
                .iter()
                .map(|simplex| FilteredSimplex {
                    key: SimplexKey(simplex.vertices().to_vec()),
                    value: simplex.grade().value(),
                })
                .collect::<Vec<_>>();
            ordered.sort_by(filtered_simplex_order);
            simplices.push(ordered);
        }
        let rows = simplex_rows(&simplices);
        Ok(Self { simplices, rows })
    }

    pub(super) fn boundaries(
        &self,
        dimension: usize,
        modulus: u32,
    ) -> Result<Vec<SparseColumn>, CertificateError> {
        let modulus = modulus as u64;
        self.simplices[dimension]
            .iter()
            .map(|simplex| {
                let mut column = SparseColumn::default();
                for removed in 0..simplex.key.0.len() {
                    let mut face = simplex.key.0.clone();
                    face.remove(removed);
                    let row = self.rows[dimension - 1]
                        .get(&SimplexKey(face))
                        .copied()
                        .ok_or_else(|| CertificateError::new("simplex boundary omits a face"))?;
                    column.insert(row, if removed % 2 == 0 { 1 } else { modulus - 1 });
                }
                Ok(column)
            })
            .collect()
    }
}

fn filtered_simplex_order(left: &FilteredSimplex, right: &FilteredSimplex) -> Ordering {
    left.value
        .total_cmp(&right.value)
        .then_with(|| right.key.0.iter().rev().cmp(left.key.0.iter().rev()))
}

fn simplex_rows(simplices: &[Vec<FilteredSimplex>]) -> Vec<BTreeMap<SimplexKey, usize>> {
    simplices
        .iter()
        .map(|dimension| {
            dimension
                .iter()
                .enumerate()
                .map(|(position, simplex)| (simplex.key.clone(), position))
                .collect()
        })
        .collect()
}

fn explicit_simplex_limit(dimension: usize, limits: CertificateLimits) -> usize {
    match dimension {
        0 => limits.max_vertices,
        _ => simplex_limit(dimension, limits),
    }
}

fn simplex_limit(dimension: usize, limits: CertificateLimits) -> usize {
    match dimension {
        1 => limits.max_edges,
        2 => limits.max_triangles,
        _ => limits.max_higher_simplices,
    }
}

fn next_simplices(
    input: &SparseDistanceMatrix,
    previous: &[FilteredSimplex],
    dimension: usize,
    threshold: f64,
    limit: usize,
) -> Result<Vec<FilteredSimplex>, CertificateError> {
    let mut next = Vec::new();
    for simplex in previous {
        extend_simplex(input, simplex, dimension, threshold, limit, &mut next)?;
    }
    Ok(next)
}

fn extend_simplex(
    input: &SparseDistanceMatrix,
    simplex: &FilteredSimplex,
    dimension: usize,
    threshold: f64,
    limit: usize,
    next: &mut Vec<FilteredSimplex>,
) -> Result<(), CertificateError> {
    let start = simplex.key.0.last().copied().unwrap_or(0) + 1;
    for vertex in start..input.len() {
        if let Some(extension) = simplex_extension(input, simplex, vertex, threshold) {
            next.push(extension);
            if next.len() > limit {
                return Err(CertificateError::new(format!(
                    "dimension {dimension} simplex count exceeds the limit {limit}"
                )));
            }
        }
    }
    Ok(())
}

fn simplex_extension(
    input: &SparseDistanceMatrix,
    simplex: &FilteredSimplex,
    vertex: usize,
    threshold: f64,
) -> Option<FilteredSimplex> {
    let mut value = simplex.value;
    for &member in &simplex.key.0 {
        let edge = input.get(member, vertex);
        if !edge.is_finite() || edge > threshold {
            return None;
        }
        value = value.max(edge);
    }
    let mut key = simplex.key.0.clone();
    key.push(vertex);
    Some(FilteredSimplex {
        key: SimplexKey(key),
        value,
    })
}
