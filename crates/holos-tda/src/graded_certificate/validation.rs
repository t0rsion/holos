use super::digest::diagrams_equal;
use crate::certificate::{CertificateError, CertificateLimits};
use crate::{Diagram, RipsParams, SparseDistanceMatrix, rips_persistence_sparse};

pub(super) fn check_compute_diagram(
    input: &SparseDistanceMatrix,
    params: &RipsParams,
    checked: &Diagram,
) -> Result<(), CertificateError> {
    let computed = rips_persistence_sparse(input, params)
        .map_err(|error| CertificateError::new(error.to_string()))?;
    if !diagrams_equal(&computed, checked) {
        return Err(CertificateError::new(format!(
            "graded certificate diagram differs from the compute engine: expected {:?}, got {:?}",
            computed.bars, checked.bars
        )));
    }
    Ok(())
}
pub(super) fn validate(
    input: &SparseDistanceMatrix,
    params: &RipsParams,
    limits: CertificateLimits,
) -> Result<(), CertificateError> {
    if params.max_dim > limits.max_dimension {
        return Err(CertificateError::new(format!(
            "homology dimension {} exceeds the graded certificate limit {}",
            params.max_dim, limits.max_dimension
        )));
    }
    if input.len() > limits.max_vertices {
        return Err(CertificateError::new(format!(
            "{} vertices exceed the limit {}",
            input.len(),
            limits.max_vertices
        )));
    }
    if params.modulus < 2 || !is_prime(params.modulus as u64) || params.modulus >= 32_768 {
        return Err(CertificateError::new(format!(
            "modulus must be a prime below 32768, got {}",
            params.modulus
        )));
    }
    checked_threshold(params.threshold)?;
    Ok(())
}

pub(super) fn checked_threshold(threshold: Option<f64>) -> Result<f64, CertificateError> {
    let value = threshold.unwrap_or(f64::INFINITY);
    if value.is_nan() || value < 0.0 {
        return Err(CertificateError::new(format!(
            "threshold must be non-negative, got {value}"
        )));
    }
    Ok(value)
}
fn is_prime(value: u64) -> bool {
    if value < 2 {
        return false;
    }
    let mut divisor = 2;
    while divisor * divisor <= value {
        if value % divisor == 0 {
            return false;
        }
        divisor += 1;
    }
    true
}
