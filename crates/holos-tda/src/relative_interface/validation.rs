use std::collections::{BTreeMap, BTreeSet};

use crate::certificate::{CertificateError, CertificateLimits};
use crate::field::{MODULUS_LIMIT, is_prime};
use crate::{RipsParams, SparseDistanceMatrix};

use super::model::InterfaceCell;

pub(super) fn check_interface_parameters(
    max_dim: usize,
    modulus: u32,
    limits: CertificateLimits,
) -> Result<(), CertificateError> {
    if max_dim > limits.max_dimension || u64::from(modulus) >= MODULUS_LIMIT {
        return Err(CertificateError::new(
            "relative interface exceeds the dimension or modulus limit",
        ));
    }
    if !is_prime(modulus as u64) {
        return Err(CertificateError::new(
            "relative interface modulus is not prime",
        ));
    }
    Ok(())
}
pub(super) fn check_protected_cells(
    input: &[Vec<InterfaceCell>],
    core: &[Vec<InterfaceCell>],
    protected: &BTreeSet<usize>,
) -> Result<(), CertificateError> {
    for (input_dimension, core_dimension) in input.iter().zip(core) {
        let core_map: BTreeMap<_, _> = core_dimension
            .iter()
            .map(|cell| (&cell.vertices, cell))
            .collect();
        for cell in input_dimension {
            if is_protected(&cell.vertices, protected)
                && core_map.get(&cell.vertices).copied() != Some(cell)
            {
                return Err(CertificateError::new(
                    "relative interface does not fix its protected subcomplex",
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn is_protected(cell: &[usize], protected: &BTreeSet<usize>) -> bool {
    !protected.is_empty() && cell.iter().all(|vertex| protected.contains(vertex))
}

pub(super) fn canonical_vertices(vertices: &[usize]) -> Result<Vec<usize>, CertificateError> {
    let mut output = vertices.to_vec();
    output.sort_unstable();
    output.dedup();
    if output.len() != vertices.len() {
        return Err(CertificateError::new(
            "protected vertex list contains a duplicate",
        ));
    }
    Ok(output)
}

pub(super) fn validate_parameters(
    input: &SparseDistanceMatrix,
    labels: &[usize],
    params: &RipsParams,
    protected: &[usize],
    limits: CertificateLimits,
) -> Result<(), CertificateError> {
    if labels.len() != input.len()
        || labels.windows(2).any(|pair| pair[0] >= pair[1])
        || labels.len() > limits.max_vertices
    {
        return Err(CertificateError::new(
            "relative interface labels must be unique, ordered, and bounded",
        ));
    }
    let label_set: BTreeSet<_> = labels.iter().copied().collect();
    if protected.iter().any(|vertex| !label_set.contains(vertex)) {
        return Err(CertificateError::new(
            "protected vertex is outside the interface scope",
        ));
    }
    if params.max_dim > limits.max_dimension
        || u64::from(params.modulus) >= MODULUS_LIMIT
        || !is_prime(params.modulus as u64)
    {
        return Err(CertificateError::new(
            "relative interface dimension or coefficient field is invalid",
        ));
    }
    checked_threshold(params.threshold)?;
    Ok(())
}

fn checked_threshold(threshold: Option<f64>) -> Result<f64, CertificateError> {
    let value = threshold.unwrap_or(f64::INFINITY);
    if value.is_nan() || value < 0.0 {
        return Err(CertificateError::new(
            "relative interface threshold must be non-negative",
        ));
    }
    Ok(value)
}

pub(super) fn enforce_cell_limits(
    cells: &[Vec<InterfaceCell>],
    limits: CertificateLimits,
) -> Result<(), CertificateError> {
    for (dimension, values) in cells.iter().enumerate() {
        enforce_dimension_limit(dimension, values.len(), limits)?;
    }
    Ok(())
}

fn enforce_dimension_limit(
    dimension: usize,
    count: usize,
    limits: CertificateLimits,
) -> Result<(), CertificateError> {
    let limit = match dimension {
        0 => limits.max_vertices,
        1 => limits.max_edges,
        2 => limits.max_triangles,
        _ => limits.max_higher_simplices,
    };
    if count > limit {
        return Err(CertificateError::new(format!(
            "relative dimension {dimension} cell count exceeds the limit {limit}"
        )));
    }
    Ok(())
}

pub(super) fn count_cells(cells: &[Vec<InterfaceCell>]) -> usize {
    cells.iter().map(Vec::len).sum()
}

pub(super) fn cell_order(left: &InterfaceCell, right: &InterfaceCell) -> std::cmp::Ordering {
    left.value
        .total_cmp(&right.value)
        .then_with(|| right.vertices.iter().rev().cmp(left.vertices.iter().rev()))
}
