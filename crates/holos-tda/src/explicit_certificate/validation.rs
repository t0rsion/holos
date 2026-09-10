use crate::Diagram;
use crate::certificate::{CertificateError, CertificateLimits};
use crate::filtration::{FilteredSimplicialComplex, ScalarGrade};

pub(super) fn validate_parameters(
    complex: &FilteredSimplicialComplex<ScalarGrade>,
    max_homology_dimension: usize,
    modulus: u32,
    limits: CertificateLimits,
) -> Result<(), CertificateError> {
    if max_homology_dimension > limits.max_dimension
        || complex.max_dimension() < max_homology_dimension + 1
    {
        return Err(certificate_error(
            "explicit complex does not cover the requested homology dimensions",
        ));
    }
    if !supported_prime(modulus) {
        return Err(certificate_error(
            "explicit certificate modulus must be a prime below 32768",
        ));
    }
    validate_complex_counts(complex, max_homology_dimension, limits)
}

fn validate_complex_counts(
    complex: &FilteredSimplicialComplex<ScalarGrade>,
    max_homology_dimension: usize,
    limits: CertificateLimits,
) -> Result<(), CertificateError> {
    for dimension in 0..=max_homology_dimension + 1 {
        let count = complex.simplices()[dimension].len();
        let maximum = simplex_limit(dimension, limits);
        if count > maximum {
            return Err(certificate_error(format!(
                "explicit simplex count in dimension {dimension} exceeds its limit"
            )));
        }
    }
    Ok(())
}

pub(super) fn truncate_complex(
    complex: &FilteredSimplicialComplex<ScalarGrade>,
    max_homology_dimension: usize,
) -> Result<FilteredSimplicialComplex<ScalarGrade>, CertificateError> {
    FilteredSimplicialComplex::new(
        complex.vertex_labels().to_vec(),
        complex.simplices()[..=max_homology_dimension + 1].to_vec(),
    )
    .map_err(|error| certificate_error(error.to_string()))
}

fn supported_prime(value: u32) -> bool {
    if !(2..32_768).contains(&value) {
        return false;
    }
    let mut divisor = 2u32;
    while divisor * divisor <= value {
        if value % divisor == 0 {
            return false;
        }
        divisor += 1;
    }
    true
}

pub(super) fn simplex_limit(dimension: usize, limits: CertificateLimits) -> usize {
    match dimension {
        0 => limits.max_vertices,
        1 => limits.max_edges,
        2 => limits.max_triangles,
        _ => limits.max_higher_simplices,
    }
}

pub(super) fn diagrams_equal(left: &Diagram, right: &Diagram) -> bool {
    left.bars.len() == right.bars.len()
        && left.bars.iter().zip(&right.bars).all(|(left, right)| {
            left.dim == right.dim
                && left.birth.to_bits() == right.birth.to_bits()
                && left.death.to_bits() == right.death.to_bits()
        })
}

pub(super) fn certificate_error(message: impl Into<String>) -> CertificateError {
    CertificateError::new(message)
}
