//! Python bindings for finite degree-Rips bipersistence modules.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use holos_tda::{
    BipersistenceArtifact, BipersistenceArtifactLimits, BipersistenceModule,
    BipersistenceRectangle, BipersistenceRegion, CircularCoordinateParams, DegreeRipsBifiltration,
    DegreeRipsParams, SparseDistanceMatrix,
};

mod records;

use records::{
    ArtifactSummary, AtlasRecord, CircularFamilyRecord, Grade, MapRecord, Term, artifact_summary,
    atlas_record, coordinate_record, extension_kind, from_grade, map_record, terms, to_grade,
    to_terms,
};

pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PyBipersistence>()?;
    module.add_class::<PyBipersistenceArtifact>()?;
    module.add_function(wrap_pyfunction!(degree_rips_bipersistence, module)?)?;
    module.add_function(wrap_pyfunction!(load_bipersistence_artifact, module)?)?;
    Ok(())
}

fn to_error(error: impl std::fmt::Display) -> PyErr {
    PyValueError::new_err(error.to_string())
}

/// A complete finite degree-Rips H1 module with checked derived queries.
#[pyclass(name = "Bipersistence")]
pub(crate) struct PyBipersistence {
    module: BipersistenceModule,
    artifact: BipersistenceArtifact,
    limits: BipersistenceArtifactLimits,
}

#[pymethods]
impl PyBipersistence {
    /// Return the prime coefficient modulus.
    #[getter]
    fn modulus(&self) -> u32 {
        self.module.modulus()
    }

    /// Return scale values in ascending order.
    #[getter]
    fn scales(&self) -> Vec<f64> {
        self.module.scales().to_vec()
    }

    /// Return minimum-degree levels in descending order.
    #[getter]
    fn minimum_degrees(&self) -> Vec<usize> {
        self.module.minimum_degrees().to_vec()
    }

    /// Return `(scale_index, density_index, rank)` for every grid node.
    #[getter]
    fn node_ranks(&self) -> Vec<(usize, usize, usize)> {
        self.module
            .nodes()
            .iter()
            .map(|node| (node.grade.scale(), node.grade.density(), node.rank))
            .collect()
    }

    /// Return every horizontal and vertical cover map.
    #[getter]
    fn cover_maps(&self) -> Vec<MapRecord> {
        self.module.cover_maps().iter().map(map_record).collect()
    }

    /// Return one exact map rank between comparable grades.
    fn map_rank(&self, lower: Grade, upper: Grade) -> PyResult<usize> {
        self.module
            .map_rank(to_grade(lower), to_grade(upper))
            .map_err(to_error)
    }

    /// Return one exact map, including its canonical matrix columns.
    fn map(&self, lower: Grade, upper: Grade) -> PyResult<MapRecord> {
        self.module
            .map(to_grade(lower), to_grade(upper))
            .map(|value| map_record(&value))
            .map_err(to_error)
    }

    /// Return the generalized rank of a closed parameter rectangle.
    fn rectangle_rank(&self, lower: Grade, upper: Grade) -> PyResult<usize> {
        let rectangle =
            BipersistenceRectangle::new(to_grade(lower), to_grade(upper)).map_err(to_error)?;
        self.module.rectangle_rank(rectangle).map_err(to_error)
    }

    /// Return the generalized rank of a connected finite region.
    fn region_rank(&self, grades: Vec<Grade>) -> PyResult<usize> {
        let region = BipersistenceRegion::new(grades.into_iter().map(to_grade).collect())
            .map_err(to_error)?;
        self.module.region_rank(&region).map_err(to_error)
    }

    /// Compute the exact extension atlas of one nonzero canonical class.
    fn class_atlas(&self, base: Grade, class: Vec<Term>) -> PyResult<AtlasRecord> {
        self.module
            .class_atlas(to_grade(base), &to_terms(class))
            .map(|value| atlas_record(&value))
            .map_err(to_error)
    }

