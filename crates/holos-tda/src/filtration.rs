//! Explicit filtered simplicial complexes and filtration grades.
//!
//! The implicit Vietoris-Rips engine remains the fast path for ordinary
//! persistence. This module defines the checked exchange boundary used by
//! relative interfaces and by future complex builders.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::SparseDistanceMatrix;

/// Failure while constructing an explicit filtered complex or grade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FiltrationError {
    message: String,
}

impl FiltrationError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Description of the violated filtration rule.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for FiltrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "filtered complex: {}", self.message)
    }
}

impl std::error::Error for FiltrationError {}

/// A grade with a decidable filtration partial order.
pub trait FiltrationGrade: Clone + Eq {
    /// Return true when `self` is no later than `other` in the filtration.
    fn precedes(&self, other: &Self) -> bool;
}

/// A filtration grade with one canonical total order.
pub trait LinearFiltrationGrade: FiltrationGrade + Ord {}

/// A finite, non-negative scalar filtration grade.
///
/// The value is stored as canonical IEEE 754 bits. Negative zero is stored as
/// positive zero, so equality and ordering agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScalarGrade(u64);

impl ScalarGrade {
    /// Construct a checked scalar grade.
    pub fn new(value: f64) -> Result<Self, FiltrationError> {
        if !value.is_finite() || value < 0.0 {
            return Err(FiltrationError::new(
                "a scalar grade must be finite and non-negative",
            ));
        }
        Ok(Self(if value == 0.0 {
            0.0f64.to_bits()
        } else {
            value.to_bits()
        }))
    }

    /// The scalar value.
    pub fn value(self) -> f64 {
        f64::from_bits(self.0)
    }

    /// Canonical IEEE 754 bits used by proof formats.
    pub fn bits(self) -> u64 {
        self.0
    }
}

impl FiltrationGrade for ScalarGrade {
    fn precedes(&self, other: &Self) -> bool {
        self <= other
    }
}

impl LinearFiltrationGrade for ScalarGrade {}

/// A coordinatewise filtration grade with `N` parameters.
///
/// This type represents the product partial order. It does not invent a total
/// order for incomparable grades.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProductGrade<const N: usize> {
    coordinates: [ScalarGrade; N],
}

impl<const N: usize> ProductGrade<N> {
    /// Construct a checked product grade from scalar coordinates.
    pub fn new(coordinates: [f64; N]) -> Result<Self, FiltrationError> {
        if N == 0 {
            return Err(FiltrationError::new(
                "a product grade requires at least one coordinate",
            ));
        }
        let mut checked = [ScalarGrade(0); N];
        for (index, value) in coordinates.into_iter().enumerate() {
            checked[index] = ScalarGrade::new(value)?;
        }
        Ok(Self {
            coordinates: checked,
        })
    }

    /// Scalar coordinates in parameter order.
    pub fn coordinates(&self) -> &[ScalarGrade; N] {
        &self.coordinates
    }
}

impl<const N: usize> FiltrationGrade for ProductGrade<N> {
    fn precedes(&self, other: &Self) -> bool {
        self.coordinates
            .iter()
            .zip(other.coordinates.iter())
            .all(|(left, right)| left.precedes(right))
    }
}

/// A declared monotone map from a grade into a scalar filtration.
pub trait ScalarProjection<G: FiltrationGrade> {
    /// Project one grade into the scalar filtration.
    fn project(&self, grade: &G) -> Result<ScalarGrade, FiltrationError>;
}

/// Projection onto one coordinate of a product grade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoordinateProjection {
    coordinate: usize,
}

impl CoordinateProjection {
    /// Select a zero-based coordinate.
    pub fn new(coordinate: usize) -> Self {
        Self { coordinate }
    }
}

impl<const N: usize> ScalarProjection<ProductGrade<N>> for CoordinateProjection {
    fn project(&self, grade: &ProductGrade<N>) -> Result<ScalarGrade, FiltrationError> {
        grade
            .coordinates
            .get(self.coordinate)
            .copied()
            .ok_or_else(|| {
                FiltrationError::new(format!(
                    "coordinate {} is outside a {N}-parameter grade",
                    self.coordinate
                ))
            })
    }
}

/// One simplex with a stable vertex key and a filtration grade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilteredSimplex<G> {
    vertices: Vec<usize>,
    grade: G,
}

