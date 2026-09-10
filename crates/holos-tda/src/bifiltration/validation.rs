//! Validation for finite multicritical bifiltrations.

use std::collections::BTreeMap;

use crate::{Error, Result};

use super::{BifiltrationLimits, BirthAntichain, MulticriticalSimplex};

pub(super) fn validate_axes(
    vertex_count: usize,
    scales: &[f64],
    minimum_degrees: &[usize],
    limits: BifiltrationLimits,
) -> Result<Vec<u64>> {
    validate_scale_count(scales, limits.max_scales)?;
    let scale_bits = validate_scale_values(scales)?;
    validate_density_levels(vertex_count, minimum_degrees, limits.max_density_levels)?;
    Ok(scale_bits)
}

fn validate_scale_count(scales: &[f64], maximum: usize) -> Result<()> {
    if scales.is_empty() || scales.len() > maximum {
        return Err(Error::InvalidInput(format!(
            "bifiltration scale count must be in 1..={maximum}"
        )));
    }
    Ok(())
}

fn validate_scale_values(scales: &[f64]) -> Result<Vec<u64>> {
    let mut scale_bits = Vec::with_capacity(scales.len());
    let mut previous = None;
    for &scale in scales {
        if !scale.is_finite() || scale < 0.0 || previous.is_some_and(|value| value >= scale) {
            return Err(Error::InvalidInput(
                "bifiltration scales must be finite, non-negative, and strictly increasing".into(),
            ));
        }
        let scale = if scale == 0.0 { 0.0 } else { scale };
        scale_bits.push(scale.to_bits());
        previous = Some(scale);
    }
    Ok(scale_bits)
}

fn validate_density_levels(
    vertex_count: usize,
    minimum_degrees: &[usize],
    maximum: usize,
) -> Result<()> {
    if minimum_degrees.is_empty() || minimum_degrees.len() > maximum {
        return Err(Error::InvalidInput(format!(
            "bifiltration density-level count must be in 1..={maximum}"
        )));
    }
    if minimum_degrees.windows(2).any(|pair| pair[0] <= pair[1])
        || minimum_degrees
            .iter()
            .any(|&degree| degree >= vertex_count.max(1))
    {
        return Err(Error::InvalidInput(
            "bifiltration minimum degrees must decrease strictly within the vertex range".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_simplex_key(
    simplex: &MulticriticalSimplex,
    dimension: usize,
    vertex_count: usize,
) -> Result<()> {
    if simplex.vertices.len() != dimension + 1
        || simplex.vertices.windows(2).any(|pair| pair[0] >= pair[1])
        || simplex
            .vertices
            .iter()
            .any(|&vertex| vertex >= vertex_count)
    {
        return Err(Error::InvalidInput(format!(
            "bifiltration simplex {:?} has an invalid key in dimension {dimension}",
            simplex.vertices
        )));
    }
    Ok(())
}

pub(super) fn validate_vertex_keys(
    vertex_count: usize,
    vertices: &[MulticriticalSimplex],
) -> Result<()> {
    let keys = vertices
        .iter()
        .map(|simplex| simplex.vertices[0])
        .collect::<Vec<_>>();
    if keys != (0..vertex_count).collect::<Vec<_>>() {
        return Err(Error::InvalidInput(
            "bifiltration zero-simplices must name every input vertex once".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_face_support(grades: &BTreeMap<Vec<usize>, &BirthAntichain>) -> Result<()> {
    for (simplex, births) in grades {
        if simplex.len() <= 1 {
            continue;
        }
        for removed in 0..simplex.len() {
            let mut face = simplex.clone();
            face.remove(removed);
            let face_births = grades.get(&face).ok_or_else(|| {
                Error::InvalidInput(format!(
                    "bifiltration simplex {simplex:?} is missing the face {face:?}"
                ))
            })?;
            if births
                .grades()
                .iter()
                .copied()
                .any(|birth| !face_births.supports(birth))
            {
                return Err(Error::InvalidInput(format!(
                    "bifiltration face {face:?} appears after its coface {simplex:?}"
                )));
            }
        }
    }
    Ok(())
}
