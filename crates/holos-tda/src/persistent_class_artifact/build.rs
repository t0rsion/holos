use crate::certificate::{CertificateError, CertificateLimits, ReductionCertificate};
use crate::field::is_prime;
use crate::{RipsParams, SparseDistanceMatrix, rips_persistence_with_classes_sparse};

use super::model::{PersistenceCycleTerm, PersistenceTriangleTerm, PersistentClassArtifact};

impl PersistentClassArtifact {
    /// Build an artifact for one H1 class-space and basis position.
    ///
    /// `space_index` addresses the interval-ordered class spaces returned by
    /// the native class producer. `basis_index` addresses that space's
    /// canonical basis. The class producer always runs its H1 profile with
    /// `max_dim` set to one. Edge collapse is rejected because the artifact
    /// binds the complete supplied weighted graph.
    pub fn build(
        input: &SparseDistanceMatrix,
        params: &RipsParams,
        space_index: usize,
        basis_index: usize,
        limits: CertificateLimits,
    ) -> Result<Self, CertificateError> {
        preflight(input, params, limits)?;
        let mut class_params = params.clone();
        class_params.max_dim = 1;
        let explained = rips_persistence_with_classes_sparse(input, &class_params)
            .map_err(|error| certificate_error(error.to_string()))?;
        let space = explained.spaces.get(space_index).ok_or_else(|| {
            certificate_error(format!(
                "persistent class space {space_index} is out of range"
            ))
        })?;
        let class = space.basis.get(basis_index).ok_or_else(|| {
            certificate_error(format!(
                "persistent class basis index {basis_index} is out of range"
            ))
        })?;
        if space.critical_pairs.len() != space.basis.len() {
            return Err(certificate_error(
                "persistent class space has an incomplete critical-pair list",
            ));
        }
        class
            .validate_provenance(input)
            .map_err(|error| certificate_error(error.to_string()))?;
        let mut reduction_params = RipsParams::new(1).with_modulus(params.modulus);
        reduction_params.threshold = params.threshold;
        let witness = ReductionCertificate::build_cycle_witness(
            input,
            &reduction_params,
            &space.critical_pairs,
            &class.cocycle,
            limits,
        )?;
        let mut cycle: Vec<_> = witness
            .cycle
            .into_iter()
            .map(|(u, v, coefficient)| PersistenceCycleTerm { u, v, coefficient })
            .collect();
        cycle.sort_unstable();
        let mut bounding_chain: Vec<_> = witness
            .bounding_chain
            .into_iter()
            .map(|(vertices, coefficient)| PersistenceTriangleTerm {
                vertices,
                coefficient,
            })
            .collect();
        bounding_chain.sort_unstable();
        let artifact = Self {
            source: input.clone(),
            threshold: params.threshold,
            class: class.clone(),
            critical_pair: witness.pair,
            cycle,
            bounding_chain,
        };
        super::wire::validate_for_build(&artifact, limits)?;
        Ok(artifact)
    }
}

fn preflight(
    input: &SparseDistanceMatrix,
    params: &RipsParams,
    limits: CertificateLimits,
) -> Result<(), CertificateError> {
    validate_parameters(params, limits)?;
    validate_source_limits(input, limits)?;
    validate_modulus(params.modulus)?;
    validate_threshold(params.threshold)?;
    let minimum = minimum_bytes(input.num_edges(), params.threshold.is_some())?;
    if minimum > limits.max_bytes {
        return Err(certificate_error(
            "persistent class artifact exceeds its byte limit",
        ));
    }
    let threshold = params.threshold.unwrap_or(f64::INFINITY);
    count_triangles(input, threshold, limits.max_triangles)?;
    Ok(())
}

fn validate_parameters(
    params: &RipsParams,
    limits: CertificateLimits,
) -> Result<(), CertificateError> {
    if params.max_dim < 1 {
        return Err(certificate_error(
            "persistent H1 classes require max_dim of at least 1",
        ));
    }
    if params.collapse_edges {
        return Err(certificate_error(
            "persistent class artifacts do not accept edge collapse",
        ));
    }
    if limits.max_dimension < 1 {
        return Err(certificate_error(
            "persistent H1 artifacts require a dimension limit of at least 1",
        ));
    }
    if limits.max_bars == 0 {
        return Err(certificate_error("persistent class bar exceeds its limit"));
    }
    if limits.max_terms < 2 {
        return Err(certificate_error(
            "persistent class cycle and cocycle exceed the term limit",
        ));
    }
    Ok(())
}