impl<G> FilteredSimplex<G> {
    /// Construct a simplex. The enclosing complex checks its vertex key.
    pub fn new(vertices: Vec<usize>, grade: G) -> Self {
        Self { vertices, grade }
    }

    /// Stable vertex labels in ascending order.
    pub fn vertices(&self) -> &[usize] {
        &self.vertices
    }

    /// Dimension of this simplex.
    pub fn dimension(&self) -> usize {
        self.vertices.len().saturating_sub(1)
    }

    /// Filtration grade of this simplex.
    pub fn grade(&self) -> &G {
        &self.grade
    }
}

/// A finite explicit filtered simplicial complex.
///
/// Vertex labels are stable keys. Every nonempty face must occur exactly once,
/// and each face grade must precede the grade of its cofaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilteredSimplicialComplex<G> {
    vertex_labels: Vec<usize>,
    simplices: Vec<Vec<FilteredSimplex<G>>>,
}

impl<G: FiltrationGrade> FilteredSimplicialComplex<G> {
    /// Validate an explicit complex grouped by simplex dimension.
    pub fn new(
        vertex_labels: Vec<usize>,
        mut simplices: Vec<Vec<FilteredSimplex<G>>>,
    ) -> Result<Self, FiltrationError> {
        if vertex_labels.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(FiltrationError::new(
                "vertex labels must be strictly increasing",
            ));
        }
        if simplices.is_empty() {
            simplices.push(Vec::new());
        }
        for dimension in &mut simplices {
            dimension.sort_by(|left, right| left.vertices.cmp(&right.vertices));
        }
        let complex = Self {
            vertex_labels,
            simplices,
        };
        complex.validate()?;
        Ok(complex)
    }

    /// Stable labels of all vertices in this complex.
    pub fn vertex_labels(&self) -> &[usize] {
        &self.vertex_labels
    }

    /// Highest stored simplex dimension, including a trailing empty group.
    pub fn max_dimension(&self) -> usize {
        self.simplices.len().saturating_sub(1)
    }

    /// Simplices grouped by dimension and ordered by vertex key.
    pub fn simplices(&self) -> &[Vec<FilteredSimplex<G>>] {
        &self.simplices
    }

    /// Number of stored simplices in all dimensions.
    pub fn simplex_count(&self) -> usize {
        self.simplices.iter().map(Vec::len).sum()
    }

    /// Apply a scalar projection and check the resulting filtration.
    pub fn project<P: ScalarProjection<G>>(
        &self,
        projection: &P,
    ) -> Result<FilteredSimplicialComplex<ScalarGrade>, FiltrationError> {
        let mut simplices = Vec::with_capacity(self.simplices.len());
        for dimension in &self.simplices {
            let mut projected = Vec::with_capacity(dimension.len());
            for simplex in dimension {
                projected.push(FilteredSimplex::new(
                    simplex.vertices.clone(),
                    projection.project(&simplex.grade)?,
                ));
            }
            simplices.push(projected);
        }
        FilteredSimplicialComplex::new(self.vertex_labels.clone(), simplices)
    }

    fn validate(&self) -> Result<(), FiltrationError> {
        let labels: BTreeSet<_> = self.vertex_labels.iter().copied().collect();
        let mut grades = BTreeMap::<Vec<usize>, &G>::new();
        for (dimension, simplices) in self.simplices.iter().enumerate() {
            for simplex in simplices {
                validate_simplex_key(simplex, dimension, &labels)?;
                insert_simplex_grade(simplex, &mut grades)?;
            }
        }
        validate_declared_vertices(&self.simplices[0], &self.vertex_labels)?;
        validate_faces(&grades)
    }
}

fn validate_simplex_key<G>(
    simplex: &FilteredSimplex<G>,
    dimension: usize,
    labels: &BTreeSet<usize>,
) -> Result<(), FiltrationError> {
    if simplex.vertices.len() != dimension + 1 {
        return Err(FiltrationError::new(format!(
            "simplex {:?} is stored in dimension {dimension}",
            simplex.vertices
        )));
    }
    if simplex.vertices.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(FiltrationError::new(format!(
            "simplex {:?} does not have a canonical vertex key",
            simplex.vertices
        )));
    }
    if simplex
        .vertices
        .iter()
        .any(|vertex| !labels.contains(vertex))
    {
        return Err(FiltrationError::new(format!(
            "simplex {:?} uses an unknown vertex label",
            simplex.vertices
        )));
    }
    Ok(())
}

