//! Euclidean point-atlas bindings.

use pyo3::prelude::*;

use super::common::*;

/// Compiled Euclidean point atlas with a conservative displacement radius.
#[pyclass(name = "PointAtlas")]
pub(crate) struct PyPointAtlas {
    points: Vec<Vec<f64>>,
    atlas: PointPersistenceAtlas,
}

#[pymethods]
impl PyPointAtlas {
    /// Conservative per-point Euclidean displacement radius.
    #[getter]
    fn coordinate_radius(&self) -> f64 {
        self.atlas.coordinate_radius()
    }

    /// Evaluate the point cloud most recently supplied to this object.
    fn result(&self, py: Python<'_>) -> PyResult<AtlasResult> {
        py.detach(|| {
            self.atlas
                .evaluate(&self.points)
                .map(to_atlas_result)
                .map_err(to_err)
        })
    }

    /// Analytic endpoint gradients with respect to point coordinates.
    fn sensitivities(&self, py: Python<'_>) -> Vec<PointSensitivityRecord> {
        py.detach(|| point_sensitivity_records(&self.atlas))
    }

    /// Evaluate points inside the conservative displacement radius.
    fn evaluate(&self, py: Python<'_>, points: Vec<Vec<f64>>) -> PyResult<AtlasResult> {
        py.detach(|| {
            self.atlas
                .evaluate(&points)
                .map(to_atlas_result)
                .map_err(to_err)
        })
    }

    /// Reuse this point atlas or recompile after a radius event.
    fn update(&mut self, py: Python<'_>, points: Vec<Vec<f64>>) -> PyResult<(String, AtlasResult)> {
        py.detach(|| {
            let update = self.atlas.update(&points).map_err(to_err)?;
            let mode = match update.mode {
                UpdateMode::Reused => "reused",
                UpdateMode::Recomputed => "recomputed",
            };
            self.points = points;
            self.atlas = update.atlas;
            Ok((mode.into(), to_atlas_result(update.evaluation)))
        })
    }
}

#[pyfunction]
#[pyo3(signature = (points, threshold, modulus=2, threads=1, factorization="off", collapse_edges=false, collapse_schedule="serial", collapse_objective="h2", collapse_work_limit=None))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn compile_points_atlas(
    py: Python<'_>,
    points: Vec<Vec<f64>>,
    threshold: f64,
    modulus: u32,
    threads: usize,
    factorization: &str,
    collapse_edges: bool,
    collapse_schedule: &str,
    collapse_objective: &str,
    collapse_work_limit: Option<u64>,
) -> PyResult<PyPointAtlas> {
    py.detach(|| {
        let params = params(
            1,
            Some(threshold),
            modulus,
            threads,
            factorization,
            collapse_edges,
            collapse_schedule,
            collapse_objective,
            collapse_work_limit,
        )?;
        let atlas = PointPersistenceAtlas::build(&points, &params).map_err(to_err)?;
        Ok(PyPointAtlas { points, atlas })
    })
}
