use std::collections::BTreeMap;

use crate::SparseDistanceMatrix;

use super::grade::{FiltrationError, ScalarGrade};
use super::simplex::{FilteredSimplex, FilteredSimplicialComplex};

/// Per-dimension bounds for an explicit complex materialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComplexLimits {
    /// Largest accepted vertex count.
    pub max_vertices: usize,
    /// Largest accepted edge count.
    pub max_edges: usize,
    /// Largest accepted triangle count.
    pub max_triangles: usize,
    /// Largest accepted simplex count in each dimension above two.
    pub max_higher_simplices: usize,
}

impl Default for ComplexLimits {
    fn default() -> Self {
        Self {
            max_vertices: 1_000_000,
            max_edges: 20_000_000,
            max_triangles: 100_000_000,
            max_higher_simplices: 100_000_000,
        }
    }
}

impl ComplexLimits {
    pub(crate) fn for_dimension(self, dimension: usize) -> usize {
        match dimension {
            0 => self.max_vertices,
            1 => self.max_edges,
            2 => self.max_triangles,
            _ => self.max_higher_simplices,
        }
    }
}

/// Limits and filtration choices for a flag-complex materialization.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlagComplexParams {
    /// Highest simplex dimension to materialize.
    pub max_dimension: usize,
    /// Largest included edge weight. `None` includes every finite listed edge.
    pub threshold: Option<f64>,
    /// Per-dimension resource bounds.
    pub limits: ComplexLimits,
}

impl FilteredSimplicialComplex<ScalarGrade> {
    /// Materialize the flag complex of a sparse graph with stable labels.
    pub fn from_flag_graph(
        input: &SparseDistanceMatrix,
        labels: &[usize],
        params: FlagComplexParams,
    ) -> Result<Self, FiltrationError> {
        let threshold = validate_flag_params(input, labels, params)?;
        let zero = ScalarGrade::new(0.0)?;
        let vertices = labels
            .iter()
            .map(|&vertex| FilteredSimplex::new(vec![vertex], zero))
            .collect::<Vec<_>>();
        let mut simplices = vec![vertices];
        let label_to_local: BTreeMap<_, _> = labels
            .iter()
            .copied()
            .enumerate()
            .map(|(local, label)| (label, local))
            .collect();
        for dimension in 1..=params.max_dimension {
            let next = extend_flag_dimension(
                input,
                labels,
                &label_to_local,
                &simplices[dimension - 1],
                dimension,
                threshold,
                params.limits.for_dimension(dimension),
            )?;
            simplices.push(next);
        }
        Self::new(labels.to_vec(), simplices)
    }
}

fn validate_flag_params(
    input: &SparseDistanceMatrix,
    labels: &[usize],
    params: FlagComplexParams,
) -> Result<f64, FiltrationError> {
    if labels.len() != input.len() || labels.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(FiltrationError::new(
            "flag-complex labels must match the graph and increase strictly",
        ));
    }
    let threshold = params.threshold.unwrap_or(f64::INFINITY);
    if threshold.is_nan() || threshold < 0.0 {
        return Err(FiltrationError::new(
            "a flag-complex threshold must be non-negative",
        ));
    }
    if labels.len() > params.limits.max_vertices {
        return Err(FiltrationError::new(
            "flag-complex vertices exceed the per-dimension limit",
        ));
    }
    Ok(threshold)
}

fn extend_flag_dimension(
    input: &SparseDistanceMatrix,
    labels: &[usize],
    label_to_local: &BTreeMap<usize, usize>,
    faces: &[FilteredSimplex<ScalarGrade>],
    dimension: usize,
    threshold: f64,
    limit: usize,
) -> Result<Vec<FilteredSimplex<ScalarGrade>>, FiltrationError> {
    let mut simplices = Vec::new();
    for simplex in faces {
        let last_local = label_to_local[&simplex.vertices()[dimension - 1]];
        for (local_vertex, &label) in labels.iter().enumerate().skip(last_local + 1) {
            if let Some(coface) = extend_flag_simplex(
                input,
                label_to_local,
                simplex,
                local_vertex,
                label,
                threshold,
            )? {
                simplices.push(coface);
                if simplices.len() > limit {
                    return Err(FiltrationError::new(format!(
                        "flag-complex dimension {dimension} exceeds the simplex limit"
                    )));
                }
            }
        }
    }
    Ok(simplices)
}

fn extend_flag_simplex(
    input: &SparseDistanceMatrix,
    label_to_local: &BTreeMap<usize, usize>,
    simplex: &FilteredSimplex<ScalarGrade>,
    local_vertex: usize,
    label: usize,
    threshold: f64,
) -> Result<Option<FilteredSimplex<ScalarGrade>>, FiltrationError> {
    let mut value = simplex.grade().value();
    for member in simplex.vertices() {
        let edge = input.get(label_to_local[member], local_vertex);
        if !edge.is_finite() || edge > threshold {
            return Ok(None);
        }
        value = value.max(edge);
    }
    let mut vertices = simplex.vertices().to_vec();
    vertices.push(label);
    Ok(Some(FilteredSimplex::new(
        vertices,
        ScalarGrade::new(value)?,
    )))
}