fn insert_simplex_grade<'a, G>(
    simplex: &'a FilteredSimplex<G>,
    grades: &mut BTreeMap<Vec<usize>, &'a G>,
) -> Result<(), FiltrationError> {
    if grades
        .insert(simplex.vertices.clone(), &simplex.grade)
        .is_some()
    {
        return Err(FiltrationError::new(format!(
            "simplex {:?} occurs more than once",
            simplex.vertices
        )));
    }
    Ok(())
}

fn validate_declared_vertices<G>(
    vertices: &[FilteredSimplex<G>],
    labels: &[usize],
) -> Result<(), FiltrationError> {
    let declared: Vec<_> = vertices.iter().map(|simplex| simplex.vertices[0]).collect();
    if declared != labels {
        return Err(FiltrationError::new(
            "zero-dimensional simplices must match the vertex labels",
        ));
    }
    Ok(())
}

fn validate_faces<G: FiltrationGrade>(
    grades: &BTreeMap<Vec<usize>, &G>,
) -> Result<(), FiltrationError> {
    for (key, grade) in grades {
        if key.len() > 1 {
            validate_simplex_faces(key, *grade, grades)?;
        }
    }
    Ok(())
}

fn validate_simplex_faces<G: FiltrationGrade>(
    key: &[usize],
    grade: &G,
    grades: &BTreeMap<Vec<usize>, &G>,
) -> Result<(), FiltrationError> {
    for removed in 0..key.len() {
        let mut face = key.to_vec();
        face.remove(removed);
        let face_grade = grades.get(&face).ok_or_else(|| {
            FiltrationError::new(format!("simplex {key:?} is missing the face {face:?}"))
        })?;
        if !face_grade.precedes(grade) {
            return Err(FiltrationError::new(format!(
                "face {face:?} appears after its coface {key:?}"
            )));
        }
    }
    Ok(())
}

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
    fn for_dimension(self, dimension: usize) -> usize {
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

#[allow(clippy::too_many_arguments)]
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
        let last_local = label_to_local[&simplex.vertices[dimension - 1]];
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
    let mut value = simplex.grade.value();
    for member in &simplex.vertices {
        let edge = input.get(label_to_local[member], local_vertex);
        if !edge.is_finite() || edge > threshold {
            return Ok(None);
        }
        value = value.max(edge);
    }
    let mut vertices = simplex.vertices.clone();
    vertices.push(label);
    Ok(Some(FilteredSimplex::new(
        vertices,
        ScalarGrade::new(value)?,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_order_keeps_incomparable_grades_incomparable() {
        let left = ProductGrade::new([1.0, 3.0]).unwrap();
        let right = ProductGrade::new([2.0, 2.0]).unwrap();
        assert!(!left.precedes(&right));
        assert!(!right.precedes(&left));
        assert_eq!(
            CoordinateProjection::new(0).project(&left).unwrap().value(),
            1.0
        );
    }

    #[test]
    fn explicit_complex_rejects_a_late_face() {
        let complex = FilteredSimplicialComplex::new(
            vec![0, 1],
            vec![
                vec![
                    FilteredSimplex::new(vec![0], ScalarGrade::new(0.0).unwrap()),
                    FilteredSimplex::new(vec![1], ScalarGrade::new(2.0).unwrap()),
                ],
                vec![FilteredSimplex::new(
                    vec![0, 1],
                    ScalarGrade::new(1.0).unwrap(),
                )],
            ],
        );
        assert_eq!(
            complex.unwrap_err().message(),
            "face [1] appears after its coface [0, 1]"
        );
    }

    #[test]
    fn flag_builder_uses_global_labels_and_clique_diameters() {
        let graph =
            SparseDistanceMatrix::from_triplets(3, &[(0, 1, 1.0), (0, 2, 2.0), (1, 2, 1.5)])
                .unwrap();
        let complex = FilteredSimplicialComplex::from_flag_graph(
            &graph,
            &[4, 8, 9],
            FlagComplexParams {
                max_dimension: 2,
                threshold: Some(2.0),
                limits: ComplexLimits {
                    max_vertices: 10,
                    max_edges: 10,
                    max_triangles: 10,
                    max_higher_simplices: 10,
                },
            },
        )
        .unwrap();
        assert_eq!(complex.simplices()[2][0].vertices(), &[4, 8, 9]);
        assert_eq!(complex.simplices()[2][0].grade().value(), 2.0);
    }
}
