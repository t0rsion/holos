use std::collections::{BTreeMap, BTreeSet};

use super::grade::{FiltrationError, FiltrationGrade, ScalarGrade, ScalarProjection};

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
    /// Construct a checked explicit complex grouped by simplex dimension.
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