fn validate_source_limits(
    input: &SparseDistanceMatrix,
    limits: CertificateLimits,
) -> Result<(), CertificateError> {
    if input.len() > limits.max_vertices {
        return Err(certificate_error(format!(
            "{} source vertices exceed the limit {}",
            input.len(),
            limits.max_vertices
        )));
    }
    if input.num_edges() > limits.max_edges {
        return Err(certificate_error(format!(
            "{} source edges exceed the limit {}",
            input.num_edges(),
            limits.max_edges
        )));
    }
    Ok(())
}

pub(super) fn count_triangles(
    input: &SparseDistanceMatrix,
    threshold: f64,
    maximum: usize,
) -> Result<usize, CertificateError> {
    let upper = filtered_upper_neighbors(input, threshold);
    let mut total = 0usize;
    for u in 0..upper.len() {
        for &v in &upper[u] {
            let left = upper[u].partition_point(|&w| w <= v);
            let right = upper[v].partition_point(|&w| w <= v);
            count_common_neighbors(&upper[u][left..], &upper[v][right..], maximum, &mut total)?;
        }
    }
    Ok(total)
}

fn filtered_upper_neighbors(input: &SparseDistanceMatrix, threshold: f64) -> Vec<Vec<usize>> {
    let mut upper = vec![Vec::new(); input.len()];
    for (u, v, value) in input.edges() {
        if value <= threshold {
            upper[u].push(v);
        }
    }
    upper
}

fn count_common_neighbors(
    left: &[usize],
    right: &[usize],
    maximum: usize,
    total: &mut usize,
) -> Result<(), CertificateError> {
    let mut left_index = 0;
    let mut right_index = 0;
    while left_index < left.len() && right_index < right.len() {
        match left[left_index].cmp(&right[right_index]) {
            std::cmp::Ordering::Less => left_index += 1,
            std::cmp::Ordering::Greater => right_index += 1,
            std::cmp::Ordering::Equal => {
                add_triangle(total, maximum)?;
                left_index += 1;
                right_index += 1;
            }
        }
    }
    Ok(())
}

fn add_triangle(total: &mut usize, maximum: usize) -> Result<(), CertificateError> {
    *total = (*total)
        .checked_add(1)
        .ok_or_else(|| certificate_error("filtered triangle count overflows"))?;
    if *total > maximum {
        return Err(certificate_error(format!(
            "filtered triangle count exceeds the limit {maximum}"
        )));
    }
    Ok(())
}

fn validate_modulus(modulus: u32) -> Result<(), CertificateError> {
    if modulus >= 32_768 || !is_prime(u64::from(modulus)) {
        return Err(certificate_error(format!(
            "modulus must be a prime below 32768, got {modulus}"
        )));
    }
    Ok(())
}

fn validate_threshold(threshold: Option<f64>) -> Result<(), CertificateError> {
    if threshold.is_some_and(|value| {
        value.is_nan() || value < 0.0 || (value == 0.0 && value.to_bits() != 0)
    }) {
        return Err(certificate_error(
            "threshold must be non-negative and not negative zero",
        ));
    }
    Ok(())
}

fn minimum_bytes(edges: usize, has_threshold: bool) -> Result<usize, CertificateError> {
    let fixed = 8usize
        .checked_add(2)
        .and_then(|value| value.checked_add(1))
        .and_then(|value| value.checked_add(4))
        .and_then(|value| value.checked_add(8 + 8))
        .and_then(|value| value.checked_add(if has_threshold { 9 } else { 1 }))
        .and_then(|value| value.checked_add(32 + 32 + 8 + 8 + 8 + 8))
        .and_then(|value| value.checked_add(8 + 20))
        .and_then(|value| value.checked_add(16 + 1))
        .and_then(|value| value.checked_add(8 + 20))
        .and_then(|value| value.checked_add(8))
        .and_then(|value| value.checked_add(32))
        .ok_or_else(|| certificate_error("persistent class artifact byte count overflows"))?;
    let source = edges
        .checked_mul(24)
        .ok_or_else(|| certificate_error("persistent class artifact byte count overflows"))?;
    fixed
        .checked_add(source)
        .ok_or_else(|| certificate_error("persistent class artifact byte count overflows"))
}

fn certificate_error(message: impl Into<String>) -> CertificateError {
    CertificateError::new(message)
}