    /// Compute checked circular coordinates for unique atlas extensions.
    #[pyo3(signature = (base, class, tolerance=1e-10, max_iterations=10_000))]
    fn circular_family(
        &self,
        base: Grade,
        class: Vec<Term>,
        tolerance: f64,
        max_iterations: usize,
    ) -> PyResult<CircularFamilyRecord> {
        let atlas = self
            .module
            .class_atlas(to_grade(base), &to_terms(class))
            .map_err(to_error)?;
        let params = CircularCoordinateParams::default()
            .with_tolerance(tolerance)
            .with_max_iterations(max_iterations);
        let family = self
            .module
            .circular_coordinate_family(&atlas, params)
            .map_err(to_error)?;
        Ok((
            from_grade(family.base_grade),
            terms(&family.base_class),
            family
                .entries
                .iter()
                .map(|entry| {
                    (
                        from_grade(entry.grade),
                        extension_kind(entry.extension).into(),
                        entry.coordinate.as_ref().map(coordinate_record),
                    )
                })
                .collect(),
        ))
    }

    /// Store one checked rectangle-rank claim in the artifact.
    fn record_rectangle(&mut self, lower: Grade, upper: Grade) -> PyResult<()> {
        let rectangle =
            BipersistenceRectangle::new(to_grade(lower), to_grade(upper)).map_err(to_error)?;
        self.artifact
            .record_rectangle(&self.module, rectangle, self.limits)
            .map_err(to_error)
    }

    /// Add one checked connected-region rank claim to the artifact.
    fn record_region(&mut self, grades: Vec<Grade>) -> PyResult<()> {
        let region = BipersistenceRegion::new(grades.into_iter().map(to_grade).collect())
            .map_err(to_error)?;
        self.artifact
            .record_region(&self.module, region, self.limits)
            .map_err(to_error)
    }

    /// Store one checked class atlas in the artifact and return its record.
    fn record_class_atlas(&mut self, base: Grade, class: Vec<Term>) -> PyResult<AtlasRecord> {
        let atlas = self
            .module
            .class_atlas(to_grade(base), &to_terms(class))
            .map_err(to_error)?;
        self.artifact
            .record_class_atlas(&self.module, &atlas, self.limits)
            .map_err(to_error)?;
        Ok(atlas_record(&atlas))
    }

    /// Store checked circular data for a class atlas already in the artifact.
    #[pyo3(signature = (base, class, tolerance=1e-10, max_iterations=10_000))]
    fn record_circular_family(
        &mut self,
        base: Grade,
        class: Vec<Term>,
        tolerance: f64,
        max_iterations: usize,
    ) -> PyResult<CircularFamilyRecord> {
        let atlas = self
            .module
            .class_atlas(to_grade(base), &to_terms(class))
            .map_err(to_error)?;
        let params = CircularCoordinateParams::default()
            .with_tolerance(tolerance)
            .with_max_iterations(max_iterations);
        let family = self
            .module
            .circular_coordinate_family(&atlas, params)
            .map_err(to_error)?;
        self.artifact
            .record_circular_family(&self.module, &atlas, params, self.limits)
            .map_err(to_error)?;
        Ok((
            from_grade(family.base_grade),
            terms(&family.base_class),
            family
                .entries
                .iter()
                .map(|entry| {
                    (
                        from_grade(entry.grade),
                        extension_kind(entry.extension).into(),
                        entry.coordinate.as_ref().map(coordinate_record),
                    )
                })
                .collect(),
        ))
    }

    /// Return canonical `HOLOSBP` bytes for the current artifact.
    #[getter]
    fn artifact<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = self.artifact.encode(self.limits).map_err(to_error)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Return structural counts for the current artifact.
    #[getter]
    fn artifact_summary(&self) -> ArtifactSummary {
        artifact_summary(&self.artifact)
    }
}

