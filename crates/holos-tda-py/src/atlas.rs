//! Sparse persistence atlas bindings.

use pyo3::prelude::*;
use pyo3::types::PyBytes;

use super::common::*;

/// Compiled sparse graph atlas with `HOLOSATL` bytes.
#[pyclass(name = "SparseAtlas")]
pub(crate) struct PySparseAtlas {
    input: SparseDistanceMatrix,
    atlas: PersistenceAtlas,
    artifact: AtlasArtifact,
    params: RipsParams,
}

#[pymethods]
impl PySparseAtlas {
    /// Canonical `HOLOSATL` bytes for the current compiled region.
    #[getter]
    fn artifact<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let bytes = self.artifact.encode().map_err(display_err)?;
        Ok(PyBytes::new(py, &bytes))
    }

    /// Evaluate the graph most recently supplied to this object.
    fn result(&self, py: Python<'_>) -> PyResult<AtlasResult> {
        py.detach(|| {
            self.atlas
                .evaluate(&self.input)
                .map(to_atlas_result)
                .map_err(to_err)
        })
    }

    /// Evaluate weights inside the current region without reduction.
    fn evaluate(
        &self,
        py: Python<'_>,
        n: usize,
        triplets: Vec<(usize, usize, f64)>,
    ) -> PyResult<AtlasResult> {
        py.detach(|| {
            let input = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
            self.atlas
                .evaluate(&input)
                .map(to_atlas_result)
                .map_err(to_err)
        })
    }

    /// Return every event that prevents reuse at new weights.
    fn events(
        &self,
        py: Python<'_>,
        n: usize,
        triplets: Vec<(usize, usize, f64)>,
    ) -> PyResult<Vec<EventRecord>> {
        py.detach(|| {
            let input = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
            Ok(self
                .atlas
                .events(&input)
                .into_iter()
                .map(event_record)
                .collect())
        })
    }

    /// Reuse this atlas or compile a new proof-carrying region.
    fn update(
        &mut self,
        py: Python<'_>,
        n: usize,
        triplets: Vec<(usize, usize, f64)>,
    ) -> PyResult<(String, Vec<EventRecord>, AtlasResult)> {
        py.detach(|| {
            let input = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
            let events = self.atlas.events(&input);
            let mode = if events.is_empty() {
                self.input = input;
                UpdateMode::Reused
            } else {
                let (artifact, atlas) =
                    AtlasArtifact::compile(&input, &self.params, CertificateLimits::default())
                        .map_err(display_err)?;
                self.input = input;
                self.atlas = atlas;
                self.artifact = artifact;
                UpdateMode::Recomputed
            };
            let result = self
                .atlas
                .evaluate(&self.input)
                .map(to_atlas_result)
                .map_err(to_err)?;
            let mode = match mode {
                UpdateMode::Reused => "reused",
                UpdateMode::Recomputed => "recomputed",
            };
            Ok((
                mode.into(),
                events.into_iter().map(event_record).collect(),
                result,
            ))
        })
    }
}

#[pyfunction]
#[pyo3(signature = (n, triplets, threshold=None, modulus=2, threads=1, factorization="off", collapse_edges=false, collapse_schedule="serial", collapse_objective="h2", collapse_work_limit=None))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn compile_sparse_atlas(
    py: Python<'_>,
    n: usize,
    triplets: Vec<(usize, usize, f64)>,
    threshold: Option<f64>,
    modulus: u32,
    threads: usize,
    factorization: &str,
    collapse_edges: bool,
    collapse_schedule: &str,
    collapse_objective: &str,
    collapse_work_limit: Option<u64>,
) -> PyResult<PySparseAtlas> {
    py.detach(|| {
        let params = params(
            1,
            threshold,
            modulus,
            threads,
            factorization,
            collapse_edges,
            collapse_schedule,
            collapse_objective,
            collapse_work_limit,
        )?;
        let input = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
        let (artifact, atlas) =
            AtlasArtifact::compile(&input, &params, CertificateLimits::default())
                .map_err(display_err)?;
        Ok(PySparseAtlas {
            input,
            atlas,
            artifact,
            params,
        })
    })
}

#[pyfunction]
pub(crate) fn load_sparse_atlas(
    py: Python<'_>,
    n: usize,
    triplets: Vec<(usize, usize, f64)>,
    artifact: Vec<u8>,
) -> PyResult<PySparseAtlas> {
    py.detach(|| {
        let input = SparseDistanceMatrix::from_triplets(n, &triplets).map_err(to_err)?;
        let artifact = AtlasArtifact::decode(
            &artifact,
            AtlasDecodeLimits::default(),
            CertificateLimits::default(),
        )
        .map_err(display_err)?;
        let atlas = artifact
            .verify(&input, CertificateLimits::default())
            .map_err(display_err)?;
        let mut params = RipsParams::new(1).with_modulus(artifact.modulus());
        params.threshold = artifact.threshold();
        Ok(PySparseAtlas {
            input,
            atlas,
            artifact,
            params,
        })
    })
}
