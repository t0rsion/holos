//! Finite multicritical bifiltrations and exact degree-Rips construction.
//!
//! A simplex carries the antichain of its minimal grades. The support of the
//! simplex is the upward closure of that antichain. This representation keeps
//! multicritical filtrations distinct from one-critical product grades.

mod degree_rips;
mod validation;

#[cfg(test)]
mod tests;

pub use degree_rips::{DegreeRipsBifiltration, DegreeRipsParams};

use validation::{
    validate_axes, validate_face_support, validate_simplex_key, validate_vertex_keys,
};

use std::collections::BTreeMap;

use crate::filtration::ComplexLimits;
use crate::{Error, Result, SparseDistanceMatrix};

/// One position in a finite two-parameter grid.
///
/// Both coordinates increase with the filtration. For degree-Rips, the
/// second index addresses a strictly decreasing list of minimum degrees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Bigrade {
    scale: usize,
    density: usize,
}

impl Bigrade {
    /// Construct a grid position from zero-based coordinate indices.
    pub fn new(scale: usize, density: usize) -> Self {
        Self { scale, density }
    }

    /// Scale-axis index.
    pub fn scale(self) -> usize {
        self.scale
    }

    /// Density-axis index.
    pub fn density(self) -> usize {
        self.density
    }

    /// Whether this grade precedes another in the product order.
    pub fn precedes(self, other: Self) -> bool {
        self.scale <= other.scale && self.density <= other.density
    }
}

/// Minimal incomparable birth grades of one simplex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BirthAntichain {
    grades: Vec<Bigrade>,
}

impl BirthAntichain {
    /// Construct a nonempty canonical antichain.
    pub fn new(mut grades: Vec<Bigrade>) -> Result<Self> {
        grades.sort_unstable();
        if grades.is_empty() {
            return Err(Error::InvalidInput(
                "a multicritical simplex needs at least one birth grade".into(),
            ));
        }
        for (position, grade) in grades.iter().copied().enumerate() {
            if grades
                .iter()
                .copied()
                .enumerate()
                .any(|(other, candidate)| other != position && candidate.precedes(grade))
            {
                return Err(Error::InvalidInput(
                    "multicritical birth grades must be pairwise incomparable".into(),
                ));
            }
        }
        Ok(Self { grades })
    }

    /// Minimal grades in lexicographic order.
    pub fn grades(&self) -> &[Bigrade] {
        &self.grades
    }

    /// Whether the simplex occurs at one grid position.
    pub fn supports(&self, grade: Bigrade) -> bool {
        self.grades
            .iter()
            .copied()
            .any(|birth| birth.precedes(grade))
    }

    fn from_candidates(candidates: impl IntoIterator<Item = Bigrade>) -> Result<Self> {
        let mut minimal = Vec::<Bigrade>::new();
        for grade in candidates {
            if minimal.iter().copied().any(|birth| birth.precedes(grade)) {
                continue;
            }
            minimal.retain(|birth| !grade.precedes(*birth));
            minimal.push(grade);
        }
        Self::new(minimal)
    }
}

/// One simplex in a finite multicritical bifiltration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MulticriticalSimplex {
    vertices: Vec<usize>,
    births: BirthAntichain,
}

impl MulticriticalSimplex {
    /// Construct a simplex. The enclosing bifiltration validates its key.
    pub fn new(vertices: Vec<usize>, births: BirthAntichain) -> Self {
        Self { vertices, births }
    }

    /// Stable vertices in ascending order.
    pub fn vertices(&self) -> &[usize] {
        &self.vertices
    }

    /// Minimal birth grades.
    pub fn births(&self) -> &BirthAntichain {
        &self.births
    }

    /// Simplex dimension.
    pub fn dimension(&self) -> usize {
        self.vertices.len().saturating_sub(1)
    }
}

/// Resource limits for a finite multicritical bifiltration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct BifiltrationLimits {
    /// Largest accepted scale count.
    pub max_scales: usize,
    /// Largest accepted density-level count.
    pub max_density_levels: usize,
    /// Largest accepted total birth-grade count.
    pub max_birth_grades: usize,
    /// Per-dimension simplex limits.
    pub complex: ComplexLimits,
}

impl Default for BifiltrationLimits {
    fn default() -> Self {
        Self {
            max_scales: 100_000,
            max_density_levels: 1_000_000,
            max_birth_grades: 100_000_000,
            complex: ComplexLimits::default(),
        }
    }
}

impl BifiltrationLimits {
    fn simplex_limit(self, dimension: usize) -> usize {
        match dimension {
            0 => self.complex.max_vertices,
            1 => self.complex.max_edges,
            2 => self.complex.max_triangles,
            _ => self.complex.max_higher_simplices,
        }
    }
}

/// A finite simplicial bifiltration represented by antichain births.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MulticriticalBifiltration {
    vertex_count: usize,
    scale_bits: Vec<u64>,
    minimum_degrees: Vec<usize>,
    simplices: Vec<Vec<MulticriticalSimplex>>,
}