/// A decoded and independently verifiable `HOLOSBP` artifact.
#[pyclass(name = "BipersistenceArtifact")]
pub(crate) struct PyBipersistenceArtifact {
    artifact: BipersistenceArtifact,
    limits: BipersistenceArtifactLimits,
}

#[pymethods]
impl PyBipersistenceArtifact {
    /// Return canonical artifact bytes.
    #[getter]
    fn bytes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = self.artifact.encode(self.limits).map_err(to_error)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Return structural counts from the artifact.
    #[getter]
    fn summary(&self) -> ArtifactSummary {
        artifact_summary(&self.artifact)
    }

    /// Verify the artifact by exact degree-Rips replay.
    fn verify(&self) -> PyResult<()> {
        self.artifact.verify(self.limits).map_err(to_error)
    }

    /// Return stored generalized rectangle-rank claims.
    #[getter]
    fn rectangles(&self) -> Vec<(Grade, Grade, usize)> {
        self.artifact
            .rectangles()
            .iter()
            .map(|claim| {
                (
                    from_grade(claim.rectangle.lower),
                    from_grade(claim.rectangle.upper),
                    claim.rank,
                )
            })
            .collect()
    }

    /// Return stored generalized connected-region rank claims.
    #[getter]
    fn regions(&self) -> Vec<(Vec<Grade>, usize)> {
        self.artifact
            .regions()
            .iter()
            .map(|claim| {
                (
                    claim
                        .region
                        .grades()
                        .iter()
                        .copied()
                        .map(from_grade)
                        .collect(),
                    claim.rank,
                )
            })
            .collect()
    }

    /// Return stored class-extension atlases.
    #[getter]
    fn class_atlases(&self) -> Vec<AtlasRecord> {
        self.artifact
            .class_atlases()
            .iter()
            .map(atlas_record)
            .collect()
    }
}

/// Build a checked finite degree-Rips H1 module from weighted sparse edges.
#[pyfunction]
#[pyo3(signature = (n, triplets, threshold=None, modulus=47, *, scales=None, minimum_degrees=None))]
fn degree_rips_bipersistence(
    py: Python<'_>,
    n: usize,
    triplets: Vec<(usize, usize, f64)>,
    threshold: Option<f64>,
    modulus: u32,
    scales: Option<Vec<f64>>,
    minimum_degrees: Option<Vec<usize>>,
) -> PyResult<PyBipersistence> {
    py.detach(|| {
        let source = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_error)?;
        let declared_threshold = scales.as_ref().and_then(|values| values.last().copied());
        let params = DegreeRipsParams {
            max_homology_dimension: 1,
            threshold: threshold.or(declared_threshold),
            limits: Default::default(),
        };
        let degree_rips = match (scales, minimum_degrees) {
            (None, None) => DegreeRipsBifiltration::from_graph(&source, params),
            (Some(scales), Some(minimum_degrees)) => {
                DegreeRipsBifiltration::from_graph_on_grid(&source, scales, minimum_degrees, params)
            }
            _ => Err(holos_tda::Error::InvalidInput(
                "a declared grid needs both scales and minimum_degrees".into(),
            )),
        }
        .map_err(to_error)?;
        let limits = BipersistenceArtifactLimits::default();
        let (artifact, module) =
            BipersistenceArtifact::build(&degree_rips, modulus, limits).map_err(to_error)?;
        Ok(PyBipersistence {
            module,
            artifact,
            limits,
        })
    })
}

/// Decode and verify `HOLOSBP` bytes without rebuilding a Python module.
#[pyfunction]
fn load_bipersistence_artifact(
    py: Python<'_>,
    bytes: Vec<u8>,
) -> PyResult<PyBipersistenceArtifact> {
    py.detach(|| {
        let limits = BipersistenceArtifactLimits::default();
        let artifact = BipersistenceArtifact::decode(&bytes, limits).map_err(to_error)?;
        Ok(PyBipersistenceArtifact { artifact, limits })
    })
}