impl MulticriticalBifiltration {
    /// Construct and validate a finite multicritical bifiltration.
    pub fn new(
        vertex_count: usize,
        scales: Vec<f64>,
        minimum_degrees: Vec<usize>,
        mut simplices: Vec<Vec<MulticriticalSimplex>>,
        limits: BifiltrationLimits,
    ) -> Result<Self> {
        let scale_bits = validate_axes(vertex_count, &scales, &minimum_degrees, limits)?;
        if simplices.is_empty() {
            simplices.push(Vec::new());
        }
        for dimension in &mut simplices {
            dimension.sort_by(|left, right| left.vertices.cmp(&right.vertices));
        }
        let bifiltration = Self {
            vertex_count,
            scale_bits,
            minimum_degrees,
            simplices,
        };
        bifiltration.validate(limits)?;
        Ok(bifiltration)
    }

    /// Number of labeled input vertices.
    pub fn vertex_count(&self) -> usize {
        self.vertex_count
    }

    /// Scale values in strict ascending order.
    pub fn scales(&self) -> impl ExactSizeIterator<Item = f64> + '_ {
        self.scale_bits.iter().copied().map(f64::from_bits)
    }

    /// Minimum vertex degrees in strict descending order.
    pub fn minimum_degrees(&self) -> &[usize] {
        &self.minimum_degrees
    }

    /// Simplices grouped by dimension and ordered by vertex key.
    pub fn simplices(&self) -> &[Vec<MulticriticalSimplex>] {
        &self.simplices
    }

    /// Largest valid grid position.
    pub fn maximum_grade(&self) -> Bigrade {
        Bigrade::new(self.scale_bits.len() - 1, self.minimum_degrees.len() - 1)
    }

    /// Materialize the simplices present at one grid position.
    pub fn slice(&self, grade: Bigrade) -> Result<BifiltrationSlice> {
        self.validate_grade(grade)?;
        let simplices = self
            .simplices
            .iter()
            .map(|dimension| {
                dimension
                    .iter()
                    .filter(|simplex| simplex.births.supports(grade))
                    .map(|simplex| simplex.vertices.clone())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let active_vertices = simplices
            .first()
            .into_iter()
            .flat_map(|vertices| vertices.iter().map(|simplex| simplex[0]))
            .collect();
        Ok(BifiltrationSlice {
            grade,
            active_vertices,
            simplices,
        })
    }

    fn validate_grade(&self, grade: Bigrade) -> Result<()> {
        if grade.scale >= self.scale_bits.len() || grade.density >= self.minimum_degrees.len() {
            return Err(Error::InvalidInput(
                "bifiltration grade is outside the finite parameter grid".into(),
            ));
        }
        Ok(())
    }

    fn validate(&self, limits: BifiltrationLimits) -> Result<()> {
        let by_key = self.validate_simplices(limits)?;
        validate_vertex_keys(self.vertex_count, &self.simplices[0])?;
        validate_face_support(&by_key)
    }

    fn validate_simplices(
        &self,
        limits: BifiltrationLimits,
    ) -> Result<BTreeMap<Vec<usize>, &BirthAntichain>> {
        let mut births = 0usize;
        let mut by_key = BTreeMap::<Vec<usize>, &BirthAntichain>::new();
        for (dimension, simplices) in self.simplices.iter().enumerate() {
            if simplices.len() > limits.simplex_limit(dimension) {
                return Err(Error::InvalidInput(format!(
                    "bifiltration dimension {dimension} exceeds its simplex limit"
                )));
            }
            for simplex in simplices {
                validate_simplex_key(simplex, dimension, self.vertex_count)?;
                for grade in simplex.births.grades() {
                    self.validate_grade(*grade)?;
                }
                births = births
                    .checked_add(simplex.births.grades().len())
                    .ok_or_else(|| {
                        Error::InvalidInput("bifiltration birth-grade count overflows".into())
                    })?;
                if births > limits.max_birth_grades {
                    return Err(Error::InvalidInput(
                        "bifiltration birth-grade count exceeds its limit".into(),
                    ));
                }
                if by_key
                    .insert(simplex.vertices.clone(), &simplex.births)
                    .is_some()
                {
                    return Err(Error::InvalidInput(format!(
                        "bifiltration simplex {:?} occurs more than once",
                        simplex.vertices
                    )));
                }
            }
        }
        Ok(by_key)
    }
}

/// One materialized value of a multicritical bifiltration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BifiltrationSlice {
    grade: Bigrade,
    active_vertices: Vec<usize>,
    simplices: Vec<Vec<Vec<usize>>>,
}

impl BifiltrationSlice {
    /// Grid position of this slice.
    pub fn grade(&self) -> Bigrade {
        self.grade
    }

    /// Vertices present in the slice.
    pub fn active_vertices(&self) -> &[usize] {
        &self.active_vertices
    }

    /// Present simplices grouped by dimension.
    pub fn simplices(&self) -> &[Vec<Vec<usize>>] {
        &self.simplices
    }

    /// Build a zero-weight graph from the one-skeleton.
    ///
    /// The graph retains the original vertex count. Use it for positive-
    /// dimensional flag cohomology. `active_vertices` records which isolated
    /// vertices actually occur, so this graph alone does not represent H0.
    pub fn h1_graph(&self, vertex_count: usize) -> Result<SparseDistanceMatrix> {
        let edges = self
            .simplices
            .get(1)
            .into_iter()
            .flatten()
            .map(|edge| (edge[0], edge[1], 0.0))
            .collect::<Vec<_>>();
        SparseDistanceMatrix::from_triplets(vertex_count, &edges)
    }
}
